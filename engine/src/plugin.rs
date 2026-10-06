//! Plugin abstraction shared by the stock ryolune processors and the external
//! CLAP, VST3, Audio Unit, LV2 and LADSPA hosts.
//!
//! Every insert and instrument is an `Instance`: an `Editor` half that stays on
//! the main thread (parameters, state, GUI) and a `Processor` half that is
//! mounted into the audio thread's `Rack`. The rack outlives graph rebuilds, so
//! reverb tails, synth voices and external plugin state survive every edit.

use crate::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Stock,
    /// A library built with the `ryolune-plugin` SDK and loaded through its C ABI.
    Native,
    Clap,
    Vst3,
    #[serde(rename = "au")]
    AudioUnit,
    Lv2,
    Ladspa,
}
impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Stock => "ryolune",
            Format::Native => "Native",
            Format::Clap => "CLAP",
            Format::Vst3 => "VST3",
            Format::AudioUnit => "AU",
            Format::Lv2 => "LV2",
            Format::Ladspa => "LADSPA",
        }
    }
    pub fn prefix(self) -> &'static str {
        match self {
            Format::Stock => "stock",
            Format::Native => "native",
            Format::Clap => "clap",
            Format::Vst3 => "vst3",
            Format::AudioUnit => "au",
            Format::Lv2 => "lv2",
            Format::Ladspa => "ladspa",
        }
    }
    pub fn parse(id: &str) -> Option<(Format, &str)> {
        let (prefix, rest) = id.split_once(':')?;
        let format = match prefix {
            "stock" => Format::Stock,
            "native" => Format::Native,
            "clap" => Format::Clap,
            "vst3" => Format::Vst3,
            "au" => Format::AudioUnit,
            "lv2" => Format::Lv2,
            "ladspa" => Format::Ladspa,
            _ => return None,
        };
        Some((format, rest))
    }
}

/// A plugin known to the browser. `id` is stable across scans:
/// `stock:<name>`, `native:<plugin id>`, `clap:<plugin id>`, `vst3:<class id hex>`,
/// `au:<type>:<subtype>:<manufacturer>`, `lv2:<plugin URI>` or `ladspa:<unique id or label>`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Descriptor {
    pub id: String,
    pub format: Format,
    pub name: String,
    #[serde(default)]
    pub vendor: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub instrument: bool,
    #[serde(default = "yes")]
    pub effect: bool,
    #[serde(default)]
    pub category: String,
}
fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamInfo {
    pub id: u32,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub unit: String,
    /// 0 for continuous parameters; otherwise the number of discrete steps.
    pub steps: u32,
    /// Frequency-like parameters read better on a logarithmic knob.
    pub log: bool,
    /// Discrete parameters may name their values.
    pub labels: Vec<String>,
}
impl ParamInfo {
    pub fn normalize(&self, value: f64) -> f64 {
        let span = self.max - self.min;
        if span <= 0.0 {
            return 0.0;
        }
        if self.log && self.min > 0.0 {
            ((value / self.min).max(1e-9).ln() / (self.max / self.min).ln()).clamp(0.0, 1.0)
        } else {
            ((value - self.min) / span).clamp(0.0, 1.0)
        }
    }
    pub fn denormalize(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        let mut v = if self.log && self.min > 0.0 {
            self.min * (self.max / self.min).powf(t)
        } else {
            self.min + t * (self.max - self.min)
        };
        if self.steps > 0 {
            let step = (self.max - self.min) / self.steps as f64;
            v = self.min + ((v - self.min) / step).round() * step;
        }
        v.clamp(self.min, self.max)
    }
    /// Read a display string back: a label ("Hall"), a number with or without its unit
    /// ("-6", "-6 dB", "2.5k" for kilo units) or a percentage of the range ("50%" when the
    /// unit is not itself %). `None` when the text means nothing for this parameter.
    pub fn parse_text(&self, text: &str) -> Option<f64> {
        let text = text.trim();
        if let Some(index) = self
            .labels
            .iter()
            .position(|l| l.trim().eq_ignore_ascii_case(text))
        {
            return Some((self.min + index as f64).min(self.max));
        }
        let lower = text.to_lowercase();
        match lower.as_str() {
            "on" | "true" | "yes" if self.steps == 1 => return Some(self.max),
            "off" | "false" | "no" if self.steps == 1 => return Some(self.min),
            _ => {}
        }
        let unit = self.unit.trim().to_lowercase();
        let mut body = lower.as_str();
        if !unit.is_empty() {
            body = body.strip_suffix(unit.as_str()).unwrap_or(body).trim_end();
        }
        let (body, scale) = if let Some(b) = body.strip_suffix('k') {
            (b, 1000.0)
        } else {
            (body, 1.0)
        };
        if let Some(percent) = body.strip_suffix('%').filter(|_| unit != "%") {
            let t: f64 = percent.trim().parse().ok()?;
            return Some(self.denormalize(t / 100.0));
        }
        let value: f64 = body.trim().trim_start_matches('+').parse().ok()?;
        let value = value * scale;
        (value.is_finite()).then(|| value.clamp(self.min, self.max))
    }
    pub fn text(&self, value: f64) -> String {
        if !self.labels.is_empty() {
            let index = ((value - self.min).round().max(0.0) as usize).min(self.labels.len() - 1);
            return self.labels[index].clone();
        }
        let magnitude = value.abs();
        let body = if self.steps > 0 || (value.fract() == 0.0 && magnitude >= 100.0) {
            format!("{}", value.round() as i64)
        } else if magnitude >= 100.0 {
            format!("{value:.0}")
        } else if magnitude >= 10.0 {
            format!("{value:.1}")
        } else {
            format!("{value:.2}")
        };
        if self.unit.is_empty() {
            body
        } else {
            format!("{body} {}", self.unit)
        }
    }
}

/// Events, transport context and the block bound are defined by the plugin SDK so native
/// plugins and the hosts agree on one layout. `Event` carries notes, controllers, pitch bend
/// and pressure; `event` names its kinds.
pub use ryolune_plugin::{event, Event, NoteEvent, ProcessContext, MAX_BLOCK};

/// A parameter value that takes effect at `frame` within the block being processed. `id` is
/// the plugin's own parameter id (CLAP id, VST3 ParamID, AU parameter, native index).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamChange {
    pub frame: u32,
    pub id: u32,
    pub value: f64,
}
impl ParamChange {
    /// A change at the start of the block.
    pub const fn now(id: u32, value: f64) -> Self {
        Self {
            frame: 0,
            id,
            value,
        }
    }
}

/// The audio-thread half of a plugin. Nothing here may allocate, block or log.
pub trait Processor: Send {
    /// Called on the audio thread when the processor is mounted.
    fn start(&mut self) {}
    /// Called on the audio thread before the processor leaves the rack.
    fn stop(&mut self) {}
    /// Silence tails and release voices.
    fn reset(&mut self) {}
    /// Process stereo audio in place. Instruments receive silence and add their
    /// output; effects transform. `events` (notes, controllers, pitch bend and pressure)
    /// and `params` are sorted by frame. A processor that does not say
    /// [`Processor::timed_params`] only ever sees changes at frame 0: the rack splits the
    /// block at every later change for it.
    fn process(
        &mut self,
        audio: &mut [[f32; 2]],
        events: &[Event],
        params: &[ParamChange],
        ctx: &ProcessContext,
    );
    fn latency(&self) -> u32 {
        0
    }
    /// The processor applies each change at its `frame` itself (CLAP events, VST3 queues,
    /// native ABI 2), so the rack passes the whole block at once.
    fn timed_params(&self) -> bool {
        false
    }
    /// The processor takes MIDI events (controllers, pitch bend and pressure) as an insert
    /// effect: native ABI 2, CLAP with a note port, VST3 with an event bus or MIDI mapping,
    /// Audio Unit music effects. An instrument always gets its track's events; an insert only
    /// when this says so. Stock effects and ABI 1 plugins keep the default, and hear nothing.
    fn accepts_events(&self) -> bool {
        false
    }
}

/// An opaque parent window handle for native plugin editors.
#[derive(Clone, Copy, Debug)]
pub enum ParentWindow {
    Cocoa(*mut std::ffi::c_void),
    Win32(*mut std::ffi::c_void),
    X11(u64),
}

/// The main-thread half of a plugin.
pub trait Editor {
    fn descriptor(&self) -> &Descriptor;
    fn params(&self) -> &[ParamInfo];
    /// Current plain value as the plugin reports it.
    fn value(&self, id: u32) -> Option<f64>;
    fn text(&self, id: u32, value: f64) -> String {
        self.params()
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.text(value))
            .unwrap_or_default()
    }
    /// Apply a parameter value on the main thread (GUI synchronisation only;
    /// the audio thread receives the same change through the rack).
    fn set_value(&mut self, _id: u32, _value: f64) {}
    fn save(&mut self) -> Option<Vec<u8>>;
    fn load(&mut self, bytes: &[u8]) -> Result<()>;
    fn has_gui(&self) -> bool {
        false
    }
    /// Create and attach the native editor. Returns its preferred size.
    fn open_gui(&mut self, _parent: ParentWindow) -> Result<(u32, u32)> {
        Err("This plugin has no editor window".into())
    }
    fn close_gui(&mut self) {}
    /// A size the plugin requested from its host since the last call.
    fn take_resize_request(&mut self) -> Option<(u32, u32)> {
        None
    }
    fn set_gui_size(&mut self, _width: u32, _height: u32) {}
    /// Periodic main-thread housekeeping (CLAP `on_main_thread`, VST3 idle).
    fn idle(&mut self) {}
    fn latency(&self) -> u32 {
        0
    }
    /// How long the plugin says it sounds after its input stops; 0 when it does not say.
    fn tail_seconds(&self) -> f64 {
        0.0
    }
    /// True after the plugin reported parameter or state changes from its own GUI.
    fn take_dirty(&mut self) -> bool {
        false
    }
    /// The plain value a display string stands for ("-6 dB", "Hall", "440 Hz"), asking the
    /// plugin where its format can (CLAP `text_to_value`, VST3 `getParamValueByString`).
    fn parse_text(&self, id: u32, text: &str) -> Option<f64> {
        self.params()
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.parse_text(text))
    }
    /// Whether the plugin lets a host automate the parameter (CLAP and VST3 flag it).
    fn automatable(&self, _id: u32) -> bool {
        true
    }
    /// The plugin's own factory programs: Audio Unit factory presets, the VST3 program
    /// list behind its program-change parameter. Empty when the format or plugin has none.
    fn programs(&mut self) -> Vec<String> {
        Vec::new()
    }
    /// The parameter that selects a program, when programs are chosen by value (VST3): the
    /// host sets it like any parameter, so the choice is document state and undoes.
    fn program_parameter(&self) -> Option<u32> {
        None
    }
    /// The factory program this instance is on, when the plugin says (Audio Units report
    /// their present preset; a program parameter is read from its value instead).
    fn current_program(&mut self) -> Option<usize> {
        None
    }
    /// Load program `index` into this instance (Audio Units). The host saves the state
    /// afterwards and restores it on the audio thread like any other state change.
    fn load_program(&mut self, _index: usize) -> Result<()> {
        Err("This plugin does not load programs by index".into())
    }
}

/// A live plugin: its editor and, until mounted, its processor.
pub struct Instance {
    pub editor: Box<dyn Editor>,
    pub processor: Option<Box<dyn Processor>>,
}

/// Bounded pending parameter changes per rack slot.
struct Pending {
    changes: Vec<ParamChange>,
    len: usize,
}

/// Notes one sub-block can hold when the rack splits a block for a processor.
const SPLIT_EVENTS: usize = 512;

/// The audio thread's plugin bank, addressed by slot. Preallocated so mounting
/// and processing never allocate.
pub struct Rack {
    slots: Vec<Option<Box<dyn Processor>>>,
    pending: Vec<Pending>,
    /// Events re-based to a sub-block, for processors that take changes only at its start.
    split_events: Vec<Event>,
}
impl Rack {
    pub fn new(capacity: usize) -> Self {
        Self::with_parameter_capacity(capacity, 512)
    }
    /// Offline restores may enqueue many saved parameters before the first
    /// block; reserve their complete document values on the calling thread.
    pub fn with_parameter_capacity(capacity: usize, parameters: usize) -> Self {
        let mut slots = Vec::with_capacity(capacity);
        let mut pending = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            slots.push(None);
            pending.push(Pending {
                changes: vec![ParamChange::now(0, 0.0); parameters.max(1)],
                len: 0,
            });
        }
        Self {
            slots,
            pending,
            split_events: vec![Event::default(); SPLIT_EVENTS],
        }
    }
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
    pub fn is_mounted(&self, slot: u32) -> bool {
        self.slots.get(slot as usize).is_some_and(|s| s.is_some())
    }
    /// Mount on the audio thread; returns the previous occupant for reclamation.
    pub fn mount(
        &mut self,
        slot: u32,
        mut processor: Box<dyn Processor>,
    ) -> Option<Box<dyn Processor>> {
        let Some(place) = self.slots.get_mut(slot as usize) else {
            return Some(processor);
        };
        processor.start();
        let mut old = place.replace(processor);
        if let Some(old) = old.as_mut() {
            old.stop();
        }
        self.pending[slot as usize].len = 0;
        old
    }
    pub fn unmount(&mut self, slot: u32) -> Option<Box<dyn Processor>> {
        let mut old = self.slots.get_mut(slot as usize)?.take();
        if let Some(old) = old.as_mut() {
            old.stop();
        }
        old
    }
    /// A change at the start of the next block.
    pub fn set_param(&mut self, slot: u32, id: u32, value: f64) {
        self.set_param_at(slot, 0, id, value);
    }
    /// A change at `frame` within the next block. Changes stay sorted by frame; a second
    /// change to the same parameter on the same frame replaces the first. A full queue drops
    /// the change (512 per slot and block by default).
    pub fn set_param_at(&mut self, slot: u32, frame: u32, id: u32, value: f64) {
        let Some(p) = self.pending.get_mut(slot as usize) else {
            return;
        };
        let at = p.changes[..p.len].partition_point(|c| c.frame <= frame);
        if let Some(existing) = p.changes[..at]
            .iter_mut()
            .rev()
            .take_while(|c| c.frame == frame)
            .find(|c| c.id == id)
        {
            existing.value = value;
            return;
        }
        if p.len == p.changes.len() {
            return;
        }
        p.changes.copy_within(at..p.len, at + 1);
        p.changes[at] = ParamChange { frame, id, value };
        p.len += 1;
    }
    /// Returns false when the slot is empty so callers can pass audio through.
    pub fn process(
        &mut self,
        slot: u32,
        audio: &mut [[f32; 2]],
        events: &[Event],
        ctx: &ProcessContext,
    ) -> bool {
        let index = slot as usize;
        let Some(Some(processor)) = self.slots.get_mut(index) else {
            return false;
        };
        let pending = &mut self.pending[index];
        let changes = &mut pending.changes[..pending.len];
        let frames = audio.len();
        if processor.timed_params() || changes.iter().all(|c| c.frame == 0) {
            processor.process(audio, events, changes, ctx);
        } else {
            // Run the stretches between changes, each with the changes on its first frame
            // (re-based to 0, like the events).
            let (mut start, mut next_change, mut next_event) = (0usize, 0usize, 0usize);
            while start < frames {
                let first = next_change;
                while changes
                    .get(next_change)
                    .is_some_and(|c| (c.frame as usize) <= start)
                {
                    next_change += 1;
                }
                let end = changes
                    .get(next_change)
                    .map_or(frames, |c| (c.frame as usize).min(frames));
                let mut count = 0;
                while let Some(event) = events.get(next_event) {
                    if event.frame as usize >= end && end < frames {
                        break;
                    }
                    if count < self.split_events.len() {
                        let mut event = *event;
                        event.frame =
                            (event.frame as usize).clamp(start, end - 1) as u32 - start as u32;
                        self.split_events[count] = event;
                        count += 1;
                    }
                    next_event += 1;
                }
                // Every change of this stretch lands on its first frame.
                for change in &mut changes[first..next_change] {
                    change.frame = 0;
                }
                let mut local = *ctx;
                local.sample_time += start as i64;
                processor.process(
                    &mut audio[start..end],
                    &self.split_events[..count],
                    &changes[first..next_change],
                    &local,
                );
                start = end;
            }
        }
        pending.len = 0;
        true
    }
    /// Whether the processor in `slot` takes events as an insert (see
    /// [`Processor::accepts_events`]). False for an empty slot.
    pub fn accepts_events(&self, slot: u32) -> bool {
        self.slots
            .get(slot as usize)
            .and_then(|s| s.as_ref())
            .is_some_and(|p| p.accepts_events())
    }
    pub fn latency(&self, slot: u32) -> u32 {
        self.slots
            .get(slot as usize)
            .and_then(|s| s.as_ref())
            .map_or(0, |p| p.latency())
    }
    pub fn reset(&mut self, slot: u32) {
        if let Some(Some(p)) = self.slots.get_mut(slot as usize) {
            p.reset();
        }
    }
    pub fn drain(&mut self) -> Vec<Box<dyn Processor>> {
        self.slots
            .iter_mut()
            .filter_map(|s| s.take())
            .map(|mut processor| {
                processor.stop();
                processor
            })
            .collect()
    }
}

impl Drop for Rack {
    fn drop(&mut self) {
        // Offline racks also own running processors. Stop processing before the
        // last instance reference deactivates and destroys the plugin.
        for processor in self.slots.iter_mut().flatten() {
            processor.stop();
        }
    }
}

/// Decibels to linear gain, shared with the plugin SDK.
pub use ryolune_plugin::dsp::db_to_gain;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parameter_normalisation_round_trips() {
        let log = ParamInfo {
            id: 0,
            name: "Cutoff".into(),
            min: 20.0,
            max: 20000.0,
            default: 1000.0,
            unit: "Hz".into(),
            steps: 0,
            log: true,
            labels: vec![],
        };
        for v in [20.0, 100.0, 1000.0, 20000.0] {
            assert!((log.denormalize(log.normalize(v)) - v).abs() < 1e-6 * v);
        }
        let stepped = ParamInfo {
            id: 1,
            name: "Mode".into(),
            min: 0.0,
            max: 2.0,
            default: 0.0,
            unit: String::new(),
            steps: 2,
            log: false,
            labels: vec!["Low".into(), "Band".into(), "High".into()],
        };
        assert_eq!(stepped.denormalize(0.4), 1.0);
        assert_eq!(stepped.text(2.0), "High");
    }
    /// Frames, changes, note frames and sample time of one `process` call.
    type Call = (usize, Vec<ParamChange>, Vec<u32>, i64);
    /// Records what each `process` call received.
    struct Recorder {
        timed: bool,
        calls: std::sync::Arc<std::sync::Mutex<Vec<Call>>>,
    }
    impl Processor for Recorder {
        fn process(
            &mut self,
            audio: &mut [[f32; 2]],
            events: &[Event],
            params: &[ParamChange],
            ctx: &ProcessContext,
        ) {
            self.calls.lock().unwrap().push((
                audio.len(),
                params.to_vec(),
                events.iter().map(|n| n.frame).collect(),
                ctx.sample_time,
            ));
        }
        fn timed_params(&self) -> bool {
            self.timed
        }
    }
    fn note(frame: u32) -> Event {
        Event::note_on(frame, 60, 100)
    }
    #[test]
    fn timed_changes_stay_sorted_and_the_same_frame_keeps_the_last_value() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let mut rack = Rack::new(1);
        rack.mount(
            0,
            Box::new(Recorder {
                timed: true,
                calls: calls.clone(),
            }),
        );
        rack.set_param_at(0, 64, 1, 0.5);
        rack.set_param_at(0, 0, 1, 0.1);
        rack.set_param_at(0, 32, 2, 0.7);
        rack.set_param_at(0, 64, 1, 0.6);
        rack.set_param(0, 1, 0.2);
        let mut audio = [[0.0; 2]; 128];
        rack.process(0, &mut audio, &[note(5)], &ProcessContext::default());
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "a timed processor gets the whole block");
        assert_eq!(
            calls[0].1,
            vec![
                ParamChange::now(1, 0.2),
                ParamChange {
                    frame: 32,
                    id: 2,
                    value: 0.7
                },
                ParamChange {
                    frame: 64,
                    id: 1,
                    value: 0.6
                },
            ]
        );
    }
    #[test]
    fn a_processor_without_timed_changes_is_split_at_each_change() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let mut rack = Rack::new(1);
        rack.mount(
            0,
            Box::new(Recorder {
                timed: false,
                calls: calls.clone(),
            }),
        );
        rack.set_param(0, 3, 1.0);
        rack.set_param_at(0, 100, 3, 2.0);
        rack.set_param_at(0, 200, 3, 3.0);
        let mut audio = [[0.0; 2]; 256];
        let ctx = ProcessContext {
            sample_time: 1000,
            ..Default::default()
        };
        rack.process(0, &mut audio, &[note(0), note(150), note(220)], &ctx);
        let calls = calls.lock().unwrap();
        let shape: Vec<_> = calls
            .iter()
            .map(|(frames, params, notes, time)| {
                (
                    *frames,
                    params.iter().map(|c| c.value).collect::<Vec<_>>(),
                    notes.clone(),
                    *time,
                )
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                (100, vec![1.0], vec![0], 1000),
                (100, vec![2.0], vec![50], 1100),
                (56, vec![3.0], vec![20], 1200),
            ]
        );
        // Without changes past frame 0 the block stays whole.
        drop(calls);
        rack.set_param(0, 3, 4.0);
        rack.process(0, &mut audio, &[], &ctx);
    }
    #[test]
    fn format_ids_parse() {
        assert_eq!(
            Format::parse("clap:org.x.y"),
            Some((Format::Clap, "org.x.y"))
        );
        assert_eq!(Format::parse("bogus"), None);
        // An LV2 URI keeps its own colons.
        assert_eq!(
            Format::parse("lv2:http://lsp-plug.in/plugins/lv2/comp_delay_x2"),
            Some((Format::Lv2, "http://lsp-plug.in/plugins/lv2/comp_delay_x2"))
        );
        assert_eq!(Format::parse("ladspa:1181"), Some((Format::Ladspa, "1181")));
        assert_eq!(Format::Lv2.label(), "LV2");
        assert_eq!(Format::Ladspa.label(), "LADSPA");
        assert_eq!(serde_json::to_string(&Format::Ladspa).unwrap(), "\"ladspa\"");
    }
}
