//! Working with other music apps: the songs ryolune opens from them and writes for them, and
//! a plain report of what survived the trip. DAWproject (the open exchange format of Bitwig
//! Studio, Studio One, Cubase and others) carries a whole song both ways
//! ([`dawproject`]); MIDI files and audio stems carry the rest, for the apps without it.
//! [`apps`] knows the apps people come from, what each exchanges with ryolune and where it is
//! installed. `session.formats`, `session.importFrom` and `session.exportTo`
//! (`control_interop.rs`) are the registry's side.

pub mod apps;
pub mod dawproject;

use crate::{
    audio::{self, Library},
    control::new_id,
    export::{self, ExportOptions},
    midi_file::{self, ImportOptions},
    model::*,
    plugin::Descriptor,
    store::{self, Store},
    Result,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// What came across, what changed on the way and what could not come at all, said so a
/// person can check the parts that changed. The same shape as kimchi's.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// The format read or written ([`FORMATS`] id).
    pub format: String,
    /// What came through as it was: "4 tracks", "12 MIDI clips with 380 notes".
    pub kept: Vec<String>,
    /// What came through close but not the same: "2 clips were time-stretched…".
    pub approximated: Vec<String>,
    /// What could not come: "3 automation lanes on plugins".
    pub dropped: Vec<String>,
    /// Audio files the project names that were not found.
    pub missing_media: Vec<String>,
}
impl Report {
    pub fn new(format: &str) -> Self {
        Self {
            format: format.into(),
            ..Self::default()
        }
    }
    pub fn kept(&mut self, what: impl Into<String>) {
        push_unique(&mut self.kept, what.into());
    }
    pub fn approximated(&mut self, what: impl Into<String>) {
        push_unique(&mut self.approximated, what.into());
    }
    pub fn dropped(&mut self, what: impl Into<String>) {
        push_unique(&mut self.dropped, what.into());
    }
    pub fn missing(&mut self, what: impl Into<String>) {
        push_unique(&mut self.missing_media, what.into());
    }
    /// Nothing was lost or changed.
    pub fn is_exact(&self) -> bool {
        self.approximated.is_empty() && self.dropped.is_empty() && self.missing_media.is_empty()
    }
}
fn push_unique(list: &mut Vec<String>, text: String) {
    if !text.is_empty() && !list.contains(&text) {
        list.push(text);
    }
}
/// "1 track", "3 tracks".
pub(crate) fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A way of carrying a song between ryolune and another app.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Format {
    pub id: &'static str,
    pub name: &'static str,
    /// File extensions it is read from or written to; empty for a folder.
    pub extensions: &'static [&'static str],
    /// `session.importFrom` reads it.
    pub opens: bool,
    /// `session.exportTo` writes it.
    pub writes: bool,
    /// What survives the trip.
    pub carries: &'static str,
}

pub const FORMATS: &[Format] = &[
    Format {
        id: "dawproject",
        name: "DAWproject",
        extensions: &[dawproject::EXTENSION],
        opens: true,
        writes: true,
        carries: "The whole song: tracks, buses and sends, volume, pan, mute and solo, MIDI clips with notes and controllers, audio clips with their audio and fades, markers, the tempo and its changes, volume and pan automation, and plugins with their settings (CLAP, VST3 and Audio Units by id, ryolune's own instruments and effects by name). Clip gain, fade curves and plugin automation do not travel.",
    },
    Format {
        id: "midi",
        name: "MIDI file",
        extensions: &["mid", "midi"],
        opens: true,
        writes: true,
        carries: "Notes, controllers, pitch bend and pressure, the tempo and its changes, the meter. No audio, mix or plugins: each track plays ryolune Synth until you choose its instrument.",
    },
    Format {
        id: "audio",
        name: "Audio files",
        extensions: audio::IMPORT_EXTENSIONS,
        opens: true,
        writes: true,
        carries: "Opens: one audio track per file from bar 1 (stems exported from another app). Writes: the mix as one WAV, AIFF, FLAC or Ogg Vorbis file.",
    },
    Format {
        id: "stems",
        name: "Stems",
        extensions: &[],
        opens: false,
        writes: true,
        carries: "One WAV per track in a new folder: the sound as you hear it, editable as audio only.",
    },
    Format {
        id: "package",
        name: "MIDI and stems",
        extensions: &[],
        opens: false,
        writes: true,
        carries: "A new folder with the song as a MIDI file and one WAV per track, for the apps that do not open DAWproject: the notes stay editable and the audio sounds as it does here.",
    },
];

pub fn format(id: &str) -> Option<&'static Format> {
    FORMATS.iter().find(|f| f.id == id)
}

/// The format a file is in, by its extension.
pub fn format_for_path(path: &Path) -> Option<&'static Format> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    FORMATS
        .iter()
        .find(|f| f.opens && f.extensions.contains(&ext.as_str()))
}

/// A song brought from another app, ready to become the open document.
pub struct Imported {
    pub session: Session,
    pub library: Library,
    pub report: Report,
}

/// A song with no tracks, the given name and ryolune's defaults (the two aux returns and
/// the Stereo Out), at 120 BPM in 4/4.
pub fn blank(name: &str) -> Session {
    let mut s = store::empty();
    s.name = name.to_string();
    s.tracks.clear();
    s.clips.clear();
    s.strips.clear();
    s.sources.clear();
    s.markers.clear();
    s.automation.clear();
    s.tempo_changes.clear();
    s.transport.tempo = 120.0;
    s.transport.time_signature = TimeSignature {
        numerator: 4,
        denominator: 4,
    };
    s.view.selected_track_id = None;
    s.normalize();
    s
}

fn song_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Imported song")
        .chars()
        .take(120)
        .collect()
}

/// Open one file (a DAWproject or a MIDI file) or several audio files as a new song.
/// `catalog` is the installed plugins, for mapping a project's plugins.
pub fn import(paths: &[PathBuf], catalog: &[Descriptor]) -> Result<Imported> {
    let first = paths.first().ok_or("Give the file to import")?;
    if paths.len() > 128 {
        return Err("Import at most 128 files at once".into());
    }
    let audio_only = paths.iter().all(|p| audio::is_importable(p));
    if paths.len() > 1 && !audio_only {
        return Err("Several files at once are audio stems only (WAV, AIFF, FLAC, MP3…); open a DAWproject or a MIDI file on its own".into());
    }
    match format_for_path(first).map(|f| f.id) {
        Some("dawproject") => dawproject::import(first, catalog),
        Some("midi") => import_midi(first),
        Some("audio") => import_audio(paths),
        _ => Err(format!(
            "ryolune opens DAWproject (.dawproject), MIDI (.mid) and audio files (.wav, .aiff, .flac, .mp3…) from other apps, not `{}`. Export one of those from the other app (session.formats says how).",
            first.display()
        )),
    }
}

fn import_midi(path: &Path) -> Result<Imported> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let session = blank(&song_name(path));
    let options = ImportOptions {
        start_bar: 0.0,
        import_tempo: true,
        keep_channels: false,
    };
    let (command, midi) = midi_file::import_bytes(&bytes, &session, &options, false)?;
    let mut store = Store::new(session)?;
    store.dispatch(command)?;
    let mut session = store.session().clone();
    session.view.selected_track_id = session.tracks.first().map(|t| t.id.clone());
    let mut report = Report::new("midi");
    report.kept(count(
        midi.track_ids.len(),
        "instrument track",
        "instrument tracks",
    ));
    report.kept(format!(
        "{} with {} and {}",
        count(midi.clip_ids.len(), "MIDI clip", "MIDI clips"),
        count(midi.notes, "note", "notes"),
        count(midi.controllers, "controller change", "controller changes")
    ));
    report.kept(format!(
        "Tempo {} BPM{}",
        session.transport.tempo,
        if midi.tempo_changes > 0 {
            format!(
                " and {}",
                count(midi.tempo_changes, "tempo change", "tempo changes")
            )
        } else {
            String::new()
        }
    ));
    report.approximated(
        "A MIDI file names no sounds: every track plays ryolune Synth until you choose its instrument",
    );
    for warning in midi.warnings {
        report.approximated(warning);
    }
    Ok(Imported {
        session,
        library: Library::new(),
        report,
    })
}

fn import_audio(paths: &[PathBuf]) -> Result<Imported> {
    let name = if paths.len() == 1 {
        song_name(&paths[0])
    } else {
        paths[0]
            .parent()
            .map(song_name)
            .unwrap_or_else(|| "Imported stems".into())
    };
    let mut session = blank(&name);
    let mut library = Library::new();
    let mut total = 0usize;
    for (index, path) in paths.iter().enumerate() {
        let buffer = Arc::new(crate::control::decode_file(path)?);
        total = total.saturating_add(buffer.frames.len() * 8);
        if total > audio::MAX_LIBRARY_BYTES {
            return Err("The files exceed ryolune's 1 GiB of decoded audio".into());
        }
        let stem = song_name(path);
        let mut track = crate::control::new_track(
            &session,
            "audio",
            Some(stem.clone()),
            crate::control::TRACK_PALETTE[index % 8].into(),
        );
        track.id = new_id("track");
        let source = Source {
            id: new_id("source"),
            name: stem.clone(),
            sample_rate: buffer.sample_rate,
            channels: 2,
            file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            duration_seconds: buffer.duration(),
            origin: "file".into(),
            seed: None,
            wave_kind: None,
        };
        let clip = Clip {
            id: new_id("clip"),
            name: stem,
            agent: false,
            track_id: track.id.clone(),
            start_bar: 0.0,
            length_bars: session.seconds_bars(0.0, buffer.duration()),
            data: ClipData::audio(source.id.clone(), 0.0),
        };
        library.insert(source.id.clone(), buffer);
        session.sources.insert(source.id.clone(), source);
        session.tracks.push(track);
        session.clips.push(clip);
    }
    session.view.selected_track_id = session.tracks.first().map(|t| t.id.clone());
    session.validate()?;
    let mut report = Report::new("audio");
    report.kept(format!(
        "{}, each on its own audio track from bar 1",
        count(paths.len(), "audio file", "audio files")
    ));
    report.approximated("Audio files carry no tempo: the song is at 120 BPM in 4/4, so set the tempo the stems were made at (Transport) before editing to the grid");
    Ok(Imported {
        session,
        library,
        report,
    })
}

/// Write the song for another app in `format` (a [`FORMATS`] id that writes).
pub fn export(session: &Session, library: &Library, path: &Path, format: &str) -> Result<Value> {
    match format {
        "dawproject" => {
            let report = dawproject::export(session, library, path)?;
            Ok(json!({ "path": path, "report": report }))
        }
        "midi" => {
            let midi = midi_file::export(session, path, None)?;
            let mut report = Report::new("midi");
            report.kept(format!(
                "{} with {}",
                count(midi.track_count, "MIDI track", "MIDI tracks"),
                count(midi.note_count, "note", "notes")
            ));
            report.kept("The tempo, its changes and the meter");
            report.dropped("Audio tracks, the mix and plugins: a MIDI file holds notes only");
            for warning in &midi.warnings {
                report.approximated(warning.clone());
            }
            Ok(json!({ "path": path, "report": report }))
        }
        "audio" => {
            let mix = export::mix(session, library, path, &ExportOptions::default())?;
            let mut report = Report::new("audio");
            report.kept("The mix as you hear it, in one file");
            report.dropped("Tracks, notes and plugins: the other app gets sound only");
            Ok(json!({ "path": path, "mix": mix, "report": report }))
        }
        "stems" => {
            let stems = export::stems(
                session,
                library,
                path,
                &ExportOptions::default(),
                None,
                true,
                false,
            )?;
            let mut report = Report::new("stems");
            report.kept(format!(
                "{}, one file per track with its effects",
                count(session.tracks.len(), "stem", "stems")
            ));
            report.dropped("Notes and plugins: the other app gets each track's sound only");
            Ok(json!({ "directory": path, "stems": stems, "report": report }))
        }
        "package" => package(session, library, path),
        other => Err(format!(
            "Unknown format `{other}`: use dawproject, midi, audio, stems or package"
        )),
    }
}

/// A new folder holding the song as a MIDI file and one WAV per track. Nothing is left
/// behind when a part fails.
fn package(session: &Session, library: &Library, folder: &Path) -> Result<Value> {
    if folder.exists() {
        return Err(format!(
            "{} already exists: choose a new folder name",
            folder.display()
        ));
    }
    std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let result = (|| {
        let name = song_name(folder);
        let mut report = Report::new("package");
        let has_midi = session.tracks.iter().any(|t| t.kind == "midi")
            && session
                .clips
                .iter()
                .any(|c| matches!(c.data, ClipData::Midi { .. }));
        let midi_path = folder.join(format!("{name}.mid"));
        let midi = if has_midi {
            let midi = midi_file::export(session, &midi_path, None)?;
            report.kept(format!(
                "{} as a MIDI file with {}",
                count(midi.track_count, "instrument track", "instrument tracks"),
                count(midi.note_count, "note", "notes")
            ));
            for warning in &midi.warnings {
                report.approximated(warning.clone());
            }
            Some(midi_path.clone())
        } else {
            None
        };
        let stems_dir = folder.join("Stems");
        let stems = export::stems(
            session,
            library,
            &stems_dir,
            &ExportOptions::default(),
            None,
            true,
            false,
        )?;
        report.kept(format!(
            "{} in Stems, each with its effects, all starting at bar 1",
            count(session.tracks.len(), "WAV stem", "WAV stems")
        ));
        report.kept(format!(
            "Tempo {} BPM in {}/{}",
            session.transport.tempo,
            session.transport.time_signature.numerator,
            session.transport.time_signature.denominator
        ));
        report.dropped("Plugins and the mix settings: the stems carry their sound instead");
        Ok(json!({ "directory": folder, "midi": midi, "stems": stems, "report": report }))
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(folder);
    }
    result
}

/// The format `session.exportTo` writes: the one asked for, else the app's best, else the
/// destination's extension (no extension makes a package folder).
pub fn export_format(path: &Path, format: Option<&str>, app: Option<&str>) -> Result<&'static str> {
    if let Some(id) = format {
        return format_id(id)
            .filter(|f| f.writes)
            .map(|f| f.id)
            .ok_or_else(|| {
                format!("Unknown format `{id}`: use dawproject, midi, audio, stems or package")
            });
    }
    if let Some(app) = app {
        let app = apps::find(app).ok_or_else(|| apps::unknown(app))?;
        return Ok(app.writes.first().copied().unwrap_or("package"));
    }
    match path.extension().and_then(|e| e.to_str()) {
        None => Ok("package"),
        Some(ext) => {
            let ext = ext.to_ascii_lowercase();
            if ext == dawproject::EXTENSION {
                Ok("dawproject")
            } else if ext == "mid" || ext == "midi" {
                Ok("midi")
            } else if crate::export::Container::for_path(path).is_ok() {
                Ok("audio")
            } else {
                Err(format!(
                    "ryolune writes .dawproject, .mid and audio files for other apps, not .{ext}: give `format`"
                ))
            }
        }
    }
}
fn format_id(id: &str) -> Option<&'static Format> {
    format(id.trim())
}
