//! Plugin hosting: scanning, instantiation and the per-format hosts. ryolune native plugins
//! (and the stock library, which is linked in) go through `native`; CLAP, VST3, Audio
//! Units, LV2 and LADSPA through their own hosts.

#[cfg(target_os = "macos")]
pub mod au;
pub mod clap;
pub mod ladspa;
pub mod lv2;
pub mod native;
pub mod scan;
pub mod vst3;

use crate::{
    plugin::{Format, Instance},
    stock, Result,
};
use base64::{engine::general_purpose::STANDARD, Engine};

pub fn decode_blob(blob: &str) -> Result<Vec<u8>> {
    STANDARD
        .decode(blob)
        .map_err(|e| format!("Invalid plugin state: {e}"))
}
pub fn encode_blob(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

/// Instantiate a plugin by descriptor id on the calling thread, which becomes
/// its main thread. `name` is the display name used when the id is unknown.
pub fn instantiate(plugin_id: &str, name: &str, rate: u32) -> Result<Instance> {
    match Format::parse(plugin_id) {
        Some((Format::Stock, stock_name)) => stock::create(stock_name, rate)
            .ok_or_else(|| format!("Unknown ryolune plugin: {stock_name}")),
        Some((Format::Native, _)) => native::instantiate(plugin_id, rate),
        Some((Format::Clap, _)) => clap::instantiate(plugin_id, name, rate),
        Some((Format::Vst3, _)) => vst3::instantiate(plugin_id, name, rate),
        #[cfg(target_os = "macos")]
        Some((Format::AudioUnit, _)) => au::instantiate(plugin_id, rate),
        #[cfg(not(target_os = "macos"))]
        Some((Format::AudioUnit, _)) => Err(format!("Audio Units are macOS only: {name}")),
        Some((Format::Lv2, _)) => lv2::instantiate(plugin_id, name, rate),
        Some((Format::Ladspa, _)) => ladspa::instantiate(plugin_id, name, rate),
        None => stock::create(name, rate).ok_or_else(|| format!("Unknown plugin: {plugin_id}")),
    }
}
