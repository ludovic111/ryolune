//! The lsuite commands: which suite apps are installed and running, and hand-offs with
//! kimchi (video). Discovery and the file formats are in `lsuite.rs`.

use crate::{
    control::wire,
    control::{decode_file, edit, opt, place_audio, query, Args, Host, Kind, Spec},
    export::{self, ExportOptions},
    lsuite,
    model::Marker,
    store::Command,
    Result,
};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};

pub const SPECS: &[Spec] = &[
    query("app.suite", "The lsuite apps installed on this computer (ryolune, kimchi…) from their discovery files in ~/.lsuite/apps: version, paths of each app and its CLI and MCP server, whether it is running and on which bridge port, and the hand-offs it accepts.", &[]),
    edit("export.toKimchi", "Render the mix (or one stem per track) and put it on a kimchi video project, on a new audio track, ready to cut picture to. When kimchi is open, kimchi places them on its open project itself (its handoff.fromRyolune, one undo step there); when it is closed, they are added to the project file (the previous one is kept as a backup). Runs as a job in the app.", &[
        opt("project", Kind::String, "When kimchi is closed: the project id or name (default: the one changed most recently). When it is open, its open project."),
        opt("stems", Kind::Boolean, "One file and one kimchi track per ryolune track instead of the mix (default false)."),
        opt("trackIds", Kind::Array, "With stems: the tracks to send (default all)."),
        opt("startSeconds", Kind::Number, "Where the audio starts on kimchi's timeline, in seconds (default 0)."),
        opt("startBar", Kind::Number, "Zero-based bar the render starts at (default 0)."),
        opt("endBar", Kind::Number, "Exclusive bar the render ends at (default: the end of the song)."),
        opt("tailSeconds", Kind::Number, "Effect tail after the end, 0-120 seconds (default 3)."),
    ]),
    edit("session.scoreCut", "Score a cut from kimchi: put its audio on a new audio track at bar 1 and its markers on the ruler at the bars where they fall, so the music can follow the picture. Takes a hand-off manifest from kimchi (handoff.inbox) or the audio, length and markers directly. One undo step.", &[
        opt("manifest", Kind::String, "A kimchi hand-off manifest (JSON file) from handoff.inbox."),
        opt("path", Kind::String, "The cut's audio (WAV, AIFF, FLAC, MP3, Ogg, AAC…), if there is no manifest."),
        opt("name", Kind::String, "Name for the audio track (default: the cut's name)."),
        opt("markers", Kind::Array, "Markers of the cut: objects with `time` (seconds) and `label`."),
        opt("durationSeconds", Kind::Number, "Length of the cut; sets the cycle over it when given."),
    ]),
    query("handoff.inbox", "Hand-offs other lsuite apps left for ryolune (a cut from kimchi to score), oldest first, from ~/.lsuite/handoff/ryolune (kimchi's handoff.toRyolune writes `<name>.kimchi-cut.json` there). Pass a manifest to session.scoreCut.", &[]),
];

/// Name of a command family served here.
pub fn serves(name: &str) -> bool {
    SPECS.iter().any(|s| s.name == name)
}

pub(crate) fn call(host: &mut dyn Host, name: &str, a: &Args, agent: bool) -> Result<Value> {
    match name {
        "app.suite" => Ok(json!({ "home": lsuite::home(), "apps": lsuite::entries() })),
        "handoff.inbox" => {
            let dir = lsuite::handoff_dir("ryolune");
            let mut items: Vec<(std::time::SystemTime, Value)> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                .filter_map(|e| {
                    let modified = e.metadata().ok()?.modified().ok()?;
                    let mut v: Value =
                        serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?;
                    v["manifest"] = json!(e.path());
                    Some((modified, v))
                })
                .collect();
            items.sort_by_key(|(t, _)| *t);
            Ok(
                json!({ "folder": dir, "handoffs": items.into_iter().map(|(_, v)| v).collect::<Vec<_>>() }),
            )
        }
        "export.toKimchi" => to_kimchi(host, a),
        "session.scoreCut" => score_cut(host, a, agent),
        _ => Err(format!("Unknown lsuite command `{name}`")),
    }
}

/// kimchi's bridge, when kimchi is running: its control file from its discovery entry.
fn kimchi_bridge() -> Option<std::path::PathBuf> {
    let entry = lsuite::entry("kimchi")?;
    entry["running"]["controlFile"]
        .as_str()
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
}

/// Render the mix (or stems) into `folder`.
fn render(host: &mut dyn Host, a: &Args, folder: &std::path::Path) -> Result<Vec<lsuite::Placed>> {
    let session = host.store().session().clone();
    let song = session.name.trim_end_matches(".ryolune").to_string();
    let options = ExportOptions {
        start_bar: a.opt_f64("startBar"),
        end_bar: a.opt_f64("endBar"),
        tail_seconds: a.opt_f64("tailSeconds").unwrap_or(3.0).clamp(0.0, 120.0),
        ..ExportOptions::default()
    };
    let stamp = lsuite::now_rfc3339().replace([':', '-'], "");
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let safe: String = song
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if a.get("stems").and_then(Value::as_bool).unwrap_or(false) {
        let ids: Option<Vec<String>> = a.get("trackIds").and_then(Value::as_array).map(|v| {
            v.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        });
        let directory = folder.join(format!("{safe} stems {stamp}"));
        let report = export::stems(
            &session,
            host.library(),
            &directory,
            &options,
            ids.as_deref(),
            true,
            false,
        )?;
        Ok(report
            .files
            .into_iter()
            .map(|f| lsuite::Placed {
                name: f
                    .path
                    .file_stem()
                    .map_or_else(|| song.clone(), |s| s.to_string_lossy().into_owned()),
                path: f.path,
                seconds: f.seconds,
            })
            .collect())
    } else {
        let path = folder.join(format!("{safe} {stamp}.wav"));
        let report = export::mix(&session, host.library(), &path, &options)?;
        Ok(vec![lsuite::Placed {
            path: report.path,
            name: song,
            seconds: report.seconds,
        }])
    }
}

fn to_kimchi(host: &mut dyn Host, a: &Args) -> Result<Value> {
    let at = a.opt_f64("startSeconds").unwrap_or(0.0).max(0.0);
    let song = host
        .store()
        .session()
        .name
        .trim_end_matches(".ryolune")
        .to_string();
    // kimchi is open: render into the hand-off folder and let kimchi place each file on its
    // open project through its own command (handoff.fromRyolune), in its own undo history.
    if let Some(control) = kimchi_bridge() {
        let files = render(host, a, &lsuite::handoff_dir("kimchi"))?;
        let mut client = wire::Client::connect_at(&control)
            .map_err(|e| format!("kimchi is running but did not answer: {e}"))?;
        let mut placed = vec![];
        for f in &files {
            let reply = client
                .call(
                    "handoff.fromRyolune",
                    &json!({ "path": f.path, "start": at }),
                    false,
                )
                .map_err(|e| format!("kimchi refused {}: {e}", f.name))?;
            placed.push(
                json!({"path": f.path, "name": f.name, "seconds": f.seconds, "kimchi": reply}),
            );
        }
        return Ok(json!({
            "placed": true, "via": "kimchi", "files": placed,
            "note": "kimchi was open: the audio went on its open project, one undo step there per file.",
        }));
    }
    // kimchi is closed: add the files to the project's file, keeping the previous one.
    let library = lsuite::kimchi_library();
    let project = lsuite::kimchi_project(&library, a.opt_str("project"))?;
    let files = render(host, a, &project.dir.join("imported"))?;
    let listed: Vec<Value> = files
        .iter()
        .map(|f| json!({"path": f.path, "name": f.name, "seconds": f.seconds}))
        .collect();
    let clips = lsuite::place_on_kimchi(&project, &files, at, &format!("ryolune · {song}"))?;
    Ok(json!({
        "project": {"id": project.id, "name": project.name, "file": project.dir.join("project.json")},
        "placed": true, "via": "file", "files": listed, "clips": clips,
    }))
}

/// A marker of a cut: seconds and a label.
fn markers_of(value: Option<&Value>) -> Vec<(f64, String)> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let time = m["time"].as_f64().or_else(|| m["seconds"].as_f64())?;
            let label = m["label"]
                .as_str()
                .or_else(|| m["name"].as_str())
                .unwrap_or("Marker");
            (time.is_finite() && time >= 0.0).then(|| (time, label.to_string()))
        })
        .collect()
}

fn score_cut(host: &mut dyn Host, a: &Args, agent: bool) -> Result<Value> {
    let manifest: Option<Value> = match a.opt_str("manifest") {
        Some(path) => Some(
            serde_json::from_str(
                &std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?,
            )
            .map_err(|e| format!("{path}: {e}"))?,
        ),
        None => None,
    };
    let m = manifest.as_ref();
    let audio = a
        .opt_str("path")
        .map(PathBuf::from)
        .or_else(|| {
            m.and_then(|m| {
                m["audio"]
                    .as_str()
                    .or_else(|| m["files"][0]["path"].as_str())
            })
            .map(PathBuf::from)
        })
        .ok_or("session.scoreCut needs the cut's audio: `path`, or a `manifest` that names it")?;
    let name = a
        .opt_str("name")
        .map(String::from)
        .or_else(|| {
            m.and_then(|m| m["name"].as_str().or_else(|| m["project"].as_str()))
                .map(String::from)
        })
        .unwrap_or_else(|| "Cut".into());
    let markers = if a.get("markers").is_some() {
        markers_of(a.get("markers"))
    } else {
        markers_of(m.map(|m| &m["markers"]))
    };
    let duration = a.opt_f64("durationSeconds").or_else(|| {
        m.and_then(|m| {
            m["durationSeconds"]
                .as_f64()
                .or_else(|| m["seconds"].as_f64())
        })
    });

    let buffer = Arc::new(decode_file(&audio)?);
    let placed = place_audio(
        host,
        buffer,
        &format!("{name} (picture)"),
        None,
        Some(0.0),
        agent,
    )?;
    let session = host.store().session().clone();
    let mut commands = vec![];
    let mut added = vec![];
    for (seconds, label) in &markers {
        let bar = session.seconds_bars(0.0, *seconds);
        let marker = Marker {
            id: crate::control::new_id("marker"),
            bar,
            name: label.chars().take(120).collect(),
            color: None,
        };
        added.push(json!({"id": marker.id, "bar": bar, "seconds": seconds, "name": marker.name}));
        commands.push(Command::PutMarker(marker));
    }
    if let Some(seconds) = duration.filter(|d| d.is_finite() && *d > 0.0) {
        let mut transport = session.transport.clone();
        transport.cycle = true;
        transport.cycle_start_bar = 0.0;
        transport.cycle_end_bar = session.seconds_bars(0.0, seconds).max(0.25);
        commands.push(Command::SetTransport(transport));
    }
    if !commands.is_empty() {
        host.dispatch(Command::Batch(commands))?;
    }
    if let Some(path) = a.opt_str("manifest") {
        // Scored: the hand-off is done.
        let _ = std::fs::remove_file(path);
    }
    Ok(json!({ "audio": placed, "markers": added, "durationSeconds": duration }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cut_markers_accept_both_spellings() {
        let m = markers_of(Some(&json!([
            {"time": 1.5, "label": "Title"},
            {"seconds": 4, "name": "Chase"},
            {"time": -1, "label": "bad"}
        ])));
        assert_eq!(
            m,
            vec![(1.5, "Title".to_string()), (4.0, "Chase".to_string())]
        );
    }
}
