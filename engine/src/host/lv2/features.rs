//! The host features ryolune gives LV2 plugins: `urid:map` / `urid:unmap`, `options:options`
//! (sample rate and block lengths), `buf-size:boundedBlockLength`, `log:log` (to `tracing`),
//! `state:makePath` / `mapPath` / `freePath`, `worker:schedule`, and the flag features
//! `lv2:isLive`, `lv2:hardRTCapable`, `lv2:inPlaceBroken` (buffers are never shared) and
//! `state:loadDefaultState` (the default state is always loaded).

use super::ffi::{self, uri, LV2_Feature, LV2_Options_Option, LV2_URID};
use std::{
    cell::Cell,
    collections::HashMap,
    ffi::{c_char, c_void, CStr, CString},
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

/// Features a plugin may require and the host provides.
pub const SUPPORTED: &[&str] = &[
    uri::URID_MAP,
    uri::URID_UNMAP,
    uri::OPTIONS,
    uri::BOUNDED_BLOCK,
    uri::LOG,
    uri::MAKE_PATH,
    uri::MAP_PATH,
    uri::FREE_PATH,
    uri::WORKER_SCHEDULE,
    uri::IS_LIVE,
    uri::HARD_RT,
    uri::IN_PLACE_BROKEN,
    uri::LOAD_DEFAULT_STATE,
];

/// The first required feature the host lacks, as "short name (URI)".
pub fn missing(required: &[String]) -> Option<String> {
    let lacking = required.iter().find(|f| !SUPPORTED.contains(&f.as_str()))?;
    let short = lacking
        .rsplit_once('#')
        .map(|(ns, name)| {
            let ext = ns.rsplit('/').next().unwrap_or(ns);
            format!("{ext}:{name}")
        })
        .unwrap_or_else(|| lacking.rsplit('/').next().unwrap_or(lacking).to_string());
    Some(format!("{short} ({lacking})"))
}

// ---------------------------------------------------------------------------
// URIDs: one map for the process, so a URID means the same thing to every instance.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Urids {
    ids: HashMap<String, LV2_URID>,
    /// URI text by URID - 1. A `CString` keeps its bytes in place when the list grows, so
    /// the pointers `unmap` hands out stay valid for the life of the process.
    uris: Vec<CString>,
}
fn urids() -> &'static Mutex<Urids> {
    static URIDS: OnceLock<Mutex<Urids>> = OnceLock::new();
    URIDS.get_or_init(Default::default)
}
pub fn map(text: &str) -> LV2_URID {
    let mut u = urids().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&id) = u.ids.get(text) {
        return id;
    }
    let Ok(c) = CString::new(text) else {
        return 0;
    };
    u.uris.push(c);
    let id = u.uris.len() as LV2_URID;
    u.ids.insert(text.to_string(), id);
    id
}
pub fn unmap(id: LV2_URID) -> Option<String> {
    let u = urids().lock().unwrap_or_else(|e| e.into_inner());
    u.uris
        .get((id as usize).checked_sub(1)?)
        .map(|c| c.to_string_lossy().into_owned())
}
unsafe extern "C" fn map_callback(_handle: *mut c_void, text: *const c_char) -> LV2_URID {
    if text.is_null() {
        return 0;
    }
    match CStr::from_ptr(text).to_str() {
        Ok(text) => map(text),
        Err(_) => 0,
    }
}
unsafe extern "C" fn unmap_callback(_handle: *mut c_void, id: LV2_URID) -> *const c_char {
    let u = urids().lock().unwrap_or_else(|e| e.into_inner());
    match (id as usize).checked_sub(1).and_then(|i| u.uris.get(i)) {
        Some(c) => c.as_ptr(),
        None => std::ptr::null(),
    }
}

// ---------------------------------------------------------------------------
// Log
// ---------------------------------------------------------------------------

thread_local! {
    /// Set while a plugin runs on the audio thread: nothing is logged from there.
    pub static IN_AUDIO_THREAD: Cell<bool> = const { Cell::new(false) };
}
pub struct AudioThreadScope(bool);
impl AudioThreadScope {
    pub fn enter() -> Self {
        Self(IN_AUDIO_THREAD.with(|flag| flag.replace(true)))
    }
}
impl Drop for AudioThreadScope {
    fn drop(&mut self) {
        IN_AUDIO_THREAD.with(|flag| flag.set(self.0));
    }
}

fn log_line(plugin: &str, kind: LV2_URID, message: &str) {
    if IN_AUDIO_THREAD.with(Cell::get) {
        return;
    }
    let message = message.trim_end();
    if message.is_empty() {
        return;
    }
    let kind = unmap(kind).unwrap_or_default();
    if kind == uri::LOG_ERROR {
        tracing::error!(plugin, "{message}");
    } else if kind == uri::LOG_WARNING {
        tracing::warn!(plugin, "{message}");
    } else if kind == uri::LOG_TRACE {
        tracing::trace!(plugin, "{message}");
    } else {
        tracing::info!(plugin, "{message}");
    }
}
unsafe fn plugin_name(handle: *mut c_void) -> String {
    if handle.is_null() {
        String::new()
    } else {
        (*(handle as *const LogContext)).plugin.clone()
    }
}
unsafe extern "C" fn log_printf(handle: *mut c_void, kind: LV2_URID, fmt: *const c_char) -> i32 {
    if fmt.is_null() {
        return 0;
    }
    // The arguments cannot be read (see `ffi::LV2_Log_Log`): the format string is the message.
    let text = CStr::from_ptr(fmt).to_string_lossy();
    log_line(&plugin_name(handle), kind, &text);
    text.len() as i32
}
#[cfg(unix)]
extern "C" {
    fn vsnprintf(buffer: *mut c_char, size: usize, fmt: *const c_char, args: *mut c_void) -> i32;
}
unsafe extern "C" fn log_vprintf(
    handle: *mut c_void,
    kind: LV2_URID,
    fmt: *const c_char,
    args: *mut c_void,
) -> i32 {
    if fmt.is_null() {
        return 0;
    }
    if IN_AUDIO_THREAD.with(Cell::get) {
        return 0;
    }
    #[cfg(unix)]
    let text = {
        // On every Unix ABI a `va_list` argument arrives as one pointer-sized value, which
        // is what the C library's `vsnprintf` takes in turn.
        let mut buffer = [0 as c_char; 1024];
        let n = vsnprintf(buffer.as_mut_ptr(), buffer.len(), fmt, args);
        if n < 0 {
            CStr::from_ptr(fmt).to_string_lossy().into_owned()
        } else {
            CStr::from_ptr(buffer.as_ptr()).to_string_lossy().into_owned()
        }
    };
    #[cfg(not(unix))]
    let text = {
        let _ = args;
        CStr::from_ptr(fmt).to_string_lossy().into_owned()
    };
    log_line(&plugin_name(handle), kind, &text);
    text.len() as i32
}
struct LogContext {
    plugin: String,
}

// ---------------------------------------------------------------------------
// Paths. Abstract paths are the absolute paths themselves: state saved in a ryolune or
// kimchi document points at files on this machine, where the plugin left them.
// ---------------------------------------------------------------------------

extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(pointer: *mut c_void);
}
/// A C string the plugin may free with `free()` (plugins written before `state:freePath`
/// do) or through `freePath`.
fn c_copy(text: &[u8]) -> *mut c_char {
    unsafe {
        let p = malloc(text.len() + 1) as *mut u8;
        if p.is_null() {
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(text.as_ptr(), p, text.len());
        *p.add(text.len()) = 0;
        p as *mut c_char
    }
}
unsafe extern "C" fn path_identity(_handle: *mut c_void, path: *const c_char) -> *mut c_char {
    if path.is_null() {
        return std::ptr::null_mut();
    }
    c_copy(CStr::from_ptr(path).to_bytes())
}
unsafe extern "C" fn make_path(handle: *mut c_void, path: *const c_char) -> *mut c_char {
    if path.is_null() || handle.is_null() {
        return std::ptr::null_mut();
    }
    let root = &*(handle as *const PathBuf);
    let relative = CStr::from_ptr(path).to_string_lossy();
    // Only names inside the instance's folder.
    let clean: PathBuf = std::path::Path::new(relative.as_ref())
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect();
    let full = root.join(clean);
    if let Some(parent) = full.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    c_copy(full.to_string_lossy().as_bytes())
}
unsafe extern "C" fn free_path(_handle: *mut c_void, path: *mut c_char) {
    if !path.is_null() {
        free(path as *mut c_void);
    }
}

/// Values the options point at.
struct OptionValues {
    sample_rate: f32,
    min_block: i32,
    max_block: i32,
    nominal_block: i32,
    sequence_size: i32,
}

/// Everything one plugin instance is given at `instantiate`, kept in place for as long as
/// the instance lives (plugins may keep any of these pointers).
pub struct Features {
    _uris: Vec<CString>,
    _map: Box<ffi::LV2_URID_Map>,
    _unmap: Box<ffi::LV2_URID_Unmap>,
    _log: Box<ffi::LV2_Log_Log>,
    _log_context: Box<LogContext>,
    _values: Box<OptionValues>,
    _options: Vec<LV2_Options_Option>,
    _map_path: Box<ffi::LV2_State_Map_Path>,
    _make_path: Box<ffi::LV2_State_Make_Path>,
    _free_path: Box<ffi::LV2_State_Free_Path>,
    _files: Box<PathBuf>,
    _schedule: Option<Box<ffi::LV2_Worker_Schedule>>,
    _list: Vec<LV2_Feature>,
    pointers: Vec<*const LV2_Feature>,
}
// SAFETY: the feature structs are immutable after construction and their callbacks are
// thread-safe (the URID map locks, the others touch no shared state).
unsafe impl Send for Features {}
unsafe impl Sync for Features {}
impl Features {
    /// `schedule` is the worker's schedule function and handle, when the plugin has a worker.
    pub fn new(
        plugin: &str,
        sample_rate: f64,
        sequence_size: usize,
        files: PathBuf,
        schedule: Option<ffi::LV2_Worker_Schedule>,
    ) -> Self {
        let mut urid_map = Box::new(ffi::LV2_URID_Map {
            handle: std::ptr::null_mut(),
            map: map_callback,
        });
        let mut unmap = Box::new(ffi::LV2_URID_Unmap {
            handle: std::ptr::null_mut(),
            unmap: unmap_callback,
        });
        let log_context = Box::new(LogContext {
            plugin: plugin.to_string(),
        });
        let mut log = Box::new(ffi::LV2_Log_Log {
            handle: &*log_context as *const LogContext as *mut c_void,
            printf: log_printf,
            vprintf: log_vprintf,
        });
        let values = Box::new(OptionValues {
            sample_rate: sample_rate as f32,
            min_block: 0,
            max_block: crate::plugin::MAX_BLOCK as i32,
            nominal_block: crate::plugin::MAX_BLOCK as i32,
            sequence_size: sequence_size as i32,
        });
        let (float, int) = (map(&format!("{}Float", uri::ATOM)), map(&format!("{}Int", uri::ATOM)));
        let option = |key: &str, size: usize, type_: LV2_URID, value: *const c_void| LV2_Options_Option {
            context: ffi::LV2_OPTIONS_INSTANCE,
            subject: 0,
            key: map(key),
            size: size as u32,
            type_,
            value,
        };
        let options = vec![
            option(uri::SAMPLE_RATE, 4, float, &values.sample_rate as *const f32 as *const c_void),
            option(uri::MIN_BLOCK, 4, int, &values.min_block as *const i32 as *const c_void),
            option(uri::MAX_BLOCK, 4, int, &values.max_block as *const i32 as *const c_void),
            option(uri::NOMINAL_BLOCK, 4, int, &values.nominal_block as *const i32 as *const c_void),
            option(uri::SEQUENCE_SIZE, 4, int, &values.sequence_size as *const i32 as *const c_void),
            LV2_Options_Option {
                context: 0,
                subject: 0,
                key: 0,
                size: 0,
                type_: 0,
                value: std::ptr::null(),
            },
        ];
        let mut map_path = Box::new(ffi::LV2_State_Map_Path {
            handle: std::ptr::null_mut(),
            abstract_path: path_identity,
            absolute_path: path_identity,
        });
        let files = Box::new(files);
        let mut make = Box::new(ffi::LV2_State_Make_Path {
            handle: &*files as *const PathBuf as *mut c_void,
            path: make_path,
        });
        let mut free = Box::new(ffi::LV2_State_Free_Path {
            handle: std::ptr::null_mut(),
            free_path,
        });
        let mut schedule = schedule.map(Box::new);
        let mut entries: Vec<(&str, *mut c_void)> = vec![
            (uri::URID_MAP, &mut *urid_map as *mut _ as *mut c_void),
            (uri::URID_UNMAP, &mut *unmap as *mut _ as *mut c_void),
            (uri::OPTIONS, options.as_ptr() as *mut c_void),
            (uri::BOUNDED_BLOCK, std::ptr::null_mut()),
            (uri::LOG, &mut *log as *mut _ as *mut c_void),
            (uri::MAP_PATH, &mut *map_path as *mut _ as *mut c_void),
            (uri::MAKE_PATH, &mut *make as *mut _ as *mut c_void),
            (uri::FREE_PATH, &mut *free as *mut _ as *mut c_void),
            (uri::IS_LIVE, std::ptr::null_mut()),
            (uri::HARD_RT, std::ptr::null_mut()),
            (uri::IN_PLACE_BROKEN, std::ptr::null_mut()),
            (uri::LOAD_DEFAULT_STATE, std::ptr::null_mut()),
        ];
        if let Some(s) = schedule.as_mut() {
            entries.push((uri::WORKER_SCHEDULE, &mut **s as *mut _ as *mut c_void));
        }
        let uris: Vec<CString> = entries
            .iter()
            .map(|(u, _)| CString::new(*u).unwrap_or_default())
            .collect();
        let list: Vec<LV2_Feature> = entries
            .iter()
            .zip(&uris)
            .map(|((_, data), u)| LV2_Feature {
                uri: u.as_ptr(),
                data: *data,
            })
            .collect();
        let mut pointers: Vec<*const LV2_Feature> =
            list.iter().map(|f| f as *const LV2_Feature).collect();
        pointers.push(std::ptr::null());
        Self {
            _uris: uris,
            _map: urid_map,
            _unmap: unmap,
            _log: log,
            _log_context: log_context,
            _values: values,
            _options: options,
            _map_path: map_path,
            _make_path: make,
            _free_path: free,
            _files: files,
            _schedule: schedule,
            _list: list,
            pointers,
        }
    }
    /// The null-terminated feature array for `instantiate`, `save` and `restore`.
    pub fn as_ptr(&self) -> *const *const LV2_Feature {
        self.pointers.as_ptr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn urids_are_stable_and_unmap() {
        let a = map("http://example.org/a");
        let b = map("http://example.org/b");
        assert_ne!(a, b);
        assert_eq!(map("http://example.org/a"), a);
        assert_eq!(unmap(b).as_deref(), Some("http://example.org/b"));
        assert_eq!(unmap(0), None);
        let text = unsafe { CStr::from_ptr(unmap_callback(std::ptr::null_mut(), a)) };
        assert_eq!(text.to_str().unwrap(), "http://example.org/a");
    }
    #[test]
    fn missing_features_are_named() {
        assert_eq!(missing(&[uri::URID_MAP.to_string()]), None);
        assert_eq!(
            missing(&[
                uri::OPTIONS.to_string(),
                "http://lv2plug.in/ns/ext/buf-size#fixedBlockLength".to_string()
            ])
            .as_deref(),
            Some("buf-size:fixedBlockLength (http://lv2plug.in/ns/ext/buf-size#fixedBlockLength)")
        );
    }
}
