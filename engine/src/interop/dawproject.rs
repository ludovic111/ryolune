//! DAWproject 1.0, the open exchange format of Bitwig Studio, Studio One, Cubase and
//! others (github.com/bitwig/dawproject, `Project.xsd`): a zip holding `project.xml`,
//! `metadata.xml`, the audio and the plugin states.
//!
//! Writing follows the schema's element order. Tracks become `Track`s with a `Channel`
//! (instrument tracks `notes`, audio tracks `audio`, bus tracks `effect` or, when tracks are
//! routed to them, `submix`; the two aux returns `effect`, the Stereo Out `master`); volume
//! is linear gain, pan normalized, sends linear. The arrangement is in beats (quarter
//! notes): MIDI clips hold `Notes` (and, with controllers, `Lanes` of `Notes` and expression
//! `Points`), audio clips hold the `Audio` in seconds with `playStart`/`playStop`, so the
//! audio plays at its own speed as it does here, and fades in seconds. Tempo changes are
//! `TempoAutomation` (a ramp is a linear point before it), markers `Markers`. Plugins are
//! `ClapPlugin`, `Vst3Plugin` (state as `.vstpreset`) and `AuPlugin` by id; ryolune's own
//! instruments and effects are `Device`s whose `deviceID` is the ryolune plugin id, with
//! their settings in a small JSON state file.
//!
//! Reading takes what other apps write too: nested `Clips` (Bitwig's audio clips), `Warps`,
//! loops (unrolled), clips in seconds, alias clips, group tracks, sends, builtin devices
//! (mapped to the nearest ryolune effect) and automation of volume and pan. What cannot come
//! is listed in the [`Report`].

use super::{count, Imported, Report};
use crate::{
    audio::{self, Library},
    automation::{AutomationLane, AutomationPoint, AutomationTarget, Interpolation},
    control::new_id,
    document,
    dsp::INSTRUMENTS,
    host::{decode_blob, encode_blob},
    model::*,
    plugin::{Descriptor, Format as PluginFormat},
    tempo::{TempoMap, TempoPoint},
    Result,
};
use roxmltree::Node;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Write as _,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

pub const EXTENSION: &str = "dawproject";
const PROJECT: &str = "project.xml";
const METADATA: &str = "metadata.xml";
const MAX_XML: u64 = 64 * 1024 * 1024;
/// The ids of the two aux returns, so a ryolune project comes back with them in place.
const AUX_IDS: [(&str, &str); 2] = [(BUS_A, "ryolune-bus-a"), (BUS_B, "ryolune-bus-b")];
/// Linear controller ramps become steps this many beats apart.
const RAMP_STEP: f64 = 0.125;

// ---------------------------------------------------------------------------------------
// Volume and pan
// ---------------------------------------------------------------------------------------

/// The fader position (0-1, 0.75 unity) for a linear gain: the inverse of `fader_gain`.
pub fn fader_position(gain: f64) -> f64 {
    if gain <= 0.0 || !gain.is_finite() {
        0.0
    } else if gain < 1.0 {
        0.75 * gain.powf(1.0 / 1.6)
    } else {
        0.75 + 20.0 * gain.log10() / 24.0
    }
}
fn pan_normalized(pan: f32) -> f64 {
    ((pan as f64 + 100.0) / 200.0).clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------------------

/// Write the song as a `.dawproject`, replacing `path` atomically.
pub fn export(session: &Session, library: &Library, path: &Path) -> Result<Report> {
    session.validate()?;
    let mut library = library.clone();
    audio::prepare_sources(session, &mut library)?;
    let written = write(session, &library)?;
    document::atomic_write(path, |file| {
        let mut zip = zip::ZipWriter::new(file);
        let deflated = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .large_file(true);
        let entries = [
            (METADATA, written.metadata.as_bytes(), deflated),
            (PROJECT, written.project.as_bytes(), deflated),
        ];
        for (name, bytes, options) in entries {
            zip.start_file(name, options).map_err(|e| e.to_string())?;
            zip.write_all(bytes).map_err(|e| e.to_string())?;
        }
        for (name, bytes) in &written.files {
            let options = if name.ends_with(".wav") {
                stored
            } else {
                deflated
            };
            zip.start_file(name.as_str(), options)
                .map_err(|e| e.to_string())?;
            zip.write_all(bytes).map_err(|e| e.to_string())?;
        }
        zip.finish().map_err(|e| e.to_string())?;
        Ok(())
    })?;
    Ok(written.report)
}

/// A project as text and the files that go beside it in the zip.
pub struct Written {
    pub project: String,
    pub metadata: String,
    pub files: Vec<(String, Vec<u8>)>,
    pub report: Report,
}

/// XML with the schema's indentation, escaped attribute values and nothing else.
struct Xml {
    out: String,
    stack: Vec<&'static str>,
}
impl Xml {
    fn new() -> Self {
        Self {
            out: "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n".into(),
            stack: vec![],
        }
    }
    fn indent(&mut self) {
        for _ in 0..self.stack.len() {
            self.out.push_str("  ");
        }
    }
    fn tag(&mut self, name: &str, attrs: &[(&str, String)], empty: bool) {
        self.indent();
        self.out.push('<');
        self.out.push_str(name);
        for (key, value) in attrs {
            let _ = write!(self.out, " {key}=\"{}\"", escape(value));
        }
        self.out.push_str(if empty { "/>\n" } else { ">\n" });
    }
    fn open(&mut self, name: &'static str, attrs: &[(&str, String)]) {
        self.tag(name, attrs, false);
        self.stack.push(name);
    }
    fn empty(&mut self, name: &str, attrs: &[(&str, String)]) {
        self.tag(name, attrs, true);
    }
    fn close(&mut self) {
        let name = self.stack.pop().expect("an open element");
        self.indent();
        let _ = writeln!(self.out, "</{name}>");
    }
    fn text(&mut self, name: &str, text: &str) {
        self.indent();
        let _ = writeln!(self.out, "<{name}>{}</{name}>", escape(text));
    }
}
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            '\t' => out.push_str("&#9;"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}
fn num(v: f64) -> String {
    if v.is_finite() {
        format!("{v}")
    } else if v > 0.0 {
        "inf".into()
    } else {
        "-inf".into()
    }
}
fn a(key: &'static str, value: impl ToString) -> (&'static str, String) {
    (key, value.to_string())
}

/// The ids one mixer channel's elements carry.
#[derive(Clone)]
struct Ids {
    track: String,
    channel: String,
    volume: String,
    pan: String,
    mute: String,
}

struct Writer<'a> {
    s: &'a Session,
    library: &'a Library,
    xml: Xml,
    next: u32,
    files: Vec<(String, Vec<u8>)>,
    names: HashSet<String>,
    audio_paths: HashMap<String, String>,
    ids: HashMap<String, Ids>,
    report: Report,
    stock: usize,
    plugins: usize,
    states: usize,
}

/// The project and metadata XML and the files of the zip, without touching the disk.
pub fn write(session: &Session, library: &Library) -> Result<Written> {
    let mut w = Writer {
        s: session,
        library,
        xml: Xml::new(),
        next: 0,
        files: vec![],
        names: HashSet::new(),
        audio_paths: HashMap::new(),
        ids: HashMap::new(),
        report: Report::new("dawproject"),
        stock: 0,
        plugins: 0,
        states: 0,
    };
    w.project()?;
    let mut meta = Xml::new();
    meta.open("MetaData", &[]);
    let title = session.name.trim_end_matches(".ryolune");
    if !title.trim().is_empty() {
        meta.text("Title", title);
    }
    meta.text("Comment", "Made with ryolune");
    meta.close();
    let Writer {
        xml, files, report, ..
    } = w;
    Ok(Written {
        project: xml.out,
        metadata: meta.out,
        files,
        report,
    })
}

impl Writer<'_> {
    fn id(&mut self) -> String {
        self.next += 1;
        format!("id{}", self.next)
    }
    fn channel_ids(&mut self, fixed: Option<&str>) -> Ids {
        let (track, channel) = match fixed {
            Some(id) => (format!("{id}-track"), id.to_string()),
            None => (self.id(), self.id()),
        };
        Ids {
            track,
            channel,
            volume: self.id(),
            pan: self.id(),
            mute: self.id(),
        }
    }
    fn project(&mut self) -> Result<()> {
        let s = self.s;
        for t in &s.tracks {
            let ids = self.channel_ids(None);
            self.ids.insert(t.id.clone(), ids);
        }
        for (bus, fixed) in AUX_IDS {
            let ids = self.channel_ids(Some(fixed));
            self.ids.insert(bus.into(), ids);
        }
        let master = self.channel_ids(None);
        self.ids.insert(MASTER.into(), master);
        let tempo_id = self.id();
        let meter_id = self.id();
        self.xml.open("Project", &[a("version", "1.0")]);
        self.xml.empty(
            "Application",
            &[
                a("name", "ryolune"),
                a("version", env!("CARGO_PKG_VERSION")),
            ],
        );
        self.xml.open("Transport", &[]);
        self.xml.empty(
            "Tempo",
            &[
                a("max", num(crate::tempo::MAX_BPM)),
                a("min", num(crate::tempo::MIN_BPM)),
                a("unit", "bpm"),
                a("value", num(s.transport.tempo)),
                a("id", &tempo_id),
                a("name", "Tempo"),
            ],
        );
        self.xml.empty(
            "TimeSignature",
            &[
                a("denominator", s.transport.time_signature.denominator),
                a("numerator", s.transport.time_signature.numerator),
                a("id", &meter_id),
            ],
        );
        self.xml.close();
        self.xml.open("Structure", &[]);
        let fed: HashSet<&str> = s
            .tracks
            .iter()
            .filter_map(|t| t.output.as_deref())
            .collect();
        for t in &s.tracks {
            let role = if !t.is_bus() {
                "regular"
            } else if fed.contains(t.id.as_str()) {
                "submix"
            } else {
                "effect"
            };
            let content = if t.kind == "midi" { "notes" } else { "audio" };
            let destination = t
                .output
                .as_deref()
                .filter(|o| self.ids.contains_key(*o))
                .unwrap_or(MASTER);
            self.track(
                &t.id,
                &t.name,
                Some(&t.color),
                content,
                role,
                destination,
                t,
            )?;
        }
        for (bus, _) in AUX_IDS {
            let track = Track {
                id: bus.into(),
                name: bus_name(bus).into(),
                color: String::new(),
                armed: false,
                monitor: Monitor::Off,
                extra: Default::default(),
                kind: "bus".into(),
                volume: 0.75,
                pan: 0.0,
                mute: false,
                solo: false,
                output: None,
            };
            self.track(bus, bus_name(bus), None, "audio", "effect", MASTER, &track)?;
        }
        let master = Track {
            id: MASTER.into(),
            name: "Master".into(),
            color: String::new(),
            armed: false,
            monitor: Monitor::Off,
            extra: Default::default(),
            kind: "bus".into(),
            volume: s.master_volume,
            pan: 0.0,
            mute: false,
            solo: false,
            output: None,
        };
        self.track(MASTER, "Master", None, "audio notes", "master", "", &master)?;
        self.xml.close();

        let arrangement = self.id();
        self.xml.open("Arrangement", &[a("id", arrangement)]);
        let lanes = self.id();
        self.xml
            .open("Lanes", &[a("timeUnit", "beats"), a("id", lanes)]);
        for t in &s.tracks {
            self.lanes(t)?;
        }
        self.master_lanes();
        self.xml.close();
        self.markers();
        self.tempo(&tempo_id);
        self.xml.close();
        self.xml.empty("Scenes", &[]);
        self.xml.close();
        self.summary();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn track(
        &mut self,
        key: &str,
        name: &str,
        color: Option<&str>,
        content: &str,
        role: &str,
        destination: &str,
        t: &Track,
    ) -> Result<()> {
        let s = self.s;
        let ids = self.ids[key].clone();
        let mut attrs = vec![
            a("contentType", content),
            a("loaded", "true"),
            a("id", &ids.track),
            a("name", name),
        ];
        if let Some(color) = color.filter(|c| c.starts_with('#')) {
            attrs.push(a("color", color));
        }
        self.xml.open("Track", &attrs);
        let mut channel = vec![a("audioChannels", 2)];
        if let Some(dest) = self.ids.get(destination) {
            channel.push(a("destination", &dest.channel));
        }
        channel.push(a("role", role));
        channel.push(a("solo", t.solo));
        channel.push(a("id", &ids.channel));
        self.xml.open("Channel", &channel);
        let strip = s.strips.get(key).cloned().unwrap_or_default();
        let inserts: Vec<&Insert> = strip.inserts.iter().filter(|i| !i.is_empty()).collect();
        let instrument = (t.kind == "midi").then(|| match &strip.synth {
            Some(insert) => insert.clone(),
            None => Insert::new(
                String::new(),
                &format!("stock:{}", strip.instrument),
                &strip.instrument,
            ),
        });
        if instrument.is_some() || !inserts.is_empty() {
            self.xml.open("Devices", &[]);
            if let Some(insert) = &instrument {
                self.device(insert, "instrument", name)?;
            }
            for insert in inserts {
                self.device(insert, "audioFX", name)?;
            }
            self.xml.close();
        }
        self.xml.empty(
            "Mute",
            &[a("value", t.mute), a("id", &ids.mute), a("name", "Mute")],
        );
        self.xml.empty(
            "Pan",
            &[
                a("max", "1.0"),
                a("min", "0.0"),
                a("unit", "normalized"),
                a("value", num(pan_normalized(t.pan))),
                a("id", &ids.pan),
                a("name", "Pan"),
            ],
        );
        let sends: Vec<(String, Option<f32>)> = strip
            .sends
            .iter()
            .enumerate()
            .filter_map(|(index, send)| {
                let target = send.target(index)?;
                let channel = self.ids.get(target)?.channel.clone();
                Some((channel, send.level_db))
            })
            .collect();
        if !sends.is_empty() {
            self.xml.open("Sends", &[]);
            for (destination, level) in sends {
                let id = self.id();
                let volume = self.id();
                self.xml.open(
                    "Send",
                    &[
                        a("destination", destination),
                        a("type", "post"),
                        a("id", id),
                        a("name", "Send"),
                    ],
                );
                let gain = level.map_or(0.0, |db| 10f64.powf(db as f64 / 20.0));
                self.xml.empty(
                    "Volume",
                    &[
                        a("max", "1.0"),
                        a("min", "0.0"),
                        a("unit", "linear"),
                        a("value", num(gain)),
                        a("id", volume),
                        a("name", "Send level"),
                    ],
                );
                self.xml.close();
            }
            self.xml.close();
        }
        self.xml.empty(
            "Volume",
            &[
                a("max", num(fader_gain(1.0) as f64)),
                a("min", "0.0"),
                a("unit", "linear"),
                a("value", num(fader_gain(t.volume) as f64)),
                a("id", &ids.volume),
                a("name", "Volume"),
            ],
        );
        self.xml.close();
        self.xml.close();
        Ok(())
    }

    fn device(&mut self, insert: &Insert, role: &str, track: &str) -> Result<()> {
        let plugin = insert.plugin_id();
        let (element, device_id, external) = if let Some(id) = plugin.strip_prefix("clap:") {
            ("ClapPlugin", id.to_string(), true)
        } else if let Some(hex) = plugin.strip_prefix("vst3:") {
            ("Vst3Plugin", uuid_from_hex(hex), true)
        } else if let Some(id) = plugin.strip_prefix("au:") {
            ("AuPlugin", id.to_string(), true)
        } else {
            ("Device", plugin.clone(), false)
        };
        if external {
            self.plugins += 1;
        } else if plugin.starts_with("stock:") {
            self.stock += 1;
        }
        let id = self.id();
        let mut attrs = vec![a("deviceID", &device_id), a("deviceName", &insert.name)];
        attrs.push(a("deviceRole", role));
        if !external {
            attrs.push(a("deviceVendor", "ryolune"));
        }
        attrs.push(a("loaded", "true"));
        attrs.push(a("id", id));
        attrs.push(a("name", &insert.name));
        self.xml.open(element, &attrs);
        if external && !insert.params.is_empty() {
            self.xml.open("Parameters", &[]);
            for (&param, &value) in &insert.params {
                let id = self.id();
                self.xml.empty(
                    "RealParameter",
                    &[
                        a("unit", "linear"),
                        a("value", num(value)),
                        a("parameterID", param as i32),
                        a("id", id),
                        a("name", format!("Parameter {param}")),
                    ],
                );
            }
            self.xml.close();
        }
        let enabled = self.id();
        self.xml.empty(
            "Enabled",
            &[
                a("value", insert.state != "bypassed"),
                a("id", enabled),
                a("name", "On/Off"),
            ],
        );
        let state = if external {
            if insert.blob.is_empty() {
                None
            } else {
                let bytes = decode_blob(&insert.blob)?;
                match element {
                    "Vst3Plugin" => match vstpreset(&plugin, &bytes) {
                        Some(preset) => Some((preset, "vstpreset")),
                        None => {
                            self.report.approximated(format!(
                                "{} on {track}: its settings could not be written as a VST3 preset, so it opens at its defaults",
                                insert.name
                            ));
                            None
                        }
                    },
                    "AuPlugin" => Some((bytes, "aupreset")),
                    _ => Some((bytes, "clap-preset")),
                }
            }
        } else if insert.params.is_empty() && insert.blob.is_empty() {
            None
        } else {
            let state = serde_json::json!({
                "plugin": plugin,
                "params": insert.params,
                "blob": insert.blob,
            });
            Some((
                serde_json::to_vec_pretty(&state).map_err(|e| e.to_string())?,
                "json",
            ))
        };
        if let Some((bytes, extension)) = state {
            self.states += 1;
            let path = self.file_name("plugins", &insert.name, extension);
            self.xml.empty("State", &[a("path", &path)]);
            self.files.push((path, bytes));
        }
        self.xml.close();
        Ok(())
    }

    /// A unique path in the zip for a file named after `name`.
    fn file_name(&mut self, folder: &str, name: &str, extension: &str) -> String {
        let mut base: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || " -_.()".contains(c) {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .trim()
            .trim_matches('.')
            .chars()
            .take(80)
            .collect();
        if base.is_empty() {
            base = "file".into();
        }
        let mut candidate = format!("{folder}/{base}.{extension}");
        let mut n = 2;
        while !self.names.insert(candidate.to_lowercase()) {
            candidate = format!("{folder}/{base} {n}.{extension}");
            n += 1;
        }
        candidate
    }

    fn lanes(&mut self, t: &Track) -> Result<()> {
        let s = self.s;
        let ids = self.ids[&t.id].clone();
        let lanes = self.id();
        self.xml
            .open("Lanes", &[a("track", &ids.track), a("id", lanes)]);
        let clips: Vec<&Clip> = s.clips.iter().filter(|c| c.track_id == t.id).collect();
        if !clips.is_empty() {
            let id = self.id();
            self.xml.open("Clips", &[a("id", id)]);
            for clip in clips {
                self.clip(clip)?;
            }
            self.xml.close();
        }
        for lane in s
            .automation
            .iter()
            .filter(|l| l.target.track_id() == Some(&t.id))
        {
            match &lane.target {
                AutomationTarget::TrackVolume { .. } => {
                    self.points(lane, &ids.volume, "linear", |v| fader_gain(v as f32) as f64)
                }
                AutomationTarget::TrackPan { .. } => {
                    self.points(lane, &ids.pan, "normalized", |v| pan_normalized(v as f32))
                }
                _ => {}
            }
        }
        self.xml.close();
        Ok(())
    }

    fn master_lanes(&mut self) {
        let s = self.s;
        let Some(lane) = s
            .automation
            .iter()
            .find(|l| l.target == AutomationTarget::MasterVolume)
        else {
            return;
        };
        let ids = self.ids[MASTER].clone();
        let id = self.id();
        self.xml
            .open("Lanes", &[a("track", &ids.track), a("id", id)]);
        self.points(lane, &ids.volume, "linear", |v| fader_gain(v as f32) as f64);
        self.xml.close();
    }

    fn points(
        &mut self,
        lane: &AutomationLane,
        parameter: &str,
        unit: &str,
        convert: impl Fn(f64) -> f64,
    ) {
        if !lane.enabled || lane.points.is_empty() {
            if !lane.enabled {
                self.report
                    .dropped(format!("The switched-off automation lane “{}”", lane.name));
            }
            return;
        }
        let id = self.id();
        self.xml.open("Points", &[a("unit", unit), a("id", id)]);
        self.xml.empty("Target", &[a("parameter", parameter)]);
        let interpolation = match lane.interpolation {
            Interpolation::Linear => "linear",
            Interpolation::Step => "hold",
        };
        for point in &lane.points {
            self.xml.empty(
                "RealPoint",
                &[
                    a("time", num(point.beat)),
                    a("value", num(convert(point.value))),
                    a("interpolation", interpolation),
                ],
            );
        }
        self.xml.close();
    }

    fn clip(&mut self, clip: &Clip) -> Result<()> {
        let s = self.s;
        let bpb = s.beats_per_bar();
        let time = clip.start_bar * bpb;
        let duration = clip.length_bars * bpb;
        match &clip.data {
            ClipData::Midi { notes, controllers } => {
                self.xml.open(
                    "Clip",
                    &[
                        a("time", num(time)),
                        a("duration", num(duration)),
                        a("playStart", "0.0"),
                        a("name", &clip.name),
                    ],
                );
                let lanes = !controllers.is_empty();
                if lanes {
                    let id = self.id();
                    self.xml.open("Lanes", &[a("id", id)]);
                }
                let id = self.id();
                self.xml.open("Notes", &[a("id", id)]);
                for note in notes {
                    self.xml.empty(
                        "Note",
                        &[
                            a("time", num(note.start)),
                            a("duration", num(note.length)),
                            a("channel", note.channel),
                            a("key", note.pitch),
                            a("vel", num(note.velocity as f64 / 127.0)),
                        ],
                    );
                }
                self.xml.close();
                let mut groups: BTreeMap<(ControllerKind, Option<u8>, u8), Vec<&Controller>> =
                    BTreeMap::new();
                for c in controllers {
                    groups.entry(c.lane()).or_default().push(c);
                }
                for ((kind, number, channel), mut points) in groups {
                    points.sort_by(|a, b| a.time.total_cmp(&b.time));
                    let id = self.id();
                    self.xml
                        .open("Points", &[a("unit", "normalized"), a("id", id)]);
                    let mut target = vec![
                        a(
                            "expression",
                            match kind {
                                ControllerKind::Cc => "channelController",
                                ControllerKind::Bend => "pitchBend",
                                ControllerKind::Pressure => "channelPressure",
                                ControllerKind::PolyPressure => "polyPressure",
                            },
                        ),
                        a("channel", channel),
                    ];
                    match kind {
                        ControllerKind::Cc => target.push(a("controller", number.unwrap_or(0))),
                        ControllerKind::PolyPressure => target.push(a("key", number.unwrap_or(0))),
                        _ => {}
                    }
                    self.xml.empty("Target", &target);
                    for point in points {
                        let value = match kind {
                            ControllerKind::Bend => (point.value as f64 + 8192.0) / 16383.0,
                            _ => point.value as f64 / 127.0,
                        };
                        self.xml.empty(
                            "RealPoint",
                            &[
                                a("time", num(point.time)),
                                a("value", num(value.clamp(0.0, 1.0))),
                                a("interpolation", "hold"),
                            ],
                        );
                    }
                    self.xml.close();
                }
                if lanes {
                    self.xml.close();
                }
                self.xml.close();
            }
            ClipData::Audio {
                source_id,
                offset_seconds,
                fade_in,
                fade_out,
                fade_curve,
                gain_db,
            } => {
                let source = s
                    .sources
                    .get(source_id)
                    .ok_or_else(|| format!("Audio missing for clip {}", clip.name))?;
                let buffer = self
                    .library
                    .get(source_id)
                    .ok_or_else(|| format!("Audio missing for {}", source.name))?
                    .clone();
                let path = match self.audio_paths.get(source_id) {
                    Some(path) => path.clone(),
                    None => {
                        let path = self.file_name("audio", &source.name, "wav");
                        self.files.push((path.clone(), audio::encode_wav(&buffer)?));
                        self.audio_paths.insert(source_id.clone(), path.clone());
                        path
                    }
                };
                let seconds = s.bars_seconds(clip.start_bar, clip.start_bar + clip.length_bars);
                let mut attrs = vec![
                    a("time", num(time)),
                    a("duration", num(duration)),
                    a("contentTimeUnit", "seconds"),
                    a("playStart", num(*offset_seconds)),
                    a("playStop", num(offset_seconds + seconds)),
                ];
                if *fade_in > 0.0 || *fade_out > 0.0 {
                    attrs.push(a("fadeTimeUnit", "seconds"));
                    attrs.push(a("fadeInTime", num(*fade_in)));
                    attrs.push(a("fadeOutTime", num(*fade_out)));
                    if *fade_curve != FadeCurve::default() {
                        self.report.approximated(
                            "Fade curves: DAWproject has fade lengths only, so the other app uses its own curve",
                        );
                    }
                }
                attrs.push(a("name", &clip.name));
                if *gain_db != 0.0 {
                    self.report.dropped(
                        "Clip gain: DAWproject has no clip gain, so those clips play at 0 dB (set it again in the other app, or bounce them first)",
                    );
                }
                self.xml.open("Clip", &attrs);
                let id = self.id();
                self.xml.open(
                    "Audio",
                    &[
                        a("channels", 2),
                        a("duration", num(buffer.duration())),
                        a("sampleRate", buffer.sample_rate),
                        a("id", id),
                    ],
                );
                self.xml.empty("File", &[a("path", path)]);
                self.xml.close();
                self.xml.close();
            }
        }
        Ok(())
    }

    fn markers(&mut self) {
        let s = self.s;
        if s.markers.is_empty() {
            return;
        }
        let bpb = s.beats_per_bar();
        let id = self.id();
        self.xml.open("Markers", &[a("id", id)]);
        for marker in &s.markers {
            let mut attrs = vec![a("time", num(marker.bar * bpb)), a("name", &marker.name)];
            if let Some(color) = marker.color.as_ref().filter(|c| c.starts_with('#')) {
                attrs.push(a("color", color));
            }
            self.xml.empty("Marker", &attrs);
        }
        self.xml.close();
    }

    fn tempo(&mut self, tempo_id: &str) {
        let s = self.s;
        if s.tempo_changes.is_empty() {
            return;
        }
        let bpb = s.beats_per_bar();
        let id = self.id();
        self.xml
            .open("TempoAutomation", &[a("unit", "bpm"), a("id", id)]);
        self.xml.empty("Target", &[a("parameter", tempo_id)]);
        // A point glides to the next one when the next is a ramp.
        let interpolation = |next: Option<&TempoPoint>| {
            if next.is_some_and(|p| p.ramp) {
                "linear"
            } else {
                "hold"
            }
        };
        self.xml.empty(
            "RealPoint",
            &[
                a("time", "0.0"),
                a("value", num(s.transport.tempo)),
                a("interpolation", interpolation(s.tempo_changes.first())),
            ],
        );
        for (i, point) in s.tempo_changes.iter().enumerate() {
            self.xml.empty(
                "RealPoint",
                &[
                    a("time", num(point.bar * bpb)),
                    a("value", num(point.bpm)),
                    a("interpolation", interpolation(s.tempo_changes.get(i + 1))),
                ],
            );
        }
        self.xml.close();
    }

    fn summary(&mut self) {
        let s = self.s;
        let midi = s.tracks.iter().filter(|t| t.kind == "midi").count();
        let audio = s.tracks.iter().filter(|t| t.kind == "audio").count();
        let buses = s.tracks.iter().filter(|t| t.is_bus()).count();
        let mut tracks = vec![];
        if midi > 0 {
            tracks.push(count(midi, "instrument track", "instrument tracks"));
        }
        if audio > 0 {
            tracks.push(count(audio, "audio track", "audio tracks"));
        }
        if buses > 0 {
            tracks.push(count(buses, "bus", "buses"));
        }
        if !tracks.is_empty() {
            self.report.kept(format!(
                "{}, with volume, pan, mute, solo, routing and sends",
                tracks.join(", ")
            ));
        }
        self.report
            .kept("The two effect returns (A · Reverb and B · Delay) and the Stereo Out");
        let midi_clips: Vec<&Clip> = s
            .clips
            .iter()
            .filter(|c| matches!(c.data, ClipData::Midi { .. }))
            .collect();
        if !midi_clips.is_empty() {
            let (notes, controllers) =
                midi_clips
                    .iter()
                    .fold((0, 0), |(n, c), clip| match &clip.data {
                        ClipData::Midi { notes, controllers } => {
                            (n + notes.len(), c + controllers.len())
                        }
                        _ => (n, c),
                    });
            let mut line = format!(
                "{} with {}",
                count(midi_clips.len(), "MIDI clip", "MIDI clips"),
                count(notes, "note", "notes")
            );
            if controllers > 0 {
                let _ = write!(
                    line,
                    " and {}",
                    count(controllers, "controller point", "controller points")
                );
            }
            self.report.kept(line);
        }
        let audio_clips = s.clips.len() - midi_clips.len();
        if audio_clips > 0 {
            self.report.kept(format!(
                "{} with {}, fades included",
                count(audio_clips, "audio clip", "audio clips"),
                count(self.audio_paths.len(), "audio file", "audio files")
            ));
        }
        if !s.markers.is_empty() {
            self.report
                .kept(count(s.markers.len(), "marker", "markers"));
        }
        self.report.kept(if s.tempo_changes.is_empty() {
            format!(
                "Tempo {} BPM in {}/{}",
                s.transport.tempo,
                s.transport.time_signature.numerator,
                s.transport.time_signature.denominator
            )
        } else {
            format!(
                "Tempo {} BPM with {}, in {}/{}",
                s.transport.tempo,
                count(s.tempo_changes.len(), "tempo change", "tempo changes"),
                s.transport.time_signature.numerator,
                s.transport.time_signature.denominator
            )
        });
        let lanes = s
            .automation
            .iter()
            .filter(|l| {
                l.enabled
                    && matches!(
                        l.target,
                        AutomationTarget::TrackVolume { .. }
                            | AutomationTarget::TrackPan { .. }
                            | AutomationTarget::MasterVolume
                    )
            })
            .count();
        if lanes > 0 {
            self.report.kept(count(
                lanes,
                "volume or pan automation lane",
                "volume and pan automation lanes",
            ));
        }
        let plugin_lanes = s
            .automation
            .iter()
            .filter(|l| matches!(l.target, AutomationTarget::PluginParameter { .. }))
            .count();
        if plugin_lanes > 0 {
            self.report.dropped(format!(
                "{} on plugin parameters: the other app gets each plugin's settings, not their automation",
                count(plugin_lanes, "automation lane", "automation lanes")
            ));
        }
        if self.plugins > 0 {
            self.report.kept(format!(
                "{} by id, {} with their settings",
                count(
                    self.plugins,
                    "CLAP, VST3 or AU plugin",
                    "CLAP, VST3 and AU plugins"
                ),
                count(self.states, "state", "states")
            ));
        }
        if self.stock > 0 {
            self.report.approximated(format!(
                "{} of ryolune's own are named devices with their settings: another app keeps their place and name but cannot play them, so choose its own there",
                count(self.stock, "instrument or effect", "instruments and effects")
            ));
        }
    }
}

/// The canonical 8-4-4-4-12 form of a VST3 class id written as 32 hex digits.
fn uuid_from_hex(hex: &str) -> String {
    if hex.len() != 32 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return hex.to_string();
    }
    let h = hex.to_ascii_uppercase();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}
fn hex_from_uuid(uuid: &str) -> Option<String> {
    let hex: String = uuid
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .chars()
        .filter(|c| *c != '-')
        .collect();
    (hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| hex.to_ascii_lowercase())
}

/// ryolune keeps a VST3 state as the component state then the controller state, each after
/// its length (u32, little-endian). A `.vstpreset` is Steinberg's container for the same two
/// chunks: a header with the class id, the data, and a list of chunks.
fn vstpreset(plugin: &str, blob: &[u8]) -> Option<Vec<u8>> {
    let hex = plugin.strip_prefix("vst3:")?;
    if hex.len() != 32 {
        return None;
    }
    let part = |at: usize| -> Option<(usize, &[u8])> {
        let len = u32::from_le_bytes(blob.get(at..at + 4)?.try_into().ok()?) as usize;
        Some((at + 4 + len, blob.get(at + 4..at + 4 + len)?))
    };
    let (next, component) = part(0)?;
    let controller = part(next).map(|(_, c)| c).unwrap_or_default();
    let mut out = Vec::with_capacity(blob.len() + 100);
    out.extend_from_slice(b"VST3");
    out.extend_from_slice(&1i32.to_le_bytes());
    out.extend_from_slice(hex.to_ascii_uppercase().as_bytes());
    let list_at = 48 + component.len() + controller.len();
    out.extend_from_slice(&(list_at as i64).to_le_bytes());
    out.extend_from_slice(component);
    out.extend_from_slice(controller);
    out.extend_from_slice(b"List");
    let chunks: Vec<(&[u8; 4], usize, usize)> = [
        (b"Comp", 48, component.len()),
        (b"Cont", 48 + component.len(), controller.len()),
    ]
    .into_iter()
    .filter(|(_, _, len)| *len > 0)
    .collect();
    out.extend_from_slice(&(chunks.len() as i32).to_le_bytes());
    for (id, offset, size) in chunks {
        out.extend_from_slice(id);
        out.extend_from_slice(&(offset as i64).to_le_bytes());
        out.extend_from_slice(&(size as i64).to_le_bytes());
    }
    Some(out)
}
/// A `.vstpreset` as ryolune's VST3 state: the class id (32 hex digits, lowercase) and the
/// component then controller state, each after its length.
fn from_vstpreset(bytes: &[u8]) -> Option<(String, Vec<u8>)> {
    if bytes.get(0..4)? != b"VST3" {
        return None;
    }
    let class = std::str::from_utf8(bytes.get(8..40)?)
        .ok()?
        .to_ascii_lowercase();
    let list = i64::from_le_bytes(bytes.get(40..48)?.try_into().ok()?);
    let list = usize::try_from(list).ok()?;
    if bytes.get(list..list + 4)? != b"List" {
        return None;
    }
    let entries = i32::from_le_bytes(bytes.get(list + 4..list + 8)?.try_into().ok()?);
    let (mut component, mut controller) = (&[][..], &[][..]);
    for i in 0..usize::try_from(entries).ok()?.min(64) {
        let at = list + 8 + i * 20;
        let id = bytes.get(at..at + 4)?;
        let offset = usize::try_from(i64::from_le_bytes(
            bytes.get(at + 4..at + 12)?.try_into().ok()?,
        ))
        .ok()?;
        let size = usize::try_from(i64::from_le_bytes(
            bytes.get(at + 12..at + 20)?.try_into().ok()?,
        ))
        .ok()?;
        let data = bytes.get(offset..offset.checked_add(size)?)?;
        match id {
            b"Comp" => component = data,
            b"Cont" => controller = data,
            _ => {}
        }
    }
    if component.is_empty() {
        return None;
    }
    let mut blob = Vec::with_capacity(component.len() + controller.len() + 8);
    blob.extend_from_slice(&(component.len() as u32).to_le_bytes());
    blob.extend_from_slice(component);
    blob.extend_from_slice(&(controller.len() as u32).to_le_bytes());
    blob.extend_from_slice(controller);
    Some((class, blob))
}

// ---------------------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------------------

/// Open a `.dawproject` as a new song. `catalog` is the installed plugins: a project's
/// plugins load when they are installed here and are named in the report when not.
pub fn import(path: &Path, catalog: &[Descriptor]) -> Result<Imported> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Imported song")
        .to_string();
    let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
    import_from(file, &name, &base, catalog)
}

/// Read a DAWproject from any zip reader. `name` names the song when the project has no
/// title; `base` is where external audio files are looked for.
pub fn import_from<R: Read + Seek>(
    reader: R,
    name: &str,
    base: &Path,
    catalog: &[Descriptor],
) -> Result<Imported> {
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| format!("This is not a DAWproject file (it is not a zip archive): {e}"))?;
    let xml = entry(&mut archive, PROJECT, MAX_XML)?
        .ok_or("This zip has no project.xml, so it is not a DAWproject file")?;
    let xml = text(xml)?;
    let title = entry(&mut archive, METADATA, MAX_XML)?
        .and_then(|bytes| text(bytes).ok())
        .and_then(|meta| {
            let doc = roxmltree::Document::parse(&meta).ok()?;
            let title = doc
                .root_element()
                .children()
                .find(|n| n.has_tag_name("Title"))?
                .text()?
                .trim()
                .to_string();
            (!title.is_empty()).then_some(title)
        });
    let doc = roxmltree::Document::parse(&xml)
        .map_err(|e| format!("The project.xml cannot be read: {e}"))?;
    let root = doc.root_element();
    if !root.has_tag_name("Project") {
        return Err("The project.xml holds no DAWproject Project".into());
    }
    let mut reader = Reader {
        archive,
        base: base.to_path_buf(),
        catalog,
        by_id: HashMap::new(),
        report: Report::new("dawproject"),
        session: super::blank(title.as_deref().unwrap_or(name)),
        library: Library::new(),
        map: TempoMap::constant(120.0),
        channels: HashMap::new(),
        tracks: HashMap::new(),
        params: HashMap::new(),
        sources: HashMap::new(),
        kinds: HashMap::new(),
        notes: HashMap::new(),
        tally: Tally::default(),
    };
    for node in root.descendants().filter(Node::is_element) {
        if let Some(id) = node.attribute("id") {
            reader.by_id.insert(id.to_string(), node);
        }
    }
    reader.read(root)?;
    let Reader {
        session,
        library,
        report,
        ..
    } = reader;
    Ok(Imported {
        session,
        library,
        report,
    })
}

fn text(bytes: Vec<u8>) -> Result<String> {
    let bytes = bytes
        .strip_prefix(&[0xef, 0xbb, 0xbf])
        .map(<[u8]>::to_vec)
        .unwrap_or(bytes);
    String::from_utf8(bytes).map_err(|_| "The project.xml is not UTF-8 text".to_string())
}

/// One file of the zip, at most `limit` bytes; `None` when it is not there.
fn entry<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>> {
    let name = name.trim_start_matches("./").trim_start_matches('/');
    let mut file = match archive.by_name(name) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(format!("{name}: {e}")),
    };
    if file.size() > limit {
        return Err(format!(
            "{name} is larger than ryolune reads ({} MiB)",
            limit >> 20
        ));
    }
    let mut bytes = Vec::with_capacity(file.size() as usize);
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{name}: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("{name} is larger than ryolune reads"));
    }
    Ok(Some(bytes))
}

/// Where a channel of the project went.
#[derive(Clone, Debug, PartialEq)]
enum Dest {
    /// A ryolune track (or bus track) by id.
    Track(String),
    Master,
    Aux(&'static str),
}
/// What an automatable parameter of the project is in ryolune.
#[derive(Clone, Debug)]
enum Param {
    Volume(Dest),
    Pan(Dest),
    Tempo,
    Mute,
    Send,
    Plugin,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Unit {
    Beats,
    Seconds,
}
fn unit_of(node: Node, inherited: Unit) -> Unit {
    match node.attribute("timeUnit") {
        Some("seconds") => Unit::Seconds,
        Some("beats") => Unit::Beats,
        _ => inherited,
    }
}

/// Where a timeline's times land in the song: content time `zero` sits at `origin`
/// (absolute beats), and only what falls between `lo` and `hi` (absolute beats) plays.
#[derive(Clone, Copy, Debug)]
struct Frame {
    origin: f64,
    zero: f64,
    unit: Unit,
    lo: f64,
    hi: f64,
    /// The arrangement itself: seconds there are song seconds, through the tempo map.
    root: bool,
}

/// What a clip's content gathered: notes and controllers in absolute beats.
#[derive(Default)]
struct Gathered {
    midi: bool,
    notes: Vec<Note>,
    controllers: Vec<Controller>,
}

#[derive(Default)]
struct Tally {
    clips_off_track: usize,
    disabled_clips: usize,
    loops: usize,
    stretched: usize,
    expressions: usize,
    note_expressions: usize,
    ramps: usize,
    automation: BTreeMap<&'static str, usize>,
    launcher: usize,
    video: usize,
    negative_offset: usize,
    crossfades: usize,
    sends_dropped: usize,
    inserts_dropped: usize,
    plugins_missing: Vec<String>,
    plugins: usize,
    states_failed: usize,
    clips_on_bus: usize,
}

struct Reader<'a, 'input, R: Read + Seek> {
    archive: zip::ZipArchive<R>,
    base: PathBuf,
    catalog: &'a [Descriptor],
    by_id: HashMap<String, Node<'a, 'input>>,
    report: Report,
    session: Session,
    library: Library,
    map: TempoMap,
    /// Channel ids to where they went.
    channels: HashMap<String, Dest>,
    /// Track ids of the project (the `track` attribute of lanes) to ryolune track ids.
    tracks: HashMap<String, Dest>,
    params: HashMap<String, Param>,
    /// Audio file paths to the source made from them (`None`: missing or unreadable).
    sources: HashMap<String, Option<String>>,
    /// Per ryolune track: holds MIDI clips, holds audio clips.
    kinds: HashMap<String, (bool, bool)>,
    /// Plugins left out per ryolune track, for the track's note.
    notes: HashMap<String, Vec<String>>,
    tally: Tally,
}

fn el<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|n| n.has_tag_name(name))
}
fn els<'a, 'i: 'a>(node: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    node.children().filter(move |n| n.has_tag_name(name))
}
fn number(node: Node, key: &str) -> Option<f64> {
    let text = node.attribute(key)?.trim();
    match text {
        "inf" | "INF" => Some(f64::INFINITY),
        "-inf" | "-INF" => Some(f64::NEG_INFINITY),
        _ => text.parse::<f64>().ok().filter(|v| v.is_finite()),
    }
}
fn boolean(node: Node, key: &str) -> Option<bool> {
    match node.attribute(key)?.trim() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}
fn css_color(text: Option<&str>) -> Option<String> {
    let text = text?.trim();
    let hex = text.strip_prefix('#')?;
    ((hex.len() == 6 || hex.len() == 8) && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| format!("#{}", &hex[..6].to_ascii_lowercase()))
}
/// A real parameter's value as a linear gain.
fn gain_of(node: Node) -> Option<f64> {
    let value = number(node, "value")?;
    Some(match node.attribute("unit") {
        Some("decibel") => 10f64.powf(value / 20.0),
        Some("normalized") => {
            let (min, max) = (
                number(node, "min").unwrap_or(0.0),
                number(node, "max").unwrap_or(1.0),
            );
            min + value * (max - min)
        }
        Some("percent") => value / 100.0,
        _ => value,
    })
}
/// A pan parameter's value as ryolune's -100 to 100.
fn pan_of(node: Node, value: f64) -> f64 {
    let (min, max) = match node.attribute("unit") {
        Some("normalized") | None => (
            number(node, "min").unwrap_or(0.0),
            number(node, "max").unwrap_or(1.0),
        ),
        Some("percent") => (
            number(node, "min").unwrap_or(-100.0),
            number(node, "max").unwrap_or(100.0),
        ),
        _ => (
            number(node, "min").unwrap_or(-1.0),
            number(node, "max").unwrap_or(1.0),
        ),
    };
    if max <= min {
        return 0.0;
    }
    (((value - min) / (max - min)) * 200.0 - 100.0).clamp(-100.0, 100.0)
}

impl<'a, 'input, R: Read + Seek> Reader<'a, 'input, R> {
    fn bpb(&self) -> f64 {
        self.session.beats_per_bar()
    }

    fn read(&mut self, root: Node<'a, 'input>) -> Result<()> {
        if let Some(app) = el(root, "Application") {
            let name = app.attribute("name").unwrap_or("another app");
            let version = app.attribute("version").unwrap_or("");
            self.report
                .kept(format!("Read from {name} {version}").trim().to_string());
        }
        let arrangement = el(root, "Arrangement");
        self.transport(root, arrangement)?;
        if let Some(structure) = el(root, "Structure") {
            self.structure(structure)?;
        }
        if let Some(arrangement) = arrangement {
            let unit = el(arrangement, "Lanes")
                .map(|l| unit_of(l, Unit::Beats))
                .unwrap_or(Unit::Beats);
            let frame = Frame {
                origin: 0.0,
                zero: 0.0,
                unit,
                lo: 0.0,
                hi: f64::INFINITY,
                root: true,
            };
            if let Some(lanes) = el(arrangement, "Lanes") {
                self.timeline(lanes, None, frame)?;
            }
            for markers in els(arrangement, "Markers") {
                self.markers(markers, frame);
            }
        }
        if let Some(scenes) = el(root, "Scenes") {
            self.tally.launcher += scenes
                .descendants()
                .filter(|n| n.has_tag_name("Clip"))
                .count();
        }
        self.finish()
    }

    // --- Tempo and meter -----------------------------------------------------------

    fn transport(
        &mut self,
        root: Node<'a, 'input>,
        arrangement: Option<Node<'a, 'input>>,
    ) -> Result<()> {
        let transport = el(root, "Transport");
        let mut tempo = 120.0;
        if let Some(node) = transport.and_then(|t| el(t, "Tempo")) {
            if let Some(id) = node.attribute("id") {
                self.params.insert(id.into(), Param::Tempo);
            }
            if let Some(v) = number(node, "value") {
                tempo = v;
            }
        }
        if let Some(node) = transport.and_then(|t| el(t, "TimeSignature")) {
            let numerator = number(node, "numerator").unwrap_or(4.0);
            let denominator = number(node, "denominator").unwrap_or(4.0);
            let valid = (1.0..=32.0).contains(&numerator)
                && numerator.fract() == 0.0
                && [1.0, 2.0, 4.0, 8.0, 16.0, 32.0].contains(&denominator);
            if valid {
                self.session.transport.time_signature = TimeSignature {
                    numerator: numerator as u32,
                    denominator: denominator as u32,
                };
            } else {
                self.report.approximated(format!(
                    "The meter {numerator}/{denominator} is not one ryolune plays: the song is in 4/4"
                ));
            }
        }
        if let Some(meters) = arrangement.and_then(|a| el(a, "TimeSignatureAutomation")) {
            let changes = els(meters, "TimeSignaturePoint")
                .filter(|p| number(*p, "time").is_some_and(|t| t > 0.0))
                .count();
            if changes > 0 {
                self.report.approximated(format!(
                    "{}: ryolune keeps one meter for the whole song, the first one",
                    count(changes, "meter change", "meter changes")
                ));
            }
        }
        let bpb = self.bpb();
        let mut points: Vec<(f64, f64, bool)> = vec![];
        if let Some(auto) = arrangement.and_then(|a| el(a, "TempoAutomation")) {
            let seconds = auto.attribute("timeUnit") == Some("seconds")
                || (auto.attribute("timeUnit").is_none()
                    && arrangement
                        .and_then(|a| el(a, "Lanes"))
                        .is_some_and(|l| l.attribute("timeUnit") == Some("seconds")));
            let mut raw: Vec<(f64, f64, bool)> = auto
                .children()
                .filter(|n| n.has_tag_name("RealPoint") || n.has_tag_name("Point"))
                .filter_map(|p| {
                    Some((
                        number(p, "time")?,
                        number(p, "value")?,
                        p.attribute("interpolation") == Some("linear"),
                    ))
                })
                .collect();
            raw.sort_by(|a, b| a.0.total_cmp(&b.0));
            if seconds {
                // Seconds to beats, segment by segment, at the tempo the points describe.
                let mut beat = 0.0;
                let mut last: Option<(f64, f64, bool)> = None;
                for (time, value, linear) in raw {
                    if let Some((t0, v0, l0)) = last {
                        let average = if l0 { (v0 + value) / 2.0 } else { v0 };
                        beat += (time - t0).max(0.0) * average / 60.0;
                        if l0 {
                            self.tally.ramps += 1;
                        }
                    } else {
                        beat = time * value / 60.0;
                    }
                    points.push((beat, value, linear));
                    last = Some((time, value, linear));
                }
                if self.tally.ramps > 0 {
                    self.report.approximated("Tempo ramps written in seconds were placed by their average tempo, so they may land a little off the beat");
                }
                self.tally.ramps = 0;
            } else {
                points = raw;
            }
        }
        let mut changes: Vec<TempoPoint> = vec![];
        let mut clamped = false;
        let mut clamp = |bpm: f64| {
            if crate::tempo::valid_bpm(bpm) {
                bpm
            } else {
                clamped = true;
                bpm.clamp(crate::tempo::MIN_BPM, crate::tempo::MAX_BPM)
            }
        };
        // Two points on one beat: the later one counts.
        points.dedup_by(|later, earlier| {
            if (later.0 - earlier.0).abs() < 1e-9 {
                *earlier = *later;
                true
            } else {
                false
            }
        });
        // Before its first point a lane holds the first point's value.
        let start = clamp(points.first().map_or(tempo, |p| p.1));
        let mut current = start;
        let mut previous_linear = points.first().is_some_and(|p| p.2);
        for &(beat, value, linear) in points.iter().skip(1) {
            let bpm = clamp(value);
            if (bpm - current).abs() > 1e-9 {
                changes.push(TempoPoint {
                    bar: beat / bpb,
                    bpm,
                    ramp: previous_linear,
                });
                current = bpm;
            }
            previous_linear = linear;
        }
        if clamped {
            self.report.approximated(format!(
                "Tempos outside {}-{} BPM were brought inside it",
                crate::tempo::MIN_BPM,
                crate::tempo::MAX_BPM
            ));
        }
        changes.truncate(crate::tempo::MAX_POINTS);
        self.session.transport.tempo = start;
        self.session.tempo_changes = changes;
        self.map = self.session.tempo_map();
        Ok(())
    }

    // --- Tracks and the mix --------------------------------------------------------

    fn structure(&mut self, structure: Node<'a, 'input>) -> Result<()> {
        // Tracks first (so routing can point anywhere), then every channel's mix.
        let mut pending: Vec<(Dest, Node<'a, 'input>, Option<String>, String)> = vec![];
        for node in structure.children().filter(Node::is_element) {
            if node.has_tag_name("Track") {
                self.track(node, None, &mut pending)?;
            } else if node.has_tag_name("Channel") {
                self.loose_channel(node, &mut pending);
            }
        }
        for (strip, channel, parent, name) in pending {
            self.mix(&strip, channel, parent.as_deref(), &name)?;
        }
        Ok(())
    }

    fn new_track(&mut self, node: Node, kind: &str, name: &str) -> String {
        let index = self.session.tracks.len();
        let mut track = crate::control::new_track(
            &self.session,
            kind,
            Some(name.chars().take(120).collect()),
            css_color(node.attribute("color"))
                .unwrap_or_else(|| crate::control::TRACK_PALETTE[index % 8].into()),
        );
        if track.name.trim().is_empty() {
            track.name = format!("Track {}", index + 1);
        }
        let id = track.id.clone();
        self.session.tracks.push(track);
        id
    }

    fn track(
        &mut self,
        node: Node<'a, 'input>,
        parent: Option<&str>,
        pending: &mut Vec<(Dest, Node<'a, 'input>, Option<String>, String)>,
    ) -> Result<()> {
        let name = node.attribute("name").unwrap_or("").to_string();
        let channel = el(node, "Channel");
        let role = channel
            .and_then(|c| c.attribute("role"))
            .unwrap_or("regular");
        let content: Vec<&str> = node
            .attribute("contentType")
            .unwrap_or("")
            .split_whitespace()
            .collect();
        let children: Vec<Node> = els(node, "Track").collect();
        let channel_id = channel.and_then(|c| c.attribute("id"));
        let aux = channel_id
            .and_then(|id| AUX_IDS.iter().find(|(_, fixed)| *fixed == id))
            .map(|(bus, _)| *bus);
        let strip = if role == "master" {
            Some(Dest::Master)
        } else if let Some(bus) = aux {
            Some(Dest::Aux(bus))
        } else if role == "vca" {
            self.report.dropped(format!(
                "The VCA fader “{name}”: ryolune has none, so the tracks it controlled keep their own levels"
            ));
            None
        } else if channel.is_none() && !children.is_empty() {
            if !name.is_empty() {
                self.report.approximated(format!(
                    "The folder “{name}” became its tracks, side by side"
                ));
            }
            None
        } else if role == "effect"
            || role == "submix"
            || content.contains(&"tracks")
            || !children.is_empty()
        {
            if self.session.tracks.iter().filter(|t| t.is_bus()).count() >= MAX_BUS_TRACKS {
                self.report.dropped(format!(
                    "The bus “{name}”: a song holds at most {MAX_BUS_TRACKS} buses"
                ));
                None
            } else {
                Some(Dest::Track(self.new_track(node, "bus", &name)))
            }
        } else if self.session.tracks.len() >= 120 {
            self.report.dropped(format!(
                "The track “{name}”: ryolune holds up to 128 tracks"
            ));
            None
        } else {
            let midi = content.contains(&"notes")
                || (!content.contains(&"audio")
                    && channel.and_then(|c| el(c, "Devices")).is_some_and(|d| {
                        d.children()
                            .any(|n| n.attribute("deviceRole") == Some("instrument"))
                    }));
            Some(Dest::Track(self.new_track(
                node,
                if midi { "midi" } else { "audio" },
                &name,
            )))
        };
        if let Some(strip) = &strip {
            if let Some(id) = node.attribute("id") {
                self.tracks.insert(id.into(), strip.clone());
            }
            if let Some(channel) = channel {
                if let Some(id) = channel_id {
                    self.channels.insert(id.into(), strip.clone());
                }
                pending.push((
                    strip.clone(),
                    channel,
                    parent.map(String::from),
                    name.clone(),
                ));
            }
        }
        let group = match &strip {
            Some(Dest::Track(id))
                if self
                    .session
                    .tracks
                    .iter()
                    .any(|t| &t.id == id && t.is_bus()) =>
            {
                Some(id.clone())
            }
            _ => parent.map(String::from),
        };
        for child in children {
            self.track(child, group.as_deref(), pending)?;
        }
        Ok(())
    }

    fn loose_channel(
        &mut self,
        channel: Node<'a, 'input>,
        pending: &mut Vec<(Dest, Node<'a, 'input>, Option<String>, String)>,
    ) {
        let name = channel.attribute("name").unwrap_or("").to_string();
        let id = channel.attribute("id");
        let strip = match channel.attribute("role") {
            Some("master") => Dest::Master,
            _ if id.is_some_and(|id| AUX_IDS.iter().any(|(_, f)| *f == id)) => {
                Dest::Aux(AUX_IDS.iter().find(|(_, f)| Some(*f) == id).unwrap().0)
            }
            _ => Dest::Track(self.new_track(
                channel,
                "bus",
                if name.is_empty() { "Bus" } else { &name },
            )),
        };
        if let Some(id) = id {
            self.channels.insert(id.into(), strip.clone());
        }
        pending.push((strip, channel, None, name));
    }

    fn mix(
        &mut self,
        strip: &Dest,
        channel: Node<'a, 'input>,
        parent: Option<&str>,
        name: &str,
    ) -> Result<()> {
        let volume = el(channel, "Volume");
        let pan = el(channel, "Pan");
        let mute = el(channel, "Mute");
        for (node, param) in [
            (volume, Param::Volume(strip.clone())),
            (pan, Param::Pan(strip.clone())),
            (mute, Param::Mute),
        ] {
            if let Some(id) = node.and_then(|n| n.attribute("id")) {
                self.params.insert(id.into(), param);
            }
        }
        let gain = volume.and_then(gain_of);
        if gain.is_some_and(|g| g > fader_gain(1.0) as f64 * 1.001) {
            self.report.approximated(format!(
                "“{name}” was louder than +6 dB, ryolune's highest fader setting"
            ));
        }
        let fader = gain.map(|g| fader_position(g).clamp(0.0, 1.0) as f32);
        let pan = pan.and_then(|p| Some(pan_of(p, number(p, "value")?) as f32));
        let mute = mute.and_then(|m| boolean(m, "value")).unwrap_or(false);
        let solo = boolean(channel, "solo").unwrap_or(false);
        match strip {
            Dest::Master => {
                if let Some(fader) = fader {
                    self.session.master_volume = fader;
                }
                if mute {
                    self.report
                        .dropped("The Stereo Out's mute: ryolune's Stereo Out cannot be muted");
                }
                let (_, inserts) = self.devices(channel, MASTER, "the Stereo Out", false)?;
                self.session
                    .strips
                    .entry(MASTER.into())
                    .or_default()
                    .inserts = inserts;
            }
            Dest::Aux(bus) => {
                let (_, inserts) = self.devices(channel, bus, bus_name(bus), false)?;
                self.session
                    .strips
                    .entry((*bus).into())
                    .or_default()
                    .inserts = inserts;
            }
            Dest::Track(id) => {
                let is_bus = self
                    .session
                    .tracks
                    .iter()
                    .any(|t| &t.id == id && t.is_bus());
                let midi = self
                    .session
                    .tracks
                    .iter()
                    .any(|t| &t.id == id && t.kind == "midi");
                let output = match channel.attribute("destination") {
                    Some(dest) => match self.channels.get(dest).cloned() {
                        None | Some(Dest::Master) => None,
                        Some(Dest::Track(target)) => {
                            let target_is_bus = self
                                .session
                                .tracks
                                .iter()
                                .any(|t| t.id == target && t.is_bus());
                            if target_is_bus && !is_bus {
                                Some(target)
                            } else {
                                self.report.approximated(format!(
                                    "“{name}” fed another {}: ryolune routes a bus to the Stereo Out, so it goes there",
                                    if is_bus { "bus" } else { "track" }
                                ));
                                None
                            }
                        }
                        Some(Dest::Aux(_)) => {
                            self.report.approximated(format!(
                                "“{name}” fed an effect return directly: it goes to the Stereo Out"
                            ));
                            None
                        }
                    },
                    None => parent.filter(|_| !is_bus).map(String::from),
                };
                let sends = self.sends(channel, is_bus, name);
                let (instrument, inserts) = self.devices(channel, id, name, midi)?;
                let mut strip = crate::model::Strip {
                    inserts,
                    sends,
                    ..Default::default()
                };
                match instrument {
                    Some(Instrument::Stock(name)) => strip.instrument = name,
                    Some(Instrument::Plugin(insert)) => strip.synth = Some(insert),
                    None => {}
                }
                if let Some(track) = self.session.tracks.iter_mut().find(|t| &t.id == id) {
                    if let Some(fader) = fader {
                        track.volume = fader;
                    }
                    if let Some(pan) = pan {
                        track.pan = pan;
                    }
                    track.mute = mute;
                    track.solo = solo;
                    track.output = output;
                }
                self.session.strips.insert(id.clone(), strip);
            }
        }
        Ok(())
    }

    fn sends(&mut self, channel: Node<'a, 'input>, from_bus: bool, name: &str) -> Vec<Send> {
        let mut out = vec![];
        let Some(sends) = el(channel, "Sends") else {
            return out;
        };
        for send in els(sends, "Send") {
            if let Some(id) = el(send, "Volume").and_then(|v| v.attribute("id")) {
                self.params.insert(id.into(), Param::Send);
            }
            let target = send
                .attribute("destination")
                .and_then(|d| self.channels.get(d).cloned());
            let bus = match target {
                Some(Dest::Aux(bus)) => bus.to_string(),
                Some(Dest::Track(id))
                    if !from_bus
                        && self.session.tracks.iter().any(|t| t.id == id && t.is_bus()) =>
                {
                    id
                }
                _ => {
                    self.tally.sends_dropped += 1;
                    continue;
                }
            };
            if out.len() >= MAX_SENDS {
                self.tally.sends_dropped += 1;
                continue;
            }
            let enabled = el(send, "Enable")
                .and_then(|e| boolean(e, "value"))
                .unwrap_or(true);
            let gain = el(send, "Volume").and_then(gain_of).unwrap_or(0.0);
            let level_db = (enabled && gain > 0.0).then(|| {
                let db = 20.0 * gain.log10();
                if db > 0.0 {
                    self.report.approximated(format!(
                        "A send on “{name}” above 0 dB is at 0 dB, ryolune's highest send level"
                    ));
                }
                db.clamp(-100.0, 0.0) as f32
            });
            if send.attribute("type") == Some("pre") {
                self.report
                    .approximated("Pre-fader sends are post-fader in ryolune");
            }
            out.push(Send {
                level_db,
                name: String::new(),
                bus: Some(bus),
            });
        }
        out
    }

    /// The channel's instrument and effects. Plugins load when installed here; the others
    /// are named in the report and in the track's note.
    fn devices(
        &mut self,
        channel: Node<'a, 'input>,
        key: &str,
        name: &str,
        instrument_track: bool,
    ) -> Result<(Option<Instrument>, Vec<Insert>)> {
        let mut instrument = None;
        let mut inserts = vec![];
        let Some(devices) = el(channel, "Devices") else {
            return Ok((None, inserts));
        };
        for device in devices.children().filter(Node::is_element) {
            for parameters in els(device, "Parameters") {
                for p in parameters.children().filter(Node::is_element) {
                    if let Some(id) = p.attribute("id") {
                        self.params.insert(id.into(), Param::Plugin);
                    }
                }
            }
            let role = device.attribute("deviceRole").unwrap_or("audioFX");
            let label = device
                .attribute("deviceName")
                .or_else(|| device.attribute("name"))
                .unwrap_or("Device")
                .to_string();
            if role == "noteFX" {
                self.report.dropped(format!(
                    "The note effect {label} on {name}: ryolune has no note effects"
                ));
                continue;
            }
            if role == "instrument" && !instrument_track {
                if !matches!(key, MASTER | BUS_A | BUS_B) {
                    self.report
                        .dropped(format!("The instrument {label} on the audio track {name}"));
                }
                continue;
            }
            let Some(mut insert) = self.plugin(device, &label, name, key)? else {
                continue;
            };
            let enabled = el(device, "Enabled")
                .and_then(|e| boolean(e, "value"))
                .unwrap_or(true);
            insert.state = if enabled { "active" } else { "bypassed" }.into();
            if role == "instrument" {
                if instrument.is_some() {
                    self.report.dropped(format!(
                        "A second instrument ({label}) on {name}: a ryolune track plays one"
                    ));
                    continue;
                }
                let stock = insert
                    .plugin
                    .strip_prefix("stock:")
                    .filter(|n| INSTRUMENTS.contains(n))
                    .map(String::from);
                instrument = Some(match stock {
                    Some(stock)
                        if insert.params.is_empty() && insert.blob.is_empty() && enabled =>
                    {
                        Instrument::Stock(stock)
                    }
                    _ => Instrument::Plugin(insert),
                });
            } else if inserts.len() >= MAX_INSERTS {
                self.tally.inserts_dropped += 1;
            } else {
                inserts.push(insert);
            }
        }
        Ok((instrument, inserts))
    }

    /// One device as a ryolune insert, or `None` when it cannot be here.
    fn plugin(
        &mut self,
        device: Node,
        label: &str,
        track: &str,
        key: &str,
    ) -> Result<Option<Insert>> {
        let tag = device.tag_name().name();
        let device_id = device.attribute("deviceID").unwrap_or("").trim();
        let builtin = match tag {
            "Equalizer" => Some("Channel EQ"),
            "Compressor" => Some("ryolune Comp"),
            "Limiter" => Some("Limiter"),
            "NoiseGate" => Some("Gate"),
            _ => None,
        };
        if let Some(stock) = builtin {
            self.report.approximated(format!(
                "{label} on {track} became ryolune's {stock} at its own settings"
            ));
            return Ok(Some(Insert::new(
                String::new(),
                &format!("stock:{stock}"),
                stock,
            )));
        }
        let (plugin, format) = match tag {
            "ClapPlugin" => (Some(format!("clap:{device_id}")), "CLAP"),
            "Vst3Plugin" => (
                hex_from_uuid(device_id).map(|h| format!("vst3:{h}")),
                "VST3",
            ),
            "AuPlugin" => (
                self.catalog
                    .iter()
                    .find(|d| {
                        d.format == PluginFormat::AudioUnit
                            && ((d.id.ends_with(device_id) && !device_id.is_empty())
                                || d.name == label)
                    })
                    .map(|d| d.id.clone()),
                "Audio Unit",
            ),
            "Vst2Plugin" => (None, "VST2"),
            "Device" | "BuiltinDevice"
                if device_id.starts_with("stock:") || device_id.starts_with("native:") =>
            {
                (Some(device_id.to_string()), "ryolune")
            }
            _ => (None, "built-in"),
        };
        let descriptor = plugin
            .as_ref()
            .and_then(|id| self.catalog.iter().find(|d| d.id.eq_ignore_ascii_case(id)));
        let Some(descriptor) = descriptor else {
            let why = match (tag, format) {
                (_, "VST2") => "ryolune does not load VST2".to_string(),
                (_, "built-in") => format!(
                    "a device of {} that only it has",
                    if tag == "Device" {
                        "the other app"
                    } else {
                        "that app"
                    }
                ),
                _ => "not installed here".to_string(),
            };
            let vendor = device
                .attribute("deviceVendor")
                .map(|v| format!(", {v}"))
                .unwrap_or_default();
            let line = format!("{label} ({format}{vendor}) on {track}: {why}, left out");
            self.report.dropped(line.clone());
            self.tally.plugins_missing.push(label.to_string());
            self.notes.entry(key.to_string()).or_default().push(line);
            return Ok(None);
        };
        self.tally.plugins += 1;
        let mut insert = Insert::new(String::new(), &descriptor.id, &descriptor.name);
        for p in els(device, "Parameters").flat_map(|ps| ps.children().filter(Node::is_element)) {
            if let (Some(id), Some(value)) = (
                p.attribute("parameterID")
                    .and_then(|v| v.trim().parse::<i64>().ok()),
                number(p, "value"),
            ) {
                insert.params.insert(id as i32 as u32, value);
            }
        }
        if let Some(state) = el(device, "State") {
            let bytes = self.file(state)?;
            match bytes {
                None => {
                    self.tally.states_failed += 1;
                }
                Some(bytes) => match format {
                    "VST3" => match from_vstpreset(&bytes) {
                        Some((_, blob)) => insert.blob = encode_blob(&blob),
                        None => self.tally.states_failed += 1,
                    },
                    "ryolune" => {
                        let state: serde_json::Value =
                            serde_json::from_slice(&bytes).unwrap_or_default();
                        if let Some(params) = state["params"].as_object() {
                            for (id, value) in params {
                                if let (Ok(id), Some(value)) = (id.parse::<u32>(), value.as_f64()) {
                                    insert.params.insert(id, value);
                                }
                            }
                        }
                        if let Some(blob) = state["blob"].as_str() {
                            insert.blob = blob.to_string();
                        }
                    }
                    _ => insert.blob = encode_blob(&bytes),
                },
            }
        }
        if insert.blob.len() > 64 * 1024 * 1024 {
            insert.blob.clear();
            self.tally.states_failed += 1;
        }
        Ok(Some(insert))
    }

    /// A file the project names: inside the zip, or beside the project when external.
    fn file(&mut self, reference: Node) -> Result<Option<Vec<u8>>> {
        let Some(path) = reference.attribute("path") else {
            return Ok(None);
        };
        if boolean(reference, "external") == Some(true) {
            let candidate = Path::new(path);
            let full = if candidate.is_absolute() {
                candidate.to_path_buf()
            } else {
                self.base.join(candidate)
            };
            let Ok(meta) = std::fs::metadata(&full) else {
                return Ok(None);
            };
            if meta.len() > audio::MAX_AUDIO_BYTES as u64 {
                return Err(format!("{} is larger than 512 MiB", full.display()));
            }
            return Ok(std::fs::read(&full).ok());
        }
        entry(&mut self.archive, path, audio::MAX_AUDIO_BYTES as u64)
    }

    // --- The arrangement -----------------------------------------------------------

    fn timeline(
        &mut self,
        node: Node<'a, 'input>,
        track: Option<Dest>,
        frame: Frame,
    ) -> Result<()> {
        let track = node
            .attribute("track")
            .map(|id| self.tracks.get(id).cloned())
            .unwrap_or(track);
        let frame = Frame {
            unit: unit_of(node, frame.unit),
            ..frame
        };
        match node.tag_name().name() {
            "Lanes" => {
                for child in node.children().filter(Node::is_element) {
                    self.timeline(child, track.clone(), frame)?;
                }
            }
            "Clips" => {
                for clip in els(node, "Clip") {
                    self.clip(clip, track.clone(), frame)?;
                }
            }
            "Points" if el(node, "Target").is_some_and(|t| t.attribute("parameter").is_some()) => {
                self.automation(node, frame);
            }
            "Notes" | "Points" | "Audio" | "Warps" => {
                // Content straight on a track's lane: one clip from where it starts.
                let mut gathered = Gathered::default();
                let mut audio = vec![];
                self.content(node, frame, &mut gathered, &mut audio, None)?;
                self.place(track.as_ref(), gathered, audio, frame, "Clip");
            }
            "Markers" => self.markers(node, frame),
            "ClipSlot" => self.tally.launcher += 1,
            "Video" => self.tally.video += 1,
            _ => {}
        }
        Ok(())
    }

    /// Absolute beats for a time of `frame`'s timeline.
    fn at(&self, frame: Frame, time: f64) -> f64 {
        match frame.unit {
            Unit::Beats => frame.origin + (time - frame.zero),
            Unit::Seconds if frame.root => self.map.beat(time.max(0.0)),
            Unit::Seconds => frame.origin + (time - frame.zero) * self.map.bpm(frame.origin) / 60.0,
        }
    }

    fn clip(&mut self, clip: Node<'a, 'input>, track: Option<Dest>, parent: Frame) -> Result<()> {
        if boolean(clip, "enable") == Some(false) {
            self.tally.disabled_clips += 1;
            return Ok(());
        }
        let Some(time) = number(clip, "time") else {
            return Ok(());
        };
        let content = clip.children().find(|n| n.is_element()).or_else(|| {
            clip.attribute("reference")
                .and_then(|r| self.by_id.get(r).copied())
        });
        let Some(content) = content else {
            return Ok(());
        };
        let unit = match clip.attribute("contentTimeUnit") {
            Some("seconds") => Unit::Seconds,
            Some("beats") => Unit::Beats,
            _ => parent.unit,
        };
        let play_start = number(clip, "playStart").unwrap_or(0.0);
        let start = self.at(parent, time);
        let end = match number(clip, "duration") {
            Some(d) => self.at(parent, time + d),
            None => {
                let stop = number(clip, "playStop").unwrap_or(play_start);
                let probe = Frame {
                    origin: start,
                    zero: play_start,
                    unit,
                    lo: start,
                    hi: f64::INFINITY,
                    root: false,
                };
                self.at(probe, stop)
            }
        };
        let (lo, hi) = (start.max(parent.lo), end.min(parent.hi));
        if hi - lo <= 1e-9 {
            return Ok(());
        }
        // Content time to beats: the length in content units of a stretch of `beats`.
        let beats_per_unit = |reader: &Self, at: f64| match unit {
            Unit::Beats => 1.0,
            Unit::Seconds => reader.map.bpm(at) / 60.0,
        };
        // The passes through the content: (content start, absolute start, absolute end).
        let mut passes = vec![];
        match (number(clip, "loopStart"), number(clip, "loopEnd")) {
            (Some(loop_start), Some(loop_end)) if loop_end > loop_start => {
                let rate = beats_per_unit(self, start);
                let mut at = start;
                let mut from = play_start;
                let mut guard = 0;
                while at < end - 1e-9 && guard < 4096 {
                    let length = ((loop_end - from) * rate).max(0.0);
                    if length <= 1e-9 {
                        break;
                    }
                    let until = (at + length).min(end);
                    passes.push((from, at, until));
                    at = until;
                    from = loop_start;
                    guard += 1;
                }
                if passes.len() > 1 {
                    self.tally.loops += 1;
                }
            }
            _ => passes.push((play_start, start, end)),
        }
        let name = clip.attribute("name").unwrap_or("").to_string();
        let fade_unit = clip.attribute("fadeTimeUnit");
        let last = passes.len().saturating_sub(1);
        for (index, (from, at, until)) in passes.into_iter().enumerate() {
            let (lo, hi) = (at.max(lo), until.min(hi));
            if hi - lo <= 1e-9 {
                continue;
            }
            let frame = Frame {
                origin: at,
                zero: from,
                unit,
                lo,
                hi,
                root: false,
            };
            let fades = Fades {
                fade_in: if index == 0 {
                    number(clip, "fadeInTime")
                } else {
                    None
                },
                fade_out: if index == last {
                    number(clip, "fadeOutTime")
                } else {
                    None
                },
                seconds: fade_unit == Some("seconds"),
            };
            let mut gathered = Gathered::default();
            let mut audio = vec![];
            self.content(content, frame, &mut gathered, &mut audio, Some(&fades))?;
            for c in &mut audio {
                if c.name.is_empty() {
                    c.name = name.clone();
                }
            }
            self.place(track.as_ref(), gathered, audio, frame, &name);
        }
        Ok(())
    }

    fn content(
        &mut self,
        node: Node<'a, 'input>,
        frame: Frame,
        gathered: &mut Gathered,
        audio: &mut Vec<Clip>,
        fades: Option<&Fades>,
    ) -> Result<()> {
        let frame = Frame {
            unit: unit_of(node, frame.unit),
            ..frame
        };
        match node.tag_name().name() {
            "Notes" => {
                gathered.midi = true;
                for note in els(node, "Note") {
                    let (Some(time), Some(duration)) =
                        (number(note, "time"), number(note, "duration"))
                    else {
                        continue;
                    };
                    let start = self.at(frame, time);
                    if start < frame.lo - 1e-9 || start >= frame.hi - 1e-9 {
                        continue;
                    }
                    let end = self.at(frame, time + duration.max(0.0)).min(frame.hi);
                    if note.children().any(|n| n.is_element()) {
                        self.tally.note_expressions += 1;
                    }
                    let key = number(note, "key").unwrap_or(60.0).clamp(0.0, 127.0) as u8;
                    let velocity = number(note, "vel")
                        .map(|v| if v > 1.0 { v } else { v * 127.0 })
                        .unwrap_or(100.0)
                        .round()
                        .clamp(1.0, 127.0) as u8;
                    gathered.notes.push(Note {
                        id: new_id("note"),
                        start,
                        length: (end - start).max(1.0 / 960.0),
                        pitch: key,
                        velocity,
                        agent: false,
                        channel: number(note, "channel").unwrap_or(0.0).clamp(0.0, 15.0) as u8,
                    });
                }
            }
            "Points" => self.expression(node, frame, gathered),
            "Lanes" => {
                for child in node.children().filter(Node::is_element) {
                    self.content(child, frame, gathered, audio, fades)?;
                }
            }
            "Clips" => {
                for clip in els(node, "Clip") {
                    // A clip inside a clip (Bitwig's audio): its own clips, cut by this one.
                    let mut inner = vec![];
                    self.nested(clip, frame, gathered, &mut inner)?;
                    audio.extend(inner);
                }
            }
            "Audio" => {
                let offset = match frame.unit {
                    Unit::Seconds => frame.zero + self.map.duration(frame.origin, frame.lo),
                    Unit::Beats => {
                        self.tally.stretched += 1;
                        (frame.zero + (frame.lo - frame.origin)) * 60.0 / self.map.bpm(frame.lo)
                    }
                };
                if let Some(clip) = self.audio_clip(node, frame, offset, fades)? {
                    audio.push(clip);
                }
            }
            "Warps" => {
                let inner = node
                    .children()
                    .find(|n| n.is_element() && !n.has_tag_name("Warp"));
                let mut warps: Vec<(f64, f64)> = els(node, "Warp")
                    .filter_map(|w| Some((number(w, "time")?, number(w, "contentTime")?)))
                    .collect();
                warps.sort_by(|a, b| a.0.total_cmp(&b.0));
                let content_seconds = node.attribute("contentTimeUnit") != Some("beats");
                if let (Some(inner), true, true) = (inner, warps.len() >= 2, content_seconds) {
                    let local = frame.zero
                        + match frame.unit {
                            Unit::Beats => frame.lo - frame.origin,
                            Unit::Seconds => self.map.duration(frame.origin, frame.lo),
                        };
                    let offset = warp(&warps, local);
                    // Seconds of audio per beat the warp asks for, against the song's own.
                    let asked = (warp(&warps, local + 1.0) - offset).abs();
                    let real = 60.0 / self.map.bpm(frame.lo);
                    let uneven = warps.windows(2).any(|w| {
                        let slope = (w[1].1 - w[0].1) / (w[1].0 - w[0].0).max(1e-9);
                        (slope - asked).abs() > asked * 0.01
                    });
                    if frame.unit == Unit::Seconds || (asked - real).abs() > real * 0.01 || uneven {
                        self.tally.stretched += 1;
                    }
                    if inner.has_tag_name("Audio") {
                        if let Some(clip) = self.audio_clip(inner, frame, offset.max(0.0), fades)? {
                            audio.push(clip);
                        }
                    }
                } else if let Some(inner) = inner {
                    self.content(inner, frame, gathered, audio, fades)?;
                }
            }
            "Video" => self.tally.video += 1,
            _ => {}
        }
        Ok(())
    }

    /// A clip inside a clip: placed inside the outer one's frame, cut to it.
    fn nested(
        &mut self,
        clip: Node<'a, 'input>,
        outer: Frame,
        gathered: &mut Gathered,
        audio: &mut Vec<Clip>,
    ) -> Result<()> {
        if boolean(clip, "enable") == Some(false) {
            self.tally.disabled_clips += 1;
            return Ok(());
        }
        let Some(time) = number(clip, "time") else {
            return Ok(());
        };
        let content = clip.children().find(|n| n.is_element()).or_else(|| {
            clip.attribute("reference")
                .and_then(|r| self.by_id.get(r).copied())
        });
        let Some(content) = content else {
            return Ok(());
        };
        let unit = match clip.attribute("contentTimeUnit") {
            Some("seconds") => Unit::Seconds,
            Some("beats") => Unit::Beats,
            _ => outer.unit,
        };
        let start = self.at(outer, time);
        let end = number(clip, "duration")
            .map(|d| self.at(outer, time + d))
            .unwrap_or(outer.hi);
        let (lo, hi) = (start.max(outer.lo), end.min(outer.hi));
        if hi - lo <= 1e-9 {
            return Ok(());
        }
        let frame = Frame {
            origin: start,
            zero: number(clip, "playStart").unwrap_or(0.0),
            unit,
            lo,
            hi,
            root: false,
        };
        let fades = Fades {
            fade_in: number(clip, "fadeInTime"),
            fade_out: number(clip, "fadeOutTime"),
            seconds: clip.attribute("fadeTimeUnit") == Some("seconds"),
        };
        self.content(content, frame, gathered, audio, Some(&fades))
    }

    fn audio_clip(
        &mut self,
        node: Node<'a, 'input>,
        frame: Frame,
        offset: f64,
        fades: Option<&Fades>,
    ) -> Result<Option<Clip>> {
        let Some(file) = el(node, "File") else {
            return Ok(None);
        };
        let path = file.attribute("path").unwrap_or("").to_string();
        let source = match self.sources.get(&path) {
            Some(source) => source.clone(),
            None => {
                let source = self.source(file, &path)?;
                self.sources.insert(path.clone(), source.clone());
                source
            }
        };
        let Some(source) = source else {
            return Ok(None);
        };
        let mut offset = offset;
        if offset < 0.0 {
            self.tally.negative_offset += 1;
            offset = 0.0;
        }
        let seconds = self.map.duration(frame.lo, frame.hi);
        let (mut fade_in, mut fade_out) = (0.0, 0.0);
        if let Some(fades) = fades {
            let to_seconds = |v: f64| {
                if fades.seconds {
                    v
                } else {
                    v * 60.0 / self.map.bpm(frame.lo)
                }
            };
            if let Some(v) = fades.fade_in {
                if v < 0.0 {
                    self.tally.crossfades += 1;
                }
                fade_in = to_seconds(v.abs());
            }
            if let Some(v) = fades.fade_out {
                fade_out = to_seconds(v.abs());
            }
        }
        let (fade_in, fade_out) = clamp_fades(fade_in, fade_out, seconds);
        let name = self
            .session
            .sources
            .get(&source)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let bpb = self.bpb();
        Ok(Some(Clip {
            id: new_id("clip"),
            name: if node.parent().is_some_and(|p| p.has_tag_name("Clip")) {
                String::new()
            } else {
                name
            },
            agent: false,
            track_id: String::new(),
            start_bar: frame.lo / bpb,
            length_bars: (frame.hi - frame.lo) / bpb,
            data: ClipData::Audio {
                source_id: source,
                offset_seconds: offset,
                fade_in,
                fade_out,
                fade_curve: FadeCurve::default(),
                gain_db: 0.0,
            },
        }))
    }

    fn source(&mut self, file: Node, path: &str) -> Result<Option<String>> {
        let bytes = self.file(file)?;
        let Some(bytes) = bytes else {
            self.report.missing(path);
            return Ok(None);
        };
        let extension = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        let buffer = match audio::decode(bytes, extension.as_deref()) {
            Ok(buffer) => buffer,
            Err(e) => {
                self.report.dropped(format!("The audio file {path}: {e}"));
                return Ok(None);
            }
        };
        if audio::library_bytes(&self.library).saturating_add(buffer.frames.len() * 8)
            > audio::MAX_LIBRARY_BYTES
        {
            return Err("The project's audio exceeds ryolune's 1 GiB of decoded audio".into());
        }
        let name: String = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Audio")
            .chars()
            .take(120)
            .collect();
        let source = Source {
            id: new_id("source"),
            name,
            sample_rate: buffer.sample_rate,
            channels: 2,
            file_name: Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned()),
            duration_seconds: buffer.duration(),
            origin: "file".into(),
            seed: None,
            wave_kind: None,
        };
        let id = source.id.clone();
        self.library.insert(id.clone(), Arc::new(buffer));
        self.session.sources.insert(id.clone(), source);
        Ok(Some(id))
    }

    /// MIDI controllers from expression points.
    fn expression(&mut self, node: Node<'a, 'input>, frame: Frame, gathered: &mut Gathered) {
        let Some(target) = el(node, "Target") else {
            return;
        };
        let kind = match target.attribute("expression") {
            Some("channelController") => ControllerKind::Cc,
            Some("pitchBend") => ControllerKind::Bend,
            Some("channelPressure") => ControllerKind::Pressure,
            Some("polyPressure") => ControllerKind::PolyPressure,
            _ => {
                self.tally.expressions += 1;
                return;
            }
        };
        gathered.midi = true;
        let number_of = match kind {
            ControllerKind::Cc => target.attribute("controller"),
            ControllerKind::PolyPressure => target.attribute("key"),
            _ => None,
        }
        .and_then(|n| n.trim().parse::<u8>().ok())
        .filter(|n| *n <= 127);
        if matches!(kind, ControllerKind::Cc | ControllerKind::PolyPressure) && number_of.is_none()
        {
            self.tally.expressions += 1;
            return;
        }
        let channel = target
            .attribute("channel")
            .and_then(|c| c.trim().parse::<u8>().ok())
            .unwrap_or(0)
            .min(15);
        let normalized = node.attribute("unit") != Some("linear");
        let value_of = |v: f64| -> i16 {
            match kind {
                ControllerKind::Bend => {
                    if normalized {
                        (v.clamp(0.0, 1.0) * 16383.0 - 8192.0)
                            .round()
                            .clamp(-8192.0, 8191.0) as i16
                    } else {
                        v.round().clamp(-8192.0, 8191.0) as i16
                    }
                }
                _ => {
                    if normalized {
                        (v.clamp(0.0, 1.0) * 127.0).round() as i16
                    } else {
                        v.round().clamp(0.0, 127.0) as i16
                    }
                }
            }
        };
        let mut points: Vec<(f64, f64, bool)> = node
            .children()
            .filter(|n| n.has_tag_name("RealPoint") || n.has_tag_name("IntegerPoint"))
            .filter_map(|p| {
                Some((
                    number(p, "time")?,
                    number(p, "value")?,
                    p.attribute("interpolation") == Some("linear"),
                ))
            })
            .collect();
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        let push = |reader: &Self, gathered: &mut Gathered, time: f64, value: f64| {
            let at = reader.at(frame, time);
            if at < frame.lo - 1e-9 || at >= frame.hi - 1e-9 {
                return;
            }
            gathered.controllers.push(Controller {
                id: new_id("ctl"),
                kind,
                number: number_of,
                time: at,
                value: value_of(value),
                agent: false,
                channel,
            });
        };
        for (i, &(time, value, linear)) in points.iter().enumerate() {
            push(self, gathered, time, value);
            if let (true, Some(&(next_time, next_value, _))) = (linear, points.get(i + 1)) {
                if (next_value - value).abs() > 1e-9 && next_time > time {
                    self.tally.ramps += 1;
                    let mut t = time + RAMP_STEP;
                    while t < next_time - 1e-9 && gathered.controllers.len() < 200_000 {
                        let x = (t - time) / (next_time - time);
                        push(self, gathered, t, value + (next_value - value) * x);
                        t += RAMP_STEP;
                    }
                }
            }
        }
    }

    /// Put a clip's notes, controllers and audio on the track.
    fn place(
        &mut self,
        track: Option<&Dest>,
        gathered: Gathered,
        audio: Vec<Clip>,
        frame: Frame,
        name: &str,
    ) {
        let track_id = match track {
            Some(Dest::Track(id)) => id.clone(),
            _ => {
                if gathered.midi || !audio.is_empty() {
                    self.tally.clips_off_track += 1;
                }
                return;
            }
        };
        if self
            .session
            .tracks
            .iter()
            .any(|t| t.id == track_id && t.is_bus())
        {
            if gathered.midi || !audio.is_empty() {
                self.tally.clips_on_bus += 1;
            }
            return;
        }
        let bpb = self.bpb();
        if gathered.midi {
            let lo = if frame.hi.is_finite() {
                frame.lo
            } else {
                gathered
                    .notes
                    .iter()
                    .map(|n| n.start)
                    .chain(gathered.controllers.iter().map(|c| c.time))
                    .fold(f64::INFINITY, f64::min)
                    .min(frame.lo.max(0.0))
                    .max(0.0)
            };
            let hi = if frame.hi.is_finite() {
                frame.hi
            } else {
                gathered
                    .notes
                    .iter()
                    .map(|n| n.start + n.length)
                    .chain(gathered.controllers.iter().map(|c| c.time))
                    .fold(lo + bpb, f64::max)
            };
            let mut notes = gathered.notes;
            for n in &mut notes {
                n.start = (n.start - lo).max(0.0);
            }
            notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
            let mut controllers = gathered.controllers;
            for c in &mut controllers {
                c.time = (c.time - lo).max(0.0);
            }
            crate::controllers::sort(&mut controllers);
            self.session.clips.push(Clip {
                id: new_id("clip"),
                name: if name.is_empty() {
                    "Clip".into()
                } else {
                    name.to_string()
                },
                agent: false,
                track_id: track_id.clone(),
                start_bar: lo / bpb,
                length_bars: ((hi - lo) / bpb).max(1.0 / 64.0),
                data: ClipData::Midi { notes, controllers },
            });
            self.kinds.entry(track_id.clone()).or_default().0 = true;
        }
        for mut clip in audio {
            clip.track_id = track_id.clone();
            if clip.name.is_empty() {
                clip.name = if name.is_empty() {
                    "Audio".into()
                } else {
                    name.to_string()
                };
            }
            self.session.clips.push(clip);
            self.kinds.entry(track_id.clone()).or_default().1 = true;
        }
    }

    fn automation(&mut self, node: Node<'a, 'input>, frame: Frame) {
        let Some(parameter) = el(node, "Target").and_then(|t| t.attribute("parameter")) else {
            return;
        };
        let drop = |reader: &mut Self, what: &'static str| {
            *reader.tally.automation.entry(what).or_default() += 1;
            None
        };
        let param = self.params.get(parameter).cloned();
        let target = match param {
            Some(Param::Volume(Dest::Master)) => Some((AutomationTarget::MasterVolume, true)),
            Some(Param::Volume(Dest::Track(id))) => {
                Some((AutomationTarget::TrackVolume { track_id: id }, true))
            }
            Some(Param::Pan(Dest::Track(id))) => {
                Some((AutomationTarget::TrackPan { track_id: id }, false))
            }
            Some(Param::Volume(Dest::Aux(_)) | Param::Pan(_)) => {
                drop(self, "on the effect returns and the Stereo Out's pan")
            }
            Some(Param::Tempo) => drop(self, "of the tempo outside the tempo track"),
            Some(Param::Mute) => drop(self, "of mute"),
            Some(Param::Send) => drop(self, "of send levels"),
            Some(Param::Plugin) | None => drop(self, "on plugin and device parameters"),
        };
        let Some((target, volume)) = target else {
            return;
        };
        let target_node = self.by_id.get(parameter).copied();
        let mut points: Vec<(f64, f64, bool)> = node
            .children()
            .filter(|n| n.has_tag_name("RealPoint"))
            .filter_map(|p| {
                Some((
                    number(p, "time")?,
                    number(p, "value")?,
                    p.attribute("interpolation") != Some("hold"),
                ))
            })
            .collect();
        if points.is_empty() {
            return;
        }
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        let unit = node
            .attribute("unit")
            .or_else(|| target_node.and_then(|t| t.attribute("unit")));
        let linear = points.iter().filter(|p| p.2).count();
        if linear > 0 && linear < points.len() {
            self.report
                .approximated("Automation that mixed held and gliding points glides throughout");
        }
        let value_of = |v: f64| -> f64 {
            if volume {
                let gain = match unit {
                    Some("decibel") => 10f64.powf(v / 20.0),
                    Some("normalized") => {
                        let max = target_node.and_then(|t| number(t, "max")).unwrap_or(1.0);
                        let min = target_node.and_then(|t| number(t, "min")).unwrap_or(0.0);
                        min + v * (max - min)
                    }
                    _ => v,
                };
                fader_position(gain).clamp(0.0, 1.0)
            } else {
                match target_node {
                    Some(t) => pan_of(t, v),
                    None => (v * 200.0 - 100.0).clamp(-100.0, 100.0),
                }
            }
        };
        let mut lane_points: Vec<AutomationPoint> = vec![];
        for (time, value, _) in &points {
            let beat = self.at(frame, *time).max(0.0);
            if lane_points
                .last()
                .is_some_and(|p| (p.beat - beat).abs() < 1e-9)
            {
                lane_points.pop();
            }
            lane_points.push(AutomationPoint {
                id: new_id("point"),
                beat,
                value: value_of(*value),
            });
        }
        lane_points.truncate(crate::automation::MAX_POINTS);
        if self.session.automation.iter().any(|l| l.target == target) {
            return;
        }
        let (name, min, max) = match &target {
            AutomationTarget::TrackPan { .. } => ("Pan", -100.0, 100.0),
            _ => ("Volume", 0.0, 1.0),
        };
        let manual_value = match &target {
            AutomationTarget::MasterVolume => self.session.master_volume as f64,
            AutomationTarget::TrackVolume { track_id } => self
                .session
                .tracks
                .iter()
                .find(|t| &t.id == track_id)
                .map_or(0.75, |t| t.volume as f64),
            AutomationTarget::TrackPan { track_id } => self
                .session
                .tracks
                .iter()
                .find(|t| &t.id == track_id)
                .map_or(0.0, |t| t.pan as f64),
            _ => 0.0,
        };
        if self.session.automation.len() < crate::automation::MAX_LANES {
            self.session.automation.push(AutomationLane {
                id: new_id("lane"),
                name: name.into(),
                target,
                min,
                max,
                manual_value,
                interpolation: if linear == 0 {
                    Interpolation::Step
                } else {
                    Interpolation::Linear
                },
                enabled: true,
                points: lane_points,
            });
        }
    }

    fn markers(&mut self, node: Node<'a, 'input>, frame: Frame) {
        let frame = Frame {
            unit: unit_of(node, frame.unit),
            ..frame
        };
        let bpb = self.bpb();
        for marker in els(node, "Marker") {
            let Some(time) = number(marker, "time") else {
                continue;
            };
            if self.session.markers.len() >= MAX_MARKERS {
                break;
            }
            let bar = self.at(frame, time).max(0.0) / bpb;
            let name: String = marker
                .attribute("name")
                .unwrap_or("Marker")
                .chars()
                .take(120)
                .collect();
            self.session.markers.push(Marker {
                id: new_id("marker"),
                bar,
                name,
                color: css_color(marker.attribute("color")),
            });
        }
    }

    // --- The song ------------------------------------------------------------------

    fn finish(&mut self) -> Result<()> {
        // A track that ended up with both MIDI and audio clips gets an audio twin; one whose
        // clips are all of the other kind changes kind.
        let ids: Vec<String> = self.session.tracks.iter().map(|t| t.id.clone()).collect();
        for id in ids {
            let (midi, audio) = self.kinds.get(&id).copied().unwrap_or_default();
            let Some(index) = self.session.tracks.iter().position(|t| t.id == id) else {
                continue;
            };
            let kind = self.session.tracks[index].kind.clone();
            if kind == "bus" {
                continue;
            }
            if midi && audio {
                let mut twin = self.session.tracks[index].clone();
                twin.id = new_id("track");
                twin.name = format!("{} (audio)", twin.name);
                twin.kind = "audio".into();
                self.session.tracks[index].kind = "midi".into();
                for clip in &mut self.session.clips {
                    if clip.track_id == id && matches!(clip.data, ClipData::Audio { .. }) {
                        clip.track_id = twin.id.clone();
                    }
                }
                if let Some(strip) = self.session.strips.get(&id).cloned() {
                    self.session.strips.insert(
                        twin.id.clone(),
                        crate::model::Strip {
                            synth: None,
                            ..strip
                        },
                    );
                }
                self.report.approximated(format!(
                    "“{}” held notes and audio: its audio is on “{}” beside it",
                    self.session.tracks[index].name, twin.name
                ));
                self.session.tracks.insert(index + 1, twin);
            } else if midi && kind != "midi" {
                self.session.tracks[index].kind = "midi".into();
            } else if audio && kind != "audio" {
                self.session.tracks[index].kind = "audio".into();
                if let Some(strip) = self.session.strips.get_mut(&id) {
                    if strip.synth.take().is_some() {
                        self.report.dropped(format!(
                            "The instrument on “{}”: its clips are audio",
                            self.session.tracks[index].name
                        ));
                    }
                }
            }
        }
        for (key, notes) in std::mem::take(&mut self.notes) {
            if let Some(track) = self.session.tracks.iter_mut().find(|t| t.id == key) {
                track
                    .extra
                    .insert("importNotes".into(), serde_json::json!(notes));
            }
        }
        if self.session.tracks.len() > 128 {
            self.session.tracks.truncate(128);
            let kept: HashSet<String> = self.session.tracks.iter().map(|t| t.id.clone()).collect();
            self.session.clips.retain(|c| kept.contains(&c.track_id));
        }
        let kept: HashSet<String> = self.session.tracks.iter().map(|t| t.id.clone()).collect();
        self.session
            .automation
            .retain(|l| l.target.track_id().is_none_or(|t| kept.contains(t)));
        self.session.markers.sort_by(|a, b| a.bar.total_cmp(&b.bar));
        for lane in &mut self.session.automation {
            lane.points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        }
        self.session.prune_routing();
        self.session.view.selected_track_id = self.session.tracks.first().map(|t| t.id.clone());
        self.session.view.scroll_bars = 0.0;
        self.tell();
        self.session.normalize();
        self.session
            .validate()
            .map_err(|e| format!("The project could not become a ryolune song: {e}"))?;
        Ok(())
    }

    /// The counts of what came and the notes on what did not.
    fn tell(&mut self) {
        let s = &self.session;
        let midi = s.tracks.iter().filter(|t| t.kind == "midi").count();
        let audio = s.tracks.iter().filter(|t| t.kind == "audio").count();
        let buses = s.tracks.iter().filter(|t| t.is_bus()).count();
        let mut tracks = vec![];
        if midi > 0 {
            tracks.push(count(midi, "instrument track", "instrument tracks"));
        }
        if audio > 0 {
            tracks.push(count(audio, "audio track", "audio tracks"));
        }
        if buses > 0 {
            tracks.push(count(buses, "bus", "buses"));
        }
        let mut kept = vec![];
        if !tracks.is_empty() {
            kept.push(format!(
                "{}, with volume, pan, mute, solo, routing and sends",
                tracks.join(", ")
            ));
        }
        let (mut midi_clips, mut notes, mut controllers, mut audio_clips) = (0, 0, 0, 0);
        for clip in &s.clips {
            match &clip.data {
                ClipData::Midi {
                    notes: n,
                    controllers: c,
                } => {
                    midi_clips += 1;
                    notes += n.len();
                    controllers += c.len();
                }
                ClipData::Audio { .. } => audio_clips += 1,
            }
        }
        if midi_clips > 0 {
            let mut line = format!(
                "{} with {}",
                count(midi_clips, "MIDI clip", "MIDI clips"),
                count(notes, "note", "notes")
            );
            if controllers > 0 {
                let _ = write!(
                    line,
                    " and {}",
                    count(controllers, "controller point", "controller points")
                );
            }
            kept.push(line);
        }
        if audio_clips > 0 {
            kept.push(format!(
                "{} with {}",
                count(audio_clips, "audio clip", "audio clips"),
                count(s.sources.len(), "audio file", "audio files")
            ));
        }
        if !s.markers.is_empty() {
            kept.push(count(s.markers.len(), "marker", "markers"));
        }
        kept.push(if s.tempo_changes.is_empty() {
            format!(
                "Tempo {} BPM in {}/{}",
                s.transport.tempo,
                s.transport.time_signature.numerator,
                s.transport.time_signature.denominator
            )
        } else {
            format!(
                "Tempo {} BPM with {}, in {}/{}",
                s.transport.tempo,
                count(s.tempo_changes.len(), "tempo change", "tempo changes"),
                s.transport.time_signature.numerator,
                s.transport.time_signature.denominator
            )
        });
        if !s.automation.is_empty() {
            kept.push(count(
                s.automation.len(),
                "volume or pan automation lane",
                "volume and pan automation lanes",
            ));
        }
        if self.tally.plugins > 0 {
            kept.push(format!(
                "{} with their settings",
                count(self.tally.plugins, "plugin", "plugins")
            ));
        }
        for line in kept {
            self.report.kept(line);
        }
        let t = &self.tally;
        let mut approximated = vec![];
        let mut dropped = vec![];
        if t.loops > 0 {
            approximated.push(format!(
                "{} were written out repeat by repeat: ryolune clips do not loop",
                count(t.loops, "looped clip", "looped clips")
            ));
        }
        if t.stretched > 0 {
            approximated.push(format!("{} time-stretched in the other app: ryolune plays audio at its own speed, so check they still line up", count(t.stretched, "audio clip was", "audio clips were")));
        }
        if t.ramps > 0 {
            approximated
                .push("Gliding controller changes became steps every 32nd note".to_string());
        }
        if t.negative_offset > 0 {
            approximated.push(format!(
                "{} started before their audio file: they start with the file",
                count(t.negative_offset, "audio clip", "audio clips")
            ));
        }
        if t.crossfades > 0 {
            approximated.push("Crossfades became a fade-in on the later clip".to_string());
        }
        if t.states_failed > 0 {
            approximated.push(format!(
                "{} could not be read: those plugins open at their defaults",
                count(t.states_failed, "plugin's settings", "plugins' settings")
            ));
        }
        if t.disabled_clips > 0 {
            dropped.push(format!(
                "{} switched off in the other app",
                count(t.disabled_clips, "clip", "clips")
            ));
        }
        if t.clips_off_track > 0 {
            dropped.push(format!(
                "{} not on any track",
                count(t.clips_off_track, "clip", "clips")
            ));
        }
        if t.clips_on_bus > 0 {
            dropped.push(format!(
                "{} on group or effect tracks: ryolune buses hold no clips",
                count(t.clips_on_bus, "clip", "clips")
            ));
        }
        if t.expressions > 0 {
            dropped.push(format!(
                "{} (timbre, per-note gain or pan, program changes…)",
                count(
                    t.expressions,
                    "expression lane MIDI cannot carry",
                    "expression lanes MIDI cannot carry"
                )
            ));
        }
        if t.note_expressions > 0 {
            dropped.push(format!(
                "Per-note expression on {}",
                count(t.note_expressions, "note", "notes")
            ));
        }
        for (what, n) in &t.automation {
            dropped.push(format!(
                "{} {what}",
                count(*n, "automation lane", "automation lanes")
            ));
        }
        if t.sends_dropped > 0 {
            dropped.push(format!(
                "{} to places ryolune cannot send (another track, or a fifth send)",
                count(t.sends_dropped, "send", "sends")
            ));
        }
        if t.inserts_dropped > 0 {
            dropped.push(format!(
                "{} past the eighth slot of a channel",
                count(t.inserts_dropped, "effect", "effects")
            ));
        }
        if t.launcher > 0 {
            dropped.push(format!(
                "{} of the clip launcher: ryolune has the arrangement only",
                count(t.launcher, "clip", "clips")
            ));
        }
        if t.video > 0 {
            dropped.push("Video: ryolune is for sound (kimchi edits video)".to_string());
        }
        for line in approximated {
            self.report.approximated(line);
        }
        for line in dropped {
            self.report.dropped(line);
        }
    }
}

enum Instrument {
    Stock(String),
    Plugin(Insert),
}
struct Fades {
    fade_in: Option<f64>,
    fade_out: Option<f64>,
    seconds: bool,
}

/// Content time for a warped time, along the warp points (straight on past either end).
fn warp(points: &[(f64, f64)], time: f64) -> f64 {
    let pair = points
        .windows(2)
        .find(|w| time <= w[1].0)
        .unwrap_or(&points[points.len() - 2..]);
    let (t0, c0) = pair[0];
    let (t1, c1) = pair[1];
    if (t1 - t0).abs() < 1e-12 {
        return c0;
    }
    c0 + (time - t0) * (c1 - c0) / (t1 - t0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fader_positions_invert_the_fader_law() {
        for v in [0.0f32, 0.1, 0.5, 0.75, 0.9, 1.0] {
            let back = fader_position(fader_gain(v) as f64);
            assert!((back - v as f64).abs() < 1e-5, "{v} came back as {back}");
        }
    }

    #[test]
    fn vst3_states_go_through_a_vstpreset_unchanged() {
        let mut blob = vec![];
        blob.extend_from_slice(&3u32.to_le_bytes());
        blob.extend_from_slice(b"abc");
        blob.extend_from_slice(&2u32.to_le_bytes());
        blob.extend_from_slice(b"xy");
        let plugin = "vst3:0123456789abcdef0123456789abcdef";
        let preset = vstpreset(plugin, &blob).unwrap();
        assert_eq!(&preset[0..4], b"VST3");
        assert_eq!(&preset[8..40], b"0123456789ABCDEF0123456789ABCDEF");
        let (class, back) = from_vstpreset(&preset).unwrap();
        assert_eq!(class, "0123456789abcdef0123456789abcdef");
        assert_eq!(back, blob);
        assert_eq!(
            uuid_from_hex("0123456789abcdef0123456789abcdef"),
            "01234567-89AB-CDEF-0123-456789ABCDEF"
        );
        assert_eq!(
            hex_from_uuid("01234567-89AB-CDEF-0123-456789ABCDEF").unwrap(),
            "0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn warps_map_beats_to_seconds_and_extrapolate() {
        let points = [(0.0, 0.0), (8.0, 4.0)];
        assert_eq!(warp(&points, 4.0), 2.0);
        assert_eq!(warp(&points, 10.0), 5.0);
    }

    #[test]
    fn text_is_escaped_for_attributes() {
        assert_eq!(escape("a<b & \"c\"\n"), "a&lt;b &amp; &quot;c&quot;&#10;");
    }
}
