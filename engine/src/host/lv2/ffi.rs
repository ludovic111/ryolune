//! The LV2 C ABI, translated from the LV2 1.18 headers (lv2core `lv2.h`, `urid.h`,
//! `options.h`, `buf-size.h`, `state.h`, `worker.h`, `log.h`, `atom.h`, `midi.h`). Only the
//! layouts the host passes or receives are here; everything else in LV2 is RDF data.

#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_void};

pub type LV2_Handle = *mut c_void;
pub type LV2_URID = u32;

#[repr(C)]
pub struct LV2_Feature {
    pub uri: *const c_char,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct LV2_Descriptor {
    pub uri: *const c_char,
    pub instantiate: Option<
        unsafe extern "C" fn(
            descriptor: *const LV2_Descriptor,
            sample_rate: f64,
            bundle_path: *const c_char,
            features: *const *const LV2_Feature,
        ) -> LV2_Handle,
    >,
    pub connect_port: Option<unsafe extern "C" fn(LV2_Handle, port: u32, data: *mut c_void)>,
    pub activate: Option<unsafe extern "C" fn(LV2_Handle)>,
    pub run: Option<unsafe extern "C" fn(LV2_Handle, sample_count: u32)>,
    pub deactivate: Option<unsafe extern "C" fn(LV2_Handle)>,
    pub cleanup: Option<unsafe extern "C" fn(LV2_Handle)>,
    pub extension_data: Option<unsafe extern "C" fn(uri: *const c_char) -> *const c_void>,
}
pub type DescriptorFn = unsafe extern "C" fn(index: u32) -> *const LV2_Descriptor;

#[repr(C)]
pub struct LV2_URID_Map {
    pub handle: *mut c_void,
    pub map: unsafe extern "C" fn(handle: *mut c_void, uri: *const c_char) -> LV2_URID,
}
#[repr(C)]
pub struct LV2_URID_Unmap {
    pub handle: *mut c_void,
    pub unmap: unsafe extern "C" fn(handle: *mut c_void, urid: LV2_URID) -> *const c_char,
}

/// `LV2_Options_Context`: options for the whole instance.
pub const LV2_OPTIONS_INSTANCE: u32 = 0;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LV2_Options_Option {
    pub context: u32,
    pub subject: u32,
    pub key: LV2_URID,
    pub size: u32,
    pub type_: LV2_URID,
    pub value: *const c_void,
}

pub const LV2_STATE_IS_POD: u32 = 1;
pub const LV2_STATE_IS_PORTABLE: u32 = 1 << 1;
pub const LV2_STATE_SUCCESS: u32 = 0;
pub const LV2_STATE_ERR_UNKNOWN: u32 = 1;
pub type LV2_State_Store_Function = unsafe extern "C" fn(
    handle: *mut c_void,
    key: u32,
    value: *const c_void,
    size: usize,
    type_: u32,
    flags: u32,
) -> u32;
pub type LV2_State_Retrieve_Function = unsafe extern "C" fn(
    handle: *mut c_void,
    key: u32,
    size: *mut usize,
    type_: *mut u32,
    flags: *mut u32,
) -> *const c_void;
#[repr(C)]
pub struct LV2_State_Interface {
    pub save: Option<
        unsafe extern "C" fn(
            instance: LV2_Handle,
            store: LV2_State_Store_Function,
            handle: *mut c_void,
            flags: u32,
            features: *const *const LV2_Feature,
        ) -> u32,
    >,
    pub restore: Option<
        unsafe extern "C" fn(
            instance: LV2_Handle,
            retrieve: LV2_State_Retrieve_Function,
            handle: *mut c_void,
            flags: u32,
            features: *const *const LV2_Feature,
        ) -> u32,
    >,
}
#[repr(C)]
pub struct LV2_State_Map_Path {
    pub handle: *mut c_void,
    pub abstract_path:
        unsafe extern "C" fn(handle: *mut c_void, absolute_path: *const c_char) -> *mut c_char,
    pub absolute_path:
        unsafe extern "C" fn(handle: *mut c_void, abstract_path: *const c_char) -> *mut c_char,
}
#[repr(C)]
pub struct LV2_State_Make_Path {
    pub handle: *mut c_void,
    pub path: unsafe extern "C" fn(handle: *mut c_void, path: *const c_char) -> *mut c_char,
}
#[repr(C)]
pub struct LV2_State_Free_Path {
    pub handle: *mut c_void,
    pub free_path: unsafe extern "C" fn(handle: *mut c_void, path: *mut c_char),
}

pub const LV2_WORKER_SUCCESS: u32 = 0;
pub const LV2_WORKER_ERR_UNKNOWN: u32 = 1;
pub const LV2_WORKER_ERR_NO_SPACE: u32 = 2;
pub type LV2_Worker_Respond_Function =
    unsafe extern "C" fn(handle: *mut c_void, size: u32, data: *const c_void) -> u32;
#[repr(C)]
pub struct LV2_Worker_Interface {
    pub work: Option<
        unsafe extern "C" fn(
            instance: LV2_Handle,
            respond: LV2_Worker_Respond_Function,
            handle: *mut c_void,
            size: u32,
            data: *const c_void,
        ) -> u32,
    >,
    pub work_response:
        Option<unsafe extern "C" fn(instance: LV2_Handle, size: u32, body: *const c_void) -> u32>,
    pub end_run: Option<unsafe extern "C" fn(instance: LV2_Handle) -> u32>,
}
#[repr(C)]
pub struct LV2_Worker_Schedule {
    pub handle: *mut c_void,
    pub schedule_work:
        unsafe extern "C" fn(handle: *mut c_void, size: u32, data: *const c_void) -> u32,
}

/// `LV2_Log_Log`. `printf` is variadic in C; Rust cannot define a variadic function on
/// stable, so the host's `printf` reads only its named arguments (the format string), which
/// every supported calling convention passes the same way as for a non-variadic call.
/// `vprintf` gets the `va_list` and formats it properly where the C library allows.
#[repr(C)]
pub struct LV2_Log_Log {
    pub handle: *mut c_void,
    pub printf: unsafe extern "C" fn(handle: *mut c_void, type_: LV2_URID, fmt: *const c_char) -> i32,
    pub vprintf: unsafe extern "C" fn(
        handle: *mut c_void,
        type_: LV2_URID,
        fmt: *const c_char,
        args: *mut c_void,
    ) -> i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct LV2_Atom {
    pub size: u32,
    pub type_: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct LV2_Atom_Sequence_Body {
    pub unit: u32,
    pub pad: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct LV2_Atom_Sequence {
    pub atom: LV2_Atom,
    pub body: LV2_Atom_Sequence_Body,
}
/// An event's header: the time in frames, then its atom; the body follows, padded to 8.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct LV2_Atom_Event {
    pub frames: i64,
    pub body: LV2_Atom,
}

pub mod uri {
    pub const CORE: &str = "http://lv2plug.in/ns/lv2core#";
    pub const ATOM: &str = "http://lv2plug.in/ns/ext/atom#";
    pub const MIDI_EVENT: &str = "http://lv2plug.in/ns/ext/midi#MidiEvent";
    pub const URID_MAP: &str = "http://lv2plug.in/ns/ext/urid#map";
    pub const URID_UNMAP: &str = "http://lv2plug.in/ns/ext/urid#unmap";
    pub const OPTIONS: &str = "http://lv2plug.in/ns/ext/options#options";
    pub const OPTIONS_INTERFACE: &str = "http://lv2plug.in/ns/ext/options#interface";
    pub const BOUNDED_BLOCK: &str = "http://lv2plug.in/ns/ext/buf-size#boundedBlockLength";
    pub const MIN_BLOCK: &str = "http://lv2plug.in/ns/ext/buf-size#minBlockLength";
    pub const MAX_BLOCK: &str = "http://lv2plug.in/ns/ext/buf-size#maxBlockLength";
    pub const NOMINAL_BLOCK: &str = "http://lv2plug.in/ns/ext/buf-size#nominalBlockLength";
    pub const SEQUENCE_SIZE: &str = "http://lv2plug.in/ns/ext/buf-size#sequenceSize";
    pub const SAMPLE_RATE: &str = "http://lv2plug.in/ns/ext/parameters#sampleRate";
    pub const STATE_INTERFACE: &str = "http://lv2plug.in/ns/ext/state#interface";
    pub const STATE_STATE: &str = "http://lv2plug.in/ns/ext/state#state";
    pub const MAP_PATH: &str = "http://lv2plug.in/ns/ext/state#mapPath";
    pub const MAKE_PATH: &str = "http://lv2plug.in/ns/ext/state#makePath";
    pub const FREE_PATH: &str = "http://lv2plug.in/ns/ext/state#freePath";
    pub const LOAD_DEFAULT_STATE: &str = "http://lv2plug.in/ns/ext/state#loadDefaultState";
    pub const WORKER_SCHEDULE: &str = "http://lv2plug.in/ns/ext/worker#schedule";
    pub const WORKER_INTERFACE: &str = "http://lv2plug.in/ns/ext/worker#interface";
    pub const LOG: &str = "http://lv2plug.in/ns/ext/log#log";
    pub const LOG_ERROR: &str = "http://lv2plug.in/ns/ext/log#Error";
    pub const LOG_WARNING: &str = "http://lv2plug.in/ns/ext/log#Warning";
    pub const LOG_TRACE: &str = "http://lv2plug.in/ns/ext/log#Trace";
    pub const IS_LIVE: &str = "http://lv2plug.in/ns/lv2core#isLive";
    pub const HARD_RT: &str = "http://lv2plug.in/ns/lv2core#hardRTCapable";
    pub const IN_PLACE_BROKEN: &str = "http://lv2plug.in/ns/lv2core#inPlaceBroken";
    pub const PRESET: &str = "http://lv2plug.in/ns/ext/presets#Preset";
    pub const PRESET_VALUE: &str = "http://lv2plug.in/ns/ext/presets#value";
    pub const UNITS: &str = "http://lv2plug.in/ns/extensions/units#";
    pub const PORT_PROPS: &str = "http://lv2plug.in/ns/ext/port-props#";
    pub const TIME_BPM: &str = "http://lv2plug.in/ns/ext/time#beatsPerMinute";
    pub const MINIMUM_SIZE: &str = "http://lv2plug.in/ns/ext/resize-port#minimumSize";
    pub const DOAP: &str = "http://usefulinc.com/ns/doap#";
    pub const FOAF_NAME: &str = "http://xmlns.com/foaf/0.1/name";
    pub const RDFS: &str = "http://www.w3.org/2000/01/rdf-schema#";
}
