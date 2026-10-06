//! Native audio export and MIDI file settings. File choosers run on workers;
//! file operations use the same asynchronous registry path as CLI and MCP.

use crate::app::Ryolune;
use ryolune_engine::{model::Session, Result};
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::PathBuf, sync::mpsc};

pub(crate) const SAMPLE_RATES: [u32; 3] = [44100, 48000, 96000];
pub(crate) const FORMATS: [(&str, &str); 3] = [
    ("pcm16", "16-bit PCM"),
    ("pcm24", "24-bit PCM"),
    ("float32", "32-bit float"),
];
/// File types a mix or stems are written as: (extension, what the dialog calls it).
pub(crate) const CONTAINERS: [(&str, &str); 4] = [
    ("wav", "WAV"),
    ("aiff", "AIFF"),
    ("flac", "FLAC (lossless, smaller)"),
    ("ogg", "Ogg Vorbis (compressed)"),
];
/// Vorbis quality steps and the stereo bitrate each comes to, roughly.
pub(crate) const OGG_QUALITIES: [(f64, &str); 5] = [
    (0.2, "Small (about 96 kbit/s)"),
    (0.4, "Good (about 128 kbit/s)"),
    (0.6, "High (about 192 kbit/s)"),
    (0.8, "Very high (about 256 kbit/s)"),
    (1.0, "Maximum (about 500 kbit/s)"),
];

/// What "Export for Another App" can write: (format id, what the sheet calls it).
pub(crate) const APP_FORMATS: [(&str, &str); 5] = [
    ("dawproject", "DAWproject (.dawproject)"),
    ("package", "MIDI and stems (a folder)"),
    ("midi", "MIDI file (.mid)"),
    ("audio", "The mix (.wav)"),
    ("stems", "Stems (a folder)"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Audio,
    MidiImport,
    MidiExport,
    /// File › Import from Another App…: `session.importFrom`.
    AppImport,
    /// File › Export for Another App…: `session.exportTo`.
    AppExport,
}

pub(crate) struct ExportDialog {
    pub(crate) open: bool,
    pub(crate) mode: Mode,
    pub(crate) sample_rate: u32,
    pub(crate) format: usize,
    /// Index in [`CONTAINERS`]: the file type of the mix or of every stem.
    pub(crate) container: usize,
    /// Ogg Vorbis quality, 0-1 (the other types use `format` and `dither`).
    pub(crate) quality: f64,
    pub(crate) dither: bool,
    pub(crate) range: bool,
    pub(crate) start_bar: f64,
    pub(crate) end_bar: f64,
    pub(crate) tail_seconds: f64,
    pub(crate) stems: bool,
    pub(crate) include_effects: bool,
    pub(crate) include_master: bool,
    pub(crate) tracks: BTreeSet<String>,
    pub(crate) folder_name: String,
    pub(crate) import_tempo: bool,
    /// The app a song comes from or goes to (index in `interop::apps::APPS`).
    pub(crate) app: Option<usize>,
    /// Index in [`APP_FORMATS`]: what Export for Another App writes.
    pub(crate) app_format: usize,
    pub(crate) chooser: Option<Chooser>,
    pub(crate) awaiting: Option<String>,
    pub(crate) report: Option<String>,
    pub(crate) error: Option<String>,
}

impl Default for ExportDialog {
    fn default() -> Self {
        Self {
            open: false,
            mode: Mode::Audio,
            sample_rate: 48000,
            format: 1,
            container: 0,
            quality: 0.6,
            dither: true,
            range: false,
            start_bar: 1.0,
            end_bar: 5.0,
            tail_seconds: 3.0,
            stems: false,
            include_effects: true,
            include_master: false,
            tracks: BTreeSet::new(),
            folder_name: "Song stems".into(),
            import_tempo: false,
            app: None,
            app_format: 0,
            chooser: None,
            awaiting: None,
            report: None,
            error: None,
        }
    }
}

pub(crate) struct Chooser {
    receiver: mpsc::Receiver<Option<Vec<PathBuf>>>,
    request: PreparedCommand,
    path_key: &'static str,
}

pub(crate) struct PreparedCommand {
    pub(crate) method: &'static str,
    pub(crate) params: Value,
}

impl ExportDialog {
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    fn show(&mut self, mode: Mode, session: &Session, position_beats: f64) {
        self.open = true;
        if self.busy() {
            return;
        }
        self.mode = mode;
        self.report = None;
        self.error = None;
        self.start_bar = if mode == Mode::MidiImport {
            position_beats / session.beats_per_bar() + 1.0
        } else {
            1.0
        };
        self.end_bar = session.end_bar().max(1.0) + 1.0;
        self.folder_name = format!("{} stems", session.name.trim_end_matches(".ryolune"));
        self.tracks = session
            .tracks
            .iter()
            .filter(|track| mode != Mode::MidiExport || track.kind == "midi")
            .map(|track| track.id.clone())
            .collect();
    }

    pub(crate) fn busy(&self) -> bool {
        self.chooser.is_some() || self.awaiting.is_some()
    }
    pub(crate) fn close(&mut self) {
        self.open = false;
    }
    /// The file chooser is open.
    pub(crate) fn choosing(&self) -> bool {
        self.chooser.is_some()
    }
    /// Why the form cannot run yet, as the dialog says it.
    pub(crate) fn problem(&self, session: &Session) -> Option<String> {
        self.request(session).err()
    }

    pub(crate) fn request(&self, session: &Session) -> Result<PreparedCommand> {
        let selected: Vec<&str> = session
            .tracks
            .iter()
            .filter(|track| self.tracks.contains(&track.id))
            .filter(|track| self.mode != Mode::MidiExport || track.kind == "midi")
            .map(|track| track.id.as_str())
            .collect();
        let request = match self.mode {
            Mode::AppImport => PreparedCommand {
                method: "session.importFrom",
                params: json!({}),
            },
            Mode::AppExport => {
                let format = APP_FORMATS[self.app_format.min(APP_FORMATS.len() - 1)].0;
                if format == "midi" && !session.tracks.iter().any(|t| t.kind == "midi") {
                    return Err("This song has no instrument tracks to write as MIDI.".into());
                }
                let mut params = json!({ "format": format });
                if let Some(app) = self
                    .app
                    .and_then(|i| ryolune_engine::interop::apps::APPS.get(i))
                {
                    params["app"] = json!(app.id);
                }
                PreparedCommand {
                    method: "session.exportTo",
                    params,
                }
            }
            Mode::MidiImport => {
                if !self.start_bar.is_finite() || self.start_bar < 1.0 {
                    return Err("The start bar must be at least 1.".into());
                }
                PreparedCommand {
                    method: "session.importMidi",
                    params: json!({"startBar": self.start_bar - 1.0, "importTempo": self.import_tempo}),
                }
            }
            Mode::MidiExport => {
                if selected.is_empty() {
                    return Err("Select at least one instrument track.".into());
                }
                PreparedCommand {
                    method: "session.exportMidi",
                    params: json!({"trackIds":selected}),
                }
            }
            Mode::Audio => {
                if !SAMPLE_RATES.contains(&self.sample_rate)
                    || self.format >= FORMATS.len()
                    || self.container >= CONTAINERS.len()
                {
                    return Err("Choose a supported sample rate, file type and encoding.".into());
                }
                let container = CONTAINERS[self.container].0;
                let lossy = container == "ogg";
                if !lossy && container != "wav" && FORMATS[self.format].0 == "float32" {
                    return Err(
                        "32-bit float is written to WAV only: choose 16 or 24-bit PCM.".into(),
                    );
                }
                if lossy && !(self.quality.is_finite() && (0.0..=1.0).contains(&self.quality)) {
                    return Err("The Ogg Vorbis quality must be between 0 and 1.".into());
                }
                if !self.tail_seconds.is_finite() || !(0.0..=120.0).contains(&self.tail_seconds) {
                    return Err("The effect tail must be between 0 and 120 seconds.".into());
                }
                if self.range
                    && (!self.start_bar.is_finite()
                        || !self.end_bar.is_finite()
                        || self.start_bar < 1.0
                        || self.end_bar <= self.start_bar)
                {
                    return Err("The end bar must follow the start bar.".into());
                }
                let mut params = json!({
                    "sampleRate":self.sample_rate,
                    "tailSeconds":self.tail_seconds,
                });
                if lossy {
                    params["quality"] = json!(self.quality);
                } else {
                    params["format"] = json!(FORMATS[self.format].0);
                    params["dither"] = json!(self.dither && FORMATS[self.format].0 != "float32");
                }
                if self.range {
                    params["startBar"] = json!(self.start_bar - 1.0);
                    params["endBar"] = json!(self.end_bar - 1.0);
                }
                if self.stems {
                    if selected.is_empty() {
                        return Err("Select at least one track for the stems.".into());
                    }
                    if !valid_folder_name(&self.folder_name) {
                        return Err(
                            "Use a new folder name without slashes or a drive prefix.".into()
                        );
                    }
                    params["trackIds"] = json!(selected);
                    params["includeEffects"] = json!(self.include_effects);
                    params["includeMaster"] = json!(self.include_master);
                    params["container"] = json!(container);
                }
                PreparedCommand {
                    method: if self.stems {
                        "session.exportStems"
                    } else {
                        "session.exportAudio"
                    },
                    params,
                }
            }
        };
        Ok(request)
    }

    pub(crate) fn choose(&mut self, session: &Session) {
        let request = match self.request(session) {
            Ok(request) => request,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let mode = self.mode;
        let stems = mode == Mode::Audio && self.stems;
        let folder = self.folder_name.trim().to_string();
        let name = session.name.trim_end_matches(".ryolune").to_string();
        let extension = CONTAINERS[self.container.min(CONTAINERS.len() - 1)].0;
        let app_format = APP_FORMATS[self.app_format.min(APP_FORMATS.len() - 1)].0;
        let app_name = self
            .app
            .and_then(|i| ryolune_engine::interop::apps::APPS.get(i))
            .map(|a| a.name);
        let (tx, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            if mode == Mode::AppImport {
                let mut extensions = vec!["dawproject", "mid", "midi"];
                extensions.extend_from_slice(ryolune_engine::audio::IMPORT_EXTENSIONS);
                let paths = rfd::FileDialog::new()
                    .set_title("Import from another app: a .dawproject, a .mid, or audio files")
                    .add_filter("Song from another app", &extensions)
                    .add_filter("DAWproject", &["dawproject"])
                    .add_filter("MIDI file", &["mid", "midi"])
                    .add_filter(
                        "Audio files (stems)",
                        ryolune_engine::audio::IMPORT_EXTENSIONS,
                    )
                    .pick_files();
                let _ = tx.send(paths.filter(|p| !p.is_empty()));
                return;
            }
            if mode == Mode::AppExport {
                let base = match app_name {
                    Some(app) => format!("{name} for {app}"),
                    None => name.clone(),
                };
                let path = match app_format {
                    "package" | "stems" => rfd::FileDialog::new()
                        .set_title("Choose where the new folder goes")
                        .pick_folder()
                        .map(|parent| parent.join(base)),
                    format => {
                        let ext = match format {
                            "dawproject" => "dawproject",
                            "midi" => "mid",
                            _ => "wav",
                        };
                        rfd::FileDialog::new()
                            .add_filter(ext, &[ext])
                            .set_file_name(format!("{base}.{ext}"))
                            .save_file()
                            .map(|mut path| {
                                if path.extension().is_none() {
                                    path.set_extension(ext);
                                }
                                path
                            })
                    }
                };
                let _ = tx.send(path.map(|p| vec![p]));
                return;
            }
            let path = match mode {
                Mode::MidiImport => rfd::FileDialog::new()
                    .add_filter("MIDI file", &["mid", "midi"])
                    .pick_file(),
                Mode::MidiExport => rfd::FileDialog::new()
                    .add_filter("MIDI file", &["mid"])
                    .set_file_name(format!("{name}.mid"))
                    .save_file()
                    .map(|mut path| {
                        if path.extension().is_none() {
                            path.set_extension("mid");
                        }
                        path
                    }),
                Mode::Audio if stems => rfd::FileDialog::new()
                    .set_title("Choose parent folder for stems")
                    .pick_folder()
                    .map(|parent| parent.join(folder)),
                Mode::Audio => {
                    // The chosen type comes first, so the chooser offers it.
                    let mut dialog = rfd::FileDialog::new();
                    let filters: [(&str, &[&str]); 4] = [
                        ("wav", &["wav"]),
                        ("aiff", &["aiff", "aif"]),
                        ("flac", &["flac"]),
                        ("ogg", &["ogg"]),
                    ];
                    for (key, extensions) in filters
                        .iter()
                        .filter(|(key, _)| *key == extension)
                        .chain(filters.iter().filter(|(key, _)| *key != extension))
                    {
                        let label = match *key {
                            "wav" => "WAV audio",
                            "aiff" => "AIFF audio",
                            "flac" => "FLAC audio",
                            _ => "Ogg Vorbis audio",
                        };
                        dialog = dialog.add_filter(label, extensions);
                    }
                    dialog
                        .set_file_name(format!("{name}.{extension}"))
                        .save_file()
                        .map(|mut path| {
                            if path.extension().is_none() {
                                path.set_extension(extension);
                            }
                            path
                        })
                }
                Mode::AppImport | Mode::AppExport => None,
            };
            let _ = tx.send(path.map(|p| vec![p]));
        });
        self.chooser = Some(Chooser {
            receiver,
            request,
            path_key: if stems { "directory" } else { "path" },
        });
        self.error = None;
        self.report = None;
    }

    fn poll_chooser(&mut self) -> Option<PreparedCommand> {
        let result = self
            .chooser
            .as_ref()
            .map(|chooser| chooser.receiver.try_recv())?;
        match result {
            Ok(paths) => {
                let mut chooser = self.chooser.take().unwrap();
                let mut paths = paths?;
                if paths.len() == 1 {
                    chooser.request.params[chooser.path_key] = json!(paths.remove(0));
                } else {
                    chooser.request.params["paths"] = json!(paths);
                }
                Some(chooser.request)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.chooser = None;
                self.error = Some("The file chooser closed unexpectedly. Try again.".into());
                None
            }
        }
    }

    pub(crate) fn completed(&mut self, method: &str, result: &Result<Value>) {
        if self.awaiting.as_deref() != Some(method) {
            return;
        }
        self.awaiting = None;
        match result {
            Ok(value) => self.report = Some(report(method, value)),
            Err(error) => self.error = Some(error.clone()),
        }
    }
}

impl Ryolune {
    pub(crate) fn open_export_dialog(&mut self) {
        self.export
            .show(Mode::Audio, self.store.session(), self.position);
    }

    pub(crate) fn import_midi_dialog(&mut self) {
        self.export
            .show(Mode::MidiImport, self.store.session(), self.position);
    }

    /// File › Import from Another App… and Export for Another App…: the sheet starts on
    /// the app the person said they came from, and on that app's best format.
    pub(crate) fn app_dialog(&mut self, import: bool) {
        let mode = if import {
            Mode::AppImport
        } else {
            Mode::AppExport
        };
        self.export.show(mode, self.store.session(), self.position);
        if self.export.busy() {
            return;
        }
        let apps = ryolune_engine::interop::apps::APPS;
        self.export.app = apps
            .iter()
            .position(|a| a.id == self.settings.onboarding.coming_from);
        self.export.app_format = best_format(self.export.app);
    }

    pub(crate) fn export_midi_dialog(&mut self) {
        self.export
            .show(Mode::MidiExport, self.store.session(), self.position);
    }

    /// Run the export or import once its file chooser answers. Called every tick.
    pub(crate) fn poll_export(&mut self) {
        if let Some(command) = self.export.poll_chooser() {
            if command.method == "session.importFrom" {
                // It replaces the open song: ask about unsaved changes first.
                self.import_from_app(command.params);
                return;
            }
            self.export.awaiting = Some(command.method.into());
            let result =
                self.run_control_command(command.method, &command.params, false, "File menu");
            if !result
                .as_ref()
                .is_ok_and(|value| value["status"] == "running")
            {
                self.export.completed(command.method, &result);
            }
        }
    }
    /// The export dialog cannot start while audio is being prepared, recorded or written.
    pub(crate) fn export_blocked(&self) -> bool {
        (self.job.is_some() && !self.preparing)
            || self.control_job.is_some()
            || self.midi_recording
            || self.recorder.is_some()
            || self.record_pending.is_some()
            || self.record_finishing.is_some()
    }
}

/// What Export for Another App writes for an app by default: its best format.
pub(crate) fn best_format(app: Option<usize>) -> usize {
    let best = app
        .and_then(|i| ryolune_engine::interop::apps::APPS.get(i))
        .and_then(|a| a.writes.first().copied())
        .unwrap_or("dawproject");
    APP_FORMATS.iter().position(|f| f.0 == best).unwrap_or(0)
}

fn valid_folder_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':'])
}

fn report(method: &str, value: &Value) -> String {
    if let Some(report) = value.get("report").filter(|r| r.is_object()) {
        return interop_report(method, value, report);
    }
    let mut lines = vec![if method == "session.importMidi" {
        "MIDI import complete".into()
    } else {
        "Export complete".into()
    }];
    for key in ["path", "directory"] {
        if let Some(path) = value[key].as_str() {
            lines.push(path.into());
        }
    }
    if let Some(duration) = value["seconds"].as_f64() {
        lines.push(format!("Duration: {duration:.2} seconds"));
    }
    if let Some(kbps) = value["kbps"].as_f64() {
        lines.push(format!("Ogg Vorbis, about {} kbit/s", kbps.round()));
    }
    if let Some(files) = value["files"].as_array() {
        lines.push(format!("{} stem files", files.len()));
        for file in files {
            if let Some(path) = file["path"].as_str() {
                lines.push(path.into());
            }
            if let Some(warnings) = file["warnings"].as_array() {
                lines.extend(warnings.iter().filter_map(Value::as_str).map(String::from));
            }
        }
    }
    if let Some(notes) = value["noteCount"]
        .as_u64()
        .or_else(|| value["notes"].as_u64())
    {
        lines.push(format!("{notes} MIDI notes"));
    }
    if let Some(count) = value["clippedSamples"].as_u64().filter(|count| *count > 0) {
        lines.push(format!("{count} samples exceeded the output range. Lower the mix level if this clipping is unwanted."));
    }
    if let Some(warnings) = value["warnings"].as_array() {
        lines.extend(warnings.iter().filter_map(Value::as_str).map(String::from));
    }
    lines.join("\n")
}

/// What a trip to or from another app kept, changed and left out, as the sheet says it.
fn interop_report(method: &str, value: &Value, report: &Value) -> String {
    let mut lines = vec![if method == "session.importFrom" {
        "Song imported as a new song. Save it to keep it.".to_string()
    } else {
        "Written for the other app.".to_string()
    }];
    for key in ["path", "directory"] {
        if let Some(path) = value[key].as_str() {
            lines.push(path.into());
        }
    }
    let list = |key: &str| -> Vec<String> {
        report[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|line| format!("  · {line}"))
            .collect()
    };
    for (key, title) in [
        ("kept", "Came across:"),
        ("approximated", "Changed on the way:"),
        ("dropped", "Left out:"),
        ("missingMedia", "Audio files not found:"),
    ] {
        let items = list(key);
        if !items.is_empty() {
            lines.push(title.into());
            lines.extend(items);
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryolune_engine::store;

    #[test]
    fn app_modes_prepare_the_interop_commands() {
        let session = store::demo();
        let mut dialog = ExportDialog::default();
        dialog.show(Mode::AppImport, &session, 0.0);
        assert_eq!(
            dialog.request(&session).unwrap().method,
            "session.importFrom"
        );
        dialog.show(Mode::AppExport, &session, 0.0);
        let bitwig = ryolune_engine::interop::apps::APPS
            .iter()
            .position(|a| a.id == "bitwig");
        dialog.app = bitwig;
        dialog.app_format = best_format(bitwig);
        let request = dialog.request(&session).unwrap();
        assert_eq!(request.method, "session.exportTo");
        assert_eq!(request.params["format"], "dawproject");
        assert_eq!(request.params["app"], "bitwig");
        let logic = ryolune_engine::interop::apps::APPS
            .iter()
            .position(|a| a.id == "logic");
        assert_eq!(APP_FORMATS[best_format(logic)].0, "package");
        let text = report(
            "session.importFrom",
            &json!({"report": {"kept": ["4 tracks"], "approximated": [], "dropped": ["Serum"], "missingMedia": []}}),
        );
        assert!(text.contains("Came across:") && text.contains("4 tracks"));
        assert!(text.contains("Left out:") && !text.contains("Changed on the way"));
    }

    #[test]
    fn musical_bar_range_maps_to_shared_export_parameters() {
        let dialog = ExportDialog {
            range: true,
            start_bar: 1.0,
            end_bar: 9.0,
            sample_rate: 96000,
            format: 2,
            ..Default::default()
        };
        let request = dialog.request(&store::demo()).unwrap();
        assert_eq!(request.method, "session.exportAudio");
        assert_eq!(request.params["startBar"], 0.0);
        assert_eq!(request.params["endBar"], 8.0);
        assert_eq!(request.params["sampleRate"], 96000);
        assert_eq!(request.params["format"], "float32");
    }

    #[test]
    fn stems_require_selected_tracks_and_a_new_folder_name() {
        let session = store::demo();
        let mut dialog = ExportDialog {
            stems: true,
            ..Default::default()
        };
        assert!(dialog.request(&session).is_err());
        dialog.tracks.insert(session.tracks[0].id.clone());
        let request = dialog.request(&session).unwrap();
        assert_eq!(request.method, "session.exportStems");
        assert_eq!(request.params["trackIds"], json!([session.tracks[0].id]));
        assert_eq!(request.params["includeMaster"], false);
        dialog.folder_name = "../existing".into();
        assert!(dialog.request(&session).is_err());
    }

    #[test]
    fn midi_export_only_selects_midi_tracks_and_import_defaults_to_current_bar() {
        let session = store::demo();
        let mut dialog = ExportDialog::default();
        dialog.show(Mode::MidiExport, &session, 0.0);
        let request = dialog.request(&session).unwrap();
        assert_eq!(
            request.params["trackIds"].as_array().unwrap().len(),
            session
                .tracks
                .iter()
                .filter(|track| track.kind == "midi")
                .count()
        );
        dialog.show(Mode::MidiImport, &session, session.beats_per_bar() * 4.0);
        let request = dialog.request(&session).unwrap();
        assert_eq!(request.params["startBar"], 4.0);
        assert_eq!(request.params["importTempo"], false);
    }

    #[test]
    fn file_types_choose_their_encoding_and_stems_carry_the_container() {
        let session = store::demo();
        let mut dialog = ExportDialog {
            container: 3,
            quality: 0.8,
            ..Default::default()
        };
        let request = dialog.request(&session).unwrap();
        assert_eq!(request.params["quality"], 0.8);
        assert!(request.params.get("format").is_none() && request.params.get("dither").is_none());
        dialog.container = 2;
        dialog.format = 2;
        assert!(dialog.request(&session).is_err(), "FLAC takes no float");
        dialog.container = 0;
        let request = dialog.request(&session).unwrap();
        assert_eq!(request.params["format"], "float32");
        assert_eq!(request.params["dither"], false, "float is never dithered");
        dialog.container = 1;
        dialog.format = 1;
        dialog.stems = true;
        dialog.tracks.insert(session.tracks[0].id.clone());
        let request = dialog.request(&session).unwrap();
        assert_eq!(request.params["container"], "aiff");
    }

    #[test]
    fn invalid_ranges_and_tails_are_rejected_before_choosing_a_file() {
        let session = store::demo();
        let mut dialog = ExportDialog {
            range: true,
            start_bar: 9.0,
            end_bar: 9.0,
            ..Default::default()
        };
        assert!(dialog.request(&session).is_err());
        dialog.range = false;
        dialog.tail_seconds = f64::NAN;
        assert!(dialog.request(&session).is_err());
    }

    #[test]
    fn completion_displays_the_matching_operation_and_warnings() {
        let mut dialog = ExportDialog {
            awaiting: Some("session.exportAudio".into()),
            ..Default::default()
        };
        dialog.completed("session.info", &Ok(json!({})));
        assert!(dialog.awaiting.is_some());
        dialog.completed(
            "session.exportAudio",
            &Ok(json!({"path":"mix.wav","clippedSamples":8,"warnings":["Tail was truncated"]})),
        );
        assert!(dialog.awaiting.is_none());
        let result = dialog.report.unwrap();
        assert!(
            result.contains("mix.wav")
                && result.contains("8 samples")
                && result.contains("Tail was truncated")
        );
    }
}
