use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Strip keys that are not tracks: the stereo output and the two aux buses.
pub const MASTER: &str = "master";
pub const BUS_A: &str = "bus-a";
pub const BUS_B: &str = "bus-b";
pub const MAX_INSERTS: usize = 8;
/// Sends per strip: the first two feed A · Reverb and B · Delay unless pointed elsewhere.
pub const MAX_SENDS: usize = 4;
/// Buses a song may add, as bus tracks.
pub const MAX_BUS_TRACKS: usize = 32;
/// The fixed strips (master and the two aux returns); a bus *track* is a track whose kind is
/// `bus`, see [`Track::is_bus`].
pub fn is_bus(id: &str) -> bool {
    id == MASTER || id == BUS_A || id == BUS_B
}
pub fn bus_name(id: &str) -> &'static str {
    match id {
        MASTER => "Stereo Out",
        BUS_A => "A · Reverb",
        BUS_B => "B · Delay",
        _ => "Bus",
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub name: String,
    pub tracks: Vec<Track>,
    pub clips: Vec<Clip>,
    pub sources: HashMap<String, Source>,
    pub strips: HashMap<String, Strip>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automation: Vec<crate::automation::AutomationLane>,
    pub transport: Transport,
    pub view: View,
    #[serde(default = "default_master_volume")]
    pub master_volume: f32,
    /// Song sections on the ruler, in bar order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
    /// Tempo changes after the start (`transport.tempo`), in bar order. Absent when the song
    /// keeps one tempo, so such files read and write exactly as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tempo_changes: Vec<crate::tempo::TempoPoint>,
    /// Stable id of the song, kept across saves and renames: what the agent's saved
    /// conversations and project memory belong to. Absent in older files; those get one from
    /// their path when opened ([`Session::id_for_path`]) and keep it from the next save.
    /// Never part of an undo step.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
/// A named position on the ruler: the start of a song section.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Marker {
    pub id: String,
    /// Zero-based bar, like clip positions.
    pub bar: f64,
    pub name: String,
    /// CSS colour; absent draws the theme's marker colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}
pub const MAX_MARKERS: usize = 1000;
fn default_master_volume() -> f32 {
    0.75
}

/// Whether the live input is heard through an audio track's strip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Monitor {
    #[default]
    Off,
    /// While the track is armed and is not playing back one of its own clips.
    Auto,
    On,
}
impl Monitor {
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "off" => Ok(Self::Off),
            "auto" => Ok(Self::Auto),
            "on" => Ok(Self::On),
            other => Err(format!("Monitor must be off, auto or on, not {other}")),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
            Self::On => "on",
        }
    }
    fn is_off(&self) -> bool {
        *self == Self::Off
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub name: String,
    pub color: String,
    pub armed: bool,
    #[serde(default, skip_serializing_if = "Monitor::is_off")]
    pub monitor: Monitor,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
    pub kind: String,
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// The bus track this track's fader feeds; absent for the Stereo Out. Bus tracks always
    /// feed the Stereo Out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}
impl Track {
    /// A bus track: no clips, no input; it sums what tracks route or send to it through its
    /// own inserts, fader and pan (a group or an aux return).
    pub fn is_bus(&self) -> bool {
        self.kind == "bus"
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub agent: bool,
    pub track_id: String,
    pub start_bar: f64,
    pub length_bars: f64,
    pub data: ClipData,
}

/// A clip's contents. The file form is [`ClipDataFile`] on the way in and [`ClipDataOut`] on
/// the way out: polyphonic pressure points live in `controllers` like every other controller
/// here, but the file keeps them in a `polyPressure` list of their own, absent when empty, so
/// an older ryolune (and the window's controller lanes) never meet a kind they do not know.
#[derive(Clone, Debug, Deserialize)]
#[serde(from = "ClipDataFile")]
pub enum ClipData {
    Midi {
        notes: Vec<Note>,
        /// Controller changes, pitch bend, channel and polyphonic pressure. Absent from the
        /// file when empty, so clips without them read and write exactly as before.
        controllers: Vec<Controller>,
    },
    Audio {
        source_id: String,
        offset_seconds: f64,
        /// Fade lengths in seconds, like the offset: the audio is not stretched with the tempo,
        /// so a fade keeps its sound when the tempo changes. Absent when zero.
        fade_in: f64,
        fade_out: f64,
        fade_curve: FadeCurve,
        /// Clip gain in dB, applied before the track's inserts. Absent when 0 dB.
        gain_db: f32,
    },
}
/// [`ClipData`] as a file holds it.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ClipDataFile {
    Midi {
        notes: Vec<Note>,
        #[serde(default)]
        controllers: Vec<Controller>,
        #[serde(rename = "polyPressure", default)]
        poly_pressure: Vec<Controller>,
    },
    Audio {
        #[serde(rename = "sourceId")]
        source_id: String,
        #[serde(rename = "offsetSeconds")]
        offset_seconds: f64,
        #[serde(rename = "fadeInSeconds", default)]
        fade_in: f64,
        #[serde(rename = "fadeOutSeconds", default)]
        fade_out: f64,
        #[serde(rename = "fadeCurve", default)]
        fade_curve: FadeCurve,
        #[serde(rename = "gainDb", default)]
        gain_db: f32,
    },
}
impl From<ClipDataFile> for ClipData {
    fn from(file: ClipDataFile) -> Self {
        match file {
            ClipDataFile::Midi {
                notes,
                mut controllers,
                poly_pressure,
            } => {
                controllers.extend(poly_pressure);
                Self::Midi { notes, controllers }
            }
            ClipDataFile::Audio {
                source_id,
                offset_seconds,
                fade_in,
                fade_out,
                fade_curve,
                gain_db,
            } => Self::Audio {
                source_id,
                offset_seconds,
                fade_in,
                fade_out,
                fade_curve,
                gain_db,
            },
        }
    }
}
/// [`ClipData`] as it is written, borrowed so saving copies nothing.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ClipDataOut<'a> {
    Midi {
        notes: &'a [Note],
        #[serde(skip_serializing_if = "Points::is_empty")]
        controllers: Points<'a>,
        #[serde(rename = "polyPressure", skip_serializing_if = "Points::is_empty")]
        poly_pressure: Points<'a>,
    },
    Audio {
        #[serde(rename = "sourceId")]
        source_id: &'a str,
        #[serde(rename = "offsetSeconds")]
        offset_seconds: f64,
        #[serde(rename = "fadeInSeconds", skip_serializing_if = "is_zero")]
        fade_in: f64,
        #[serde(rename = "fadeOutSeconds", skip_serializing_if = "is_zero")]
        fade_out: f64,
        #[serde(rename = "fadeCurve", skip_serializing_if = "FadeCurve::is_default")]
        fade_curve: FadeCurve,
        #[serde(rename = "gainDb", skip_serializing_if = "is_zero_f32")]
        gain_db: f32,
    },
}
/// The controller points of one list in the file: polyphonic pressure or everything else.
struct Points<'a> {
    all: &'a [Controller],
    poly: bool,
}
impl Points<'_> {
    fn iter(&self) -> impl Iterator<Item = &Controller> {
        self.all
            .iter()
            .filter(|p| (p.kind == ControllerKind::PolyPressure) == self.poly)
    }
    fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }
}
impl Serialize for Points<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}
impl Serialize for ClipData {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Midi { notes, controllers } => ClipDataOut::Midi {
                notes,
                controllers: Points {
                    all: controllers,
                    poly: false,
                },
                poly_pressure: Points {
                    all: controllers,
                    poly: true,
                },
            },
            Self::Audio {
                source_id,
                offset_seconds,
                fade_in,
                fade_out,
                fade_curve,
                gain_db,
            } => ClipDataOut::Audio {
                source_id,
                offset_seconds: *offset_seconds,
                fade_in: *fade_in,
                fade_out: *fade_out,
                fade_curve: *fade_curve,
                gain_db: *gain_db,
            },
        }
        .serialize(serializer)
    }
}
impl ClipData {
    /// An audio clip at `offset_seconds` into its source, without fades and at 0 dB.
    pub fn audio(source_id: impl Into<String>, offset_seconds: f64) -> Self {
        Self::Audio {
            source_id: source_id.into(),
            offset_seconds,
            fade_in: 0.0,
            fade_out: 0.0,
            fade_curve: FadeCurve::default(),
            gain_db: 0.0,
        }
    }
}
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}
fn is_zero_f32(v: &f32) -> bool {
    *v == 0.0
}
fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}
/// MIDI channels are 0-15 in the file and on the wire (channels 1-16 to a musician).
pub const MIDI_CHANNELS: u8 = 16;
/// Clip gain range in dB.
pub const CLIP_GAIN_MIN_DB: f32 = -60.0;
pub const CLIP_GAIN_MAX_DB: f32 = 24.0;

/// The shape of an audio clip's fades. Fade-outs mirror fade-ins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FadeCurve {
    /// Equal power (a quarter sine): crossfades keep their loudness. The default.
    #[default]
    EqualPower,
    Linear,
    /// Slow start, fast finish: sounds even to the ear on long fades in.
    Exponential,
}
impl FadeCurve {
    pub const ALL: [FadeCurve; 3] = [Self::EqualPower, Self::Linear, Self::Exponential];
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "equalPower" => Ok(Self::EqualPower),
            "linear" => Ok(Self::Linear),
            "exponential" => Ok(Self::Exponential),
            other => Err(format!(
                "Fade curve must be equalPower, linear or exponential, not {other}"
            )),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EqualPower => "equalPower",
            Self::Linear => "linear",
            Self::Exponential => "exponential",
        }
    }
    fn is_default(&self) -> bool {
        *self == Self::EqualPower
    }
    /// Gain 0-1 at `x` of the way through a fade-in (0 = silent, 1 = full).
    #[inline]
    pub fn gain(self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::Linear => x,
            Self::EqualPower => (x * std::f64::consts::FRAC_PI_2).sin(),
            Self::Exponential => ((4.0 * x).exp() - 1.0) / (4f64.exp() - 1.0),
        }
    }
}

/// Keep fades inside a clip of `length` seconds: each fits the clip, and when together they
/// would overlap they shrink in proportion.
pub fn clamp_fades(fade_in: f64, fade_out: f64, length: f64) -> (f64, f64) {
    let length = length.max(0.0);
    let (fade_in, fade_out) = (fade_in.clamp(0.0, length), fade_out.clamp(0.0, length));
    let sum = fade_in + fade_out;
    if sum > length && sum > 0.0 {
        (fade_in * length / sum, fade_out * length / sum)
    } else {
        (fade_in, fade_out)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Note {
    pub id: String,
    pub start: f64,
    pub length: f64,
    pub pitch: u8,
    pub velocity: u8,
    #[serde(default)]
    pub agent: bool,
    /// The MIDI channel the note plays on, 0-15, as it was played in. Absent from the file
    /// on channel 0 (channel 1 to a musician), so older files and older versions read as
    /// before.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub channel: u8,
}

/// What a [`Controller`] point moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ControllerKind {
    /// A MIDI control change; `number` is 0-127 and `value` 0-127.
    Cc,
    /// Pitch bend; `value` is -8192 (full down) to 8191 (full up), 0 centred.
    Bend,
    /// Channel pressure (aftertouch); `value` is 0-127.
    Pressure,
    /// Polyphonic key pressure; `number` is the key (0-127) and `value` 0-127.
    #[serde(rename = "poly")]
    PolyPressure,
}
impl ControllerKind {
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "cc" => Ok(Self::Cc),
            "bend" => Ok(Self::Bend),
            "pressure" => Ok(Self::Pressure),
            "poly" => Ok(Self::PolyPressure),
            other => Err(format!(
                "Controller kind must be cc, bend, pressure or poly, not {other}"
            )),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cc => "cc",
            Self::Bend => "bend",
            Self::Pressure => "pressure",
            Self::PolyPressure => "poly",
        }
    }
    /// Lowest and highest value a point of this kind may hold.
    pub fn range(self) -> (i16, i16) {
        match self {
            Self::Bend => (-8192, 8191),
            _ => (0, 127),
        }
    }
}

/// One controller point in a MIDI clip. The value holds until the next point of the same
/// lane (kind and number), as MIDI does.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Controller {
    pub id: String,
    pub kind: ControllerKind,
    /// The controller number for `cc`, the key for `poly`; absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u8>,
    /// Beats from the clip start.
    pub time: f64,
    pub value: i16,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub agent: bool,
    /// The MIDI channel, 0-15; absent from the file on channel 0.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub channel: u8,
}
impl Controller {
    /// The lane this point belongs to: kind, number and channel.
    pub fn lane(&self) -> (ControllerKind, Option<u8>, u8) {
        (self.kind, self.number, self.channel)
    }
    pub fn is_valid(&self) -> bool {
        let (low, high) = self.kind.range();
        valid_time(self.time)
            && self.channel < MIDI_CHANNELS
            && (low..=high).contains(&self.value)
            && match self.kind {
                ControllerKind::Cc | ControllerKind::PolyPressure => {
                    self.number.is_some_and(|n| n <= 127)
                }
                _ => self.number.is_none(),
            }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
    #[serde(default)]
    pub file_name: Option<String>,
    pub duration_seconds: f64,
    pub origin: String,
    pub seed: Option<u32>,
    pub wave_kind: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Strip {
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
    #[serde(default = "default_instrument")]
    pub instrument: String,
    #[serde(default)]
    pub inserts: Vec<Insert>,
    #[serde(default)]
    pub sends: Vec<Send>,
    /// An external instrument plugin. `None` plays the stock instrument named by `instrument`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synth: Option<Insert>,
}
/// Stock plugins that carried the project's former name answer to their current one, so songs
/// saved before the rename keep their instruments and inserts.
pub fn current_stock_name(name: &str) -> String {
    match name.strip_prefix("Ondera ") {
        Some(rest @ ("Synth" | "Comp")) => format!("ryolune {rest}"),
        _ => name.to_string(),
    }
}
fn default_instrument() -> String {
    "ryolune Synth".into()
}
impl Default for Strip {
    fn default() -> Self {
        Self {
            extra: HashMap::new(),
            instrument: default_instrument(),
            inserts: vec![],
            sends: vec![],
            synth: None,
        }
    }
}
impl Strip {
    /// The rack key of this strip's instrument, unique per track and plugin.
    pub fn synth_key(&self, track_id: &str) -> String {
        match &self.synth {
            Some(insert) => insert.id.clone(),
            None => format!("{track_id}/synth/{}", self.instrument),
        }
    }
    pub fn instrument_name(&self) -> String {
        match &self.synth {
            Some(insert) => insert.name.clone(),
            None => self.instrument.clone(),
        }
    }
}

/// One slot of a channel strip. `plugin` is a descriptor id (`stock:Space`,
/// `clap:…`, `vst3:…`, `au:…`); when empty the stock plugin named by `name` is meant.
#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct Insert {
    pub name: String,
    pub state: String,
    #[serde(default)]
    pub meta: String,
    #[serde(default)]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub plugin: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<u32, f64>,
    /// Base64 plugin state captured on save.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub blob: String,
}
impl Insert {
    pub fn empty_slot() -> Self {
        Self {
            name: "Empty slot".into(),
            state: "empty".into(),
            ..Default::default()
        }
    }
    pub fn new(id: String, plugin_id: &str, name: &str) -> Self {
        Self {
            name: name.into(),
            state: "active".into(),
            meta: String::new(),
            id,
            plugin: plugin_id.into(),
            params: BTreeMap::new(),
            blob: String::new(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.state == "empty"
    }
    pub fn plugin_id(&self) -> String {
        if self.plugin.is_empty() {
            format!("stock:{}", self.name)
        } else {
            self.plugin.clone()
        }
    }
}

/// A plugin instance the session needs, keyed for the audio-thread rack.
#[derive(Clone, Debug, PartialEq)]
pub struct Need {
    pub key: String,
    pub plugin: String,
    pub name: String,
    pub params: BTreeMap<u32, f64>,
    pub blob: String,
    pub instrument: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Send {
    pub level_db: Option<f32>,
    #[serde(default)]
    pub name: String,
    /// Where it goes: a bus track, or `bus-a` / `bus-b`. Absent, the first send feeds
    /// A · Reverb and the second B · Delay, as they always did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bus: Option<String>,
}
impl Send {
    /// The strip this send feeds when it is the `index`th of its strip.
    pub fn target(&self, index: usize) -> Option<&str> {
        self.bus.as_deref().or(match index {
            0 => Some(BUS_A),
            1 => Some(BUS_B),
            _ => None,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transport {
    #[serde(default)]
    pub playing: bool,
    #[serde(default)]
    pub recording: bool,
    pub position_beats: f64,
    pub key: String,
    pub snap_division: u32,
    pub tempo: f64,
    pub time_signature: TimeSignature,
    pub cycle: bool,
    pub cycle_start_bar: f64,
    pub cycle_end_bar: f64,
    pub metronome: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TimeSignature {
    pub numerator: u32,
    pub denominator: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub selected_track_id: Option<String>,
    pub selected_clip_id: Option<String>,
    pub editor_clip_id: Option<String>,
    pub selected_note_id: Option<String>,
    pub pixels_per_bar: f32,
    pub scroll_bars: f64,
    pub follow_playhead: bool,
    pub editor_mode: String,
    /// Browser tab: instruments, loops, plugins or files. Sessions have carried it and the
    /// selected row since the first format; they are fields now so commands can reach them.
    #[serde(default = "default_browser_tab")]
    pub browser_tab: String,
    /// The browser row that is selected, by name.
    #[serde(default)]
    pub browser_selection: Option<String>,
    /// Lowest pitch the piano roll shows; `None` lets it frame the open clip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_low_pitch: Option<u8>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
pub const BROWSER_TABS: [&str; 4] = ["instruments", "loops", "plugins", "files"];
fn default_browser_tab() -> String {
    BROWSER_TABS[0].into()
}

impl Session {
    /// Give a song without an id a new random one (a new song, the demo).
    pub fn ensure_id(&mut self) {
        if self.id.is_empty() {
            self.id = crate::lsuite::uuid_v4();
        }
    }
    /// The id an older file without one gets: derived from where it is, so opening it again
    /// before it is saved finds the same conversations.
    pub fn id_for_path(path: &std::path::Path) -> String {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        // FNV-1a: stable across runs and Rust versions, unlike the std hasher.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in path.to_string_lossy().bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("file-{hash:016x}")
    }
    /// Fill in missing insert ids and default buses so every strip has stable rack keys.
    pub fn normalize(&mut self) {
        for strip in self.strips.values_mut() {
            strip.instrument = current_stock_name(&strip.instrument);
            for insert in strip.inserts.iter_mut().chain(strip.synth.iter_mut()) {
                insert.name = current_stock_name(&insert.name);
                if let Some(name) = insert.plugin.strip_prefix("stock:") {
                    insert.plugin = format!("stock:{}", current_stock_name(name));
                }
                if insert.meta == "Ondera" {
                    insert.meta = "ryolune".into();
                }
            }
        }
        let mut used: HashSet<String> = HashSet::new();
        for strip in self.strips.values() {
            used.extend(strip.inserts.iter().map(|i| i.id.clone()));
            if let Some(s) = &strip.synth {
                used.insert(s.id.clone());
            }
        }
        let mut serial = 0u64;
        let mut fresh = |used: &mut HashSet<String>| loop {
            serial += 1;
            let id = format!("insert-{serial}");
            if used.insert(id.clone()) {
                return id;
            }
        };
        // Two slots with one id (a hand-merged file) would share one plugin instance in the
        // audio rack: the second one gets an id of its own.
        let mut seen: HashSet<String> = HashSet::new();
        let mut keys: Vec<String> = self.strips.keys().cloned().collect();
        keys.sort();
        for key in keys {
            let strip = self.strips.get_mut(&key).expect("key from the map");
            for insert in strip.inserts.iter_mut().chain(strip.synth.iter_mut()) {
                if insert.id.is_empty() || !seen.insert(insert.id.clone()) {
                    insert.id = fresh(&mut used);
                    seen.insert(insert.id.clone());
                }
            }
        }
        // The piano roll shows 20 rows up from here; the command allows 0-108.
        if let Some(pitch) = self.view.editor_low_pitch {
            self.view.editor_low_pitch = Some(pitch.min(108));
        }
        for (bus, effect, mix_index) in [(BUS_A, "Space", 4), (BUS_B, "Echo", 5)] {
            if !self.strips.contains_key(bus) {
                let mut insert = Insert::new(fresh(&mut used), &format!("stock:{effect}"), effect);
                insert.params.insert(mix_index, 100.0);
                self.strips.insert(
                    bus.into(),
                    Strip {
                        inserts: vec![insert],
                        ..Default::default()
                    },
                );
            }
        }
        if !self.strips.contains_key(MASTER) {
            self.strips.insert(MASTER.into(), Strip::default());
        }
    }
    /// Every plugin instance the session needs, in a deterministic order.
    pub fn needs(&self) -> Vec<Need> {
        let mut needs = vec![];
        fn push_inserts(needs: &mut Vec<Need>, inserts: &[Insert]) {
            for insert in inserts.iter().filter(|i| !i.is_empty()) {
                needs.push(Need {
                    key: insert.id.clone(),
                    plugin: insert.plugin_id(),
                    name: insert.name.clone(),
                    params: insert.params.clone(),
                    blob: insert.blob.clone(),
                    instrument: false,
                });
            }
        }
        for track in &self.tracks {
            let Some(strip) = self.strips.get(&track.id) else {
                if track.kind == "midi" {
                    let strip = Strip::default();
                    needs.push(Need {
                        key: strip.synth_key(&track.id),
                        plugin: format!("stock:{}", strip.instrument),
                        name: strip.instrument.clone(),
                        params: BTreeMap::new(),
                        blob: String::new(),
                        instrument: true,
                    });
                }
                continue;
            };
            if track.kind == "midi" {
                needs.push(match &strip.synth {
                    Some(s) => Need {
                        key: s.id.clone(),
                        plugin: s.plugin_id(),
                        name: s.name.clone(),
                        params: s.params.clone(),
                        blob: s.blob.clone(),
                        instrument: true,
                    },
                    None => Need {
                        key: strip.synth_key(&track.id),
                        plugin: format!("stock:{}", strip.instrument),
                        name: strip.instrument.clone(),
                        params: BTreeMap::new(),
                        blob: String::new(),
                        instrument: true,
                    },
                });
            }
            push_inserts(&mut needs, &strip.inserts);
        }
        for bus in [BUS_A, BUS_B, MASTER] {
            if let Some(strip) = self.strips.get(bus) {
                push_inserts(&mut needs, &strip.inserts);
            }
        }
        needs
    }
    pub fn beats_per_bar(&self) -> f64 {
        self.transport.time_signature.numerator as f64 * 4.0
            / self.transport.time_signature.denominator as f64
    }
    /// Seconds and tempo for every beat of the song.
    pub fn tempo_map(&self) -> crate::tempo::TempoMap {
        crate::tempo::TempoMap::new(
            self.transport.tempo,
            &self.tempo_changes,
            self.beats_per_bar(),
        )
    }
    /// Seconds between two bars.
    pub fn bars_seconds(&self, start_bar: f64, end_bar: f64) -> f64 {
        let bpb = self.beats_per_bar();
        self.tempo_map().duration(start_bar * bpb, end_bar * bpb)
    }
    /// Bars that `seconds` of audio cover from `start_bar` on.
    pub fn seconds_bars(&self, start_bar: f64, seconds: f64) -> f64 {
        let bpb = self.beats_per_bar();
        self.tempo_map().beats_for(start_bar * bpb, seconds) / bpb
    }
    /// The tempo at the start of `bar`.
    pub fn tempo_at_bar(&self, bar: f64) -> f64 {
        self.tempo_map().bpm(bar * self.beats_per_bar())
    }
    pub fn end_bar(&self) -> f64 {
        self.clips
            .iter()
            .map(|c| c.start_bar + c.length_bars)
            .fold(1.0, f64::max)
    }
    pub fn validate(&self) -> Result<()> {
        crate::automation::validate(self)?;
        let t = &self.transport;
        if !self.view.pixels_per_bar.is_finite()
            || !(12.0..=480.0).contains(&self.view.pixels_per_bar)
            || !valid_time(self.view.scroll_bars)
        {
            return Err("Invalid view scale or position".into());
        }
        if !valid_time(t.position_beats) || ![1, 2, 4, 8, 16, 32, 64].contains(&t.snap_division) {
            return Err("Invalid transport".into());
        }
        if !crate::tempo::valid_bpm(t.tempo) {
            return Err("Tempo must be between 20 and 400 BPM".into());
        }
        crate::tempo::validate(&self.tempo_changes)?;
        if !(1..=32).contains(&t.time_signature.numerator)
            || ![1, 2, 4, 8, 16, 32].contains(&t.time_signature.denominator)
        {
            return Err("Invalid time signature".into());
        }
        if self.tracks.len() > 128 || self.clips.len() > 50_000 || self.sources.len() > 10_000 {
            return Err("Session exceeds engine capacity".into());
        }
        if !valid_time(t.cycle_start_bar)
            || !valid_time(t.cycle_end_bar)
            || t.cycle_end_bar <= t.cycle_start_bar
        {
            return Err("Invalid cycle range".into());
        }
        let mut ids = HashSet::new();
        for track in &self.tracks {
            if !ids.insert(&track.id)
                || is_bus(&track.id)
                || !["audio", "midi", "bus"].contains(&track.kind.as_str())
                || !track.volume.is_finite()
                || !(0.0..=1.0).contains(&track.volume)
                || !track.pan.is_finite()
                || !(-100.0..=100.0).contains(&track.pan)
            {
                return Err("Invalid or duplicate track".into());
            }
        }
        self.validate_routing()?;
        let mut clips = HashSet::new();
        let mut notes = 0;
        for c in &self.clips {
            if !clips.insert(&c.id)
                || !ids.contains(&c.track_id)
                || !valid_time(c.start_bar)
                || !valid_time(c.length_bars)
                || c.length_bars <= 0.0
            {
                return Err("Invalid clip".into());
            }
            let track = self.tracks.iter().find(|t| t.id == c.track_id).unwrap();
            if track.is_bus() {
                return Err("Bus tracks hold no clips: route or send tracks to them".into());
            }
            match &c.data {
                ClipData::Midi {
                    notes: ns,
                    controllers: cs,
                } => {
                    if track.kind != "midi" {
                        return Err("MIDI clip on audio track".into());
                    }
                    notes += ns.len() + cs.len();
                    if cs.iter().any(|c| !c.is_valid()) {
                        return Err("Invalid MIDI controller point".into());
                    }
                    if ns.iter().any(|n| {
                        !valid_time(n.start)
                            || !valid_time(n.length)
                            || n.length <= 0.0
                            || n.pitch > 127
                            || n.channel >= MIDI_CHANNELS
                            || n.velocity == 0
                            || n.velocity > 127
                    }) {
                        return Err("Invalid MIDI note".into());
                    }
                }
                ClipData::Audio {
                    source_id,
                    offset_seconds,
                    fade_in,
                    fade_out,
                    gain_db,
                    ..
                } => {
                    if track.kind != "audio"
                        || !self.sources.contains_key(source_id)
                        || !valid_time(*offset_seconds)
                    {
                        return Err("Invalid audio clip or missing source".into());
                    }
                    if !valid_time(*fade_in)
                        || !valid_time(*fade_out)
                        || !gain_db.is_finite()
                        || !(CLIP_GAIN_MIN_DB..=CLIP_GAIN_MAX_DB).contains(gain_db)
                    {
                        return Err("Invalid audio clip fades or gain".into());
                    }
                }
            }
        }
        if notes > 200_000 {
            return Err("Too many notes and controller points".into());
        }
        if self.markers.len() > MAX_MARKERS {
            return Err("Too many markers".into());
        }
        let mut marker_ids = HashSet::new();
        for m in &self.markers {
            if !marker_ids.insert(&m.id)
                || m.id.is_empty()
                || !valid_time(m.bar)
                || m.name.chars().count() > 120
            {
                return Err("Invalid or duplicate marker".into());
            }
        }
        let mut decoded_bytes = 0.0;
        for (id, src) in &self.sources {
            if !(8000..=384000).contains(&src.sample_rate) || !(1..=2).contains(&src.channels) {
                return Err("Invalid source sample rate or channels".into());
            }
            decoded_bytes += src.duration_seconds
                * if src.origin == "generated" {
                    48000.0
                } else {
                    src.sample_rate as f64
                }
                * 8.0;
            if id != &src.id
                || !valid_time(src.duration_seconds)
                || src.duration_seconds > 14_400.0
                || !["generated", "file", "recording"].contains(&src.origin.as_str())
            {
                return Err("Invalid audio source".into());
            }
        }
        if decoded_bytes > crate::audio::MAX_LIBRARY_BYTES as f64 {
            return Err("Session audio exceeds the 1 GiB decoded-audio limit".into());
        }
        if !self.master_volume.is_finite() || !(0.0..=1.0).contains(&self.master_volume) {
            return Err("Invalid master volume".into());
        }
        for strip in self.strips.values() {
            if strip.inserts.len() > MAX_INSERTS
                || strip.sends.len() > MAX_SENDS
                || strip.sends.iter().any(|s| {
                    s.level_db
                        .is_some_and(|v| !v.is_finite() || !(-100.0..=0.0).contains(&v))
                })
            {
                return Err("Invalid channel strip".into());
            }
            for insert in strip.inserts.iter().chain(strip.synth.iter()) {
                if insert.params.values().any(|v| !v.is_finite())
                    || insert.blob.len() > 64 * 1024 * 1024
                    || !["active", "bypassed", "empty"].contains(&insert.state.as_str())
                {
                    return Err("Invalid insert".into());
                }
            }
        }
        Ok(())
    }
}
impl Session {
    pub fn bus_track(&self, id: &str) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id && t.is_bus())
    }
    /// Outputs and sends point at buses that exist, in one direction only: tracks feed bus
    /// tracks, bus tracks feed A, B and the Stereo Out. So the graph never loops.
    fn validate_routing(&self) -> Result<()> {
        if self.tracks.iter().filter(|t| t.is_bus()).count() > MAX_BUS_TRACKS {
            return Err(format!("A song holds at most {MAX_BUS_TRACKS} buses"));
        }
        for track in &self.tracks {
            if track.is_bus() && (track.armed || track.monitor != Monitor::Off) {
                return Err("A bus track cannot be armed or monitor an input".into());
            }
            if let Some(output) = &track.output {
                if track.is_bus() {
                    return Err(format!(
                        "Bus \"{}\" feeds the Stereo Out; buses do not feed other buses",
                        track.name
                    ));
                }
                if self.bus_track(output).is_none() {
                    return Err(format!(
                        "Track \"{}\" outputs to `{output}`, which is not a bus track",
                        track.name
                    ));
                }
            }
        }
        for (id, strip) in &self.strips {
            let from_bus = self.bus_track(id).is_some();
            for (index, send) in strip.sends.iter().enumerate() {
                match send.target(index) {
                    None => return Err("A third or fourth send needs a bus".into()),
                    Some(BUS_A | BUS_B) => {}
                    Some(target) if !from_bus && !is_bus(id) && self.bus_track(target).is_some() => {}
                    Some(target) => {
                        return Err(format!(
                            "A send from `{id}` cannot feed `{target}`: tracks send to bus tracks, A or B; buses send to A or B"
                        ))
                    }
                }
            }
        }
        Ok(())
    }
    /// Forget outputs and sends that point at a bus track no longer in the song: the track
    /// goes back to the Stereo Out and the send back to its default (or away).
    pub fn prune_routing(&mut self) {
        let buses: HashSet<String> = self
            .tracks
            .iter()
            .filter(|t| t.is_bus())
            .map(|t| t.id.clone())
            .collect();
        for track in &mut self.tracks {
            if track
                .output
                .as_ref()
                .is_some_and(|o| !buses.contains(o) || track.kind == "bus")
            {
                track.output = None;
            }
        }
        for strip in self.strips.values_mut() {
            let mut index = 0;
            strip.sends.retain_mut(|send| {
                let dangling = send
                    .bus
                    .as_ref()
                    .is_some_and(|b| b != BUS_A && b != BUS_B && !buses.contains(b));
                let keep = if dangling {
                    send.bus = None;
                    send.level_db = None;
                    index < 2
                } else {
                    true
                };
                index += 1;
                keep
            });
        }
    }
}
pub fn valid_time(v: f64) -> bool {
    v.is_finite() && (0.0..=1_000_000.0).contains(&v)
}
pub fn fader_gain(v: f32) -> f32 {
    if v <= 0.0 {
        0.0
    } else if v < 0.75 {
        (v / 0.75).powf(1.6)
    } else {
        10.0_f32.powf(((v - 0.75) * 24.0) / 20.0)
    }
}
