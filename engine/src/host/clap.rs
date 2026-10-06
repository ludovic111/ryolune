//! CLAP host. Instances are created, activated and destroyed on the thread that
//! instantiates them (the main thread); `ClapProcessor` runs on the audio
//! thread as the CLAP specification allows.

use crate::{plugin::*, Result};
use clap_sys::{
    audio_buffer::clap_audio_buffer,
    entry::clap_plugin_entry,
    events::*,
    ext::{
        audio_ports::*, gui::*, latency::*, log::*, note_ports::*, params::*, state::*,
        thread_check::*,
    },
    factory::plugin_factory::*,
    fixedpoint::{CLAP_BEATTIME_FACTOR, CLAP_SECTIME_FACTOR},
    host::clap_host,
    id::CLAP_INVALID_ID,
    plugin::{clap_plugin, clap_plugin_descriptor},
    process::*,
    stream::{clap_istream, clap_ostream},
    version::CLAP_VERSION,
};
use std::{
    cell::Cell,
    collections::HashMap,
    ffi::{c_char, c_void, CStr, CString},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread::ThreadId,
};

// ---------------------------------------------------------------------------
// Library loading. Entries stay loaded for the life of the process.
// ---------------------------------------------------------------------------

struct Loaded {
    _library: libloading::Library,
    entry: *const clap_plugin_entry,
}
unsafe impl Send for Loaded {}
unsafe impl Sync for Loaded {}

fn binary_path(bundle: &Path) -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        if bundle.is_dir() {
            let dir = bundle.join("Contents/MacOS");
            let mut entries: Vec<_> = std::fs::read_dir(&dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect();
            entries.sort();
            return entries
                .into_iter()
                .next()
                .ok_or_else(|| format!("No executable in {}", dir.display()));
        }
    }
    Ok(bundle.to_path_buf())
}
fn load(bundle: &Path) -> Result<Arc<Loaded>> {
    static LOADED: OnceLock<Mutex<HashMap<PathBuf, Arc<Loaded>>>> = OnceLock::new();
    let cache = LOADED.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(loaded) = guard.get(bundle) {
        return Ok(loaded.clone());
    }
    let binary = binary_path(bundle)?;
    // SAFETY: loading a plugin binary runs its initialisers; this is inherent to hosting.
    let library = unsafe { libloading::Library::new(&binary) }
        .map_err(|e| format!("Cannot load {}: {e}", binary.display()))?;
    let entry: *const clap_plugin_entry = unsafe {
        *library
            .get::<*const clap_plugin_entry>(b"clap_entry\0")
            .map_err(|e| format!("Not a CLAP plugin ({e})"))?
    };
    if entry.is_null() {
        return Err("Empty CLAP entry".into());
    }
    let path = CString::new(bundle.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    unsafe {
        let e = &*entry;
        if !clap_sys::version::clap_version_is_compatible(e.clap_version) {
            return Err("Incompatible CLAP version".into());
        }
        if !e.init.is_some_and(|init| init(path.as_ptr())) {
            return Err("CLAP entry refused to initialise".into());
        }
    }
    let loaded = Arc::new(Loaded {
        _library: library,
        entry,
    });
    guard.insert(bundle.to_path_buf(), loaded.clone());
    Ok(loaded)
}
unsafe fn factory(loaded: &Loaded) -> Result<*const clap_plugin_factory> {
    let get = (*loaded.entry)
        .get_factory
        .ok_or("CLAP entry has no factory")?;
    let factory = get(CLAP_PLUGIN_FACTORY_ID.as_ptr()) as *const clap_plugin_factory;
    if factory.is_null() {
        return Err("CLAP plugin factory missing".into());
    }
    Ok(factory)
}
unsafe fn text(ptr: *const c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}
unsafe fn features(desc: &clap_plugin_descriptor) -> Vec<String> {
    let mut out = vec![];
    if desc.features.is_null() {
        return out;
    }
    let mut p = desc.features;
    while !(*p).is_null() {
        out.push(text(*p));
        p = p.add(1);
    }
    out
}

/// Probe a bundle: every plugin id it exports.
pub fn scan(bundle: &Path) -> Result<Vec<Descriptor>> {
    let loaded = load(bundle)?;
    let mut out = vec![];
    unsafe {
        let f = &*factory(&loaded)?;
        let count = f.get_plugin_count.map_or(0, |c| c(f));
        for i in 0..count {
            let Some(get) = f.get_plugin_descriptor else {
                break;
            };
            let desc = get(f, i);
            if desc.is_null() {
                continue;
            }
            let desc = &*desc;
            let features = features(desc);
            let instrument = features.iter().any(|f| f == "instrument");
            let effect = !instrument
                || features
                    .iter()
                    .any(|f| f == "audio-effect" || f == "note-effect");
            out.push(Descriptor {
                id: format!("clap:{}", text(desc.id)),
                format: Format::Clap,
                name: text(desc.name),
                vendor: text(desc.vendor),
                path: bundle.to_string_lossy().into_owned(),
                instrument,
                effect,
                category: features
                    .iter()
                    .find(|f| {
                        !["instrument", "audio-effect", "stereo", "mono"].contains(&f.as_str())
                    })
                    .cloned()
                    .unwrap_or_default(),
            });
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Host callbacks
// ---------------------------------------------------------------------------

const FLAG_CALLBACK: u32 = 1;
const FLAG_RESTART: u32 = 2;
const FLAG_DIRTY: u32 = 4;
const FLAG_RESCAN: u32 = 8;
const FLAG_GUI_CLOSED: u32 = 16;
const FLAG_FLUSH: u32 = 32;

struct HostData {
    host: clap_host,
    main_thread: ThreadId,
    flags: AtomicU32,
    resize: Mutex<Option<(u32, u32)>>,
}
thread_local! {
    static IN_AUDIO_THREAD: Cell<bool> = const { Cell::new(false) };
}
struct AudioThreadScope(bool);
impl AudioThreadScope {
    fn enter() -> Self {
        Self(IN_AUDIO_THREAD.with(|flag| flag.replace(true)))
    }
}
impl Drop for AudioThreadScope {
    fn drop(&mut self) {
        IN_AUDIO_THREAD.with(|flag| flag.set(self.0));
    }
}

unsafe fn data<'a>(host: *const clap_host) -> &'a HostData {
    &*((*host).host_data as *const HostData)
}
unsafe extern "C" fn host_get_extension(
    host: *const clap_host,
    id: *const c_char,
) -> *const c_void {
    let _ = host;
    if id.is_null() {
        return std::ptr::null();
    }
    let id = CStr::from_ptr(id);
    if id == CLAP_EXT_PARAMS {
        &HOST_PARAMS as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &HOST_STATE as *const _ as *const c_void
    } else if id == CLAP_EXT_GUI {
        &HOST_GUI as *const _ as *const c_void
    } else if id == CLAP_EXT_LOG {
        &HOST_LOG as *const _ as *const c_void
    } else if id == CLAP_EXT_THREAD_CHECK {
        &HOST_THREAD_CHECK as *const _ as *const c_void
    } else if id == CLAP_EXT_LATENCY {
        &HOST_LATENCY as *const _ as *const c_void
    } else if id == CLAP_EXT_AUDIO_PORTS {
        &HOST_AUDIO_PORTS as *const _ as *const c_void
    } else if id == CLAP_EXT_NOTE_PORTS {
        &HOST_NOTE_PORTS as *const _ as *const c_void
    } else {
        std::ptr::null()
    }
}
unsafe extern "C" fn host_request_restart(host: *const clap_host) {
    data(host).flags.fetch_or(FLAG_RESTART, Ordering::Relaxed);
}
unsafe extern "C" fn host_request_process(_host: *const clap_host) {}
unsafe extern "C" fn host_request_callback(host: *const clap_host) {
    data(host).flags.fetch_or(FLAG_CALLBACK, Ordering::Relaxed);
}
unsafe extern "C" fn params_rescan(host: *const clap_host, _flags: clap_param_rescan_flags) {
    data(host)
        .flags
        .fetch_or(FLAG_RESCAN | FLAG_DIRTY, Ordering::Relaxed);
}
unsafe extern "C" fn params_clear(
    _host: *const clap_host,
    _id: u32,
    _flags: clap_param_clear_flags,
) {
}
unsafe extern "C" fn params_request_flush(host: *const clap_host) {
    data(host).flags.fetch_or(FLAG_FLUSH, Ordering::Relaxed);
}
unsafe extern "C" fn state_mark_dirty(host: *const clap_host) {
    data(host).flags.fetch_or(FLAG_DIRTY, Ordering::Relaxed);
}
unsafe extern "C" fn gui_resize_hints_changed(_host: *const clap_host) {}
unsafe extern "C" fn gui_request_resize(host: *const clap_host, width: u32, height: u32) -> bool {
    if let Ok(mut r) = data(host).resize.lock() {
        *r = Some((width, height));
    }
    true
}
unsafe extern "C" fn gui_request_show(_host: *const clap_host) -> bool {
    false
}
unsafe extern "C" fn gui_request_hide(_host: *const clap_host) -> bool {
    false
}
unsafe extern "C" fn gui_closed(host: *const clap_host, _was_destroyed: bool) {
    data(host)
        .flags
        .fetch_or(FLAG_GUI_CLOSED, Ordering::Relaxed);
}
unsafe extern "C" fn log_log(
    _host: *const clap_host,
    severity: clap_log_severity,
    msg: *const c_char,
) {
    if severity >= CLAP_LOG_ERROR && !IN_AUDIO_THREAD.with(Cell::get) {
        crate::diagnostics::warn(&format!("[clap] {}", text(msg)));
    }
}
unsafe extern "C" fn thread_is_main(host: *const clap_host) -> bool {
    !IN_AUDIO_THREAD.with(Cell::get) && std::thread::current().id() == data(host).main_thread
}
unsafe extern "C" fn thread_is_audio(_host: *const clap_host) -> bool {
    IN_AUDIO_THREAD.with(Cell::get)
}
unsafe extern "C" fn latency_changed(host: *const clap_host) {
    data(host).flags.fetch_or(FLAG_RESTART, Ordering::Relaxed);
}
unsafe extern "C" fn audio_ports_flag_supported(_host: *const clap_host, _flag: u32) -> bool {
    false
}
unsafe extern "C" fn audio_ports_rescan(_host: *const clap_host, _flags: u32) {}
unsafe extern "C" fn note_ports_dialects(_host: *const clap_host) -> clap_note_dialect {
    CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI
}
unsafe extern "C" fn note_ports_rescan(_host: *const clap_host, _flags: u32) {}

static HOST_PARAMS: clap_host_params = clap_host_params {
    rescan: Some(params_rescan),
    clear: Some(params_clear),
    request_flush: Some(params_request_flush),
};
static HOST_STATE: clap_host_state = clap_host_state {
    mark_dirty: Some(state_mark_dirty),
};
static HOST_GUI: clap_host_gui = clap_host_gui {
    resize_hints_changed: Some(gui_resize_hints_changed),
    request_resize: Some(gui_request_resize),
    request_show: Some(gui_request_show),
    request_hide: Some(gui_request_hide),
    closed: Some(gui_closed),
};
static HOST_LOG: clap_host_log = clap_host_log { log: Some(log_log) };
static HOST_THREAD_CHECK: clap_host_thread_check = clap_host_thread_check {
    is_main_thread: Some(thread_is_main),
    is_audio_thread: Some(thread_is_audio),
};
static HOST_LATENCY: clap_host_latency = clap_host_latency {
    changed: Some(latency_changed),
};
static HOST_AUDIO_PORTS: clap_host_audio_ports = clap_host_audio_ports {
    is_rescan_flag_supported: Some(audio_ports_flag_supported),
    rescan: Some(audio_ports_rescan),
};
static HOST_NOTE_PORTS: clap_host_note_ports = clap_host_note_ports {
    supported_dialects: Some(note_ports_dialects),
    rescan: Some(note_ports_rescan),
};

// ---------------------------------------------------------------------------
// Shared instance state
// ---------------------------------------------------------------------------

struct Ext {
    params: *const clap_plugin_params,
    state: *const clap_plugin_state,
    gui: *const clap_plugin_gui,
    latency: *const clap_plugin_latency,
}
/// MIDI-only CLAP instruments must receive raw MIDI events, never CLAP notes.
fn note_dialect(
    supported: clap_note_dialect,
    preferred: clap_note_dialect,
) -> Option<clap_note_dialect> {
    let supported =
        supported & (CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI | CLAP_NOTE_DIALECT_MIDI_MPE);
    [
        preferred,
        CLAP_NOTE_DIALECT_CLAP,
        CLAP_NOTE_DIALECT_MIDI,
        CLAP_NOTE_DIALECT_MIDI_MPE,
    ]
    .into_iter()
    .find(|&dialect| dialect.count_ones() == 1 && supported & dialect != 0)
}

struct PortLayout {
    /// Channel counts of every input port, main port first.
    inputs: Vec<u32>,
    outputs: Vec<u32>,
    note_input: Option<(u16, clap_note_dialect)>,
    /// The note port also takes the MIDI dialect, so controllers, pitch bend and pressure
    /// can reach it as `CLAP_EVENT_MIDI`. A port that only speaks CLAP notes gets notes only.
    midi_controllers: bool,
}
struct Shared {
    _loaded: Arc<Loaded>,
    host: Box<HostData>,
    plugin: *const clap_plugin,
    ext: Ext,
    layout: PortLayout,
    activated: AtomicBool,
}
// SAFETY: the plugin pointer is shared between the main thread (init, activate,
// params, state, gui, destroy) and the audio thread (start/stop_processing,
// process), exactly the split CLAP defines; each function is only ever called
// from the thread its extension documents.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}
impl Drop for Shared {
    fn drop(&mut self) {
        unsafe {
            let p = &*self.plugin;
            if self.activated.swap(false, Ordering::AcqRel) {
                if let Some(deactivate) = p.deactivate {
                    deactivate(self.plugin);
                }
            }
            if let Some(destroy) = p.destroy {
                destroy(self.plugin);
            }
        }
    }
}

/// Instantiate `clap:<id>` from the scan cache.
pub fn instantiate(plugin_id: &str, name: &str, rate: u32) -> Result<Instance> {
    let desc = super::scan::lookup(plugin_id)
        .ok_or_else(|| format!("{name} is not installed or has not been scanned"))?;
    instantiate_from(&desc, rate)
}
pub fn instantiate_from(desc: &Descriptor, rate: u32) -> Result<Instance> {
    let loaded = load(Path::new(&desc.path))?;
    let (_, id) = Format::parse(&desc.id).ok_or("Bad CLAP id")?;
    let id = CString::new(id).map_err(|e| e.to_string())?;
    let mut host = Box::new(HostData {
        host: clap_host {
            clap_version: CLAP_VERSION,
            host_data: std::ptr::null_mut(),
            name: c"ryolune".as_ptr(),
            vendor: c"ryolune".as_ptr(),
            url: c"https://ryolune.app".as_ptr(),
            version: c"0.1.0".as_ptr(),
            get_extension: Some(host_get_extension),
            request_restart: Some(host_request_restart),
            request_process: Some(host_request_process),
            request_callback: Some(host_request_callback),
        },
        main_thread: std::thread::current().id(),
        flags: AtomicU32::new(0),
        resize: Mutex::new(None),
    });
    host.host.host_data = &*host as *const HostData as *mut c_void;
    let shared = unsafe {
        let f = &*factory(&loaded)?;
        let create = f.create_plugin.ok_or("Factory cannot create plugins")?;
        let plugin = create(f, &host.host, id.as_ptr());
        if plugin.is_null() {
            return Err(format!("{} refused to instantiate", desc.name));
        }
        let p = &*plugin;
        if !p.init.is_some_and(|init| init(plugin)) {
            if let Some(destroy) = p.destroy {
                destroy(plugin);
            }
            return Err(format!("{} failed to initialise", desc.name));
        }
        let get = |id: &CStr| -> *const c_void {
            p.get_extension
                .map_or(std::ptr::null(), |g| g(plugin, id.as_ptr()))
        };
        let ext = Ext {
            params: get(CLAP_EXT_PARAMS) as *const clap_plugin_params,
            state: get(CLAP_EXT_STATE) as *const clap_plugin_state,
            gui: get(CLAP_EXT_GUI) as *const clap_plugin_gui,
            latency: get(CLAP_EXT_LATENCY) as *const clap_plugin_latency,
        };
        let audio_ports = get(CLAP_EXT_AUDIO_PORTS) as *const clap_plugin_audio_ports;
        let mut layout = PortLayout {
            inputs: vec![],
            outputs: vec![],
            note_input: None,
            midi_controllers: false,
        };
        if !audio_ports.is_null() {
            let ap = &*audio_ports;
            for (is_input, list) in [(true, &mut layout.inputs), (false, &mut layout.outputs)] {
                let count = ap.count.map_or(0, |c| c(plugin, is_input));
                if count > 64 {
                    if let Some(destroy) = p.destroy {
                        destroy(plugin);
                    }
                    return Err(format!("{} declares more than 64 audio ports", desc.name));
                }
                for i in 0..count {
                    let mut info: clap_audio_port_info = std::mem::zeroed();
                    if ap.get.is_some_and(|g| g(plugin, i, is_input, &mut info)) {
                        if !(1..=16).contains(&info.channel_count) {
                            if let Some(destroy) = p.destroy {
                                destroy(plugin);
                            }
                            return Err(format!(
                                "{} requires an unsupported {}-channel audio port",
                                desc.name, info.channel_count
                            ));
                        }
                        list.push(info.channel_count);
                    } else {
                        list.push(2);
                    }
                }
            }
        }
        let note_ports = get(CLAP_EXT_NOTE_PORTS) as *const clap_plugin_note_ports;
        if !note_ports.is_null() {
            let ports = &*note_ports;
            for index in 0..ports.count.map_or(0, |c| c(plugin, true)) {
                let mut info: clap_note_port_info = std::mem::zeroed();
                if ports
                    .get
                    .is_some_and(|get| get(plugin, index, true, &mut info))
                {
                    if let Some(dialect) =
                        note_dialect(info.supported_dialects, info.preferred_dialect)
                    {
                        layout.note_input = Some((index as u16, dialect));
                        layout.midi_controllers = info.supported_dialects
                            & (CLAP_NOTE_DIALECT_MIDI | CLAP_NOTE_DIALECT_MIDI_MPE)
                            != 0;
                        break;
                    }
                }
            }
        }
        let shared = Arc::new(Shared {
            _loaded: loaded,
            host,
            plugin,
            ext,
            layout,
            activated: AtomicBool::new(false),
        });
        if !p
            .activate
            .is_some_and(|a| a(plugin, rate as f64, 1, MAX_BLOCK as u32))
        {
            return Err(format!("{} could not be activated at {rate} Hz", desc.name));
        }
        shared.activated.store(true, Ordering::Release);
        shared
    };
    let editor = ClapEditor::new(shared.clone(), desc.clone());
    let processor = ClapProcessor::new(shared);
    Ok(Instance {
        editor: Box::new(editor),
        processor: Some(Box::new(processor)),
    })
}

// ---------------------------------------------------------------------------
// Editor (main thread)
// ---------------------------------------------------------------------------

pub struct ClapEditor {
    shared: Arc<Shared>,
    desc: Descriptor,
    params: Vec<ParamInfo>,
    gui_open: bool,
    /// Parameters the plugin does not flag automatable.
    fixed: std::collections::HashSet<u32>,
}
impl ClapEditor {
    fn new(shared: Arc<Shared>, desc: Descriptor) -> Self {
        let mut editor = Self {
            shared,
            desc,
            params: vec![],
            gui_open: false,
            fixed: Default::default(),
        };
        editor.read_params();
        editor
    }
    fn read_params(&mut self) {
        self.params.clear();
        self.fixed.clear();
        let ext = self.shared.ext.params;
        if ext.is_null() {
            return;
        }
        unsafe {
            let params = &*ext;
            let plugin = self.shared.plugin;
            let count = params.count.map_or(0, |c| c(plugin));
            for i in 0..count.min(4096) {
                let mut info: clap_param_info = std::mem::zeroed();
                if !params.get_info.is_some_and(|g| g(plugin, i, &mut info)) {
                    continue;
                }
                if info.flags & CLAP_PARAM_IS_HIDDEN != 0 {
                    continue;
                }
                if info.flags & CLAP_PARAM_IS_AUTOMATABLE == 0 {
                    self.fixed.insert(info.id);
                }
                let stepped = info.flags & CLAP_PARAM_IS_STEPPED != 0;
                let steps = if stepped {
                    ((info.max_value - info.min_value).round().max(0.0) as u32).min(100_000)
                } else {
                    0
                };
                let mut labels = vec![];
                if stepped && steps > 0 && steps <= 32 {
                    for s in 0..=steps {
                        labels.push(self.value_text(info.id, info.min_value + s as f64));
                    }
                }
                let name = text(info.name.as_ptr());
                let module = text(info.module.as_ptr());
                self.params.push(ParamInfo {
                    id: info.id,
                    name: if module.is_empty() {
                        name
                    } else {
                        format!("{module}/{name}")
                            .rsplit('/')
                            .next()
                            .unwrap_or_default()
                            .to_string()
                    },
                    min: info.min_value,
                    max: info.max_value,
                    default: info.default_value,
                    unit: String::new(),
                    steps,
                    log: false,
                    labels,
                });
            }
        }
    }
    fn value_text(&self, id: u32, value: f64) -> String {
        let ext = self.shared.ext.params;
        if ext.is_null() {
            return format!("{value:.2}");
        }
        unsafe {
            let mut buffer = [0 as c_char; 128];
            if (*ext).value_to_text.is_some_and(|f| {
                f(
                    self.shared.plugin,
                    id,
                    value,
                    buffer.as_mut_ptr(),
                    buffer.len() as u32,
                )
            }) {
                text(buffer.as_ptr())
            } else {
                format!("{value:.2}")
            }
        }
    }
    fn gui(&self) -> Option<&clap_plugin_gui> {
        if self.shared.ext.gui.is_null() {
            None
        } else {
            Some(unsafe { &*self.shared.ext.gui })
        }
    }
}
#[cfg(target_os = "macos")]
const WINDOW_API: &CStr = CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
const WINDOW_API: &CStr = CLAP_WINDOW_API_WIN32;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const WINDOW_API: &CStr = CLAP_WINDOW_API_X11;

impl Editor for ClapEditor {
    fn descriptor(&self) -> &Descriptor {
        &self.desc
    }
    fn params(&self) -> &[ParamInfo] {
        &self.params
    }
    fn value(&self, id: u32) -> Option<f64> {
        let ext = self.shared.ext.params;
        if ext.is_null() {
            return None;
        }
        unsafe {
            let mut out = 0.0;
            (*ext)
                .get_value
                .is_some_and(|g| g(self.shared.plugin, id, &mut out))
                .then_some(out)
        }
    }
    fn text(&self, id: u32, value: f64) -> String {
        self.value_text(id, value)
    }
    fn save(&mut self) -> Option<Vec<u8>> {
        let ext = self.shared.ext.state;
        if ext.is_null() {
            return None;
        }
        unsafe extern "C" fn write(
            stream: *const clap_ostream,
            buffer: *const c_void,
            size: u64,
        ) -> i64 {
            let out = &mut *((*stream).ctx as *mut Vec<u8>);
            if out.len() as u64 + size > 256 * 1024 * 1024 {
                return -1;
            }
            out.extend_from_slice(std::slice::from_raw_parts(
                buffer as *const u8,
                size as usize,
            ));
            size as i64
        }
        let mut bytes: Vec<u8> = Vec::new();
        let stream = clap_ostream {
            ctx: &mut bytes as *mut Vec<u8> as *mut c_void,
            write: Some(write),
        };
        unsafe {
            (*ext)
                .save
                .is_some_and(|s| s(self.shared.plugin, &stream))
                .then_some(bytes)
        }
    }
    fn load(&mut self, bytes: &[u8]) -> Result<()> {
        let ext = self.shared.ext.state;
        if ext.is_null() {
            return Err("Plugin has no state extension".into());
        }
        struct Reader<'a> {
            data: &'a [u8],
            pos: usize,
        }
        unsafe extern "C" fn read(
            stream: *const clap_istream,
            buffer: *mut c_void,
            size: u64,
        ) -> i64 {
            let r = &mut *((*stream).ctx as *mut Reader);
            let n = (size as usize).min(r.data.len() - r.pos);
            std::ptr::copy_nonoverlapping(r.data.as_ptr().add(r.pos), buffer as *mut u8, n);
            r.pos += n;
            n as i64
        }
        let mut reader = Reader {
            data: bytes,
            pos: 0,
        };
        let stream = clap_istream {
            ctx: &mut reader as *mut Reader as *mut c_void,
            read: Some(read),
        };
        let ok = unsafe { (*ext).load.is_some_and(|l| l(self.shared.plugin, &stream)) };
        if ok {
            Ok(())
        } else {
            Err(format!("{} rejected its saved state", self.desc.name))
        }
    }
    fn has_gui(&self) -> bool {
        self.gui().is_some_and(|g| unsafe {
            g.is_api_supported
                .is_some_and(|f| f(self.shared.plugin, WINDOW_API.as_ptr(), false))
        })
    }
    fn open_gui(&mut self, parent: ParentWindow) -> Result<(u32, u32)> {
        let plugin = self.shared.plugin;
        let gui = self.gui().ok_or("No editor")?;
        unsafe {
            if !gui
                .create
                .is_some_and(|c| c(plugin, WINDOW_API.as_ptr(), false))
            {
                return Err("The plugin could not create its editor".into());
            }
            if let Some(scale) = gui.set_scale {
                scale(plugin, 1.0);
            }
            let (mut w, mut h) = (600, 400);
            if let Some(size) = gui.get_size {
                size(plugin, &mut w, &mut h);
            }
            let handle = match parent {
                ParentWindow::Cocoa(p) => clap_window_handle { cocoa: p },
                ParentWindow::Win32(p) => clap_window_handle { win32: p },
                ParentWindow::X11(x) => clap_window_handle { x11: x as _ },
            };
            let window = clap_window {
                api: WINDOW_API.as_ptr(),
                specific: handle,
            };
            if !gui.set_parent.is_some_and(|s| s(plugin, &window)) {
                if let Some(destroy) = gui.destroy {
                    destroy(plugin);
                }
                return Err("The plugin refused the host window".into());
            }
            if let Some(show) = gui.show {
                show(plugin);
            }
            self.gui_open = true;
            self.shared
                .host
                .flags
                .fetch_and(!FLAG_GUI_CLOSED, Ordering::Relaxed);
            Ok((w.max(1), h.max(1)))
        }
    }
    fn close_gui(&mut self) {
        if !self.gui_open {
            return;
        }
        self.gui_open = false;
        if let Some(gui) = self.gui() {
            unsafe {
                if let Some(hide) = gui.hide {
                    hide(self.shared.plugin);
                }
                if let Some(destroy) = gui.destroy {
                    destroy(self.shared.plugin);
                }
            }
        }
    }
    fn take_resize_request(&mut self) -> Option<(u32, u32)> {
        self.shared.host.resize.lock().ok()?.take()
    }
    fn set_gui_size(&mut self, width: u32, height: u32) {
        if let Some(gui) = self.gui() {
            unsafe {
                if let Some(set) = gui.set_size {
                    set(self.shared.plugin, width, height);
                }
            }
        }
    }
    fn idle(&mut self) {
        let flags = self.shared.host.flags.fetch_and(
            !(FLAG_CALLBACK | FLAG_RESCAN | FLAG_FLUSH),
            Ordering::AcqRel,
        );
        unsafe {
            if flags & FLAG_CALLBACK != 0 {
                if let Some(cb) = (*self.shared.plugin).on_main_thread {
                    cb(self.shared.plugin);
                }
            }
            if flags & FLAG_FLUSH != 0
                && !self.shared.activated.load(Ordering::Acquire)
                && !self.shared.ext.params.is_null()
            {
                let empty_in = clap_input_events {
                    ctx: std::ptr::null_mut(),
                    size: Some(events_size_empty),
                    get: Some(events_get_empty),
                };
                let out = clap_output_events {
                    ctx: std::ptr::null_mut(),
                    try_push: Some(events_push_ignore),
                };
                if let Some(flush) = (*self.shared.ext.params).flush {
                    flush(self.shared.plugin, &empty_in, &out);
                }
            }
        }
        if flags & FLAG_RESCAN != 0 {
            self.read_params();
        }
    }
    fn latency(&self) -> u32 {
        let ext = self.shared.ext.latency;
        if ext.is_null() {
            0
        } else {
            unsafe { (*ext).get.map_or(0, |g| g(self.shared.plugin)) }
        }
    }
    fn take_dirty(&mut self) -> bool {
        self.shared
            .host
            .flags
            .fetch_and(!FLAG_DIRTY, Ordering::AcqRel)
            & FLAG_DIRTY
            != 0
    }
    fn parse_text(&self, id: u32, input: &str) -> Option<f64> {
        let ext = self.shared.ext.params;
        let asked = (!ext.is_null())
            .then(|| {
                let c = std::ffi::CString::new(input.trim()).ok()?;
                let mut out = 0.0;
                unsafe {
                    (*ext)
                        .text_to_value
                        .is_some_and(|f| f(self.shared.plugin, id, c.as_ptr(), &mut out))
                        .then_some(out)
                }
            })
            .flatten()
            .filter(|v| v.is_finite());
        asked.or_else(|| {
            self.params
                .iter()
                .find(|p| p.id == id)
                .and_then(|p| p.parse_text(input))
        })
    }
    fn automatable(&self, id: u32) -> bool {
        !self.fixed.contains(&id)
    }
}
impl Drop for ClapEditor {
    fn drop(&mut self) {
        self.close_gui();
    }
}
unsafe extern "C" fn events_size_empty(_: *const clap_input_events) -> u32 {
    0
}
unsafe extern "C" fn events_get_empty(
    _: *const clap_input_events,
    _: u32,
) -> *const clap_event_header {
    std::ptr::null()
}
unsafe extern "C" fn events_push_ignore(
    _: *const clap_output_events,
    _: *const clap_event_header,
) -> bool {
    true
}

// ---------------------------------------------------------------------------
// Processor (audio thread)
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
union ClapEvent {
    header: clap_event_header,
    note: clap_event_note,
    midi: clap_event_midi,
    param: clap_event_param_value,
    expression: clap_event_note_expression,
}
struct Port {
    channels: Vec<Vec<f32>>,
    pointers: Vec<*mut f32>,
}
impl Port {
    fn new(count: u32) -> Self {
        let mut channels: Vec<Vec<f32>> = (0..count.max(1)).map(|_| vec![0.0; MAX_BLOCK]).collect();
        let pointers = channels.iter_mut().map(|c| c.as_mut_ptr()).collect();
        Self { channels, pointers }
    }
}
pub struct ClapProcessor {
    shared: Arc<Shared>,
    inputs: Vec<Port>,
    outputs: Vec<Port>,
    input_buffers: Vec<clap_audio_buffer>,
    output_buffers: Vec<clap_audio_buffer>,
    events: Vec<ClapEvent>,
    transport: clap_event_transport,
    steady_time: i64,
    started: bool,
}
// SAFETY: raw pointers reference buffers owned by this struct; see `Shared`.
unsafe impl Send for ClapProcessor {}
impl ClapProcessor {
    fn new(shared: Arc<Shared>) -> Self {
        let mut inputs: Vec<Port> = shared.layout.inputs.iter().map(|&c| Port::new(c)).collect();
        let mut outputs: Vec<Port> = shared
            .layout
            .outputs
            .iter()
            .map(|&c| Port::new(c))
            .collect();
        let buffer = |port: &mut Port| clap_audio_buffer {
            data32: port.pointers.as_mut_ptr(),
            data64: std::ptr::null_mut(),
            channel_count: port.channels.len() as u32,
            latency: 0,
            constant_mask: 0,
        };
        let input_buffers = inputs.iter_mut().map(buffer).collect();
        let output_buffers = outputs.iter_mut().map(buffer).collect();
        let parameter_count = if shared.ext.params.is_null() {
            0
        } else {
            unsafe {
                (*shared.ext.params)
                    .count
                    .map_or(0, |count| count(shared.plugin))
                    .min(8192) as usize
            }
        };
        Self {
            shared,
            inputs,
            outputs,
            input_buffers,
            output_buffers,
            events: Vec::with_capacity(parameter_count.max(64) + 1024),
            transport: unsafe { std::mem::zeroed() },
            steady_time: 0,
            started: false,
        }
    }
    fn push(&mut self, event: ClapEvent) {
        if self.events.len() < self.events.capacity() {
            self.events.push(event);
        }
    }
}
/// Polyphonic pressure as a CLAP note expression (pressure 0-1 on one key and channel).
fn poly_expression(event: &Event, port: u16, time: u32) -> Option<clap_event_note_expression> {
    (event.kind == event::POLY_PRESSURE).then(|| clap_event_note_expression {
        header: clap_event_header {
            size: std::mem::size_of::<clap_event_note_expression>() as u32,
            time,
            space_id: CLAP_CORE_EVENT_SPACE_ID,
            type_: CLAP_EVENT_NOTE_EXPRESSION,
            flags: 0,
        },
        expression_id: CLAP_NOTE_EXPRESSION_PRESSURE,
        note_id: -1,
        port_index: port as i16,
        channel: (event.channel & 15) as i16,
        key: event.key.min(127) as i16,
        value: event.value.min(127) as f64 / 127.0,
    })
}
unsafe extern "C" fn events_size(list: *const clap_input_events) -> u32 {
    (*((*list).ctx as *const Vec<ClapEvent>)).len() as u32
}
unsafe extern "C" fn events_get(
    list: *const clap_input_events,
    index: u32,
) -> *const clap_event_header {
    let events = &*((*list).ctx as *const Vec<ClapEvent>);
    events.get(index as usize).map_or(std::ptr::null(), |e| {
        e as *const ClapEvent as *const clap_event_header
    })
}
impl Processor for ClapProcessor {
    fn start(&mut self) {
        let _audio_thread = AudioThreadScope::enter();
        if self.started || !self.shared.activated.load(Ordering::Acquire) {
            return;
        }
        unsafe {
            if let Some(start) = (*self.shared.plugin).start_processing {
                self.started = start(self.shared.plugin);
            }
        }
    }
    fn stop(&mut self) {
        let _audio_thread = AudioThreadScope::enter();
        if !self.started {
            return;
        }
        self.started = false;
        unsafe {
            if let Some(stop) = (*self.shared.plugin).stop_processing {
                stop(self.shared.plugin);
            }
        }
    }
    fn reset(&mut self) {
        let _audio_thread = AudioThreadScope::enter();
        unsafe {
            if let Some(reset) = (*self.shared.plugin).reset {
                reset(self.shared.plugin);
            }
        }
    }
    fn latency(&self) -> u32 {
        0
    }
    /// Parameter value events carry their `time`.
    fn timed_params(&self) -> bool {
        true
    }
    /// An effect with a note input port hears its track's controllers.
    fn accepts_events(&self) -> bool {
        self.shared.layout.note_input.is_some()
    }
    fn process(
        &mut self,
        audio: &mut [[f32; 2]],
        events: &[Event],
        params: &[ParamChange],
        ctx: &ProcessContext,
    ) {
        if !self.started {
            self.start();
            if !self.started {
                return;
            }
        }
        let n = audio.len().min(MAX_BLOCK);
        if n == 0 {
            return;
        }
        self.events.clear();
        // CLAP wants one list sorted by time: parameter values and events are merged, a
        // value first when both land on the same frame.
        let note_input = self.shared.layout.note_input;
        let midi_controllers = self.shared.layout.midi_controllers;
        let events = if note_input.is_some() { events } else { &[] };
        let mut changes = params.iter().peekable();
        for event in events.iter().map(Some).chain(std::iter::once(None)) {
            let until = event.map_or(u32::MAX, |event| event.frame);
            while let Some(change) = changes.next_if(|change| change.frame <= until) {
                self.push(ClapEvent {
                    param: clap_event_param_value {
                        header: clap_event_header {
                            size: std::mem::size_of::<clap_event_param_value>() as u32,
                            time: (change.frame as usize).min(n - 1) as u32,
                            space_id: CLAP_CORE_EVENT_SPACE_ID,
                            type_: CLAP_EVENT_PARAM_VALUE,
                            flags: 0,
                        },
                        param_id: change.id,
                        cookie: std::ptr::null_mut(),
                        note_id: -1,
                        port_index: -1,
                        channel: -1,
                        key: -1,
                        value: change.value,
                    },
                });
            }
            let (Some(event), Some((port, dialect))) = (event, note_input) else {
                continue;
            };
            let time = (event.frame as usize).min(n - 1) as u32;
            let note = event.as_note();
            let midi = match note {
                Some(_) if dialect == CLAP_NOTE_DIALECT_CLAP => None,
                Some(_) => event.to_midi(),
                None if midi_controllers => event.to_midi(),
                None => {
                    // A plugin that speaks only CLAP notes still hears polyphonic pressure,
                    // as the note expression CLAP has for it.
                    if let Some(expression) = poly_expression(event, port, time) {
                        self.push(ClapEvent { expression });
                    }
                    continue;
                }
            };
            if let Some(data) = midi {
                self.push(ClapEvent {
                    midi: clap_event_midi {
                        header: clap_event_header {
                            size: std::mem::size_of::<clap_event_midi>() as u32,
                            time,
                            space_id: CLAP_CORE_EVENT_SPACE_ID,
                            type_: CLAP_EVENT_MIDI,
                            flags: 0,
                        },
                        port_index: port,
                        data,
                    },
                });
                continue;
            }
            let Some(note) = note else {
                continue;
            };
            self.push(ClapEvent {
                note: clap_event_note {
                    header: clap_event_header {
                        size: std::mem::size_of::<clap_event_note>() as u32,
                        time,
                        space_id: CLAP_CORE_EVENT_SPACE_ID,
                        type_: if note.on {
                            CLAP_EVENT_NOTE_ON
                        } else {
                            CLAP_EVENT_NOTE_OFF
                        },
                        flags: 0,
                    },
                    note_id: -1,
                    port_index: port as i16,
                    channel: note.channel as i16,
                    key: note.pitch as i16,
                    velocity: note.velocity as f64 / 127.0,
                },
            });
        }
        // Main input gets the track signal; extra ports stay silent.
        for (i, port) in self.inputs.iter_mut().enumerate() {
            let mono = port.channels.len() == 1;
            for (c, channel) in port.channels.iter_mut().enumerate() {
                if i == 0 && mono {
                    for (k, frame) in audio[..n].iter().enumerate() {
                        channel[k] = (frame[0] + frame[1]) * 0.5;
                    }
                } else if i == 0 && c < 2 {
                    for (k, frame) in audio[..n].iter().enumerate() {
                        channel[k] = frame[c];
                    }
                } else {
                    channel[..n].fill(0.0);
                }
            }
        }
        for port in &mut self.outputs {
            for channel in &mut port.channels {
                channel[..n].fill(0.0);
            }
        }
        let beats = (ctx.position_beats * CLAP_BEATTIME_FACTOR as f64) as i64;
        let seconds = (ctx.position_seconds * CLAP_SECTIME_FACTOR as f64) as i64;
        let mut flags = CLAP_TRANSPORT_HAS_TEMPO
            | CLAP_TRANSPORT_HAS_BEATS_TIMELINE
            | CLAP_TRANSPORT_HAS_SECONDS_TIMELINE
            | CLAP_TRANSPORT_HAS_TIME_SIGNATURE;
        if ctx.playing {
            flags |= CLAP_TRANSPORT_IS_PLAYING;
        }
        if ctx.recording {
            flags |= CLAP_TRANSPORT_IS_RECORDING;
        }
        if ctx.cycle.is_some() {
            flags |= CLAP_TRANSPORT_IS_LOOP_ACTIVE;
        }
        let (loop_start, loop_end) = ctx.cycle.unwrap_or((0.0, 0.0));
        self.transport = clap_event_transport {
            header: clap_event_header {
                size: std::mem::size_of::<clap_event_transport>() as u32,
                time: 0,
                space_id: CLAP_CORE_EVENT_SPACE_ID,
                type_: CLAP_EVENT_TRANSPORT,
                flags: 0,
            },
            flags,
            song_pos_beats: beats,
            song_pos_seconds: seconds,
            tempo: ctx.tempo,
            tempo_inc: 0.0,
            loop_start_beats: (loop_start * CLAP_BEATTIME_FACTOR as f64) as i64,
            loop_end_beats: (loop_end * CLAP_BEATTIME_FACTOR as f64) as i64,
            loop_start_seconds: 0,
            loop_end_seconds: 0,
            bar_start: (ctx.bar_start_beats * CLAP_BEATTIME_FACTOR as f64) as i64,
            bar_number: if ctx.numerator > 0 {
                (ctx.position_beats / (ctx.numerator as f64 * 4.0 / ctx.denominator.max(1) as f64))
                    .floor() as i32
            } else {
                0
            },
            tsig_num: ctx.numerator as u16,
            tsig_denom: ctx.denominator as u16,
        };
        let in_events = clap_input_events {
            ctx: &self.events as *const Vec<ClapEvent> as *mut c_void,
            size: Some(events_size),
            get: Some(events_get),
        };
        let out_events = clap_output_events {
            ctx: std::ptr::null_mut(),
            try_push: Some(events_push_ignore),
        };
        let process = clap_process {
            steady_time: self.steady_time,
            frames_count: n as u32,
            transport: &self.transport,
            audio_inputs: self.input_buffers.as_ptr(),
            audio_outputs: self.output_buffers.as_mut_ptr(),
            audio_inputs_count: self.input_buffers.len() as u32,
            audio_outputs_count: self.output_buffers.len() as u32,
            in_events: &in_events,
            out_events: &out_events,
        };
        let status = unsafe {
            let _audio_thread = AudioThreadScope::enter();
            (*self.shared.plugin)
                .process
                .map_or(CLAP_PROCESS_ERROR, |p| p(self.shared.plugin, &process))
        };
        self.steady_time += n as i64;
        if status == CLAP_PROCESS_ERROR {
            return;
        }
        if let Some(port) = self.outputs.first() {
            for (k, frame) in audio[..n].iter_mut().enumerate() {
                let l = port.channels[0][k];
                let r = port.channels.get(1).map_or(l, |c| c[k]);
                *frame = [
                    if l.is_finite() { l } else { 0.0 },
                    if r.is_finite() { r } else { 0.0 },
                ];
            }
        } else {
            audio[..n].fill([0.0; 2]);
        }
    }
}
impl Drop for ClapProcessor {
    fn drop(&mut self) {
        // stop_processing belongs to the audio thread; the rack calls `stop`
        // before handing the processor back.
        let _ = CLAP_INVALID_ID;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn polyphonic_pressure_is_a_note_expression_for_clap_notes() {
        let e = poly_expression(&Event::poly_pressure(3, 64, 127).on_channel(5), 1, 3).unwrap();
        assert_eq!(e.expression_id, CLAP_NOTE_EXPRESSION_PRESSURE);
        assert_eq!((e.port_index, e.channel, e.key), (1, 5, 64));
        assert_eq!(e.value, 1.0);
        assert_eq!(e.header.type_, CLAP_EVENT_NOTE_EXPRESSION);
        assert!(poly_expression(&Event::channel_pressure(0, 9), 0, 0).is_none());
    }
    #[test]
    fn negotiates_supported_note_dialect() {
        assert_eq!(
            note_dialect(CLAP_NOTE_DIALECT_MIDI, CLAP_NOTE_DIALECT_MIDI),
            Some(CLAP_NOTE_DIALECT_MIDI)
        );
        assert_eq!(
            note_dialect(
                CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI,
                CLAP_NOTE_DIALECT_MIDI
            ),
            Some(CLAP_NOTE_DIALECT_MIDI)
        );
        assert_eq!(
            note_dialect(CLAP_NOTE_DIALECT_CLAP, CLAP_NOTE_DIALECT_MIDI),
            Some(CLAP_NOTE_DIALECT_CLAP)
        );
        assert_eq!(
            note_dialect(CLAP_NOTE_DIALECT_MIDI2, CLAP_NOTE_DIALECT_MIDI2),
            None
        );
    }
}
