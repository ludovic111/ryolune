//! The plugin library as a musician browses it: every plugin filed in a sound folder
//! (Synths, Drums, Dynamics, Space & Time…), favourites, and what was used last.
use crate::{
    control::{edit, opt, query, req, Args, Host, Kind, Spec},
    host::scan,
    plugin::Descriptor,
    settings::Plugins,
    Result,
};
use serde_json::{json, Value};

/// Folders in the order the browser shows them.
pub const INSTRUMENT_FOLDERS: &[&str] = &[
    "Synths",
    "Keys",
    "Bass",
    "Drums",
    "Pads",
    "Samplers",
    "Textures",
    "Other Instruments",
];
pub const EFFECT_FOLDERS: &[&str] = &[
    "Dynamics",
    "EQ & Filter",
    "Distortion",
    "Modulation",
    "Space & Time",
    "Pitch",
    "Channel Strips",
    "Mastering",
    "Restoration",
    "Utility",
    "Other Effects",
];

pub const SPECS: &[Spec] = &[
    query("plugin.folders", "The sound folders of the plugin library with how many instruments and effects each holds, plus favourites and recents.", &[]),
    edit("plugin.setFavorite", "Star or unstar a plugin so it shows under Favourites in the browser.", &[
        req("pluginId", Kind::String, "Plugin id from plugin.list."),
        req("favorite", Kind::Boolean, "Star (true) or unstar (false)."),
    ]),
    edit("plugin.setFolder", "File a plugin under another sound folder, or a new one of your own. Omit folder to return it to the automatic one.", &[
        req("pluginId", Kind::String, "Plugin id from plugin.list."),
        opt("folder", Kind::String, "Folder name, 1-40 characters."),
    ]),
    edit("plugin.scaffold", "Start a new ryolune native plugin in Rust at a path of your choice: writes a crate with a working effect or instrument, its plugin.toml, a test that runs it through the real plugin ABI, and build notes. Build it with cargo, then plugin.install. plugin.new does the same in the lsuite sources folder, for plugin.build and plugin.publishLocal.", &[
        req("path", Kind::String, "Directory to create. It must not exist yet."),
        req("name", Kind::String, "Plugin display name, for example Warm Drive."),
        opt("kind", Kind::String, "effect (default) or instrument."),
        opt("vendor", Kind::String, "Your name or label, default My Studio."),
    ]),
    edit("plugin.install", "Install a built plugin: an lsuite bundle (a folder with plugin.toml and its library) goes to ~/.lsuite/plugins/ryolune and is loaded at once; a bare library (.dylib, .so, .dll or .onplug) is copied into ryolune's plugin folder, then plugin.scan loads it.", &[
        req("path", Kind::String, "The bundle folder, or the built library such as target/release/libwarm_drive.dylib."),
    ]),
];

pub(crate) const EFFECT_TEMPLATE: &str = r#"use ryolune_plugin::{export_plugins, prelude::*};

/// A drive stage with a wet/dry blend. Replace the maths in `process` with your own.
pub struct __TYPE__ {
    drive: Smoother,
    mix: f32,
}

impl Plugin for __TYPE__ {
    const INFO: Info = Info::effect("__ID__", "__NAME__", "__VENDOR__", "Distortion")
        .describe("Soft saturation with a blend control.");

    fn params() -> Vec<ParamSpec> {
        vec![param("Drive", 0.0, 36.0, 12.0, "dB"), param("Mix", 0.0, 100.0, 100.0, "%")]
    }
    fn new(rate: f64) -> Self {
        Self { drive: Smoother::new(rate, 0.02, db_to_gain(12.0)), mix: 1.0 }
    }
    fn set_param(&mut self, index: usize, value: f64) {
        match index {
            0 => self.drive.set(db_to_gain(value)),
            1 => self.mix = (value / 100.0) as f32,
            _ => {}
        }
    }
    // Audio thread: no allocation, locks, files or logging in here. The host splits the block
    // wherever a parameter changes, so `set_param` has already landed on the right sample.
    fn process(&mut self, audio: &mut [[f32; 2]], _notes: &[NoteEvent], _ctx: &ProcessContext) {
        for frame in audio {
            let drive = self.drive.step();
            for sample in frame.iter_mut() {
                let wet = (*sample * drive).tanh() / drive.sqrt();
                *sample += (wet - *sample) * self.mix;
            }
        }
    }
    // Also yours to override (plugin ABI 2), each with a sensible default:
    //   process_events  controllers, pitch bend, pressure, and parameter changes by frame
    //   save / load     state that is not a parameter (parameters are saved for you)
    //   tail_seconds    how long the output rings after the input stops
    //   latency         return a new value when it changes; the host is told
}

export_plugins!(__TYPE__);

#[cfg(test)]
mod tests {
    use super::*;
    use ryolune_plugin::testing::Bench;

    #[test]
    fn drives_without_blowing_up_and_bypasses_at_zero_mix() {
        let mut bench = Bench::<__TYPE__>::new(48_000.0);
        bench.set("Drive", 36.0);
        let loud = bench.sine(220.0, 1.0, 0.25);
        Bench::<__TYPE__>::assert_sane(&loud);
        bench.set("Mix", 0.0);
        let dry = bench.sine(220.0, 0.5, 0.25);
        assert!((Bench::<__TYPE__>::peak(&dry) - 0.5).abs() < 0.01);
    }

    #[test]
    fn a_parameter_change_lands_on_its_frame() {
        let mut bench = Bench::<__TYPE__>::new(48_000.0);
        bench.set("Mix", 0.0);
        let mut audio = vec![[0.5f32; 2]; 400];
        let wet_from_200 = [bench.at(200, "Mix", 100.0)];
        bench.process_events(&mut audio, &[], &wet_from_200);
        assert_eq!(audio[199], [0.5; 2], "still dry one frame before");
        assert_ne!(audio[200], [0.5; 2], "wet from the frame that was asked for");
    }
}
"#;

pub(crate) const INSTRUMENT_TEMPLATE: &str = r#"use ryolune_plugin::{export_plugins, prelude::*};
use std::f64::consts::TAU;

const VOICES: usize = 16;

#[derive(Clone, Copy, Default)]
struct Voice {
    pitch: u8,
    phase: f64,
    level: f32,
    held: bool,
    active: bool,
}

/// A sixteen-voice sine synth with a release that follows the pitch wheel. Replace the
/// oscillator with your own sound.
pub struct __TYPE__ {
    rate: f64,
    voices: [Voice; VOICES],
    release: f32,
    release_ms: f64,
    gain: f32,
    /// Pitch wheel, -1..1, worth two semitones either way.
    bend: f32,
}

impl Plugin for __TYPE__ {
    const INFO: Info = Info::instrument("__ID__", "__NAME__", "__VENDOR__")
        .describe("A small polyphonic synth.");

    fn params() -> Vec<ParamSpec> {
        vec![param("Release", 10.0, 4000.0, 300.0, "ms"), param("Level", -24.0, 6.0, -6.0, "dB")]
    }
    fn new(rate: f64) -> Self {
        let mut plugin = Self {
            rate,
            voices: [Voice::default(); VOICES],
            release: 0.0,
            release_ms: 300.0,
            gain: 0.5,
            bend: 0.0,
        };
        plugin.set_param(0, 300.0);
        plugin
    }
    fn set_param(&mut self, index: usize, value: f64) {
        match index {
            0 => {
                self.release_ms = value;
                self.release = (-1.0 / (self.rate * value / 1000.0)).exp() as f32;
            }
            1 => self.gain = db_to_gain(value),
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.voices = [Voice::default(); VOICES];
    }
    /// The release takes about seven time constants to fall below hearing.
    fn tail_seconds(&self) -> f64 {
        self.release_ms / 1000.0 * 7.0
    }
    /// Hosts from before plugin ABI 2 call this one: notes only.
    fn process(&mut self, audio: &mut [[f32; 2]], notes: &[NoteEvent], ctx: &ProcessContext) {
        let mut events = [Event::default(); 64];
        let count = notes.len().min(events.len());
        for (event, note) in events.iter_mut().zip(notes) {
            *event = (*note).into();
        }
        self.process_events(audio, &events[..count], &[], ctx);
    }
    // Audio thread: no allocation, locks, files or logging in here. Events and parameter
    // changes are sorted by frame; handle each when the loop reaches its frame.
    fn process_events(
        &mut self,
        audio: &mut [[f32; 2]],
        events: &[Event],
        params: &[TimedParam],
        _ctx: &ProcessContext,
    ) {
        let (mut next, mut next_param) = (0, 0);
        for (i, frame) in audio.iter_mut().enumerate() {
            while next_param < params.len() && params[next_param].frame as usize <= i {
                self.set_param(params[next_param].index as usize, params[next_param].value);
                next_param += 1;
            }
            while next < events.len() && events[next].frame as usize <= i {
                let e = events[next];
                next += 1;
                match e.kind {
                    event::NOTE_ON => {
                        let slot = self.voices.iter().position(|v| !v.active).unwrap_or(0);
                        self.voices[slot] = Voice {
                            pitch: e.key,
                            phase: 0.0,
                            level: e.value as f32 / 127.0,
                            held: true,
                            active: true,
                        };
                    }
                    event::NOTE_OFF => {
                        for v in self.voices.iter_mut().filter(|v| v.held && v.pitch == e.key) {
                            v.held = false;
                        }
                    }
                    event::PITCH_BEND => self.bend = e.bend_amount(),
                    // event::CONTROL (e.key is the controller), CHANNEL_PRESSURE, POLY_PRESSURE
                    _ => {}
                }
            }
            let mut sum = 0.0;
            for v in self.voices.iter_mut().filter(|v| v.active) {
                let semitones = v.pitch as f64 - 69.0 + self.bend as f64 * 2.0;
                let hz = 440.0 * 2f64.powf(semitones / 12.0);
                v.phase = (v.phase + hz / self.rate).fract();
                sum += (TAU * v.phase).sin() as f32 * v.level;
                if !v.held {
                    v.level *= self.release;
                    v.active = v.level > 1e-4;
                }
            }
            // Instruments add to the buffer; the host hands them silence.
            frame[0] += sum * self.gain * 0.3;
            frame[1] += sum * self.gain * 0.3;
        }
    }
}

export_plugins!(__TYPE__);

#[cfg(test)]
mod tests {
    use super::*;
    use ryolune_plugin::testing::Bench;

    #[test]
    fn a_note_sounds_and_then_fades_to_silence() {
        let mut bench = Bench::<__TYPE__>::new(48_000.0);
        bench.set("Release", 50.0);
        let out = bench.note(60, 100, 0.2, 2.0);
        Bench::<__TYPE__>::assert_sane(&out);
        assert!(Bench::<__TYPE__>::peak(&out[..9_600]) > 0.05, "the note is audible");
        assert!(Bench::<__TYPE__>::peak(&out[86_400..]) < 1e-3, "and it ends");
    }

    #[test]
    fn the_pitch_wheel_raises_the_note() {
        let cycles = |audio: &[[f32; 2]]| {
            audio.windows(2).filter(|w| w[0][0] <= 0.0 && w[1][0] > 0.0).count()
        };
        let mut bench = Bench::<__TYPE__>::new(48_000.0);
        let plain = bench.play(1.0, &[Event::note_on(0, 69, 100)], &[]);
        let mut bench = Bench::<__TYPE__>::new(48_000.0);
        let bent = bench.play(1.0, &[Event::pitch_bend(0, 1.0), Event::note_on(0, 69, 100)], &[]);
        assert!((cycles(&plain) as f64 - 440.0).abs() <= 2.0);
        assert!((cycles(&bent) as f64 - 493.9).abs() <= 2.0, "two semitones up");
    }
}
"#;

fn has(text: &str, words: &[&str]) -> bool {
    words.iter().any(|w| text.contains(w))
}

/// The folder a plugin belongs in when the user has not filed it themselves. Formats describe
/// themselves differently (CLAP features, VST3 sub-categories, a bare AU type), so the name
/// is read first and the category only when the name gives nothing away: a word the category
/// happens to contain must not beat the product the name says it is, or the VST3 and the
/// Audio Unit of one plugin land in different folders.
pub fn automatic_folder(d: &Descriptor) -> &'static str {
    // ryolune's own plugins, and native ones written for it, name their folder outright. A
    // VST3 or CLAP category that happens to read "Dynamics" is only a hint, like any other.
    let own: &[&str] = if d.instrument && !d.effect {
        INSTRUMENT_FOLDERS
    } else {
        EFFECT_FOLDERS
    };
    let ryolune = matches!(
        d.format,
        crate::plugin::Format::Stock | crate::plugin::Format::Native
    );
    if let Some(named) = own
        .iter()
        .find(|f| ryolune && f.eq_ignore_ascii_case(d.category.trim()) && !f.starts_with("Other"))
    {
        return named;
    }
    let base = layout_of(&d.name).0.trim().to_lowercase();
    let vendor = d.vendor.to_lowercase();
    if let Some((_, _, folder)) = PRODUCTS
        .iter()
        .find(|(maker, name, _)| vendor.contains(maker) && base == *name)
    {
        return folder;
    }
    // Padded, so a rule can ask for a whole word (" q1 ") wherever the name puts it.
    let name = format!(" {base} ");
    let category = format!(" {} ", d.category).to_lowercase();
    if d.instrument && (!d.effect || has(&format!("{category}{name}"), &["instrument", "synth"])) {
        return instrument_folder(&name)
            .or_else(|| instrument_folder(&category))
            .unwrap_or("Other Instruments");
    }
    effect_folder(&name)
        .or_else(|| effect_folder(&category))
        .unwrap_or("Other Effects")
}

fn instrument_folder(text: &str) -> Option<&'static str> {
    Some(if has(text, &["bass", "808"]) {
        "Bass"
    } else if has(text, &["drum", "percuss", "kick", "snare", "beat"]) {
        "Drums"
    } else if has(
        text,
        &[
            "piano", "keys", "organ", "rhodes", "clav", "wurli", "mallet", "electric",
        ],
    ) {
        "Keys"
    } else if has(text, &["pad", "choir", "string", "ambient", "atmos"]) {
        "Pads"
    } else if has(text, &["sampl", "rompler", "player"]) {
        "Samplers"
    } else if has(text, &["riser", "texture", "noise", "drone", "fx"]) {
        "Textures"
    } else if has(
        text,
        &["synth", "instrument", "lead", "pluck", "wavetable", "fm"],
    ) {
        "Synths"
    } else {
        return None;
    })
}

fn effect_folder(text: &str) -> Option<&'static str> {
    PRIORITY_RULES
        .iter()
        .chain(EFFECT_RULES)
        .find(|(_, words)| has(text, words))
        .map(|(folder, _)| *folder)
}

/// Products whose names are too plain to classify by word, by vendor (a substring of it,
/// lower case) and exact name without its channel layout.
const PRODUCTS: &[(&str, &str, &str)] = &[
    ("antares", "warm", "Distortion"),
    ("antares", "punch", "Dynamics"),
    ("antares", "mic mod", "Channel Strips"),
    ("antares", "metamorph", "Pitch"),
    ("fabfilter", "micro", "EQ & Filter"),
    // Effect racks whose presets lean on chorus, flanging, delay and movement.
    ("xfer", "serum 2 fx", "Modulation"),
    ("xfer", "serumfx", "Modulation"),
    ("waves", "waves gemstones", "Modulation"),
    ("waves", "codex", "Synths"),
    ("waves", "element", "Synths"),
    ("waves", "flow motion", "Synths"),
];

/// Product names that contain a word another folder's rule would take first: "Marshall"
/// holds "hall", "Amp Room" holds "room", a tape machine is not a delay.
const PRIORITY_RULES: &[(&str, &[&str])] = &[
    (
        "EQ & Filter",
        &["kramer hls", "preamp and eq", "channel eq"],
    ),
    ("Modulation", &["spacemodulator", "ubermod"]),
    (
        "Distortion",
        &[
            "marshall",
            "amp room",
            "reverb amp",
            "tape recorder",
            "oxide tape",
            "kramer tape",
            "j37 tape",
            "master tape",
            "vinyl",
        ],
    ),
];

/// Effect folders by the words that give them away, most specific first. Audio Units carry
/// no category at all, so well-known hardware model names are part of the vocabulary.
const EFFECT_RULES: &[(&str, &[&str])] = &[
    (
        "Channel Strips",
        &[
            "channel strip",
            "cla ",
            "jjp",
            "maserati",
            "channel",
            "console",
            "strip",
            "vocal suite",
            "vocals",
            "microphone",
            "mic collection",
            "redd.",
            "tg12345",
            "audiotrack",
            "vt-737",
            "voxbox",
            "ua 610",
            "cs-1",
            "centric",
            "preamp",
            "ekramer",
            "88rs",
        ],
    ),
    (
        "Mastering",
        &[
            "mastering",
            "master ",
            "ozone",
            "loudness",
            "maximiz",
            "l2-",
            "impusher",
            "inflator",
            "masterdesk",
            "bx_digital",
            "elysia alpha",
        ],
    ),
    (
        "Restoration",
        &[
            "restoration",
            "denois",
            "de-nois",
            "declick",
            "de-click",
            "dehum",
            "x-noise",
            "x-hum",
            "x-click",
            "x-crackle",
            "z-noise",
            "clarity",
            "fdbk",
            "feedback supp",
            "soothe",
            "debreath",
            " ns1 ",
            "wns",
            "w43",
            "soundsoap",
            "soundisolation",
            "feedback hunter",
            "c-suite",
            "silk vocal",
        ],
    ),
    (
        "Pitch",
        &[
            "pitch",
            "tune",
            "auto-key",
            "vocoder",
            "harmon",
            "shift",
            "formant",
            "melodyne",
            "vocal bender",
            "sync vx",
            "torque",
            "throat",
            "mutator",
            "articulator",
            "aspire",
            "key detect",
            "key finder",
        ],
    ),
    (
        "Dynamics",
        &[
            "compress",
            "limit",
            "gate",
            "dynamic",
            "transient",
            "transx",
            "expander",
            "de-ess",
            "deess",
            "comp",
            "rider",
            "1176",
            "la-2",
            "la2a",
            "la-3",
            "2500",
            "660",
            "670",
            "fairchild",
            "dbx",
            "vca",
            "opto",
            "leveler",
            "levell",
            "multiband",
            "c1 ",
            "c4 ",
            "c6 ",
            "l1 ",
            "l2 ",
            "l3",
            "maxxvolume",
            "rs124",
            "cla-76",
            "cla-2a",
            "cla-3a",
            "dpr-402",
            "c6-",
            "mv2",
            "linmb",
            "pro-c",
            "pro-l",
            "pro-ds",
            "pro-g",
            "pro-mb",
            "rvox",
            " axx",
            "smack attack",
            "louder",
            "pressure",
            "pumper",
            "sibilance",
            "distressor",
            "fatso",
            "variable mu",
            "2254",
            "envolution",
            "supresser",
            "tla-100",
            "cl 1b",
            "ua 175",
            "ua 176",
            "dyna-mite",
            "vsc-2",
            "mpressor",
            "tone shaper",
            "tripled",
            "intrigger",
            "emo-d5",
            " pse ",
            "kramer pie",
            "33609",
        ],
    ),
    (
        "Space & Time",
        &[
            "reverb",
            "verb",
            "delay",
            "echo",
            "space",
            "room",
            "hall",
            "plate",
            "shimmer",
            "spring",
            "emt",
            "supertap",
            "tapped delay",
            "ir-l",
            "ir1",
            "ir360",
            "chambers",
            "irlive",
            "bx 20",
            "dmx 15",
            "rmx16",
            "time cube",
            "sdd-3000",
            "lexicon",
            "ocean way studios",
            "sound city",
            "re-201",
            "pro-r",
            "timeless",
            "wetter",
            "reflection engine",
        ],
    ),
    (
        "Modulation",
        &[
            "chorus",
            "phaser",
            "flang",
            "tremolo",
            "modulat",
            "vibrato",
            "rotary",
            "doubler",
            "adt",
            "ce-1",
            "ensemble",
            "autopan",
            "auto pan",
            "mondomod",
            "metaflanger",
            "brauer motion",
            "doppler",
            "enigma",
            "kaleidoscopes",
            "multimod rack",
            "cyclosonic",
            "dimension d",
            "morphoder",
            "vocodist",
            "ovox",
            " choir ",
            " duo ",
        ],
    ),
    (
        "Distortion",
        &[
            "distort",
            "satur",
            "drive",
            "fuzz",
            "crush",
            "amp",
            "tape",
            "clip",
            "lo-fi",
            "lofi",
            "guitar",
            "gtr",
            "stomp",
            "vinyl",
            "retro",
            "j37",
            "kramer",
            "exciter",
            "enhancer",
            "diezel",
            "marshall",
            "fender",
            "cabinet",
            "magma",
            "mdmx",
            "screamer",
            "nls ",
            "saphira",
            "prs ",
            "engl ",
            "friedman",
            "fuchs",
            "suhr",
            "eden wt",
            "gallien",
            "gav19t",
            "bermuda triangle",
            "biscuit",
            "studer",
            "culture vulture",
            "twintube",
            "vsm-3",
            "uad raw",
            "verve",
            "vitamin",
        ],
    ),
    (
        "EQ & Filter",
        &[
            "eq",
            "filter",
            "tilt",
            "shelf",
            "pultec",
            "puigtec",
            "1073",
            "1081",
            "helios",
            "curves",
            "bass",
            "loair",
            "brighter",
            "q4",
            "q6",
            "q10",
            "f6",
            "api-5",
            "api 5",
            "bandpass",
            "hipass",
            "lowpass",
            "pro-q",
            " q1 ",
            " q2 ",
            " q3 ",
            " q8 ",
            "q-clone",
            "q-capture",
            "scheps 73",
            "cambridge",
            "harrison",
            "massive passive",
            "neve 1084",
            "neve 31102",
            "trident",
            "me 1b",
            "pe 1c",
            "little labs vog",
            "bx_refinement",
            "bx_subsynth",
            "submarine",
            "fresh air",
            "vitalizer",
            "volcano",
            "simplon",
            "phatter",
            "parallel particles",
            "emo-f2",
        ],
    ),
    (
        "Utility",
        &[
            "util",
            "gain",
            "meter",
            "analy",
            "stereo",
            "width",
            "tool",
            "imager",
            "trim",
            "surround",
            "360",
            "5.1",
            "7.1",
            "5.0",
            "immersive",
            "wrapper",
            "phase",
            "send",
            "receive",
            "relay",
            "recall",
            "dorrough",
            "monitor",
            "nx ",
            "ps22",
            "s1 ms",
            "s1 shuffler",
            " center ",
            "paz",
            "wlm",
            "tonal balance",
            "sub align",
            "um22",
            "tract",
            "emo-generator",
            "rogerbeep",
            "roundtrip",
            "studioverse",
            "little labs ibp",
            "studio 3",
            "wood works",
        ],
    ),
];

/// The automatic folder of every installed plugin, decided per product. The Audio Unit of a
/// synth often has no category where its VST3 says "Synth", so each product (vendor, kind and
/// name without the channel layout) takes the first named folder any of its formats gives,
/// asking the formats that describe themselves best first.
pub struct AutoFolders(std::collections::HashMap<String, &'static str>);

impl AutoFolders {
    pub fn new(installed: &[Descriptor]) -> Self {
        let rank = |d: &Descriptor| match d.format {
            crate::plugin::Format::Stock | crate::plugin::Format::Native => 0,
            crate::plugin::Format::Clap => 1,
            crate::plugin::Format::Vst3 => 2,
            crate::plugin::Format::AudioUnit => 3,
        };
        let mut ordered: Vec<&Descriptor> = installed.iter().collect();
        ordered.sort_by_key(|d| rank(d));
        let mut map = std::collections::HashMap::new();
        for d in ordered {
            let found = automatic_folder(d);
            let slot = map.entry(product_key(d)).or_insert(found);
            if slot.starts_with("Other") && !found.starts_with("Other") {
                *slot = found;
            }
        }
        Self(map)
    }
    pub fn of(&self, d: &Descriptor) -> &'static str {
        self.0
            .get(&product_key(d))
            .copied()
            .unwrap_or_else(|| automatic_folder(d))
    }
}

fn product_key(d: &Descriptor) -> String {
    format!(
        "{}|{}|{}",
        d.vendor.to_lowercase(),
        d.instrument,
        layout_of(&d.name).0.trim().to_lowercase()
    )
}

/// The folder shown for a plugin: the user's filing when there is one.
pub fn folder(d: &Descriptor, library: &Plugins, auto: &AutoFolders) -> String {
    library
        .folders
        .get(&d.id)
        .cloned()
        .unwrap_or_else(|| auto.of(d).to_string())
}

fn entry(d: &Descriptor, library: &Plugins, auto: &AutoFolders) -> Value {
    let mut value = serde_json::to_value(d).unwrap_or_else(|_| json!({}));
    value["folder"] = json!(folder(d, library, auto));
    value["favorite"] = json!(library.favorites.contains(&d.id));
    value["enabled"] = json!(!library.disabled.contains(&d.id));
    value
}

/// Split a channel-layout suffix off a plugin name: `"API-2500 (m->s)"` is `API-2500` in
/// the layout `m->s`. Waves registers every layout of an Audio Unit as a plugin of its own.
pub fn layout_of(name: &str) -> (&str, Option<&str>) {
    let trimmed = name.trim_end();
    // The VST3 spelling of the same thing: "C1 comp Mono", "Doubler2 Mono/Stereo".
    for (suffix, layout) in [(" Mono/Stereo", "m->s"), (" Stereo", "s"), (" Mono", "m")] {
        if let Some(base) = trimmed.strip_suffix(suffix).filter(|b| !b.is_empty()) {
            return (base, Some(layout));
        }
    }
    let Some(open) = trimmed.rfind(" (") else {
        return (name, None);
    };
    let Some(layout) = trimmed[open + 2..].strip_suffix(')') else {
        return (name, None);
    };
    let channel = |part: &str| {
        matches!(part, "m" | "s" | "5.0" | "5.1" | "7.0" | "7.1" | "quad")
            || (part.len() == 1 && part.chars().all(|c| c.is_ascii_digit()))
    };
    let known = match layout.split_once("->") {
        Some((from, to)) => channel(from) && channel(to),
        None => channel(layout),
    };
    if known {
        (&trimmed[..open], Some(layout))
    } else {
        (name, None)
    }
}
/// How well a layout suits a track, which is always stereo: stereo, then mono in and stereo
/// out, then mono, then the surround ones.
fn layout_rank(layout: Option<&str>) -> u8 {
    match layout {
        Some("s") | None => 0,
        Some("m->s") => 1,
        Some("m") => 2,
        Some(_) => 3,
    }
}
/// One row per plugin: its formats and channel layouts fold into the one ryolune loads (CLAP,
/// then VST3, then AU, in the layout a stereo track wants), and the others stay reachable under
/// `formats` and `layouts`. Order follows the first of each group.
/// What every format and layout of one plugin have in common, and nothing else shares.
fn row_key(plugin: &Descriptor) -> String {
    let (base, _) = layout_of(&plugin.name);
    format!(
        "{}|{}|{}",
        plugin.vendor.trim().to_lowercase(),
        plugin.instrument,
        base.trim().to_lowercase()
    )
}
fn collapse_layouts(plugins: Vec<Descriptor>, library: &Plugins, auto: &AutoFolders) -> Vec<Value> {
    let mut rows: Vec<Vec<Descriptor>> = vec![];
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for plugin in plugins {
        // Stock and native plugins are ryolune's own: never folded with anything else.
        let key = match plugin.format.prefix() {
            "stock" | "native" => plugin.id.clone(),
            _ => row_key(&plugin),
        };
        match index.get(&key) {
            Some(&i) => rows[i].push(plugin),
            None => {
                index.insert(key, rows.len());
                rows.push(vec![plugin]);
            }
        }
    }
    rows.into_iter()
        .map(|group| {
            let best = group
                .iter()
                .min_by_key(|d| (format_rank(d), layout_rank(layout_of(&d.name).1)))
                .expect("a group has at least one plugin");
            let mut value = entry(best, library, auto);
            // The row names the plugin, not its channel layout, even when only one layout is
            // installed ("CODEX Stereo", "Element (0->2)").
            let (base, layout) = layout_of(&best.name);
            if layout.is_some() {
                value["name"] = json!(base);
            }
            if group.len() > 1 {
                value["favorite"] = json!(group.iter().any(|d| library.favorites.contains(&d.id)));
            }
            let layouts: Vec<&Descriptor> =
                group.iter().filter(|d| d.format == best.format).collect();
            if layouts.len() > 1 {
                value["layouts"] = layouts
                    .iter()
                    .map(|d| json!({ "id": d.id, "layout": layout_of(&d.name).1 }))
                    .collect();
            }
            let mut formats: Vec<&Descriptor> = vec![];
            for d in &group {
                if formats.iter().all(|f| f.format != d.format) {
                    formats.push(
                        group
                            .iter()
                            .filter(|o| o.format == d.format)
                            .min_by_key(|o| layout_rank(layout_of(&o.name).1))
                            .unwrap_or(d),
                    );
                }
            }
            if formats.len() > 1 {
                formats.sort_by_key(|d| format_rank(d));
                value["formats"] = formats
                    .iter()
                    .map(|d| json!({ "id": d.id, "format": d.format.prefix() }))
                    .collect();
            }
            value
        })
        .collect()
}

/// Plain words a musician (or an agent) searches with, per sound folder, so "reverb" finds
/// Space and every third-party reverb filed under Space & Time.
pub fn folder_words(folder: &str) -> &'static str {
    match folder {
        "Synths" => "synth synthesizer lead pluck analog",
        "Keys" => "keys piano electric piano organ rhodes",
        "Bass" => "bass sub 808",
        "Drums" => "drums drum machine beat percussion kit",
        "Pads" => "pad pads strings choir ambient",
        "Samplers" => "sampler sample rompler",
        "Textures" => "texture fx riser noise atmosphere",
        "Dynamics" => "dynamics compressor compression limiter gate expander transient",
        "EQ & Filter" => "eq equalizer equaliser filter tone",
        "Distortion" => "distortion saturation saturator overdrive drive tape bitcrusher lofi",
        "Modulation" => "modulation chorus flanger phaser tremolo vibrato auto pan",
        "Space & Time" => "reverb delay echo room hall plate space",
        "Pitch" => "pitch shift tune tuning harmonizer",
        "Channel Strips" => "channel strip console",
        "Mastering" => "mastering limiter loudness",
        "Restoration" => "restoration denoise de-esser declick repair",
        "Utility" => "utility gain meter analyzer stereo width",
        _ => "",
    }
}

/// How well a search matches a plugin: its name first, then vendor and name together
/// ("fabfilter pro q"), then its id, folder, category, what its folder is for and, for stock
/// plugins, their description. `None` when it does not match.
fn relevance(query: &str, d: &Descriptor, library: &Plugins, auto: &AutoFolders) -> Option<u32> {
    use crate::control_refs::score;
    let name = score(query, &d.name).map(|s| s + 1000);
    let vendor = score(query, &format!("{} {}", d.vendor, d.name)).map(|s| s + 500);
    let folder = folder(d, library, auto);
    let described = if d.format.prefix() == "stock" {
        crate::stock::description(&d.name).unwrap_or("")
    } else {
        ""
    };
    let rest = score(
        query,
        &format!(
            "{} {} {} {} {} {}",
            d.id,
            folder,
            d.category,
            d.format.prefix(),
            folder_words(&folder),
            described
        ),
    );
    name.max(vendor).max(rest)
}

/// Formats in the order a search prefers them when one plugin is installed in several.
fn format_rank(d: &Descriptor) -> u8 {
    match d.format.prefix() {
        "stock" => 0,
        "native" => 1,
        "clap" => 2,
        "vst3" => 3,
        _ => 4,
    }
}

/// The plugin an agent means: an exact descriptor id, else a name search among plugins that
/// fit the slot (instruments for an instrument, effects for an insert). A plugin installed in
/// several formats or channel layouts is one match, loaded as CLAP, then VST3, then AU, in
/// the layout a stereo track wants; two different plugins matching equally is an error that
/// lists them.
pub fn choose(
    plugin_id: Option<&str>,
    search: Option<&str>,
    kind: Option<bool>,
) -> Result<Descriptor> {
    // Plugins turned off in the Plugins window are not offered.
    let disabled = crate::settings::Settings::load().plugins.disabled;
    if let Some(id) = plugin_id.filter(|id| disabled.iter().any(|d| d == id)) {
        return Err(format!(
            "{id} is turned off in the Plugins window; plugin.enable turns it back on."
        ));
    }
    let installed: Vec<Descriptor> = scan::installed()
        .into_iter()
        .filter(|d| !disabled.contains(&d.id))
        .collect();
    let instrument = kind.unwrap_or(false);
    let fits = |d: &Descriptor| match kind {
        None => true,
        Some(true) => d.instrument,
        Some(false) => d.effect,
    };
    let role = match kind {
        None => "a plugin",
        Some(true) => "an instrument",
        Some(false) => "an effect",
    };
    let wrong_kind = |d: &Descriptor| {
        format!(
            "{} is not {role}: {}",
            d.name,
            if instrument {
                "load it into an insert slot (pass slot or firstFreeSlot)"
            } else {
                "load it as a MIDI track's instrument (omit slot)"
            }
        )
    };
    let search = match (plugin_id, search) {
        (Some(_), Some(_)) => return Err("Give pluginId or plugin, not both".into()),
        (None, None) => return Err("Name the plugin with pluginId or plugin (a name)".into()),
        (Some(id), None) => {
            if let Some(d) = installed.iter().find(|d| d.id == id) {
                return if fits(d) {
                    Ok(d.clone())
                } else {
                    Err(wrong_kind(d))
                };
            }
            if id.contains(':') {
                return Err(format!(
                    "Unknown plugin `{id}`. Run plugin.scan, then plugin.list query=..."
                ));
            }
            id
        }
        (None, Some(name)) => name,
    };
    let library = Plugins::default();
    let auto = AutoFolders::new(&installed);
    let mut matches: Vec<(u32, &Descriptor)> = installed
        .iter()
        .filter_map(|d| {
            let exact = layout_of(&d.name)
                .0
                .trim()
                .eq_ignore_ascii_case(search.trim())
                || d.name.trim().eq_ignore_ascii_case(search.trim());
            relevance(search, d, &library, &auto).map(|s| (if exact { s + 10_000 } else { s }, d))
        })
        .collect();
    if matches.is_empty() {
        let near = crate::control_refs::suggest(search, installed.iter().map(|d| d.name.as_str()))
            .map(|n| format!(" Did you mean {n}?"))
            .unwrap_or_default();
        return Err(format!(
            "No installed plugin matches `{search}`.{near} Search with plugin.list query=..., or run plugin.scan after installing."
        ));
    }
    let any_kind = matches.first().map(|m| m.1.clone());
    matches.retain(|(_, d)| fits(d));
    let Some(best) = matches.iter().map(|m| m.0).max() else {
        return Err(any_kind.map_or_else(
            || format!("No {role} matches `{search}`"),
            |d| wrong_kind(&d),
        ));
    };
    // One product in several formats and layouts is one choice.
    let product = |d: &Descriptor| {
        format!(
            "{}|{}",
            crate::control_refs::normalize(&d.vendor),
            crate::control_refs::normalize(layout_of(&d.name).0)
        )
    };
    let top: Vec<&Descriptor> = matches
        .iter()
        .filter(|m| m.0 == best)
        .map(|m| m.1)
        .collect();
    let mut products: Vec<String> = top.iter().map(|d| product(d)).collect();
    products.sort();
    products.dedup();
    // One or two letters single out a plugin only by naming it exactly.
    let short = crate::control_refs::normalize(search).len() < 3 && best < 10_000;
    if short {
        let mut near: Vec<&(u32, &Descriptor)> = matches.iter().collect();
        near.sort_by_key(|a| std::cmp::Reverse(a.0));
        return Err(format!(
            "`{search}` is too short to choose a plugin. Some that match: {}.",
            near.iter()
                .take(8)
                .map(|(_, d)| d.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if products.len() > 1 {
        return Err(format!(
            "`{search}` matches several plugins: {}. Pass pluginId or a fuller name.",
            top.iter()
                .take(10)
                .map(|d| format!("{} ({}, {}) {}", d.name, d.vendor, d.format.prefix(), d.id))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    top.into_iter()
        .min_by_key(|d| (format_rank(d), layout_rank(layout_of(&d.name).1)))
        .cloned()
        .ok_or_else(|| format!("No {role} matches `{search}`"))
}

pub(crate) fn page(args: &Args, library: &Plugins) -> Result<Value> {
    let format = args.opt_str("format");
    if format.is_some_and(|format| !["stock", "native", "clap", "vst3", "au"].contains(&format)) {
        return Err("Plugin format must be stock, native, clap, vst3 or au".into());
    }
    let kind = args.opt_str("kind");
    if kind.is_some_and(|kind| !["instrument", "effect"].contains(&kind)) {
        return Err("Plugin kind must be instrument or effect".into());
    }
    // A search ranks by how well each plugin matches unless a sort is asked for.
    let sort = args
        .opt_str("sort")
        .unwrap_or(if args.opt_str("query").is_some() {
            "relevance"
        } else {
            "name"
        });
    if !["name", "recent", "relevance"].contains(&sort) {
        return Err("Plugin sort must be name, recent or relevance".into());
    }
    let limit = args.opt_int("limit").unwrap_or(50);
    let offset = args.opt_int("offset").unwrap_or(0);
    if !(1..=200).contains(&limit) || offset < 0 {
        return Err("Plugin limit must be 1-200 and offset must be non-negative".into());
    }
    let query = args.opt_str("query").unwrap_or("").trim().to_string();
    let wanted_folder = args.opt_str("folder").map(str::to_lowercase);
    let favorite = args.opt_bool("favorite").unwrap_or(false);
    let with_disabled = args.opt_bool("includeDisabled").unwrap_or(false);
    let installed = scan::installed();
    let auto = AutoFolders::new(&installed);
    let mut filtered: Vec<Descriptor> = installed
        .into_iter()
        .filter(|plugin| {
            format.is_none_or(|format| format == plugin.format.prefix())
                && kind.is_none_or(|kind| {
                    if kind == "instrument" {
                        plugin.instrument
                    } else {
                        plugin.effect
                    }
                })
                && (!favorite || library.favorites.contains(&plugin.id))
                && (with_disabled || !library.disabled.contains(&plugin.id))
                && wanted_folder
                    .as_ref()
                    .is_none_or(|f| folder(plugin, library, &auto).to_lowercase() == *f)
                && (query.is_empty() || relevance(&query, plugin, library, &auto).is_some())
        })
        .collect();
    if sort == "relevance" && !query.is_empty() {
        // Stable: equally good matches keep the library's order.
        filtered
            .sort_by_key(|d| std::cmp::Reverse(relevance(&query, d, library, &auto).unwrap_or(0)));
    }
    if sort == "recent" {
        let rank = |d: &Descriptor| {
            library
                .recent
                .iter()
                .position(|id| *id == d.id)
                .unwrap_or(usize::MAX)
        };
        filtered.retain(|d| rank(d) != usize::MAX);
        filtered.sort_by_key(rank);
    }
    let rows: Vec<Value> = if args.opt_bool("everyLayout").unwrap_or(false) {
        filtered.iter().map(|d| entry(d, library, &auto)).collect()
    } else {
        collapse_layouts(filtered, library, &auto)
    };
    let total = rows.len();
    let offset = usize::try_from(offset).map_err(|_| "Plugin offset is too large")?;
    let end = offset.saturating_add(limit as usize).min(total);
    let page: Vec<Value> = rows[offset.min(total)..end].to_vec();
    Ok(
        json!({"plugins":page,"total":total,"offset":offset,"limit":limit,
        "nextOffset":if end<total {Some(end)} else {None},"cachePath":scan::cache_path()}),
    )
}

/// Remember that a plugin was just loaded, for the browser's Recent folder. Only the window
/// keeps this list: a headless host (the CLI on a file, a bounce, a test) must never rewrite
/// the person's settings as a side effect of an edit, and concurrent headless hosts doing a
/// read-modify-write of one file would overwrite each other. Best effort even when live: a
/// read-only settings file must not fail the edit that loaded the plugin.
pub(crate) fn note_recent(host: &mut dyn Host, plugin_id: &str) {
    if host.mode() != "live" {
        return;
    }
    let mut settings = host.settings();
    if settings
        .plugins
        .recent
        .first()
        .is_some_and(|id| id == plugin_id)
    {
        return;
    }
    settings.plugins.recent.retain(|id| id != plugin_id);
    settings.plugins.recent.insert(0, plugin_id.to_string());
    settings.plugins.recent.truncate(12);
    let _ = host.update_settings(settings);
}

pub(crate) fn call(host: &mut dyn Host, name: &str, a: &Args) -> Result<Value> {
    let installed = scan::installed();
    let auto = AutoFolders::new(&installed);
    let known = |id: &str| -> Result<Descriptor> {
        installed
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .ok_or_else(|| format!("Unknown plugin `{id}`. Run plugin.scan, then plugin.list."))
    };
    match name {
        "plugin.folders" => {
            let library = host.settings().plugins;
            let mut names: Vec<String> = INSTRUMENT_FOLDERS
                .iter()
                .chain(EFFECT_FOLDERS)
                .map(|s| s.to_string())
                .collect();
            for custom in library.folders.values() {
                if !names.contains(custom) {
                    names.push(custom.clone());
                }
            }
            let folders: Vec<Value> = names
                .iter()
                .map(|name| {
                    // Counted as the browser shows them: one per plugin, not one per layout.
                    let mut rows = std::collections::HashSet::new();
                    let inside: Vec<&Descriptor> = installed
                        .iter()
                        .filter(|d| folder(d, &library, &auto) == *name)
                        .filter(|d| rows.insert(row_key(d)))
                        .collect();
                    json!({ "name": name,
                        "custom": !INSTRUMENT_FOLDERS.contains(&name.as_str()) && !EFFECT_FOLDERS.contains(&name.as_str()),
                        "instruments": inside.iter().filter(|d| d.instrument).count(),
                        "effects": inside.iter().filter(|d| d.effect).count() })
                })
                .filter(|f| f["instruments"] != 0 || f["effects"] != 0 || f["custom"] == true)
                .collect();
            Ok(
                json!({ "folders": folders, "favorites": library.favorites, "recent": library.recent }),
            )
        }
        "plugin.setFavorite" => {
            let plugin = known(a.str("pluginId")?)?;
            let favorite = a.bool("favorite")?;
            let mut settings = host.settings();
            settings.plugins.favorites.retain(|id| *id != plugin.id);
            if favorite {
                settings.plugins.favorites.push(plugin.id.clone());
            }
            let reply = entry(&plugin, &settings.plugins, &auto);
            host.update_settings(settings)?;
            Ok(reply)
        }
        "plugin.setFolder" => {
            let plugin = known(a.str("pluginId")?)?;
            let mut settings = host.settings();
            match a.opt_str("folder").map(str::trim).filter(|f| !f.is_empty()) {
                Some(name) if name == auto.of(&plugin) => {
                    settings.plugins.folders.remove(&plugin.id);
                }
                Some(name) => {
                    settings
                        .plugins
                        .folders
                        .insert(plugin.id.clone(), name.to_string());
                }
                None => {
                    settings.plugins.folders.remove(&plugin.id);
                }
            }
            settings.validate()?;
            let reply = entry(&plugin, &settings.plugins, &auto);
            host.update_settings(settings)?;
            Ok(reply)
        }
        "plugin.scaffold" => {
            let kind = a.opt_str("kind").unwrap_or("effect");
            if !["effect", "instrument"].contains(&kind) {
                return Err("kind must be effect or instrument".into());
            }
            crate::plugin_dev::scaffold(
                std::path::Path::new(a.str("path")?),
                a.str("name")?,
                kind == "instrument",
                a.opt_str("vendor").unwrap_or("My Studio"),
            )
        }
        "plugin.install" => {
            let source = std::path::Path::new(a.str("path")?);
            if source.join("plugin.toml").is_file() {
                // An lsuite bundle: plugin.toml and its library.
                let mut reply = crate::plugin_dev::install_bundle(source)?;
                reply["scan"] = crate::plugin_dev::rescan()?;
                return Ok(reply);
            }
            let extension = source
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if !["dylib", "so", "dll", "onplug"].contains(&extension.as_str()) {
                return Err("Install a built library: .dylib, .so, .dll or .onplug".into());
            }
            if !source.is_file() {
                return Err(format!(
                    "{} is not a file; build it with cargo build --release",
                    source.display()
                ));
            }
            let directory = scan::data_dir().join("plugins");
            std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
            let target = directory.join(source.file_name().ok_or("The path has no file name")?);
            // Copy beside the destination first so a failed copy never truncates a working plugin.
            let staged = target.with_extension(format!("{extension}.part"));
            std::fs::copy(source, &staged).map_err(|e| e.to_string())?;
            std::fs::rename(&staged, &target).map_err(|e| {
                let _ = std::fs::remove_file(&staged);
                e.to_string()
            })?;
            Ok(json!({ "installed": target, "next": "plugin.scan" }))
        }
        _ => Err(format!("Unknown command `{name}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_writes_a_crate_and_refuses_to_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("warm-drive");
        let reply = crate::plugin_dev::scaffold(&path, "Warm Drive 2", false, "Night Owl").unwrap();
        assert_eq!(reply["crate"], "warm-drive-2");
        assert_eq!(reply["pluginId"], "native:com.nightowl.warmdrive2");
        let lib = std::fs::read_to_string(path.join("src/lib.rs")).unwrap();
        assert!(
            lib.contains("pub struct WarmDrive2") && lib.contains("export_plugins!(WarmDrive2);")
        );
        assert!(!lib.contains("__"));
        assert!(crate::plugin_dev::scaffold(&path, "Warm Drive 2", false, "Night Owl").is_err());
        let synth = dir.path().join("synth");
        crate::plugin_dev::scaffold(&synth, "9 Lives", true, "x").unwrap();
        assert!(std::fs::read_to_string(synth.join("src/lib.rs"))
            .unwrap()
            .contains("pub struct P9Lives"));
    }

    #[test]
    fn well_known_third_party_names_find_their_folder() {
        let effect = |name: &str, category: &str| Descriptor {
            id: format!("au:{name}"),
            format: crate::plugin::Format::AudioUnit,
            name: name.into(),
            vendor: String::new(),
            path: String::new(),
            instrument: false,
            effect: true,
            category: category.into(),
        };
        for (name, category, folder) in [
            ("UAD Teletronix LA-2A Silver", "", "Dynamics"),
            ("PuigChild 660 (m)", "", "Dynamics"),
            ("UAD Neve 1073", "", "EQ & Filter"),
            ("SSLGChannel (s)", "", "Channel Strips"),
            ("Ozone 11 Stabilizer", "", "Mastering"),
            ("UAD EMT 250", "", "Space & Time"),
            ("Doubler2 (m)", "", "Modulation"),
            ("GTR Stomp 2 (m->s)", "", "Distortion"),
            ("Auto-Key", "", "Pitch"),
            ("soothe2", "", "Restoration"),
            ("Immersive Wrapper 7.0.6", "Surround", "Utility"),
            ("Pro-Q 3", "Fx|EQ", "EQ & Filter"),
            ("Channel EQ", "EQ & Filter", "EQ & Filter"),
            ("MaxxBass (m)", "Bass", "EQ & Filter"),
            ("CLA Drums (m->s)", "Drums", "Channel Strips"),
            // From what was in Other Effects on the owner's Mac, 2026-09-19.
            ("Pro-Q 3", "", "EQ & Filter"),
            ("Q1 (m)", "", "EQ & Filter"),
            ("Q10 Stereo", "", "EQ & Filter"),
            ("API-550A (s)", "", "EQ & Filter"),
            ("UAD Manley Massive Passive MST", "", "EQ & Filter"),
            ("AULowpass", "", "EQ & Filter"),
            ("CLA-76 (m)", "", "Dynamics"),
            ("Pro-MB", "", "Dynamics"),
            ("UADx Empirical Labs Distressor", "", "Dynamics"),
            ("UAD Tube-Tech CL 1B mk II", "", "Dynamics"),
            ("Renaissance Axx (s)", "", "Dynamics"),
            ("OneKnob Pumper (m)", "", "Dynamics"),
            ("Pro-L 2", "", "Dynamics"),
            ("S360 Panner (2->6)", "Spatial + Panner", "Utility"),
            ("UAD Moog Multimode Filter XL", "", "EQ & Filter"),
            ("UAD bx_masterdesk Classic", "", "Mastering"),
            ("Abbey Road Chambers (m->s)", "", "Space & Time"),
            ("UAD Lexicon 480L", "", "Space & Time"),
            ("Timeless 3", "", "Space & Time"),
            ("Pro-R 2", "", "Space & Time"),
            ("Brauer Motion (s)", "", "Modulation"),
            ("UAD Roland Dimension D", "", "Modulation"),
            ("OVox (s)", "", "Modulation"),
            ("UAD Friedman BE100", "", "Distortion"),
            ("UAD Studer A800", "", "Distortion"),
            ("Magma BB Tubes (s)", "", "Distortion"),
            ("Melodyne", "", "Pitch"),
            ("Vocal Bender (m)", "", "Pitch"),
            ("WNS (m)", "", "Restoration"),
            ("DeBreath (m)", "", "Restoration"),
            ("Abbey Road REDD.37.51 (s)", "", "Channel Strips"),
            ("UAD Avalon VT-737sp", "", "Channel Strips"),
            ("S1 Shuffler (s)", "", "Utility"),
            ("PAZ- Position (s)", "", "Utility"),
            ("Center (s)", "", "Utility"),
            // Names too plain to guess from stay where the user can see and re-file them.
            ("Warm", "", "Other Effects"),
            // A word inside a product name must not win over the product: Marshall holds
            // "hall", Amp Room holds "room", Tape holds "tap", 33609 holds "360".
            ("UAD Marshall Plexi Classic", "", "Distortion"),
            ("UAD Softube Amp Room Half-Stack", "", "Distortion"),
            ("Kramer Tape (m)", "", "Distortion"),
            ("UADx Oxide Tape Recorder", "", "Distortion"),
            ("UAD Neve 33609 C", "", "Dynamics"),
            ("Kramer PIE (s)", "", "Dynamics"),
            ("Kramer HLS (s)", "", "EQ & Filter"),
            ("EKramer VC (m)", "", "Channel Strips"),
            ("UAD Neve Preamp", "", "Channel Strips"),
            ("ValhallaSpaceModulator", "", "Modulation"),
            ("SuperTap 6-Taps (s)", "", "Space & Time"),
            // The name decides before the category: the VST3 of these says something else.
            ("Abbey Road Vinyl (s)", "Fx|Modulation", "Distortion"),
            ("UAD Little Labs IBP", "Fx|Delay", "Utility"),
            ("Vocal Rider (m)", "Fx|Channel Strip", "Dynamics"),
            ("Waves Tune", "Fx|Pitch Shift", "Pitch"),
            ("Obscure Thing", "Fx|Reverb", "Space & Time"),
            // Only ryolune's own plugins name their folder by category.
            ("Ozone 11 Dynamics", "Dynamics", "Mastering"),
            ("NLS Channel (s)", "Distortion", "Channel Strips"),
        ] {
            assert_eq!(automatic_folder(&effect(name, category)), folder, "{name}");
        }
        let by = |vendor: &str, name: &str| Descriptor {
            vendor: vendor.into(),
            ..effect(name, "")
        };
        for (vendor, name, folder) in [
            ("Antares", "Warm", "Distortion"),
            ("Antares", "Punch", "Dynamics"),
            ("Antares", "Mic Mod", "Channel Strips"),
            ("Antares", "Metamorph", "Pitch"),
            ("FabFilter", "Micro", "EQ & Filter"),
            ("Xfer Records", "Serum 2 FX", "Modulation"),
            ("Waves", "Waves Gemstones (m->s)", "Modulation"),
        ] {
            assert_eq!(automatic_folder(&by(vendor, name)), folder, "{name}");
        }
    }

    #[test]
    fn every_format_of_one_product_shares_its_folder() {
        let plugin = |format, id: &str, name: &str, category: &str| Descriptor {
            id: id.into(),
            format,
            name: name.into(),
            vendor: "FabFilter".into(),
            path: String::new(),
            instrument: true,
            effect: false,
            category: category.into(),
        };
        use crate::plugin::Format;
        let au = plugin(Format::AudioUnit, "au:twin", "Twin 3", "");
        let clap = plugin(Format::Clap, "clap:twin", "Twin 3", "");
        let vst3 = plugin(Format::Vst3, "vst3:twin", "Twin 3", "Synth");
        assert_eq!(automatic_folder(&au), "Other Instruments");
        let auto = AutoFolders::new(&[au.clone(), clap.clone(), vst3.clone()]);
        for d in [&au, &clap, &vst3] {
            assert_eq!(auto.of(d), "Synths", "{}", d.id);
        }
        // An effect of the same name is another product.
        let effect = Descriptor {
            instrument: false,
            effect: true,
            ..au.clone()
        };
        assert_eq!(auto.of(&effect), "Other Effects");
        let library = Plugins::default();
        assert_eq!(folder(&au, &library, &auto), "Synths");
    }

    #[test]
    fn channel_layouts_of_one_plugin_share_a_browser_row() {
        assert_eq!(layout_of("API-2500 (m->s)"), ("API-2500", Some("m->s")));
        assert_eq!(
            layout_of("S360 Panner (2->6)"),
            ("S360 Panner", Some("2->6"))
        );
        assert_eq!(
            layout_of("PS22 Spread(10) (s)"),
            ("PS22 Spread(10)", Some("s"))
        );
        assert_eq!(layout_of("Pro-Q 3"), ("Pro-Q 3", None));
        assert_eq!(layout_of("C1 comp Mono"), ("C1 comp", Some("m")));
        assert_eq!(
            layout_of("Doubler2 Mono/Stereo"),
            ("Doubler2", Some("m->s"))
        );
        assert_eq!(layout_of("Stereo"), ("Stereo", None));
        assert_eq!(layout_of("Reverb (Hall)"), ("Reverb (Hall)", None));
        let au = |name: &str, vendor: &str| Descriptor {
            id: format!("au:{name}"),
            format: crate::plugin::Format::AudioUnit,
            name: name.into(),
            vendor: vendor.into(),
            path: String::new(),
            instrument: false,
            effect: true,
            category: String::new(),
        };
        let mut library = Plugins::default();
        library.favorites.push("au:C1 comp (m)".into());
        let rows = collapse_layouts(
            vec![
                au("C1 comp (m)", "Waves"),
                au("C1 comp (m->s)", "Waves"),
                au("C1 comp (s)", "Waves"),
                au("Doubler2 (m)", "Waves"),
                au("Doubler2 (m->s)", "Waves"),
                au("Pro-Q 3", "FabFilter"),
                au("C1 comp (s)", "Someone Else"),
                au("CODEX (0->2)", "Waves"),
            ],
            &library,
            &AutoFolders::new(&[]),
        );
        let seen: Vec<(&str, &str)> = rows
            .iter()
            .map(|r| (r["name"].as_str().unwrap(), r["id"].as_str().unwrap()))
            .collect();
        assert_eq!(
            seen,
            [
                ("C1 comp", "au:C1 comp (s)"),
                // No stereo layout: mono in, stereo out is the one a stereo track wants.
                ("Doubler2", "au:Doubler2 (m->s)"),
                ("Pro-Q 3", "au:Pro-Q 3"),
                // One layout alone still reads as the plugin's name.
                ("C1 comp", "au:C1 comp (s)"),
                ("CODEX", "au:CODEX (0->2)"),
            ]
        );
        assert_eq!(rows[0]["layouts"].as_array().unwrap().len(), 3);
        assert_eq!(rows[0]["layouts"][0]["layout"], "m");
        assert_eq!(
            rows[0]["favorite"], true,
            "a star on any layout stars the row"
        );
        assert!(rows[2].get("layouts").is_none());
    }

    #[test]
    fn one_row_per_plugin_across_formats() {
        let plugin = |format: crate::plugin::Format, prefix: &str, name: &str| Descriptor {
            id: format!("{prefix}:{name}"),
            format,
            name: name.into(),
            vendor: "FabFilter".into(),
            path: String::new(),
            instrument: false,
            effect: true,
            category: String::new(),
        };
        use crate::plugin::Format::{AudioUnit, Clap, Vst3};
        let rows = collapse_layouts(
            vec![
                plugin(AudioUnit, "au", "Pro-Q 3"),
                plugin(Vst3, "vst3", "Pro-Q 3"),
                plugin(Clap, "clap", "Pro-Q 3"),
                plugin(Vst3, "vst3", "Pro-R"),
            ],
            &Plugins::default(),
            &AutoFolders::new(&[]),
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0]["id"], "clap:Pro-Q 3",
            "CLAP first, as the agent loads it"
        );
        let formats: Vec<&str> = rows[0]["formats"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["format"].as_str().unwrap())
            .collect();
        assert_eq!(formats, ["clap", "vst3", "au"]);
        assert!(rows[1].get("formats").is_none());
    }

    #[test]
    fn every_stock_plugin_lands_in_a_named_folder() {
        for d in crate::stock::descriptors() {
            let folder = automatic_folder(&d);
            assert!(
                !folder.starts_with("Other"),
                "{} fell into {folder}",
                d.name
            );
            let list = if d.instrument {
                INSTRUMENT_FOLDERS
            } else {
                EFFECT_FOLDERS
            };
            assert!(list.contains(&folder), "{} -> {folder}", d.name);
        }
    }
}
