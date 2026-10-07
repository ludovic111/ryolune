//! ryolune native plugins: libraries built with the `ryolune-plugin` SDK and loaded through
//! its C ABI. The stock library is linked into the engine and goes through the very same
//! vtables, so the ABI adapter below runs in every session, not only when a third-party
//! library is installed.
//!
//! Two ABI versions are served. A library is asked for `ryolune_plugin_entry_v2` first; one
//! built before ABI 2 only has `ryolune_plugin_entry` and runs through its ABI 1 table exactly
//! as it always did: notes only, parameters at block starts, state made of parameter values.
//! An ABI 2 plugin also gets controllers, pitch bend and pressure, parameter changes at
//! their frames, an opaque state blob, a tail length and latency-change notices.
//!
//! Loading keeps a library resident for the life of the process (like the CLAP host), so
//! vtable references are `'static`. Instances are opaque pointers: the editor half owns the
//! parameter values and the processor half owns the instance on the audio thread. State
//! restores go through a lock-free handoff that the processor applies before its next block.

use crate::{
    plugin::{Descriptor, Editor, Format, Instance, ParamChange, ParamInfo, Processor},
    Result,
};
use base64::Engine as _;
use ryolune_plugin::{
    ffi::{
        self, Entry, Entry2, Entry2Fn, EntryFn, Manifest, PluginVTable, PluginVTable2, RawContext,
    },
    Event, Kind, NoteEvent, ProcessContext, TimedParam, ENTRY_SYMBOL, ENTRY_SYMBOL_V2,
    LEGACY_ENTRY_SYMBOL, LEGACY_ENTRY_SYMBOL_V2,
};
use std::{
    collections::HashMap,
    ffi::c_void,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
};

/// One plugin type as its library exports it: always the ABI 1 table, and the ABI 2 table
/// around it when the library has one.
#[derive(Clone, Copy)]
pub struct Table {
    pub base: &'static PluginVTable,
    pub v2: Option<&'static PluginVTable2>,
}
impl Table {
    pub fn abi(&self) -> u32 {
        if self.v2.is_some() {
            2
        } else {
            1
        }
    }
}
impl From<&'static PluginVTable> for Table {
    fn from(base: &'static PluginVTable) -> Self {
        Self { base, v2: None }
    }
}
impl From<&'static PluginVTable2> for Table {
    fn from(table: &'static PluginVTable2) -> Self {
        Self {
            base: &table.base,
            v2: Some(table),
        }
    }
}

struct Loaded {
    _library: libloading::Library,
    tables: Vec<Table>,
}
// Vtables are immutable function tables and the library is never unloaded.
unsafe impl Send for Loaded {}
unsafe impl Sync for Loaded {}

/// Platform extension of a loadable library.
pub fn library_extension() -> &'static str {
    if cfg!(target_os = "macos") {
        "dylib"
    } else if cfg!(windows) {
        "dll"
    } else {
        "so"
    }
}
/// Whether a path looks like a native plugin: an `.onplug` bundle or a bare library.
pub fn looks_like_plugin(path: &Path) -> bool {
    path.extension().is_some_and(|e| {
        e.eq_ignore_ascii_case("onplug") || e.eq_ignore_ascii_case(library_extension())
    })
}
/// The library inside an `.onplug` bundle, or the file itself.
pub fn library_path(bundle: &Path) -> Result<PathBuf> {
    if !bundle.is_dir() {
        return Ok(bundle.to_path_buf());
    }
    let mut candidates = vec![];
    for dir in [bundle.join("Contents/MacOS"), bundle.to_path_buf()] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .filter(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case(library_extension()))
                    || dir.ends_with("Contents/MacOS")
            })
            .collect();
        found.sort();
        candidates.extend(found);
    }
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| format!("No plugin library in {}", bundle.display()))
}

fn cache() -> &'static Mutex<HashMap<PathBuf, Arc<Loaded>>> {
    static LOADED: OnceLock<Mutex<HashMap<PathBuf, Arc<Loaded>>>> = OnceLock::new();
    LOADED.get_or_init(|| Mutex::new(HashMap::new()))
}
/// Read every vtable an entry exports, checking the ABI version first.
///
/// # Safety
/// `entry` must point at an `Entry` produced by the SDK's export macro.
unsafe fn tables_of(entry: *const Entry) -> Result<Vec<&'static PluginVTable>> {
    if entry.is_null() {
        return Err("Plugin entry is null".into());
    }
    let entry = &*entry;
    if entry.abi_version != ryolune_plugin::BASE_ABI_VERSION {
        return Err(format!(
            "Plugin ABI {} is not supported by this ryolune (ABI {} to {})",
            entry.abi_version,
            ryolune_plugin::BASE_ABI_VERSION,
            ryolune_plugin::ABI_VERSION
        ));
    }
    if entry.plugin_count == 0 || entry.plugin_count > 256 {
        return Err("Plugin library exports no plugins or too many".into());
    }
    let mut tables = vec![];
    for index in 0..entry.plugin_count {
        let table = (entry.plugin)(index);
        if table.is_null() {
            return Err(format!("Plugin {index} has no vtable"));
        }
        tables.push(&*table);
    }
    Ok(tables)
}
/// The same for an ABI 2 entry. A later ABI is refused here and the caller falls back to
/// the library's ABI 1 entry, which every library keeps exporting.
///
/// # Safety
/// `entry` must point at an `Entry2` produced by the SDK's export macro.
unsafe fn tables_of_v2(entry: *const Entry2) -> Result<Vec<Table>> {
    if entry.is_null() {
        return Err("Plugin entry is null".into());
    }
    let entry = &*entry;
    if entry.abi_version != 2 {
        return Err(format!(
            "Plugin ABI {} is newer than this ryolune",
            entry.abi_version
        ));
    }
    if entry.plugin_count == 0 || entry.plugin_count > 256 {
        return Err("Plugin library exports no plugins or too many".into());
    }
    (0..entry.plugin_count)
        .map(|index| {
            ffi::checked_v2((entry.plugin)(index))
                .map(Table::from)
                .ok_or_else(|| format!("Plugin {index} has no usable ABI 2 vtable"))
        })
        .collect()
}
fn load(bundle: &Path) -> Result<Arc<Loaded>> {
    let mut guard = cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(loaded) = guard.get(bundle) {
        return Ok(loaded.clone());
    }
    let loaded = Arc::new(load_uncached(bundle)?);
    guard.insert(bundle.to_path_buf(), loaded.clone());
    Ok(loaded)
}

// Only cached host instances may retain the vtables. Inspection returns owned metadata and
// releases the module, leaving Cargo free to replace its output on Windows.
fn load_uncached(bundle: &Path) -> Result<Loaded> {
    let binary = library_path(bundle)?;
    // SAFETY: loading a plugin binary runs its initialisers; this is inherent to hosting.
    let library = unsafe { libloading::Library::new(&binary) }
        .map_err(|e| format!("Cannot load {}: {e}", binary.display()))?;
    let tables = unsafe {
        // Libraries built before the rename export the same entries as `ondera_plugin_entry*`.
        let newer = [ENTRY_SYMBOL_V2, LEGACY_ENTRY_SYMBOL_V2]
            .iter()
            .find_map(|name| {
                library
                    .get::<Entry2Fn>(format!("{name}\0").as_bytes())
                    .ok()
                    .and_then(|entry| tables_of_v2(entry()).ok())
            });
        match newer {
            Some(tables) => tables,
            None => {
                let entry: libloading::Symbol<EntryFn> = library
                    .get(format!("{ENTRY_SYMBOL}\0").as_bytes())
                    .or_else(|_| library.get(format!("{LEGACY_ENTRY_SYMBOL}\0").as_bytes()))
                    .map_err(|e| format!("Not a ryolune plugin ({e})"))?;
                tables_of(entry())?.into_iter().map(Table::from).collect()
            }
        }
    };
    Ok(Loaded {
        _library: library,
        tables,
    })
}

fn descriptor_of(manifest: &Manifest, path: &Path) -> Descriptor {
    Descriptor {
        id: format!("native:{}", manifest.id),
        format: Format::Native,
        name: manifest.name.clone(),
        vendor: manifest.vendor.clone(),
        path: path.to_string_lossy().into_owned(),
        instrument: manifest.kind == Kind::Instrument,
        effect: manifest.kind == Kind::Effect,
        category: manifest.category.clone(),
    }
}
/// Probe a bundle: every plugin it exports. Runs in the `--scan-plugin` child process.
pub fn scan(bundle: &Path) -> Result<Vec<Descriptor>> {
    let loaded = load(bundle)?;
    let mut out = vec![];
    for table in &loaded.tables {
        let manifest = unsafe { ffi::read_manifest(table.base)? };
        out.push(descriptor_of(&manifest, bundle));
    }
    Ok(out)
}
/// Inspect a build artifact without keeping it loaded or locking its file on Windows.
pub fn inspect(bundle: &Path) -> Result<Vec<Descriptor>> {
    let loaded = load_uncached(bundle)?;
    loaded
        .tables
        .iter()
        .map(|table| {
            // SAFETY: the library remains loaded until every manifest is copied into owned data.
            let manifest = unsafe { ffi::read_manifest(table.base)? };
            Ok(descriptor_of(&manifest, bundle))
        })
        .collect()
}

/// Manifests of a static in-process table (the stock library, tests).
pub fn manifests(tables: &'static [PluginVTable]) -> Result<Vec<Manifest>> {
    tables
        .iter()
        .map(|table| unsafe { ffi::read_manifest(table) })
        .collect()
}
pub fn param_infos(manifest: &Manifest) -> Vec<ParamInfo> {
    manifest
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| ParamInfo {
            id: i as u32,
            name: p.name.clone(),
            min: p.min,
            max: p.max,
            default: p.default,
            unit: p.unit.clone(),
            steps: p.steps,
            log: p.log,
            labels: p.labels.clone(),
        })
        .collect()
}

/// Instantiate a scanned native plugin by descriptor id.
pub fn instantiate(plugin_id: &str, rate: u32) -> Result<Instance> {
    let descriptor = super::scan::lookup(plugin_id)
        .ok_or_else(|| format!("Unknown native plugin `{plugin_id}`; rescan plugins"))?;
    let bundle = PathBuf::from(&descriptor.path);
    let loaded = load(&bundle)?;
    for table in &loaded.tables {
        let manifest = unsafe { ffi::read_manifest(table.base)? };
        if format!("native:{}", manifest.id) == plugin_id {
            return instance_from(*table, &manifest, descriptor_of(&manifest, &bundle), rate);
        }
    }
    Err(format!(
        "{} no longer exports `{plugin_id}`",
        bundle.display()
    ))
}

/// What the two halves share. Parameter values wait here for the audio thread after a state
/// restore; an ABI 2 restore also sends a whole new instance, already loaded on the main
/// thread, and gets the old one back to destroy there.
struct Shared {
    table: Table,
    values: Vec<AtomicU64>,
    pending: AtomicBool,
    latency: AtomicU32,
    incoming: AtomicPtr<c_void>,
    outgoing: AtomicPtr<c_void>,
}
impl Shared {
    fn destroy(&self, instance: *mut c_void) {
        if !instance.is_null() {
            unsafe { (self.table.base.destroy)(instance) };
        }
    }
}
impl Drop for Shared {
    fn drop(&mut self) {
        self.destroy(self.incoming.swap(std::ptr::null_mut(), Ordering::AcqRel));
        self.destroy(self.outgoing.swap(std::ptr::null_mut(), Ordering::AcqRel));
    }
}

/// Events and parameter changes one block can carry to an ABI 2 plugin.
const EVENT_CAPACITY: usize = 1024;

/// Build an editor/processor pair around one plugin type.
pub fn instance_from(
    table: impl Into<Table>,
    manifest: &Manifest,
    descriptor: Descriptor,
    rate: u32,
) -> Result<Instance> {
    let table = table.into();
    let params = param_infos(manifest);
    let values: Vec<f64> = params.iter().map(|p| p.default).collect();
    let create = || -> Result<*mut c_void> {
        // SAFETY: the vtable comes from the SDK export macro or a static table built with it.
        let instance = unsafe { (table.base.create)(rate as f64) };
        if instance.is_null() {
            return Err(format!("{} could not be created", descriptor.name));
        }
        Ok(instance)
    };
    let instance = create()?;
    for (i, value) in values.iter().enumerate() {
        unsafe { (table.base.set_param)(instance, i as u32, *value) };
    }
    let latency = unsafe { (table.base.latency)(instance) };
    // ABI 2 state is saved from, and first loaded into, an instance of its own that never
    // leaves this thread: the one in the rack belongs to the audio thread.
    let model = match table.v2 {
        Some(_) => match create() {
            Ok(model) => model,
            Err(error) => {
                unsafe { (table.base.destroy)(instance) };
                return Err(error);
            }
        },
        None => std::ptr::null_mut(),
    };
    let tail_seconds = table
        .v2
        .map_or(0.0, |v2| unsafe { (v2.tail_seconds)(instance) });
    let shared = Arc::new(Shared {
        table,
        values: values.iter().map(|v| AtomicU64::new(v.to_bits())).collect(),
        pending: AtomicBool::new(false),
        latency: AtomicU32::new(latency),
        incoming: AtomicPtr::new(std::ptr::null_mut()),
        outgoing: AtomicPtr::new(std::ptr::null_mut()),
    });
    Ok(Instance {
        editor: Box::new(NativeEditor {
            descriptor,
            params,
            values,
            rate,
            model,
            state: Vec::new(),
            tail_seconds,
            shared: shared.clone(),
        }),
        processor: Some(Box::new(NativeProcessor {
            table,
            instance,
            shared,
            events: Vec::with_capacity(EVENT_CAPACITY),
            notes: Vec::with_capacity(if table.v2.is_some() {
                0
            } else {
                EVENT_CAPACITY
            }),
            changes: Vec::with_capacity(EVENT_CAPACITY),
        })),
    })
}

struct NativeEditor {
    descriptor: Descriptor,
    params: Vec<ParamInfo>,
    values: Vec<f64>,
    rate: u32,
    /// ABI 2 only: the main-thread instance state is saved from. Null for ABI 1.
    model: *mut c_void,
    /// The opaque state last loaded, kept so a save without a model change is lossless.
    state: Vec<u8>,
    tail_seconds: f64,
    shared: Arc<Shared>,
}
impl NativeEditor {
    fn retire(&self) {
        self.shared.destroy(
            self.shared
                .outgoing
                .swap(std::ptr::null_mut(), Ordering::AcqRel),
        );
    }
    /// The plugin's own state, from the model instance with the current parameter values.
    fn opaque_state(&self) -> Option<Vec<u8>> {
        let v2 = self.shared.table.v2?;
        for (index, value) in self.values.iter().enumerate() {
            unsafe { (v2.base.set_param)(self.model, index as u32, *value) };
        }
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut len = 0usize;
        let code = unsafe { (v2.save)(self.model, &mut ptr, &mut len) };
        if code != 0 {
            // A plugin that cannot save keeps what it was given rather than losing it.
            return (!self.state.is_empty()).then(|| self.state.clone());
        }
        if ptr.is_null() {
            return None;
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len).to_vec() };
        unsafe { (v2.base.free_bytes)(ptr, len) };
        Some(bytes)
    }
}
impl Drop for NativeEditor {
    fn drop(&mut self) {
        self.shared.destroy(self.model);
        self.retire();
    }
}
impl Editor for NativeEditor {
    fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }
    fn params(&self) -> &[ParamInfo] {
        &self.params
    }
    fn value(&self, id: u32) -> Option<f64> {
        self.values.get(id as usize).copied()
    }
    fn set_value(&mut self, id: u32, value: f64) {
        if let Some(v) = self.values.get_mut(id as usize) {
            *v = value;
        }
    }
    fn latency(&self) -> u32 {
        self.shared.latency.load(Ordering::Relaxed)
    }
    fn tail_seconds(&self) -> f64 {
        self.tail_seconds
    }
    fn idle(&mut self) {
        self.retire();
    }
    /// State is the parameter value list: the same JSON array the stock library has always
    /// written, so older sessions load unchanged. An ABI 2 plugin with state of its own gets
    /// `{"values": [...], "state": "<base64>"}` instead.
    fn save(&mut self) -> Option<Vec<u8>> {
        match self.opaque_state() {
            Some(state) => serde_json::to_vec(&serde_json::json!({
                "values": self.values,
                "state": base64::engine::general_purpose::STANDARD.encode(state),
            }))
            .ok(),
            None => serde_json::to_vec(&self.values).ok(),
        }
    }
    fn load(&mut self, bytes: &[u8]) -> Result<()> {
        let parsed: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| format!("Invalid plugin state: {e}"))?;
        let values = parsed
            .as_array()
            .or_else(|| parsed.get("values").and_then(|v| v.as_array()))
            .ok_or("Plugin state must be a list of parameter values")?;
        let state = match parsed.get("state").and_then(|s| s.as_str()) {
            Some(text) => base64::engine::general_purpose::STANDARD
                .decode(text)
                .map_err(|e| format!("Invalid plugin state: {e}"))?,
            None => Vec::new(),
        };
        let mut next = self.values.clone();
        for (i, v) in values.iter().enumerate() {
            if let (Some(slot), Some(v)) = (next.get_mut(i), v.as_f64()) {
                if v.is_finite() {
                    *slot = v.clamp(self.params[i].min, self.params[i].max);
                }
            }
        }
        if let Some(v2) = self.shared.table.v2.filter(|_| !state.is_empty()) {
            // Load into a new instance here, where allocating is fine, and swap it in on the
            // audio thread. Nothing changes if the plugin refuses the state.
            let fresh = unsafe { (v2.base.create)(self.rate as f64) };
            if fresh.is_null() {
                return Err(format!("{} could not be created", self.descriptor.name));
            }
            for target in [fresh, self.model] {
                for (index, value) in next.iter().enumerate() {
                    unsafe { (v2.base.set_param)(target, index as u32, *value) };
                }
            }
            let code = unsafe { (v2.load)(fresh, state.as_ptr(), state.len()) };
            if code != 0 {
                self.shared.destroy(fresh);
                return Err(format!(
                    "{} could not read its saved state (code {code})",
                    self.descriptor.name
                ));
            }
            unsafe { (v2.load)(self.model, state.as_ptr(), state.len()) };
            self.retire();
            // A restore the audio thread has not picked up yet is superseded.
            self.shared
                .destroy(self.shared.incoming.swap(fresh, Ordering::AcqRel));
        }
        self.state = state;
        self.values = next;
        for (value, shared) in self.values.iter().zip(&self.shared.values) {
            shared.store(value.to_bits(), Ordering::Relaxed);
        }
        self.shared.pending.store(true, Ordering::Release);
        Ok(())
    }
}

struct NativeProcessor {
    table: Table,
    instance: *mut c_void,
    shared: Arc<Shared>,
    events: Vec<Event>,
    notes: Vec<NoteEvent>,
    changes: Vec<TimedParam>,
}
// The SDK requires `Plugin: Send`; the instance pointer is owned by exactly one processor.
unsafe impl Send for NativeProcessor {}
impl NativeProcessor {
    /// Take a restored instance from the main thread, once the previous one has been
    /// collected. Neither side allocates or frees here.
    fn adopt_restored(&mut self) {
        if !self.shared.outgoing.load(Ordering::Acquire).is_null() {
            return;
        }
        let fresh = self
            .shared
            .incoming
            .swap(std::ptr::null_mut(), Ordering::AcqRel);
        if !fresh.is_null() {
            self.shared.outgoing.store(self.instance, Ordering::Release);
            self.instance = fresh;
        }
    }
}
impl Processor for NativeProcessor {
    fn reset(&mut self) {
        unsafe { (self.table.base.reset)(self.instance) };
    }
    fn process(
        &mut self,
        audio: &mut [[f32; 2]],
        events: &[Event],
        params: &[ParamChange],
        ctx: &ProcessContext,
    ) {
        self.adopt_restored();
        if self.shared.pending.swap(false, Ordering::Acquire) {
            for (index, value) in self.shared.values.iter().enumerate() {
                unsafe {
                    (self.table.base.set_param)(
                        self.instance,
                        index as u32,
                        f64::from_bits(value.load(Ordering::Relaxed)),
                    )
                };
            }
        }
        let raw = RawContext::from(ctx);
        let Some(v2) = self.table.v2 else {
            // ABI 1 applies values between blocks; the rack splits the block at each change.
            for change in params {
                unsafe { (self.table.base.set_param)(self.instance, change.id, change.value) };
            }
            // ABI 1 knows notes only; controllers, bend and pressure stop here.
            self.notes.clear();
            self.notes.extend(
                events
                    .iter()
                    .filter_map(Event::as_note)
                    .take(EVENT_CAPACITY),
            );
            unsafe {
                (self.table.base.process)(
                    self.instance,
                    audio.as_mut_ptr(),
                    audio.len() as u32,
                    self.notes.as_ptr(),
                    self.notes.len() as u32,
                    &raw,
                )
            };
            return;
        };
        self.events.clear();
        self.events
            .extend(events.iter().take(EVENT_CAPACITY).copied());
        self.changes.clear();
        let last = audio.len().saturating_sub(1) as u32;
        self.changes
            .extend(params.iter().take(EVENT_CAPACITY).map(|change| TimedParam {
                frame: change.frame.min(last),
                index: change.id,
                value: change.value,
            }));
        let flags = unsafe {
            (v2.process_events)(
                self.instance,
                audio.as_mut_ptr(),
                audio.len() as u32,
                self.events.as_ptr(),
                self.events.len() as u32,
                self.changes.as_ptr(),
                self.changes.len() as u32,
                &raw,
            )
        };
        if flags & ffi::FLAG_LATENCY_CHANGED != 0 {
            let latency = unsafe { (v2.base.latency)(self.instance) };
            self.shared.latency.store(latency, Ordering::Relaxed);
        }
    }
    fn latency(&self) -> u32 {
        unsafe { (self.table.base.latency)(self.instance) }
    }
    /// ABI 2 carries each change's frame to the plugin.
    fn timed_params(&self) -> bool {
        self.table.v2.is_some()
    }
    /// Every ABI 2 plugin takes every event kind (and ignores the ones it does not know), so an
    /// ABI 2 effect hears its track's controllers. ABI 1 knows notes only, which an insert
    /// never gets.
    fn accepts_events(&self) -> bool {
        self.table.v2.is_some()
    }
}
impl Drop for NativeProcessor {
    fn drop(&mut self) {
        unsafe { (self.table.base.destroy)(self.instance) };
    }
}

/// Load a library from a raw entry pointer, for tests that link a plugin crate directly.
///
/// # Safety
/// `entry` must come from the SDK's `ryolune_plugin_entry`.
pub unsafe fn tables_from_entry(entry: *const Entry) -> Result<Vec<&'static PluginVTable>> {
    tables_of(entry)
}
/// The ABI 2 counterpart of [`tables_from_entry`].
///
/// # Safety
/// `entry` must come from the SDK's `ryolune_plugin_entry_v2`.
pub unsafe fn tables_from_entry_v2(entry: *const Entry2) -> Result<Vec<Table>> {
    tables_of_v2(entry)
}
/// Which ABI a bundle on disk is served through: 2, or 1 for a library built before it.
pub fn abi_of(bundle: &Path) -> Result<u32> {
    Ok(load(bundle)?.tables.first().map_or(1, Table::abi))
}
