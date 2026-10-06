//! LADSPA host, written to `ladspa.h` version 1.1 (2002). A library exports
//! `ladspa_descriptor(index)`; each descriptor is one plugin with audio and control ports
//! and range hints (bounded, toggled, sample-rate bounds, logarithmic, integer and the
//! default hints). Instances are created, activated, deactivated and cleaned up on the
//! thread that instantiates them; `run` is called on the audio thread.
//!
//! LADSPA has no state: parameters are all there is, and they are document state already.
//! A plugin with an output control port named "latency" reports its latency through it (the
//! convention other hosts follow). A plugin with one audio input and one audio output runs as
//! two instances, one per stereo channel, as in the LV2 host.

use crate::{
    plugin::{Descriptor, Editor, Event, Format, Instance, ParamChange, ParamInfo, ProcessContext, Processor, MAX_BLOCK},
    Result,
};
use std::{
    collections::HashMap,
    ffi::{c_char, c_int, c_ulong, c_void, CStr},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex, OnceLock,
    },
};

// ---------------------------------------------------------------------------
// ladspa.h
// ---------------------------------------------------------------------------

type LadspaData = f32;
type LadspaHandle = *mut c_void;
pub const PORT_INPUT: c_int = 0x1;
pub const PORT_OUTPUT: c_int = 0x2;
pub const PORT_CONTROL: c_int = 0x4;
pub const PORT_AUDIO: c_int = 0x8;
pub const HINT_BOUNDED_BELOW: c_int = 0x1;
pub const HINT_BOUNDED_ABOVE: c_int = 0x2;
pub const HINT_TOGGLED: c_int = 0x4;
pub const HINT_SAMPLE_RATE: c_int = 0x8;
pub const HINT_LOGARITHMIC: c_int = 0x10;
pub const HINT_INTEGER: c_int = 0x20;
pub const HINT_DEFAULT_MASK: c_int = 0x3C0;
pub const HINT_DEFAULT_MINIMUM: c_int = 0x40;
pub const HINT_DEFAULT_LOW: c_int = 0x80;
pub const HINT_DEFAULT_MIDDLE: c_int = 0xC0;
pub const HINT_DEFAULT_HIGH: c_int = 0x100;
pub const HINT_DEFAULT_MAXIMUM: c_int = 0x140;
pub const HINT_DEFAULT_0: c_int = 0x200;
pub const HINT_DEFAULT_1: c_int = 0x240;
pub const HINT_DEFAULT_100: c_int = 0x280;
pub const HINT_DEFAULT_440: c_int = 0x2C0;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LadspaPortRangeHint {
    pub hint_descriptor: c_int,
    pub lower_bound: LadspaData,
    pub upper_bound: LadspaData,
}
#[repr(C)]
pub struct LadspaDescriptor {
    pub unique_id: c_ulong,
    pub label: *const c_char,
    pub properties: c_int,
    pub name: *const c_char,
    pub maker: *const c_char,
    pub copyright: *const c_char,
    pub port_count: c_ulong,
    pub port_descriptors: *const c_int,
    pub port_names: *const *const c_char,
    pub port_range_hints: *const LadspaPortRangeHint,
    pub implementation_data: *mut c_void,
    pub instantiate:
        Option<unsafe extern "C" fn(descriptor: *const LadspaDescriptor, sample_rate: c_ulong) -> LadspaHandle>,
    pub connect_port: Option<unsafe extern "C" fn(LadspaHandle, port: c_ulong, data: *mut LadspaData)>,
    pub activate: Option<unsafe extern "C" fn(LadspaHandle)>,
    pub run: Option<unsafe extern "C" fn(LadspaHandle, sample_count: c_ulong)>,
    pub run_adding: Option<unsafe extern "C" fn(LadspaHandle, sample_count: c_ulong)>,
    pub set_run_adding_gain: Option<unsafe extern "C" fn(LadspaHandle, gain: LadspaData)>,
    pub deactivate: Option<unsafe extern "C" fn(LadspaHandle)>,
    pub cleanup: Option<unsafe extern "C" fn(LadspaHandle)>,
}
type DescriptorFn = unsafe extern "C" fn(index: c_ulong) -> *const LadspaDescriptor;

// ---------------------------------------------------------------------------
// Libraries
// ---------------------------------------------------------------------------

struct Library {
    _library: libloading::Library,
    descriptor: DescriptorFn,
}
fn load(path: &Path) -> Result<Arc<Library>> {
    static LOADED: OnceLock<Mutex<HashMap<PathBuf, Arc<Library>>>> = OnceLock::new();
    let cache = LOADED.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(loaded) = guard.get(path) {
        return Ok(loaded.clone());
    }
    // SAFETY: loading a plugin library runs its initialisers; this is inherent to hosting.
    let library = unsafe { libloading::Library::new(path) }
        .map_err(|e| format!("Cannot load {}: {e}", path.display()))?;
    let descriptor: DescriptorFn = unsafe {
        *library
            .get::<DescriptorFn>(b"ladspa_descriptor\0")
            .map_err(|_| format!("{} is not a LADSPA library", path.display()))?
    };
    let loaded = Arc::new(Library {
        _library: library,
        descriptor,
    });
    guard.insert(path.to_path_buf(), loaded.clone());
    Ok(loaded)
}
unsafe fn text(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        CStr::from_ptr(p).to_string_lossy().trim().to_string()
    }
}
/// Every descriptor of a library.
fn descriptors(library: &Library) -> Vec<&'static LadspaDescriptor> {
    let mut out = vec![];
    for index in 0..10_000 {
        let d = unsafe { (library.descriptor)(index) };
        if d.is_null() {
            break;
        }
        // SAFETY: the library stays loaded for the life of the process.
        out.push(unsafe { &*d });
    }
    out
}
/// The id part of `ladspa:<id>`: the unique id, or the label when the plugin has none.
fn id_of(d: &LadspaDescriptor) -> String {
    if d.unique_id != 0 {
        d.unique_id.to_string()
    } else {
        unsafe { text(d.label) }
    }
}

/// One port as the host reads it.
#[derive(Clone, Debug)]
struct Port {
    name: String,
    input: bool,
    audio: bool,
    hint: LadspaPortRangeHint,
}
unsafe fn ports(d: &LadspaDescriptor) -> Result<Vec<Port>> {
    let count = d.port_count as usize;
    if count > 4096 || (count > 0 && (d.port_descriptors.is_null() || d.port_range_hints.is_null())) {
        return Err(format!("{} describes its ports wrongly", text(d.name)));
    }
    let mut out = vec![];
    for i in 0..count {
        let kind = *d.port_descriptors.add(i);
        let name = if d.port_names.is_null() {
            format!("Port {i}")
        } else {
            text(*d.port_names.add(i))
        };
        out.push(Port {
            name,
            input: kind & PORT_INPUT != 0,
            audio: kind & PORT_AUDIO != 0,
            hint: *d.port_range_hints.add(i),
        });
    }
    Ok(out)
}

fn descriptor_of(d: &LadspaDescriptor, library: &Path) -> Result<Descriptor> {
    let ports = unsafe { ports(d)? };
    let audio_in = ports.iter().filter(|p| p.audio && p.input).count();
    let name = unsafe { text(d.name) };
    Ok(Descriptor {
        id: format!("ladspa:{}", id_of(d)),
        format: Format::Ladspa,
        name: if name.is_empty() { unsafe { text(d.label) } } else { name },
        vendor: unsafe { text(d.maker) },
        path: library.to_string_lossy().into_owned(),
        // LADSPA has no notes: a plugin without audio inputs is a generator, still an effect.
        instrument: false,
        effect: true,
        category: if audio_in == 0 { "Generator".into() } else { String::new() },
    })
}

/// Probe a library: every plugin it exports. Runs in the `--scan-plugin` child process.
pub fn scan(path: &Path) -> Result<Vec<Descriptor>> {
    let library = load(path)?;
    descriptors(&library)
        .into_iter()
        .map(|d| descriptor_of(d, path))
        .collect()
}

/// The plain value range of a control port, with LADSPA's defaults when it gives none.
fn range(hint: &LadspaPortRangeHint, rate: u32) -> (f32, f32) {
    let h = hint.hint_descriptor;
    if h & HINT_TOGGLED != 0 {
        return (0.0, 1.0);
    }
    let scale = if h & HINT_SAMPLE_RATE != 0 { rate as f32 } else { 1.0 };
    let lo = if h & HINT_BOUNDED_BELOW != 0 { hint.lower_bound * scale } else { 0.0 };
    let hi = if h & HINT_BOUNDED_ABOVE != 0 {
        hint.upper_bound * scale
    } else {
        lo.max(0.0) + 1.0
    };
    if hi > lo {
        (lo, hi)
    } else {
        (lo, lo + 1.0)
    }
}
/// The default value the hints ask for (ladspa.h: low and high are a quarter of the way in,
/// on a logarithmic scale when the port is logarithmic).
fn default_value(hint: &LadspaPortRangeHint, rate: u32) -> f32 {
    let h = hint.hint_descriptor;
    let (lo, hi) = range(hint, rate);
    let log = h & HINT_LOGARITHMIC != 0 && lo > 0.0 && hi > 0.0;
    let between = |t: f32| {
        if log {
            (lo.ln() * (1.0 - t) + hi.ln() * t).exp()
        } else {
            lo * (1.0 - t) + hi * t
        }
    };
    let value = match h & HINT_DEFAULT_MASK {
        HINT_DEFAULT_MINIMUM => lo,
        HINT_DEFAULT_LOW => between(0.25),
        HINT_DEFAULT_MIDDLE => between(0.5),
        HINT_DEFAULT_HIGH => between(0.75),
        HINT_DEFAULT_MAXIMUM => hi,
        HINT_DEFAULT_0 => 0.0,
        HINT_DEFAULT_1 => 1.0,
        HINT_DEFAULT_100 => 100.0,
        HINT_DEFAULT_440 => 440.0,
        _ => {
            if h & HINT_BOUNDED_BELOW != 0 {
                lo
            } else {
                0.0
            }
        }
    };
    let value = value.clamp(lo, hi);
    if h & HINT_INTEGER != 0 {
        value.round()
    } else {
        value
    }
}
fn param_info(index: usize, port: &Port, rate: u32) -> ParamInfo {
    let h = port.hint.hint_descriptor;
    let (min, max) = range(&port.hint, rate);
    let toggled = h & HINT_TOGGLED != 0;
    let steps = if toggled {
        1
    } else if h & HINT_INTEGER != 0 {
        ((max - min).round().max(1.0) as u32).min(100_000)
    } else {
        0
    };
    ParamInfo {
        id: index as u32,
        name: port.name.clone(),
        min: min as f64,
        max: max as f64,
        default: default_value(&port.hint, rate) as f64,
        unit: String::new(),
        steps,
        log: h & HINT_LOGARITHMIC != 0 && min > 0.0,
        labels: if toggled {
            vec!["Off".into(), "On".into()]
        } else {
            vec![]
        },
    }
}

// ---------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------

struct Shared {
    _library: Arc<Library>,
    descriptor: &'static LadspaDescriptor,
    ports: Vec<Port>,
    handles: Vec<LadspaHandle>,
    values: Vec<AtomicU32>,
    generation: AtomicU32,
    latency: AtomicU32,
    activated: AtomicBool,
}
// SAFETY: instantiate, activate, deactivate and cleanup happen on the creating thread while
// no processor runs; `run` and `connect_port` on the audio thread, as LADSPA allows.
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}
impl Drop for Shared {
    fn drop(&mut self) {
        let d = self.descriptor;
        for &handle in &self.handles {
            unsafe {
                if self.activated.load(Ordering::Acquire) {
                    if let Some(deactivate) = d.deactivate {
                        deactivate(handle);
                    }
                }
                if let Some(cleanup) = d.cleanup {
                    cleanup(handle);
                }
            }
        }
    }
}

/// Instantiate `ladspa:<id>` from the scan cache.
pub fn instantiate(plugin_id: &str, name: &str, rate: u32) -> Result<Instance> {
    let desc = super::scan::lookup(plugin_id)
        .ok_or_else(|| format!("{name} is not installed or has not been scanned"))?;
    instantiate_from(&desc, rate)
}
pub fn instantiate_from(desc: &Descriptor, rate: u32) -> Result<Instance> {
    let path = Path::new(&desc.path);
    let library = load(path)?;
    let (_, wanted) = Format::parse(&desc.id).ok_or("Bad LADSPA plugin id")?;
    let descriptor = descriptors(&library)
        .into_iter()
        .find(|d| id_of(d) == wanted || unsafe { text(d.label) } == wanted)
        .ok_or_else(|| format!("{} no longer contains {}", path.display(), desc.name))?;
    let ports = unsafe { ports(descriptor)? };
    let audio_in = ports.iter().filter(|p| p.audio && p.input).count();
    let audio_out = ports.iter().filter(|p| p.audio && !p.input).count();
    let count = if audio_in == 1 && audio_out == 1 { 2 } else { 1 };
    let mut handles = vec![];
    for _ in 0..count {
        let handle = unsafe {
            descriptor
                .instantiate
                .map_or(std::ptr::null_mut(), |f| f(descriptor, rate as c_ulong))
        };
        if handle.is_null() {
            break;
        }
        handles.push(handle);
    }
    let values = ports
        .iter()
        .map(|p| AtomicU32::new(if p.audio { 0.0f32 } else { default_value(&p.hint, rate) }.to_bits()))
        .collect();
    let failed = handles.len() < count;
    let shared = Arc::new(Shared {
        _library: library,
        descriptor,
        ports,
        handles,
        values,
        generation: AtomicU32::new(1),
        latency: AtomicU32::new(0),
        activated: AtomicBool::new(false),
    });
    if failed {
        return Err(format!("{} could not be created at {rate} Hz", desc.name));
    }
    let mut processor = LadspaProcessor::new(shared.clone());
    if let Some(activate) = descriptor.activate {
        for &handle in &shared.handles {
            unsafe { activate(handle) };
        }
    }
    shared.activated.store(true, Ordering::Release);
    if processor.latency_port.is_some() {
        let mut quiet = [[0.0f32; 2]; 64];
        processor.process(&mut quiet, &[], &[], &ProcessContext::default());
    }
    let params = shared
        .ports
        .iter()
        .enumerate()
        .filter(|(i, p)| !p.audio && p.input && Some(*i) != processor.latency_port)
        .map(|(i, p)| param_info(i, p, rate))
        .collect();
    let editor = LadspaEditor {
        shared,
        desc: desc.clone(),
        params,
    };
    Ok(Instance {
        editor: Box::new(editor),
        processor: Some(Box::new(processor)),
    })
}

pub struct LadspaEditor {
    shared: Arc<Shared>,
    desc: Descriptor,
    params: Vec<ParamInfo>,
}
impl LadspaEditor {
    fn control_input(&self, id: u32) -> bool {
        self.shared
            .ports
            .get(id as usize)
            .is_some_and(|p| !p.audio && p.input)
    }
}
impl Editor for LadspaEditor {
    fn descriptor(&self) -> &Descriptor {
        &self.desc
    }
    fn params(&self) -> &[ParamInfo] {
        &self.params
    }
    fn value(&self, id: u32) -> Option<f64> {
        self.control_input(id)
            .then(|| f32::from_bits(self.shared.values[id as usize].load(Ordering::Relaxed)) as f64)
    }
    fn set_value(&mut self, id: u32, value: f64) {
        if self.control_input(id) {
            self.shared.values[id as usize].store((value as f32).to_bits(), Ordering::Relaxed);
        }
    }
    /// LADSPA plugins have no state beyond their parameters, which the document keeps.
    fn save(&mut self) -> Option<Vec<u8>> {
        None
    }
    fn load(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.is_empty() {
            Ok(())
        } else {
            Err(format!("{} is a LADSPA plugin, which has no saved state", self.desc.name))
        }
    }
    fn latency(&self) -> u32 {
        self.shared.latency.load(Ordering::Relaxed)
    }
}

pub struct LadspaProcessor {
    shared: Arc<Shared>,
    /// Port buffers per instance: control values (one per port) and audio blocks.
    controls: Vec<Box<[f32]>>,
    audio: Vec<Vec<Option<Box<[f32]>>>>,
    audio_in: Vec<usize>,
    audio_out: Vec<usize>,
    latency_port: Option<usize>,
    seen: u32,
}
// SAFETY: see `Shared`.
unsafe impl Send for LadspaProcessor {}
impl LadspaProcessor {
    fn new(shared: Arc<Shared>) -> Self {
        let ports = &shared.ports;
        let audio_in = (0..ports.len()).filter(|&i| ports[i].audio && ports[i].input).collect();
        let audio_out = (0..ports.len()).filter(|&i| ports[i].audio && !ports[i].input).collect();
        let latency_port = ports
            .iter()
            .position(|p| !p.audio && !p.input && p.name.trim().eq_ignore_ascii_case("latency"));
        let mut controls = vec![];
        let mut audio = vec![];
        for &handle in &shared.handles {
            let mut c: Box<[f32]> = shared
                .values
                .iter()
                .map(|v| f32::from_bits(v.load(Ordering::Relaxed)))
                .collect();
            let mut a: Vec<Option<Box<[f32]>>> = ports
                .iter()
                .map(|p| p.audio.then(|| vec![0.0f32; MAX_BLOCK].into_boxed_slice()))
                .collect();
            if let Some(connect) = shared.descriptor.connect_port {
                for i in 0..ports.len() {
                    let pointer = match a[i].as_mut() {
                        Some(buffer) => buffer.as_mut_ptr(),
                        None => &mut c[i] as *mut f32,
                    };
                    unsafe { connect(handle, i as c_ulong, pointer) };
                }
            }
            controls.push(c);
            audio.push(a);
        }
        Self {
            shared,
            controls,
            audio,
            audio_in,
            audio_out,
            latency_port,
            seen: 0,
        }
    }
}
impl Processor for LadspaProcessor {
    fn process(
        &mut self,
        audio: &mut [[f32; 2]],
        _events: &[Event],
        params: &[ParamChange],
        _ctx: &ProcessContext,
    ) {
        let n = audio.len().min(MAX_BLOCK);
        if n == 0 || !self.shared.activated.load(Ordering::Acquire) {
            return;
        }
        let ports = &self.shared.ports;
        let generation = self.shared.generation.load(Ordering::Acquire);
        if generation != self.seen {
            self.seen = generation;
            for (i, p) in ports.iter().enumerate() {
                if !p.audio && p.input {
                    let v = f32::from_bits(self.shared.values[i].load(Ordering::Relaxed));
                    for c in &mut self.controls {
                        c[i] = v;
                    }
                }
            }
        }
        for change in params {
            let i = change.id as usize;
            if ports.get(i).is_some_and(|p| !p.audio && p.input) {
                let v = change.value as f32;
                self.shared.values[i].store(v.to_bits(), Ordering::Relaxed);
                for c in &mut self.controls {
                    c[i] = v;
                }
            }
        }
        let dual = self.shared.handles.len() == 2;
        for (k, buffers) in self.audio.iter_mut().enumerate() {
            for (slot, &port) in self.audio_in.iter().enumerate() {
                let Some(buffer) = buffers[port].as_mut() else { continue };
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
        if let Some(run) = self.shared.descriptor.run {
            for &handle in &self.shared.handles {
                unsafe { run(handle, n as c_ulong) };
            }
        }
        let clean = |v: f32| if v.is_finite() { v } else { 0.0 };
        if dual {
            let port = self.audio_out[0];
            if let (Some(l), Some(r)) = (self.audio[0][port].as_ref(), self.audio[1][port].as_ref()) {
                for (f, frame) in audio[..n].iter_mut().enumerate() {
                    *frame = [clean(l[f]), clean(r[f])];
                }
            }
        } else if let Some(&first) = self.audio_out.first() {
            let left = self.audio[0][first].as_ref();
            let right = self.audio_out.get(1).and_then(|&p| self.audio[0][p].as_ref());
            if let Some(left) = left {
                for (f, frame) in audio[..n].iter_mut().enumerate() {
                    let l = clean(left[f]);
                    *frame = [l, right.map_or(l, |r| clean(r[f]))];
                }
            }
        }
        if let Some(i) = self.latency_port {
            let v = self.controls[0][i];
            let frames = if v.is_finite() { v.round().clamp(0.0, 10_000_000.0) as u32 } else { 0 };
            self.shared.latency.store(frames, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hint(h: c_int, lo: f32, hi: f32) -> LadspaPortRangeHint {
        LadspaPortRangeHint {
            hint_descriptor: h,
            lower_bound: lo,
            upper_bound: hi,
        }
    }
    #[test]
    fn range_hints_give_the_defaults_ladspa_h_describes() {
        let bounded = HINT_BOUNDED_BELOW | HINT_BOUNDED_ABOVE;
        assert_eq!(default_value(&hint(bounded | HINT_DEFAULT_MIDDLE, 0.0, 10.0), 48_000), 5.0);
        assert_eq!(default_value(&hint(bounded | HINT_DEFAULT_LOW, 0.0, 8.0), 48_000), 2.0);
        let log = default_value(&hint(bounded | HINT_LOGARITHMIC | HINT_DEFAULT_MIDDLE, 10.0, 1000.0), 48_000);
        assert!((log - 100.0).abs() < 0.01, "{log}");
        assert_eq!(default_value(&hint(bounded | HINT_DEFAULT_440, 0.0, 1000.0), 48_000), 440.0);
        let rate = hint(bounded | HINT_SAMPLE_RATE | HINT_DEFAULT_MAXIMUM, 0.0, 0.5);
        assert_eq!(range(&rate, 48_000), (0.0, 24_000.0));
        assert_eq!(default_value(&rate, 48_000), 24_000.0);
        let toggled = param_info(0, &Port { name: "On".into(), input: true, audio: false, hint: hint(HINT_TOGGLED | HINT_DEFAULT_1, 0.0, 0.0) }, 48_000);
        assert_eq!((toggled.steps, toggled.default, toggled.labels.len()), (1, 1.0, 2));
        let int = param_info(0, &Port { name: "N".into(), input: true, audio: false, hint: hint(bounded | HINT_INTEGER | HINT_DEFAULT_HIGH, 1.0, 9.0) }, 48_000);
        assert_eq!((int.steps, int.default), (8, 7.0));
        // No hints: from 0, default 0.
        assert_eq!(default_value(&hint(0, 0.0, 0.0), 48_000), 0.0);
    }
}
