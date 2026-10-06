//! Native audio and document services. No webview or GUI dependencies.
pub mod audio;
pub mod automation;
pub mod control;
pub mod control_app;
pub mod control_arrange;
pub mod control_automation;
pub mod control_controllers;
pub mod control_edit;
pub mod control_generate;
pub mod control_media;
pub mod control_overview;
pub mod control_params;
pub mod control_plugins;
pub mod control_refs;
pub mod control_routing;
pub mod control_suite;
pub mod control_tempo;
pub mod controllers;
pub mod device;
pub mod diagnostics;
pub mod document;
pub mod dsp;
pub mod export;
pub mod flac;
pub mod host;
pub mod lsuite;
pub mod midi;
pub mod midi_file;
pub mod model;
pub mod plugin;
pub mod preset;
pub mod recovery;
pub mod release_notes;
pub mod render;
pub mod sample_keys;
pub mod session_file;
pub mod settings;
pub mod stock;
pub mod store;
pub mod tempo;

pub type Result<T> = std::result::Result<T, String>;

pub mod takes;

mod midi_tools;

mod rhythm;
