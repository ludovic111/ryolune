//! LV2 host, written to LV2 1.18 (lv2plug.in) without lilv: bundles are read by the Turtle
//! reader in [`turtle`] and described by [`world`]; binaries are loaded with `libloading` and
//! reached through `lv2_descriptor`.
//!
//! What is supported: audio, control and CV ports, atom ports taking MIDI (`midi:MidiEvent`
//! in an `atom:Sequence`) for instruments and MIDI effects, port properties (bounds, default,
//! integer, toggled, enumeration with scale points, logarithmic, sample-rate bounds, units),
//! the latency port (`lv2:reportsLatency` / `lv2:latency`), the `lv2:enabled`,
//! `lv2:freeWheeling` and `time:beatsPerMinute` designations, the features in
//! [`features::SUPPORTED`], State (saved into ryolune's state blob with the control ports),
//! the default state, Worker (on a thread of the instance) and presets (`pset:Preset`).
//! A plugin that requires any other feature is refused with that feature's name.
//!
//! Not supported: plugin UIs (`ui:` X11, Cocoa, external and others; the parameter panel
//! works for every plugin), `lv2_lib_descriptor` libraries, the old event extension
//! (`ev:EventPort`) and options a plugin wants to change at run time.
//!
//! A plugin with one audio input and one audio output runs as two instances, one per stereo
//! channel, with the same parameters: what other hosts do, so a mono EQ does not fold a
//! stereo track to mono. Any other layout runs as one instance; the first two audio ports
//! carry left and right, a single port carries the mono sum in and both channels out.

pub mod features;
pub mod ffi;
pub mod state;
pub mod turtle;
pub mod worker;
pub mod world;

use crate::{
    plugin::{Descriptor, Editor, Event, Format, Instance, ParamChange, ParamInfo, ProcessContext, Processor, MAX_BLOCK},
    Result,
};
use ffi::{uri, LV2_Descriptor, LV2_Handle, LV2_State_Interface, LV2_Worker_Interface};
use std::{
    collections::HashMap,
    ffi::{c_void, CStr, CString},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex, OnceLock,
    },
};
use world::{Bundle, Direction, PluginData, PortKind};

/// Bytes in each atom port buffer (more when a port asks with `rsz:minimumSize`).
const ATOM_CAPACITY: usize = 32 * 1024;

// ---------------------------------------------------------------------------
// Libraries stay loaded for the life of the process, like the other hosts' entries.
// ---------------------------------------------------------------------------

struct Library {
    _library: libloading::Library,
    descriptor: ffi::DescriptorFn,
}
fn load(binary: &Path) -> Result<Arc<Library>> {
    static LOADED: OnceLock<Mutex<HashMap<PathBuf, Arc<Library>>>> = OnceLock::new();
    let cache = LOADED.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(loaded) = guard.get(binary) {
        return Ok(loaded.clone());
    }
    if !binary.is_file() {
        return Err(format!("The plugin's binary {} is missing", binary.display()));
    }
    // SAFETY: loading a plugin binary runs its initialisers; this is inherent to hosting.
    let library = unsafe { libloading::Library::new(binary) }
        .map_err(|e| format!("Cannot load {}: {e}", binary.display()))?;
    let descriptor: ffi::DescriptorFn = unsafe {
        *library
            .get::<ffi::DescriptorFn>(b"lv2_descriptor\0")
            .map_err(|_| {
                format!(
                    "{} has no lv2_descriptor (libraries that only export lv2_lib_descriptor are not supported)",
                    binary.display()
                )
            })?
    };
    let loaded = Arc::new(Library {
        _library: library,
        descriptor,
    });
    guard.insert(binary.to_path_buf(), loaded.clone());
    Ok(loaded)
}
/// The descriptor a library exports for `plugin_uri`.
fn find(library: &Library, plugin_uri: &str) -> Result<*const LV2_Descriptor> {
    for index in 0..10_000 {
        let d = unsafe { (library.descriptor)(index) };
        if d.is_null() {
            break;
        }
        let u = unsafe { (*d).uri };
        if !u.is_null() && unsafe { CStr::from_ptr(u) }.to_bytes() == plugin_uri.as_bytes() {
            return Ok(d);
        }
    }
    Err(format!("The binary does not contain the plugin {plugin_uri}"))
}

fn descriptor_of(p: &PluginData) -> Descriptor {
    let instrument = p.instrument();
    Descriptor {
        id: format!("lv2:{}", p.uri),
        format: Format::Lv2,
        name: p.name.clone(),
        vendor: p.vendor.clone(),
        path: p.bundle.to_string_lossy().into_owned(),
        instrument,
        effect: !instrument || p.audio(Direction::Input) > 0,
        category: p.category(),
    }
}

/// Probe a bundle: every plugin it declares whose binary loads and exports it. Runs in the
/// `--scan-plugin` child process (loading a binary runs its code). A bundle of presets only
/// has no plugins and is not an error.
pub fn scan(bundle: &Path) -> Result<Vec<Descriptor>> {
    let mut b = Bundle::open(bundle)?;
    let plugins = b.plugins()?;
    let mut out = vec![];
    let mut first_error = None;
    for p in &plugins {
        match load(&p.binary).and_then(|lib| find(&lib, &p.uri)) {
            Ok(_) => out.push(descriptor_of(p)),
            Err(e) => {
                first_error.get_or_insert(format!("{}: {e}", p.name));
            }
        }
    }
    match first_error {
        Some(e) if out.is_empty() => Err(e),
        _ => Ok(out),
    }
}

/// The LV2 bundles in the scan folders, for presets saved in bundles of their own.
pub fn bundles() -> Vec<PathBuf> {
    let mut out = vec![];
    for dir in super::scan::directories(Format::Lv2) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lv2")) && path.is_dir() {
                out.push(path);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

// ---------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------

struct RawInstance {
    handle: LV2_Handle,
    /// Stopped before the instance is cleaned up.
    worker: Option<worker::Worker>,
    features: Box<features::Features>,
}
struct Shared {
    data: PluginData,
    _library: Arc<Library>,
    descriptor: *const LV2_Descriptor,
    state: *const LV2_State_Interface,
    work: *const LV2_Worker_Interface,
    instances: Vec<RawInstance>,
    /// Control input values (f32 bits) by port index: what the editor reads and the
    /// processor applies when `generation` moves.
    values: Vec<AtomicU32>,
    generation: AtomicU32,
    latency: AtomicU32,
    activated: AtomicBool,
    rate: u32,
}
// SAFETY: the instance handles are used as LV2's threading classes allow: instantiation-class
// calls (instantiate, activate, restore, deactivate, cleanup) on the thread that created
// them while no processor runs, `run` on the audio thread, `save` from the main thread
// (allowed concurrently with audio-class calls) and `work` on the worker thread.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}
impl Shared {
    fn value(&self, port: usize) -> f32 {
        f32::from_bits(self.values[port].load(Ordering::Relaxed))
    }
    fn set(&self, port: usize, value: f32) {
        if let Some(v) = self.values.get(port) {
            v.store(value.to_bits(), Ordering::Relaxed);
        }
    }
    fn port_by_symbol(&self, symbol: &str) -> Option<usize> {
        self.data
            .ports
            .iter()
            .position(|p| p.symbol == symbol && p.is_control_input())
    }
    fn restore(&self, properties: &world::StateProperties) -> Result<()> {
        for instance in &self.instances {
            unsafe {
                state::restore(instance.handle, self.state, instance.features.as_ptr(), properties)?;
            }
        }
        Ok(())
    }
}
impl Drop for Shared {
    fn drop(&mut self) {
        for instance in &mut self.instances {
            // The worker may be inside `work`: stop it before the instance goes.
            drop(instance.worker.take());
            unsafe {
                if self.activated.load(Ordering::Acquire) {
                    if let Some(deactivate) = (*self.descriptor).deactivate {
                        deactivate(instance.handle);
                    }
                }
                if let Some(cleanup) = (*self.descriptor).cleanup {
                    cleanup(instance.handle);
                }
            }
        }
    }
}

/// Instantiate `lv2:<uri>` from the scan cache.
pub fn instantiate(plugin_id: &str, name: &str, rate: u32) -> Result<Instance> {
    let desc = super::scan::lookup(plugin_id)
        .ok_or_else(|| format!("{name} is not installed or has not been scanned"))?;
    let (_, plugin_uri) = Format::parse(plugin_id).ok_or("Bad LV2 plugin id")?;
    instantiate_bundle(Path::new(&desc.path), plugin_uri, rate)
}

/// Instantiate a plugin from its bundle folder, on the calling thread (its main thread).
pub fn instantiate_bundle(bundle: &Path, plugin_uri: &str, rate: u32) -> Result<Instance> {
    let data = Bundle::open(bundle)?.plugin(plugin_uri)?;
    if let Some(missing) = features::missing(&data.required_features) {
        return Err(format!(
            "{} needs the LV2 feature {missing}, which this host does not provide",
            data.name
        ));
    }
    if let Some(port) = data
        .ports
        .iter()
        .find(|p| matches!(p.kind, PortKind::Unknown(_)) && !p.optional)
    {
        let PortKind::Unknown(kind) = &port.kind else { unreachable!() };
        return Err(format!(
            "{} has a port of a kind this host does not support ({kind}, port `{}`)",
            data.name, port.symbol
        ));
    }
    let library = load(&data.binary)?;
    let descriptor = find(&library, plugin_uri)?;
    let extension = |id: &str| -> *const c_void {
        let c = CString::new(id).unwrap_or_default();
        unsafe {
            (*descriptor)
                .extension_data
                .map_or(std::ptr::null(), |f| f(c.as_ptr()))
        }
    };
    let state_interface = extension(uri::STATE_INTERFACE) as *const LV2_State_Interface;
    let work = extension(uri::WORKER_INTERFACE) as *const LV2_Worker_Interface;
    let dual = data.audio(Direction::Input) == 1
        && data.audio(Direction::Output) == 1
        && !data.ports.iter().any(|p| matches!(p.kind, PortKind::Atom { .. }));
    let mut bundle_path = bundle.to_string_lossy().into_owned();
    if !bundle_path.ends_with(std::path::MAIN_SEPARATOR) {
        bundle_path.push(std::path::MAIN_SEPARATOR);
    }
    let bundle_c = CString::new(bundle_path).map_err(|e| e.to_string())?;
    let files = super::scan::data_dir().join("lv2-files").join(
        data.uri
            .rsplit(['/', '#', ':'])
            .find(|s| !s.is_empty())
            .unwrap_or("plugin")
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect::<String>(),
    );
    let mut instances = vec![];
    for _ in 0..if dual { 2 } else { 1 } {
        let worker = (!work.is_null()).then(worker::Worker::new);
        let features = Box::new(features::Features::new(
            &data.name,
            rate as f64,
            ATOM_CAPACITY,
            files.clone(),
            worker.as_ref().map(|w| w.schedule()),
        ));
        let handle = unsafe {
            (*descriptor).instantiate.map_or(std::ptr::null_mut(), |f| {
                f(descriptor, rate as f64, bundle_c.as_ptr(), features.as_ptr())
            })
        };
        if handle.is_null() {
            // Instances made so far are cleaned up by `Shared` below going out of scope.
            drop(Shared {
                data: data.clone(),
                _library: library.clone(),
                descriptor,
                state: state_interface,
                work,
                instances,
                values: vec![],
                generation: AtomicU32::new(0),
                latency: AtomicU32::new(0),
                activated: AtomicBool::new(false),
                rate,
            });
            return Err(format!("{} could not be created at {rate} Hz", data.name));
        }
        let mut worker = worker;
        if let Some(w) = worker.as_mut() {
            w.start(&data.name, handle, work);
        }
        instances.push(RawInstance {
            handle,
            worker,
            features,
        });
    }
    let values = data
        .ports
        .iter()
        .map(|p| AtomicU32::new(port_default(p, rate).to_bits()))
        .collect();
    let mut shared = Shared {
        data,
        _library: library,
        descriptor,
        state: state_interface,
        work,
        instances,
        values,
        generation: AtomicU32::new(1),
        latency: AtomicU32::new(0),
        activated: AtomicBool::new(false),
        rate,
    };
    let responses: Vec<Option<worker::Responses>> = shared
        .instances
        .iter_mut()
        .map(|i| {
            i.worker
                .as_mut()
                .and_then(|w| w.take_responses())
                .map(worker::Responses::new)
        })
        .collect();
    if let Some(default) = shared.data.default_state.clone() {
        if !shared.state.is_null() {
            if let Err(e) = shared.restore(&default) {
                tracing::warn!(plugin = %shared.data.name, "default state not loaded: {e}");
            }
        }
    }
    let shared = Arc::new(shared);
    let mut processor = Lv2Processor::new(shared.clone(), responses);
    unsafe {
        if let Some(activate) = (*descriptor).activate {
            for instance in &shared.instances {
                activate(instance.handle);
            }
        }
    }
    shared.activated.store(true, Ordering::Release);
    if processor.latency_port.is_some() {
        // The latency port is only written by `run`: one quiet block reads it.
        let mut quiet = [[0.0f32; 2]; 64];
        processor.process(&mut quiet, &[], &[], &ProcessContext::default());
    }
    let rate_f = rate as f32;
    let editor = Lv2Editor::new(shared, rate_f);
    Ok(Instance {
        editor: Box::new(editor),
        processor: Some(Box::new(processor)),
    })
}

fn bounds(p: &world::Port, rate: u32) -> (f32, f32) {
    let scale = if p.sample_rate { rate as f32 } else { 1.0 };
    let (lo, hi) = if p.toggled {
        (0.0, 1.0)
    } else {
        (p.minimum.unwrap_or(0.0) * scale, p.maximum.unwrap_or(1.0) * scale)
    };
    if hi > lo {
        (lo, hi)
    } else {
        (lo, lo + 1.0)
    }
}
fn port_default(p: &world::Port, rate: u32) -> f32 {
    if p.kind != PortKind::Control {
        return 0.0;
    }
    let scale = if p.sample_rate { rate as f32 } else { 1.0 };
    let (lo, hi) = bounds(p, rate);
    match p.default {
        Some(d) => (d * scale).clamp(lo, hi),
        None if p.minimum.is_some() => lo,
        None => 0f32.clamp(lo, hi),
    }
}
/// Whether a control input is the host's to set, not a parameter.
fn host_driven(p: &world::Port) -> bool {
    p.designated(&format!("{}enabled", uri::CORE))
        || p.designated(&format!("{}freeWheeling", uri::CORE))
        || p.designated(&format!("{}latency", uri::CORE))
        || p.designation
            .as_deref()
            .is_some_and(|d| d.starts_with("http://lv2plug.in/ns/ext/time#"))
}
fn param_info(p: &world::Port, rate: u32) -> ParamInfo {
    let (min, max) = bounds(p, rate);
    let stepped = p.toggled || p.integer || p.enumeration;
    let steps = if p.toggled {
        1
    } else if stepped {
        ((max - min).round().max(1.0) as u32).min(100_000)
    } else {
        0
    };
    let labels = if p.toggled {
        vec!["Off".to_string(), "On".to_string()]
    } else if stepped && steps <= 64 && min.fract() == 0.0 {
        // Names for every whole value from min to max, when the scale points give them.
        let named: Option<Vec<String>> = (0..=steps)
            .map(|i| {
                let v = min + i as f32;
                p.scale_points
                    .iter()
                    .find(|(sv, _)| (sv - v).abs() < 1e-4)
                    .map(|(_, l)| l.clone())
            })
            .collect();
        named.unwrap_or_default()
    } else {
        vec![]
    };
    ParamInfo {
        id: p.index,
        name: p.name.clone(),
        min: min as f64,
        max: max as f64,
        default: port_default(p, rate) as f64,
        unit: p.unit.clone(),
        steps,
        log: p.logarithmic && min > 0.0,
        labels,
    }
}

// ---------------------------------------------------------------------------
// Editor (main thread)
// ---------------------------------------------------------------------------

pub struct Lv2Editor {
    shared: Arc<Shared>,
    desc: Descriptor,
    params: Vec<ParamInfo>,
    presets: Option<Vec<world::Preset>>,
}
impl Lv2Editor {
    fn new(shared: Arc<Shared>, rate: f32) -> Self {
        let params = shared
            .data
            .ports
            .iter()
            .filter(|p| p.is_control_input() && !host_driven(p) && !p.hidden)
            .map(|p| param_info(p, rate as u32))
            .collect();
        Self {
            desc: descriptor_of(&shared.data),
            shared,
            params,
            presets: None,
        }
    }
    fn presets(&mut self) -> &[world::Preset] {
        if self.presets.is_none() {
            let mut dirs = bundles();
            if !dirs.contains(&self.shared.data.bundle) {
                dirs.insert(0, self.shared.data.bundle.clone());
            }
            self.presets = Some(world::presets(&self.shared.data.uri, &dirs));
        }
        self.presets.as_deref().unwrap_or_default()
    }
    fn port(&self, id: u32) -> Option<&world::Port> {
        self.shared.data.ports.get(id as usize)
    }
    fn apply_ports(&self, ports: impl IntoIterator<Item = (String, f32)>) {
        for (symbol, value) in ports {
            if let Some(index) = self.shared.port_by_symbol(&symbol) {
                let p = &self.shared.data.ports[index];
                let (lo, hi) = bounds(p, self.shared.rate);
                self.shared.set(index, value.clamp(lo, hi));
            }
        }
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
    }
}
impl Editor for Lv2Editor {
    fn descriptor(&self) -> &Descriptor {
        &self.desc
    }
    fn params(&self) -> &[ParamInfo] {
        &self.params
    }
    fn value(&self, id: u32) -> Option<f64> {
        self.port(id)
            .filter(|p| p.is_control_input())
            .map(|_| self.shared.value(id as usize) as f64)
    }
    fn set_value(&mut self, id: u32, value: f64) {
        if self.port(id).is_some_and(|p| p.is_control_input()) {
            self.shared.set(id as usize, value as f32);
        }
    }
    fn text(&self, id: u32, value: f64) -> String {
        if let Some((_, label)) = self.port(id).and_then(|p| {
            p.scale_points
                .iter()
                .find(|(v, _)| (*v as f64 - value).abs() < 1e-4)
        }) {
            return label.clone();
        }
        self.params
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.text(value))
            .unwrap_or_default()
    }
    fn parse_text(&self, id: u32, text: &str) -> Option<f64> {
        if let Some((value, _)) = self.port(id).and_then(|p| {
            p.scale_points
                .iter()
                .find(|(_, l)| l.trim().eq_ignore_ascii_case(text.trim()))
        }) {
            return Some(*value as f64);
        }
        self.params
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.parse_text(text))
    }
    fn save(&mut self) -> Option<Vec<u8>> {
        let ports = self
            .shared
            .data
            .ports
            .iter()
            .filter(|p| p.is_control_input() && !host_driven(p))
            .map(|p| (p.symbol.clone(), self.shared.value(p.index as usize)))
            .collect();
        let instance = self.shared.instances.first()?;
        let properties = unsafe {
            state::save(instance.handle, self.shared.state, instance.features.as_ptr())
        };
        let state = match properties {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(plugin = %self.desc.name, "{e}");
                return None;
            }
        };
        serde_json::to_vec(&state::Blob {
            lv2: 1,
            plugin: self.shared.data.uri.clone(),
            ports,
            state,
        })
        .ok()
    }
    fn load(&mut self, bytes: &[u8]) -> Result<()> {
        let blob = state::Blob::parse(bytes)?;
        if blob.plugin != self.shared.data.uri {
            return Err(format!(
                "This state belongs to {}, not {}",
                blob.plugin, self.shared.data.uri
            ));
        }
        let properties = blob.properties()?;
        self.apply_ports(blob.ports);
        if !properties.0.is_empty() {
            self.shared.restore(&properties)?;
        }
        Ok(())
    }
    fn latency(&self) -> u32 {
        self.shared.latency.load(Ordering::Relaxed)
    }
    fn programs(&mut self) -> Vec<String> {
        self.presets().iter().map(|p| p.label.clone()).collect()
    }
    fn load_program(&mut self, index: usize) -> Result<()> {
        let preset = self
            .presets()
            .get(index)
            .cloned()
            .ok_or_else(|| format!("{} has no preset {index}", self.desc.name))?;
        let data = world::load_preset(&preset)?;
        self.apply_ports(data.ports);
        if let Some(state) = data.state {
            if !self.shared.state.is_null() {
                self.shared.restore(&state)?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Processor (audio thread)
// ---------------------------------------------------------------------------

/// The buffers of one instance, connected once. Indexes are port indexes.
struct Buffers {
    controls: Box<[f32]>,
    audio: Vec<Option<Box<[f32]>>>,
    atoms: Vec<Option<Box<[u64]>>>,
    responses: Option<worker::Responses>,
}
pub struct Lv2Processor {
    shared: Arc<Shared>,
    buffers: Vec<Buffers>,
    seen: u32,
    audio_in: Vec<usize>,
    audio_out: Vec<usize>,
    midi_in: Option<usize>,
    atom_in: Vec<usize>,
    atom_out: Vec<usize>,
    latency_port: Option<usize>,
    enabled: Option<usize>,
    freewheel: Option<usize>,
    bpm: Option<usize>,
    control_in: Vec<usize>,
    sequence: u32,
    chunk: u32,
    midi: u32,
    reset: bool,
}
// SAFETY: buffers are owned here; the plugin pointers follow LV2 threading (see `Shared`).
unsafe impl Send for Lv2Processor {}
impl Lv2Processor {
    fn new(shared: Arc<Shared>, mut responses: Vec<Option<worker::Responses>>) -> Self {
        let ports = &shared.data.ports;
        let indexes = |f: &dyn Fn(&world::Port) -> bool| -> Vec<usize> {
            ports.iter().filter(|p| f(p)).map(|p| p.index as usize).collect()
        };
        let audio_in = indexes(&|p| p.kind == PortKind::Audio && p.direction == Direction::Input);
        let audio_out = indexes(&|p| p.kind == PortKind::Audio && p.direction == Direction::Output);
        let atom_in = indexes(&|p| matches!(p.kind, PortKind::Atom { .. }) && p.direction == Direction::Input);
        let atom_out = indexes(&|p| matches!(p.kind, PortKind::Atom { .. }) && p.direction == Direction::Output);
        let midi_in = atom_in
            .iter()
            .copied()
            .find(|&i| matches!(ports[i].kind, PortKind::Atom { midi: true, .. }));
        let designated = |name: &str| {
            ports
                .iter()
                .find(|p| p.is_control_input() && p.designated(name))
                .map(|p| p.index as usize)
        };
        let latency_port = ports
            .iter()
            .find(|p| {
                p.kind == PortKind::Control
                    && p.direction == Direction::Output
                    && (p.reports_latency || p.designated(&format!("{}latency", uri::CORE)))
            })
            .map(|p| p.index as usize);
        let mut buffers = vec![];
        for (n, instance) in shared.instances.iter().enumerate() {
            let mut b = Buffers {
                controls: ports
                    .iter()
                    .map(|p| shared.value(p.index as usize))
                    .collect(),
                audio: ports
                    .iter()
                    .map(|p| {
                        matches!(p.kind, PortKind::Audio | PortKind::Cv)
                            .then(|| vec![0.0f32; MAX_BLOCK].into_boxed_slice())
                    })
                    .collect(),
                atoms: ports
                    .iter()
                    .map(|p| match p.kind {
                        PortKind::Atom { min_size, .. } => {
                            Some(vec![0u64; ATOM_CAPACITY.max(min_size).div_ceil(8)].into_boxed_slice())
                        }
                        _ => None,
                    })
                    .collect(),
                responses: responses.get_mut(n).and_then(Option::take),
            };
            unsafe {
                let connect = (*shared.descriptor).connect_port;
                for p in ports {
                    let i = p.index as usize;
                    let pointer: *mut c_void = match p.kind {
                        PortKind::Control => &mut b.controls[i] as *mut f32 as *mut c_void,
                        PortKind::Audio | PortKind::Cv => b.audio[i]
                            .as_mut()
                            .map_or(std::ptr::null_mut(), |a| a.as_mut_ptr() as *mut c_void),
                        PortKind::Atom { .. } => b.atoms[i]
                            .as_mut()
                            .map_or(std::ptr::null_mut(), |a| a.as_mut_ptr() as *mut c_void),
                        PortKind::Unknown(_) => std::ptr::null_mut(),
                    };
                    if let Some(connect) = connect {
                        connect(instance.handle, p.index, pointer);
                    }
                }
            }
            buffers.push(b);
        }
        let control_in = indexes(&|p| p.is_control_input());
        let atom = |name: &str| features::map(&format!("{}{name}", uri::ATOM));
        Self {
            seen: 0,
            audio_in,
            audio_out,
            midi_in,
            atom_in,
            atom_out,
            latency_port,
            enabled: designated(&format!("{}enabled", uri::CORE)),
            freewheel: designated(&format!("{}freeWheeling", uri::CORE)),
            bpm: designated(uri::TIME_BPM),
            control_in,
            sequence: atom("Sequence"),
            chunk: atom("Chunk"),
            midi: features::map(uri::MIDI_EVENT),
            reset: false,
            buffers,
            shared,
        }
    }
    /// Write `events` as MIDI into the instance's sequence on `port`.
    fn write_midi(&mut self, port: usize, events: &[Event], frames: usize) {
        let (sequence, midi, reset) = (self.sequence, self.midi, self.reset);
        let Some(buffer) = self.buffers[0].atoms[port].as_mut() else {
            return;
        };
        let capacity = buffer.len() * 8;
        let bytes =
            unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u8, capacity) };
        // Header: atom (size, type), then the sequence body (unit 0 = frames, pad).
        let mut at = 16usize;
        let mut put = |frame: u32, data: [u8; 3], bytes: &mut [u8]| {
            if at + 24 > capacity {
                return;
            }
            bytes[at..at + 8].copy_from_slice(&(frame as i64).to_ne_bytes());
            let size: u32 = if data[0] & 0xf0 == 0xd0 { 2 } else { 3 };
            bytes[at + 8..at + 12].copy_from_slice(&size.to_ne_bytes());
            bytes[at + 12..at + 16].copy_from_slice(&midi.to_ne_bytes());
            bytes[at + 16..at + 24].fill(0);
            bytes[at + 16..at + 16 + size as usize].copy_from_slice(&data[..size as usize]);
            at += 24;
        };
        if reset {
            for channel in 0..16u8 {
                put(0, [0xb0 | channel, 123, 0], bytes);
                put(0, [0xb0 | channel, 120, 0], bytes);
            }
        }
        for event in events {
            if let Some(data) = event.to_midi() {
                put((event.frame as usize).min(frames.saturating_sub(1)) as u32, data, bytes);
            }
        }
        let body = (at - 8) as u32;
        bytes[0..4].copy_from_slice(&body.to_ne_bytes());
        bytes[4..8].copy_from_slice(&sequence.to_ne_bytes());
        bytes[8..16].fill(0);
    }
    fn empty_sequence(buffer: &mut [u64], sequence: u32) {
        let bytes = unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u8, 16) };
        bytes[0..4].copy_from_slice(&8u32.to_ne_bytes());
        bytes[4..8].copy_from_slice(&sequence.to_ne_bytes());
        bytes[8..16].fill(0);
    }
}
impl Processor for Lv2Processor {
    fn reset(&mut self) {
        self.reset = true;
    }
    fn accepts_events(&self) -> bool {
        self.midi_in.is_some()
    }
    fn process(
        &mut self,
        audio: &mut [[f32; 2]],
        events: &[Event],
        params: &[ParamChange],
        ctx: &ProcessContext,
    ) {
        let n = audio.len().min(MAX_BLOCK);
        if n == 0 || !self.shared.activated.load(Ordering::Acquire) {
            return;
        }
        let _audio_thread = features::AudioThreadScope::enter();
        let generation = self.shared.generation.load(Ordering::Acquire);
        if generation != self.seen {
            self.seen = generation;
            for &i in &self.control_in {
                let v = self.shared.value(i);
                for b in &mut self.buffers {
                    b.controls[i] = v;
                }
            }
        }
        for change in params {
            let i = change.id as usize;
            if self.shared.data.ports.get(i).is_some_and(|p| p.is_control_input()) {
                let v = change.value as f32;
                self.shared.set(i, v);
                for b in &mut self.buffers {
                    b.controls[i] = v;
                }
            }
        }
        for b in &mut self.buffers {
            if let Some(i) = self.enabled {
                b.controls[i] = 1.0;
            }
            if let Some(i) = self.freewheel {
                b.controls[i] = 0.0;
            }
            if let Some(i) = self.bpm {
                if ctx.tempo > 0.0 {
                    b.controls[i] = ctx.tempo as f32;
                }
            }
        }
        // Audio in.
        let dual = self.buffers.len() == 2;
        for (k, b) in self.buffers.iter_mut().enumerate() {
            for (slot, &port) in self.audio_in.iter().enumerate() {
                let Some(buffer) = b.audio[port].as_mut() else { continue };
                for (f, frame) in audio[..n].iter().enumerate() {
                    buffer[f] = if dual {
                        frame[k]
                    } else if self.audio_in.len() == 1 {
                        (frame[0] + frame[1]) * 0.5
                    } else if slot < 2 {
                        frame[slot]
                    } else {
                        0.0
                    };
                }
            }
        }
        // Events in, atom outputs emptied to their capacity.
        let (sequence, chunk) = (self.sequence, self.chunk);
        for k in 0..self.atom_in.len() {
            let i = self.atom_in[k];
            if Some(i) == self.midi_in {
                self.write_midi(i, events, n);
            } else if let Some(buffer) = self.buffers[0].atoms[i].as_mut() {
                Self::empty_sequence(buffer, sequence);
            }
        }
        self.reset = false;
        for &i in &self.atom_out {
            if let Some(buffer) = self.buffers[0].atoms[i].as_mut() {
                let capacity = (buffer.len() * 8 - 8) as u32;
                let bytes = unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u8, 8) };
                bytes[0..4].copy_from_slice(&capacity.to_ne_bytes());
                bytes[4..8].copy_from_slice(&chunk.to_ne_bytes());
            }
        }
        unsafe {
            if let Some(run) = (*self.shared.descriptor).run {
                for instance in &self.shared.instances {
                    run(instance.handle, n as u32);
                }
            }
            if !self.shared.work.is_null() {
                for (instance, b) in self.shared.instances.iter().zip(&mut self.buffers) {
                    if let Some(responses) = b.responses.as_mut() {
                        responses.deliver(instance.handle, self.shared.work);
                    }
                }
            }
        }
        // Audio out.
        let clean = |v: f32| if v.is_finite() { v } else { 0.0 };
        if dual {
            let (l, r) = (&self.buffers[0], &self.buffers[1]);
            let port = self.audio_out[0];
            if let (Some(left), Some(right)) = (l.audio[port].as_ref(), r.audio[port].as_ref()) {
                for (f, frame) in audio[..n].iter_mut().enumerate() {
                    *frame = [clean(left[f]), clean(right[f])];
                }
            }
        } else if let Some(&first) = self.audio_out.first() {
            let b = &self.buffers[0];
            let left = b.audio[first].as_ref();
            let right = self.audio_out.get(1).and_then(|&p| b.audio[p].as_ref());
            if let Some(left) = left {
                for (f, frame) in audio[..n].iter_mut().enumerate() {
                    let l = clean(left[f]);
                    *frame = [l, right.map_or(l, |r| clean(r[f]))];
                }
            }
        }
        if let Some(i) = self.latency_port {
            let v = self.buffers[0].controls[i];
            let frames = if v.is_finite() { v.round().clamp(0.0, 10_000_000.0) as u32 } else { 0 };
            self.shared.latency.store(frames, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn port(min: Option<f32>, max: Option<f32>, default: Option<f32>) -> world::Port {
        world::Port {
            index: 0,
            symbol: "x".into(),
            name: "X".into(),
            direction: Direction::Input,
            kind: PortKind::Control,
            default,
            minimum: min,
            maximum: max,
            integer: false,
            toggled: false,
            enumeration: false,
            logarithmic: false,
            sample_rate: false,
            scale_points: vec![],
            unit: String::new(),
            designation: None,
            reports_latency: false,
            optional: false,
            hidden: false,
        }
    }
    #[test]
    fn port_properties_become_parameters() {
        let mut p = port(Some(0.0), Some(0.5), Some(0.25));
        p.sample_rate = true;
        p.logarithmic = true;
        let info = param_info(&p, 48_000);
        assert_eq!((info.min, info.max, info.default), (0.0, 24_000.0, 12_000.0));
        assert!(!info.log, "a logarithmic range from zero stays linear");
        let mut mode = port(Some(0.0), Some(2.0), None);
        mode.enumeration = true;
        mode.scale_points = vec![(0.0, "Low".into()), (1.0, "Mid".into()), (2.0, "High".into())];
        let info = param_info(&mode, 48_000);
        assert_eq!((info.steps, info.labels.clone()), (2, vec!["Low".into(), "Mid".into(), "High".into()]));
        let mut toggle = port(None, None, Some(1.0));
        toggle.toggled = true;
        let info = param_info(&toggle, 48_000);
        assert_eq!((info.steps, info.default), (1, 1.0));
        // Missing bounds: 0 to 1, default at the minimum.
        assert_eq!(port_default(&port(Some(-10.0), Some(10.0), None), 48_000), -10.0);
        assert_eq!(port_default(&port(None, None, Some(7.0)), 48_000), 1.0);
    }
}
