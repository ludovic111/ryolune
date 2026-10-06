//! What a bundle says about its plugins: the LV2 1.18 data model read from `manifest.ttl` and
//! the files it points to with `rdfs:seeAlso`. Plugins, their ports and port properties,
//! required features, the default state and presets (`pset:Preset`, in the plugin's bundle
//! or in preset bundles of their own, which is where other hosts save them).

use super::ffi::uri;
use super::turtle::{url_to_path, Graph, Node, RDF};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Input,
    Output,
}
#[derive(Clone, Debug, PartialEq)]
pub enum PortKind {
    Audio,
    Control,
    Cv,
    /// An atom port; `midi` when it takes or gives `midi:MidiEvent`s.
    Atom {
        midi: bool,
        min_size: usize,
    },
    /// A port type the host does not know (the old event extension, for instance).
    Unknown(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Port {
    pub index: u32,
    pub symbol: String,
    pub name: String,
    pub direction: Direction,
    pub kind: PortKind,
    pub default: Option<f32>,
    pub minimum: Option<f32>,
    pub maximum: Option<f32>,
    pub integer: bool,
    pub toggled: bool,
    pub enumeration: bool,
    pub logarithmic: bool,
    /// Bounds are fractions of the sample rate.
    pub sample_rate: bool,
    pub scale_points: Vec<(f32, String)>,
    pub unit: String,
    /// `lv2:designation` (`lv2:latency`, `lv2:enabled`, `time:beatsPerMinute`, …).
    pub designation: Option<String>,
    pub reports_latency: bool,
    pub optional: bool,
    pub hidden: bool,
}
impl Port {
    pub fn is_control_input(&self) -> bool {
        self.kind == PortKind::Control && self.direction == Direction::Input
    }
    pub fn designated(&self, what: &str) -> bool {
        self.designation.as_deref() == Some(what)
    }
}

/// One `state:state` (a preset's, or a plugin's default) as typed properties.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StateProperties(pub Vec<Property>);
#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub key: String,
    /// The value's type URI (`atom:Float`, `atom:String`, `atom:Path`, a plugin's own…).
    pub type_uri: String,
    /// The value as the atom body the plugin expects.
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PluginData {
    pub uri: String,
    pub name: String,
    pub vendor: String,
    /// `lv2:` classes other than `lv2:Plugin` (`ReverbPlugin`, `InstrumentPlugin`, …).
    pub classes: Vec<String>,
    pub bundle: PathBuf,
    pub binary: PathBuf,
    pub ports: Vec<Port>,
    pub required_features: Vec<String>,
    pub default_state: Option<StateProperties>,
}
impl PluginData {
    pub fn audio(&self, direction: Direction) -> usize {
        self.ports
            .iter()
            .filter(|p| p.kind == PortKind::Audio && p.direction == direction)
            .count()
    }
    pub fn midi_input(&self) -> bool {
        self.ports.iter().any(|p| {
            p.direction == Direction::Input && matches!(p.kind, PortKind::Atom { midi: true, .. })
        })
    }
    pub fn instrument(&self) -> bool {
        self.classes.iter().any(|c| c == "InstrumentPlugin")
            || (self.midi_input()
                && self.audio(Direction::Input) == 0
                && self.audio(Direction::Output) > 0)
    }
    /// A ryolune category word from the plugin's class.
    pub fn category(&self) -> String {
        const NAMES: &[(&str, &str)] = &[
            ("ReverbPlugin", "Reverb"),
            ("DelayPlugin", "Delay"),
            ("CompressorPlugin", "Compressor"),
            ("LimiterPlugin", "Limiter"),
            ("GatePlugin", "Gate"),
            ("ExpanderPlugin", "Expander"),
            ("EnvelopePlugin", "Envelope"),
            ("DynamicsPlugin", "Dynamics"),
            ("AmplifierPlugin", "Amplifier"),
            ("WaveshaperPlugin", "Distortion"),
            ("DistortionPlugin", "Distortion"),
            ("ParaEQPlugin", "EQ"),
            ("MultiEQPlugin", "EQ"),
            ("EQPlugin", "EQ"),
            ("LowpassPlugin", "Filter"),
            ("HighpassPlugin", "Filter"),
            ("BandpassPlugin", "Filter"),
            ("CombPlugin", "Filter"),
            ("AllpassPlugin", "Filter"),
            ("FilterPlugin", "Filter"),
            ("ChorusPlugin", "Chorus"),
            ("FlangerPlugin", "Flanger"),
            ("PhaserPlugin", "Phaser"),
            ("ModulatorPlugin", "Modulation"),
            ("PitchPlugin", "Pitch"),
            ("SpatialPlugin", "Spatial"),
            ("SpectralPlugin", "Spectral"),
            ("SimulatorPlugin", "Simulator"),
            ("AnalyserPlugin", "Analyzer"),
            ("MixerPlugin", "Mixer"),
            ("ConverterPlugin", "Utility"),
            ("FunctionPlugin", "Utility"),
            ("UtilityPlugin", "Utility"),
            ("OscillatorPlugin", "Oscillator"),
            ("ConstantPlugin", "Generator"),
            ("GeneratorPlugin", "Generator"),
            ("InstrumentPlugin", "Instrument"),
            ("MIDIPlugin", "MIDI"),
        ];
        NAMES
            .iter()
            .find(|(class, _)| self.classes.iter().any(|c| c == class))
            .map(|(_, name)| name.to_string())
            .unwrap_or_default()
    }
}

fn core(name: &str) -> String {
    format!("{}{name}", uri::CORE)
}
fn rdf_type() -> String {
    format!("{RDF}type")
}
fn see_also() -> String {
    format!("{}seeAlso", uri::RDFS)
}

/// A bundle's manifest, read.
pub struct Bundle {
    pub dir: PathBuf,
    pub graph: Graph,
}
impl Bundle {
    pub fn open(dir: &Path) -> Result<Self, String> {
        let manifest = dir.join("manifest.ttl");
        if !manifest.is_file() {
            return Err(format!("{} has no manifest.ttl", dir.display()));
        }
        let mut graph = Graph::new();
        graph.load(&manifest)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            graph,
        })
    }
    /// The plugin URIs the manifest declares, in order.
    pub fn plugin_uris(&self) -> Vec<String> {
        self.graph
            .instances_of(&core("Plugin"))
            .iter()
            .filter_map(|n| n.as_iri().map(str::to_string))
            .collect()
    }
    /// Read the files the manifest names for `subject` (inside this bundle only).
    fn read_more(&mut self, subject: &Node) -> Result<(), String> {
        let files: Vec<PathBuf> = self
            .graph
            .objects(subject, &see_also())
            .filter_map(|n| n.as_iri().and_then(url_to_path))
            .filter(|p| p.starts_with(&self.dir))
            .collect();
        for file in files {
            if file.is_file() {
                self.graph.load(&file)?;
            }
        }
        Ok(())
    }
    /// Every plugin of the bundle, fully described.
    pub fn plugins(&mut self) -> Result<Vec<PluginData>, String> {
        let uris = self.plugin_uris();
        for u in &uris {
            self.read_more(&Node::iri(u.clone()))?;
        }
        uris.iter().map(|u| self.describe(u)).collect()
    }
    pub fn plugin(&mut self, plugin_uri: &str) -> Result<PluginData, String> {
        let node = Node::iri(plugin_uri);
        if !self.graph.has(&node, &rdf_type(), &core("Plugin")) {
            return Err(format!(
                "{} does not declare the LV2 plugin {plugin_uri}",
                self.dir.display()
            ));
        }
        self.read_more(&node)?;
        self.describe(plugin_uri)
    }
    fn describe(&self, plugin_uri: &str) -> Result<PluginData, String> {
        let g = &self.graph;
        let node = Node::iri(plugin_uri);
        let binary = g
            .object(&node, &core("binary"))
            .and_then(|n| n.as_iri())
            .and_then(url_to_path)
            .ok_or_else(|| format!("{plugin_uri} has no lv2:binary"))?;
        let name = text(g, &node, &format!("{}name", uri::DOAP))
            .or_else(|| text(g, &node, &format!("{}label", uri::RDFS)))
            .unwrap_or_else(|| plugin_uri.rsplit(['/', '#']).next().unwrap_or(plugin_uri).to_string());
        let vendor = vendor(g, &node).unwrap_or_else(|| host_of(plugin_uri));
        let classes = g
            .objects(&node, &rdf_type())
            .filter_map(|n| n.as_iri())
            .filter_map(|c| c.strip_prefix(uri::CORE))
            .filter(|c| *c != "Plugin")
            .map(str::to_string)
            .collect();
        let required_features = g
            .objects(&node, &core("requiredFeature"))
            .filter_map(|n| n.as_iri().map(str::to_string))
            .collect();
        let mut ports = vec![];
        for port in g.objects(&node, &core("port")) {
            ports.push(read_port(g, port).map_err(|e| format!("{name}: {e}"))?);
        }
        ports.sort_by_key(|p| p.index);
        for (i, port) in ports.iter().enumerate() {
            if port.index as usize != i {
                return Err(format!(
                    "{name}: port indices must run from 0 without gaps (found {} at {i})",
                    port.index
                ));
            }
        }
        let default_state = g
            .object(&node, uri::STATE_STATE)
            .map(|state| state_properties(g, state));
        Ok(PluginData {
            uri: plugin_uri.to_string(),
            name,
            vendor,
            classes,
            bundle: self.dir.clone(),
            binary,
            ports,
            required_features,
            default_state,
        })
    }
}

/// Prefer an English or untagged literal.
fn text(g: &Graph, node: &Node, predicate: &str) -> Option<String> {
    let mut best: Option<(u8, &str)> = None;
    for n in g.objects(node, predicate) {
        let Some(l) = n.as_literal() else { continue };
        let rank = match l.lang.as_deref() {
            None => 0,
            Some(lang) if lang.starts_with("en") => 1,
            Some(_) => 2,
        };
        if best.is_none_or(|(r, _)| rank < r) {
            best = Some((rank, &l.value));
        }
    }
    best.map(|(_, t)| t.trim().to_string()).filter(|t| !t.is_empty())
}
fn vendor(g: &Graph, plugin: &Node) -> Option<String> {
    let person_name = |who: &Node| text(g, who, uri::FOAF_NAME);
    for predicate in ["maintainer", "developer"] {
        let p = format!("{}{predicate}", uri::DOAP);
        if let Some(name) = g.objects(plugin, &p).find_map(person_name) {
            return Some(name);
        }
    }
    // Or through the plugin's project.
    let project = g.object(plugin, &core("project"))?;
    for predicate in ["maintainer", "developer"] {
        let p = format!("{}{predicate}", uri::DOAP);
        if let Some(name) = g.objects(project, &p).find_map(person_name) {
            return Some(name);
        }
    }
    text(g, project, &format!("{}name", uri::DOAP))
}
fn host_of(iri: &str) -> String {
    iri.split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("")
        .trim_start_matches("www.")
        .to_string()
}

fn read_port(g: &Graph, port: &Node) -> Result<Port, String> {
    let index = g
        .object(port, &core("index"))
        .and_then(Node::as_f64)
        .filter(|i| *i >= 0.0 && i.fract() == 0.0)
        .ok_or("a port has no lv2:index")? as u32;
    let symbol = text(g, port, &core("symbol")).ok_or_else(|| format!("port {index} has no lv2:symbol"))?;
    let name = text(g, port, &core("name")).unwrap_or_else(|| symbol.clone());
    let types: Vec<&str> = g.objects(port, &rdf_type()).filter_map(Node::as_iri).collect();
    let is = |t: &str| types.contains(&t);
    let direction = if is(&core("InputPort")) {
        Direction::Input
    } else if is(&core("OutputPort")) {
        Direction::Output
    } else {
        return Err(format!("port `{symbol}` is neither an input nor an output"));
    };
    let atom_port = format!("{}AtomPort", uri::ATOM);
    let kind = if is(&core("AudioPort")) {
        PortKind::Audio
    } else if is(&core("ControlPort")) {
        PortKind::Control
    } else if is(&core("CVPort")) {
        PortKind::Cv
    } else if is(&atom_port) {
        let supports = format!("{}supports", uri::ATOM);
        PortKind::Atom {
            midi: g
                .objects(port, &supports)
                .any(|n| n.as_iri() == Some(uri::MIDI_EVENT)),
            min_size: g
                .object(port, uri::MINIMUM_SIZE)
                .and_then(Node::as_f64)
                .map_or(0, |v| v.max(0.0) as usize),
        }
    } else {
        PortKind::Unknown(
            types
                .iter()
                .find(|t| !t.ends_with("InputPort") && !t.ends_with("OutputPort"))
                .map_or("no type", |t| t)
                .to_string(),
        )
    };
    let number = |p: &str| g.object(port, &core(p)).and_then(Node::as_f64).map(|v| v as f32);
    let properties: Vec<&str> = g
        .objects(port, &core("portProperty"))
        .filter_map(Node::as_iri)
        .collect();
    let property = |name: &str| properties.contains(&core(name).as_str());
    let pprop = |name: &str| properties.contains(&format!("{}{name}", uri::PORT_PROPS).as_str());
    let scale_points = g
        .objects(port, &core("scalePoint"))
        .filter_map(|point| {
            let value = g.object(point, &format!("{RDF}value"))?.as_f64()? as f32;
            let label = text(g, point, &format!("{}label", uri::RDFS))?;
            Some((value, label))
        })
        .collect::<Vec<_>>();
    let unit_uri = g
        .object(port, &format!("{}unit", uri::UNITS))
        .and_then(|u| match u {
            Node::Iri(iri) => Some(iri.clone()),
            // A unit described in place: `units:unit [ units:symbol "x" ]`.
            other => text(g, other, &format!("{}symbol", uri::UNITS)).map(|s| format!("symbol:{s}")),
        });
    Ok(Port {
        index,
        name,
        direction,
        kind,
        default: number("default"),
        minimum: number("minimum"),
        maximum: number("maximum"),
        integer: property("integer"),
        toggled: property("toggled"),
        enumeration: property("enumeration"),
        logarithmic: pprop("logarithmic"),
        sample_rate: property("sampleRate"),
        scale_points,
        unit: unit_uri.as_deref().map(unit_symbol).unwrap_or_default(),
        designation: g
            .object(port, &core("designation"))
            .and_then(Node::as_iri)
            .map(str::to_string),
        reports_latency: property("reportsLatency"),
        optional: property("connectionOptional"),
        hidden: pprop("notOnGUI"),
        symbol,
    })
}
fn unit_symbol(unit: &str) -> String {
    if let Some(symbol) = unit.strip_prefix("symbol:") {
        return symbol.to_string();
    }
    let Some(name) = unit.strip_prefix(uri::UNITS) else {
        return String::new();
    };
    match name {
        "db" => "dB",
        "hz" => "Hz",
        "khz" => "kHz",
        "mhz" => "MHz",
        "ms" => "ms",
        "s" => "s",
        "min" => "min",
        "pc" => "%",
        "bpm" => "BPM",
        "beat" => "beats",
        "bar" => "bars",
        "cent" => "ct",
        "semitone12TET" => "st",
        "oct" => "oct",
        "degree" => "°",
        "frame" => "frames",
        "m" => "m",
        "cm" => "cm",
        "mm" => "mm",
        "km" => "km",
        "inch" => "in",
        "mile" => "mi",
        "coef" => "",
        "midiNote" => "",
        _ => "",
    }
    .to_string()
}

/// Read a `state:state [ key value … ]` node into atom-typed properties. Numbers and strings
/// become the matching atom types, `xsd:base64Binary` a chunk, IRIs paths or URIDs; other
/// literal datatypes keep their datatype as the type and their text as the value.
pub fn state_properties(g: &Graph, state: &Node) -> StateProperties {
    let atom = |name: &str| format!("{}{name}", uri::ATOM);
    let xsd = "http://www.w3.org/2001/XMLSchema#";
    let mut out = vec![];
    for t in g.about(state) {
        let (type_uri, value) = match &t.object {
            // As sratom reads them: a file IRI is a path, any other IRI a URID (its text
            // here; the host maps it when it restores).
            Node::Iri(iri) => match url_to_path(iri) {
                Some(path) => (atom("Path"), nul_terminated(&path.to_string_lossy())),
                None => (atom("URID"), nul_terminated(iri)),
            },
            Node::Blank(_) => continue,
            Node::Literal(l) => {
                let value = l.value.trim();
                match l.datatype.as_deref().map(|d| d.strip_prefix(xsd).unwrap_or(d)) {
                    Some("int") | Some("integer") | Some("short") => {
                        (atom("Int"), value.parse::<i32>().unwrap_or(0).to_ne_bytes().to_vec())
                    }
                    Some("long") => (atom("Long"), value.parse::<i64>().unwrap_or(0).to_ne_bytes().to_vec()),
                    Some("float") | Some("decimal") => {
                        (atom("Float"), value.parse::<f32>().unwrap_or(0.0).to_ne_bytes().to_vec())
                    }
                    Some("double") => (atom("Double"), value.parse::<f64>().unwrap_or(0.0).to_ne_bytes().to_vec()),
                    Some("boolean") => (atom("Bool"), ((value == "true") as i32).to_ne_bytes().to_vec()),
                    Some("base64Binary") => {
                        use base64::Engine;
                        let bytes = base64::engine::general_purpose::STANDARD
                            .decode(value.split_whitespace().collect::<String>())
                            .unwrap_or_default();
                        (atom("Chunk"), bytes)
                    }
                    Some("string") | None => (atom("String"), nul_terminated(&l.value)),
                    Some(other) => {
                        // A datatype of the plugin's own or atom:Path: the text, as a string.
                        let full = l.datatype.clone().unwrap_or_else(|| other.to_string());
                        (full, nul_terminated(&l.value))
                    }
                }
            }
        };
        out.push(Property {
            key: t.predicate.clone(),
            type_uri,
            value,
        });
    }
    StateProperties(out)
}
fn nul_terminated(text: &str) -> Vec<u8> {
    let mut v = text.as_bytes().to_vec();
    v.push(0);
    v
}

/// One preset a plugin can load.
#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub uri: String,
    pub label: String,
    /// The bundle that declares it.
    pub bundle: PathBuf,
}
/// A preset, read: port values by symbol and the plugin state it carries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresetData {
    pub ports: Vec<(String, f32)>,
    pub state: Option<StateProperties>,
}

/// Every preset for `plugin_uri` in `bundles` (each bundle's manifest, and the files it
/// names for the preset when the label is not in the manifest), sorted by label.
pub fn presets(plugin_uri: &str, bundles: &[PathBuf]) -> Vec<Preset> {
    let applies = core("appliesTo");
    let mut out: Vec<Preset> = vec![];
    for dir in bundles {
        let Ok(mut bundle) = Bundle::open(dir) else {
            continue;
        };
        let presets: Vec<Node> = bundle
            .graph
            .instances_of(uri::PRESET)
            .into_iter()
            .filter(|p| bundle.graph.objects(p, &applies).any(|o| o.as_iri() == Some(plugin_uri)))
            .collect();
        for preset in presets {
            let Some(preset_uri) = preset.as_iri().map(str::to_string) else {
                continue;
            };
            let label_of = |g: &Graph| text(g, &preset, &format!("{}label", uri::RDFS));
            let label = match label_of(&bundle.graph) {
                Some(label) => Some(label),
                None => {
                    let _ = bundle.read_more(&preset);
                    label_of(&bundle.graph)
                }
            };
            let label = label.unwrap_or_else(|| {
                preset_uri
                    .rsplit(['/', '#'])
                    .next()
                    .unwrap_or(&preset_uri)
                    .to_string()
            });
            if !out.iter().any(|p| p.uri == preset_uri) {
                out.push(Preset {
                    uri: preset_uri,
                    label,
                    bundle: dir.clone(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    out
}

/// Read a preset's port values and state.
pub fn load_preset(preset: &Preset) -> Result<PresetData, String> {
    let mut bundle = Bundle::open(&preset.bundle)?;
    let node = Node::iri(preset.uri.clone());
    bundle.read_more(&node)?;
    // A preset IRI that is itself a file (as other hosts save them) is read too.
    if let Some(path) = url_to_path(&preset.uri) {
        if path.is_file() && path.starts_with(&bundle.dir) {
            bundle.graph.load(&path)?;
        }
    }
    let g = &bundle.graph;
    let mut data = PresetData::default();
    for port in g.objects(&node, &core("port")) {
        let symbol = text(g, port, &core("symbol"));
        let value = g.object(port, uri::PRESET_VALUE).and_then(Node::as_f64);
        if let (Some(symbol), Some(value)) = (symbol, value) {
            data.ports.push((symbol, value as f32));
        }
    }
    data.state = g.object(&node, uri::STATE_STATE).map(|s| state_properties(g, s));
    if data.ports.is_empty() && data.state.is_none() {
        return Err(format!("The preset {} has no values", preset.label));
    }
    Ok(data)
}
