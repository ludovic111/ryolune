//! Two LV2 plugins for the host's tests, written against the LV2 1.18 C ABI by hand. The
//! bundle's Turtle files are in `bundle/`; tests copy them next to this library.
//!
//! - `Fixture Gain` (`http://ryolune.app/lv2/fixture#gain`): stereo gain with a latency port
//!   (always 7 frames), an `lv2:enabled` port, State (a label and the number of worker
//!   answers) and a Worker: setting `ping` sends a request whose answer is counted.
//! - `Fixture Sine` (`http://ryolune.app/lv2/fixture#sine`): an instrument that plays a sine
//!   at the pitch of the last MIDI note on its atom input.

#![allow(clippy::missing_safety_doc)]

use std::ffi::{c_char, c_void, CStr};

#[repr(C)]
pub struct Feature {
    uri: *const c_char,
    data: *mut c_void,
}
#[repr(C)]
pub struct Descriptor {
    uri: *const c_char,
    instantiate: unsafe extern "C" fn(*const Descriptor, f64, *const c_char, *const *const Feature) -> *mut c_void,
    connect_port: unsafe extern "C" fn(*mut c_void, u32, *mut c_void),
    activate: Option<unsafe extern "C" fn(*mut c_void)>,
    run: unsafe extern "C" fn(*mut c_void, u32),
    deactivate: Option<unsafe extern "C" fn(*mut c_void)>,
    cleanup: unsafe extern "C" fn(*mut c_void),
    extension_data: unsafe extern "C" fn(*const c_char) -> *const c_void,
}
unsafe impl Sync for Descriptor {}
#[repr(C)]
struct UridMap {
    handle: *mut c_void,
    map: unsafe extern "C" fn(*mut c_void, *const c_char) -> u32,
}
#[repr(C)]
struct LogLog {
    handle: *mut c_void,
    printf: unsafe extern "C" fn(*mut c_void, u32, *const c_char) -> i32,
}
#[repr(C)]
struct Schedule {
    handle: *mut c_void,
    schedule_work: unsafe extern "C" fn(*mut c_void, u32, *const c_void) -> u32,
}
type Store = unsafe extern "C" fn(*mut c_void, u32, *const c_void, usize, u32, u32) -> u32;
type Retrieve = unsafe extern "C" fn(*mut c_void, u32, *mut usize, *mut u32, *mut u32) -> *const c_void;
#[repr(C)]
struct StateInterface {
    save: unsafe extern "C" fn(*mut c_void, Store, *mut c_void, u32, *const *const Feature) -> u32,
    restore: unsafe extern "C" fn(*mut c_void, Retrieve, *mut c_void, u32, *const *const Feature) -> u32,
}
type Respond = unsafe extern "C" fn(*mut c_void, u32, *const c_void) -> u32;
#[repr(C)]
struct WorkerInterface {
    work: unsafe extern "C" fn(*mut c_void, Respond, *mut c_void, u32, *const c_void) -> u32,
    work_response: unsafe extern "C" fn(*mut c_void, u32, *const c_void) -> u32,
    end_run: Option<unsafe extern "C" fn(*mut c_void) -> u32>,
}

const NS: &str = "http://ryolune.app/lv2/fixture#";

unsafe fn feature(features: *const *const Feature, uri: &str) -> *mut c_void {
    if features.is_null() {
        return std::ptr::null_mut();
    }
    let mut p = features;
    while !(*p).is_null() {
        if CStr::from_ptr((**p).uri).to_bytes() == uri.as_bytes() {
            return (**p).data;
        }
        p = p.add(1);
    }
    std::ptr::null_mut()
}
unsafe fn map(m: *const UridMap, uri: &str) -> u32 {
    let c = std::ffi::CString::new(uri).unwrap();
    ((*m).map)((*m).handle, c.as_ptr())
}

// ---------------------------------------------------------------------------
// Fixture Gain
// ---------------------------------------------------------------------------

struct Gain {
    ports: [*mut f32; 8],
    schedule: *const Schedule,
    pinged: bool,
    answers: i32,
    label: String,
    urid_int: u32,
    urid_string: u32,
    key_answers: u32,
    key_label: u32,
}
unsafe extern "C" fn gain_instantiate(
    _: *const Descriptor,
    _rate: f64,
    _bundle: *const c_char,
    features: *const *const Feature,
) -> *mut c_void {
    let m = feature(features, "http://lv2plug.in/ns/ext/urid#map") as *const UridMap;
    if m.is_null() || feature(features, "http://lv2plug.in/ns/ext/options#options").is_null() {
        return std::ptr::null_mut();
    }
    let log = feature(features, "http://lv2plug.in/ns/ext/log#log") as *const LogLog;
    if !log.is_null() {
        let note = map(m, "http://lv2plug.in/ns/ext/log#Note");
        ((*log).printf)((*log).handle, note, c"Fixture Gain ready".as_ptr());
    }
    Box::into_raw(Box::new(Gain {
        ports: [std::ptr::null_mut(); 8],
        schedule: feature(features, "http://lv2plug.in/ns/ext/worker#schedule") as *const Schedule,
        pinged: false,
        answers: 0,
        label: String::new(),
        urid_int: map(m, "http://lv2plug.in/ns/ext/atom#Int"),
        urid_string: map(m, "http://lv2plug.in/ns/ext/atom#String"),
        key_answers: map(m, &format!("{NS}answers")),
        key_label: map(m, &format!("{NS}label")),
    })) as *mut c_void
}
unsafe extern "C" fn gain_connect(h: *mut c_void, port: u32, data: *mut c_void) {
    let g = &mut *(h as *mut Gain);
    if let Some(slot) = g.ports.get_mut(port as usize) {
        *slot = data as *mut f32;
    }
}
unsafe extern "C" fn gain_run(h: *mut c_void, n: u32) {
    let g = &mut *(h as *mut Gain);
    let gain = *g.ports[4];
    let enabled = *g.ports[7] > 0.5;
    *g.ports[5] = 7.0;
    for c in 0..2 {
        let (input, output) = (g.ports[c], g.ports[c + 2]);
        for i in 0..n as usize {
            *output.add(i) = if enabled { *input.add(i) * gain } else { 0.0 };
        }
    }
    let ping = *g.ports[6] > 0.5;
    if ping && !g.pinged && !g.schedule.is_null() {
        ((*g.schedule).schedule_work)((*g.schedule).handle, 4, b"ping".as_ptr() as *const c_void);
    }
    g.pinged = ping;
}
unsafe extern "C" fn gain_cleanup(h: *mut c_void) {
    drop(Box::from_raw(h as *mut Gain));
}
unsafe extern "C" fn gain_save(h: *mut c_void, store: Store, handle: *mut c_void, _: u32, _: *const *const Feature) -> u32 {
    let g = &*(h as *mut Gain);
    store(handle, g.key_answers, &g.answers as *const i32 as *const c_void, 4, g.urid_int, 1);
    let mut label = g.label.clone().into_bytes();
    label.push(0);
    store(handle, g.key_label, label.as_ptr() as *const c_void, label.len(), g.urid_string, 1);
    0
}
unsafe extern "C" fn gain_restore(h: *mut c_void, retrieve: Retrieve, handle: *mut c_void, _: u32, _: *const *const Feature) -> u32 {
    let g = &mut *(h as *mut Gain);
    let (mut size, mut type_, mut flags) = (0usize, 0u32, 0u32);
    let v = retrieve(handle, g.key_answers, &mut size, &mut type_, &mut flags);
    if !v.is_null() && type_ == g.urid_int && size == 4 {
        g.answers = *(v as *const i32);
    }
    let v = retrieve(handle, g.key_label, &mut size, &mut type_, &mut flags);
    if !v.is_null() && type_ == g.urid_string {
        g.label = CStr::from_ptr(v as *const c_char).to_string_lossy().into_owned();
    }
    0
}
unsafe extern "C" fn gain_work(_: *mut c_void, respond: Respond, handle: *mut c_void, size: u32, data: *const c_void) -> u32 {
    let request = std::slice::from_raw_parts(data as *const u8, size as usize);
    if request == b"ping" {
        respond(handle, 4, b"pong".as_ptr() as *const c_void);
    }
    0
}
unsafe extern "C" fn gain_work_response(h: *mut c_void, size: u32, data: *const c_void) -> u32 {
    let g = &mut *(h as *mut Gain);
    if std::slice::from_raw_parts(data as *const u8, size as usize) == b"pong" {
        g.answers += 1;
    }
    0
}
static GAIN_STATE: StateInterface = StateInterface {
    save: gain_save,
    restore: gain_restore,
};
static GAIN_WORKER: WorkerInterface = WorkerInterface {
    work: gain_work,
    work_response: gain_work_response,
    end_run: None,
};
unsafe extern "C" fn gain_extension(uri: *const c_char) -> *const c_void {
    match CStr::from_ptr(uri).to_bytes() {
        b"http://lv2plug.in/ns/ext/state#interface" => &GAIN_STATE as *const _ as *const c_void,
        b"http://lv2plug.in/ns/ext/worker#interface" => &GAIN_WORKER as *const _ as *const c_void,
        _ => std::ptr::null(),
    }
}

// ---------------------------------------------------------------------------
// Fixture Sine
// ---------------------------------------------------------------------------

struct Sine {
    ports: [*mut c_void; 4],
    rate: f64,
    phase: f64,
    pitch: Option<u8>,
    midi: u32,
}
unsafe extern "C" fn sine_instantiate(
    _: *const Descriptor,
    rate: f64,
    _bundle: *const c_char,
    features: *const *const Feature,
) -> *mut c_void {
    let m = feature(features, "http://lv2plug.in/ns/ext/urid#map") as *const UridMap;
    if m.is_null() {
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(Sine {
        ports: [std::ptr::null_mut(); 4],
        rate,
        phase: 0.0,
        pitch: None,
        midi: map(m, "http://lv2plug.in/ns/ext/midi#MidiEvent"),
    })) as *mut c_void
}
unsafe extern "C" fn sine_connect(h: *mut c_void, port: u32, data: *mut c_void) {
    let s = &mut *(h as *mut Sine);
    if let Some(slot) = s.ports.get_mut(port as usize) {
        *slot = data;
    }
}
unsafe extern "C" fn sine_run(h: *mut c_void, n: u32) {
    let s = &mut *(h as *mut Sine);
    let sequence = s.ports[0] as *const u8;
    let level = *(s.ports[3] as *const f32);
    // Events of the sequence: (frame, MIDI bytes).
    let mut events = vec![];
    let total = *(sequence as *const u32) as usize + 8;
    let mut at = 16;
    while at + 16 <= total {
        let frame = *(sequence.add(at) as *const i64);
        let size = *(sequence.add(at + 8) as *const u32) as usize;
        let kind = *(sequence.add(at + 12) as *const u32);
        if kind == s.midi && size >= 2 {
            let data = std::slice::from_raw_parts(sequence.add(at + 16), size);
            events.push((frame as usize, [data[0], data[1], *data.get(2).unwrap_or(&0)]));
        }
        at += 16 + size.div_ceil(8) * 8;
    }
    let (left, right) = (s.ports[1] as *mut f32, s.ports[2] as *mut f32);
    let mut next = 0;
    for i in 0..n as usize {
        while next < events.len() && events[next].0 <= i {
            let [status, key, velocity] = events[next].1;
            match status & 0xf0 {
                0x90 if velocity > 0 => s.pitch = Some(key),
                0x80 | 0x90 if s.pitch == Some(key) => s.pitch = None,
                0xb0 if key == 123 => s.pitch = None,
                _ => {}
            }
            next += 1;
        }
        let v = match s.pitch {
            Some(p) => {
                let f = 440.0 * 2f64.powf((p as f64 - 69.0) / 12.0);
                s.phase = (s.phase + f / s.rate).fract();
                (s.phase * std::f64::consts::TAU).sin() as f32 * level
            }
            None => 0.0,
        };
        *left.add(i) = v;
        *right.add(i) = v;
    }
}
unsafe extern "C" fn sine_cleanup(h: *mut c_void) {
    drop(Box::from_raw(h as *mut Sine));
}
unsafe extern "C" fn no_extension(_: *const c_char) -> *const c_void {
    std::ptr::null()
}

static GAIN: Descriptor = Descriptor {
    uri: c"http://ryolune.app/lv2/fixture#gain".as_ptr(),
    instantiate: gain_instantiate,
    connect_port: gain_connect,
    activate: None,
    run: gain_run,
    deactivate: None,
    cleanup: gain_cleanup,
    extension_data: gain_extension,
};
static SINE: Descriptor = Descriptor {
    uri: c"http://ryolune.app/lv2/fixture#sine".as_ptr(),
    instantiate: sine_instantiate,
    connect_port: sine_connect,
    activate: None,
    run: sine_run,
    deactivate: None,
    cleanup: sine_cleanup,
    extension_data: no_extension,
};

/// The LV2 entry point.
#[no_mangle]
pub extern "C" fn lv2_descriptor(index: u32) -> *const Descriptor {
    match index {
        0 => &GAIN,
        1 => &SINE,
        _ => std::ptr::null(),
    }
}

/// The bundle's Turtle files, by name, for tests that assemble the bundle.
pub const BUNDLE_FILES: &[(&str, &str)] = &[
    ("manifest.ttl", include_str!("../bundle/manifest.ttl")),
    ("fixture.ttl", include_str!("../bundle/fixture.ttl")),
    ("presets.ttl", include_str!("../bundle/presets.ttl")),
];
