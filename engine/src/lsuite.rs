//! lsuite: how ryolune finds the other apps of the suite and hands work to them.
//!
//! **Discovery.** Every lsuite app writes `~/.lsuite/apps/<app>.json` when it starts and
//! clears its `running` part when it quits (`$LSUITE_HOME` replaces `~/.lsuite`). The format
//! (format 1, as kimchi and zenith write it too; lsuite's STANDARD.md) is small on purpose:
//!
//! ```json
//! { "format": 1, "app": "ryolune", "version": "0.13.0", "kind": "music",
//!   "appPath": "/Applications/ryolune.app", "executable": "…/MacOS/ryolune",
//!   "cli": "…/MacOS/ryolune-cli", "mcp": "…/MacOS/ryolune-mcp", "dataDir": "…",
//!   "documents": { "extensions": ["ryolune"], "description": "ryolune song (JSON, audio inside)" },
//!   "running": { "pid": 4242, "controlFile": "~/.ryolune/control.json", "port": 51234,
//!                "since": "2026-10-02T09:00:00Z" },
//!   "updatedAt": "2026-10-02T09:00:00Z" }
//! ```
//!
//! No secret is ever written there: the bridge token stays in the app's own 0600 control
//! file. A reader treats `running` as stale when its pid is gone and ignores fields it does
//! not know (ryolune adds `handoffs` and `commands`).
//!
//! **Hand-offs.** Media goes between apps as plain files, in `~/.lsuite/handoff/<app>/`.
//! `export.toKimchi` renders the mix or stems for kimchi: when kimchi is open, kimchi places
//! them through its own `handoff.fromRyolune` (on its bridge, which speaks ryolune's
//! protocol); when it is closed, they go into the project file. A cut from kimchi
//! (`handoff.toRyolune` writes `<name>.kimchi-cut.json` beside its WAV) is scored with
//! `session.scoreCut`; while ryolune runs, kimchi also places the cut itself through ryolune's
//! bridge (`session.importAudio`, `marker.add`).

use crate::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const FORMAT: u64 = 1;

/// `$LSUITE_HOME`, or `~/.lsuite`.
pub fn home() -> PathBuf {
    if let Some(dir) = std::env::var_os("LSUITE_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(sandbox) = crate::host::scan::test_sandbox() {
        return sandbox.join("lsuite");
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".lsuite")
}
pub fn app_file(app: &str) -> PathBuf {
    home().join("apps").join(format!("{app}.json"))
}
/// Where files handed to an app wait: `~/.lsuite/handoff/<app>` (as kimchi writes them).
pub fn handoff_dir(app: &str) -> PathBuf {
    home().join("handoff").join(app)
}

/// Write a discovery entry, atomically.
pub fn write_entry(entry: &Value) -> Result<PathBuf> {
    let app = entry["app"]
        .as_str()
        .ok_or("A discovery entry names its app")?;
    let path = app_file(app);
    let text = serde_json::to_string_pretty(entry).map_err(|e| e.to_string())?;
    write_file(&path, text.as_bytes())?;
    Ok(path)
}

/// Every app's entry, with `running` cleared when its process is gone.
pub fn entries() -> Vec<Value> {
    let Ok(dir) = std::fs::read_dir(home().join("apps")) else {
        return vec![];
    };
    let mut out: Vec<Value> = dir
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v["app"].is_string())
        .map(|mut v| {
            let alive = v["running"]["pid"]
                .as_u64()
                .is_some_and(|pid| alive(pid as u32));
            if !alive {
                v["running"] = Value::Null;
            }
            v
        })
        .collect();
    out.sort_by(|a, b| a["app"].as_str().cmp(&b["app"].as_str()));
    out
}
pub fn entry(app: &str) -> Option<Value> {
    entries().into_iter().find(|e| e["app"] == app)
}

/// Whether a process is still there.
pub fn alive(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    #[cfg(windows)]
    {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
    }
    #[cfg(not(any(unix, windows)))]
    {
        true
    }
}

/// A random UUID (version 4), the id format kimchi documents use.
pub fn uuid_v4() -> String {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut bytes = [0u8; 16];
    for half in 0..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        bytes[half * 8..half * 8 + 8].copy_from_slice(&h.finish().to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Now as RFC 3339 in UTC, to the second.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    rfc3339(secs)
}
pub(crate) fn rfc3339(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// kimchi's project library: its discovery entry's `dataDir`, else the platform default.
pub fn kimchi_library() -> PathBuf {
    if let Some(dir) = std::env::var_os("KIMCHI_LIBRARY").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(dir) = std::env::var_os("KIMCHI_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(sandbox) = crate::host::scan::test_sandbox() {
        return sandbox.join("kimchi");
    }
    if let Some(dir) = entry("kimchi").and_then(|e| e["dataDir"].as_str().map(PathBuf::from)) {
        return dir;
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/kimchi")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or(home)
            .join("kimchi")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("kimchi")
    }
}

/// A kimchi project on disk.
#[derive(Clone, Debug)]
pub struct KimchiProject {
    pub id: String,
    pub name: String,
    pub updated: String,
    pub dir: PathBuf,
}

/// kimchi's projects, most recently changed first (`<library>/projects/<id>/project.json`).
pub fn kimchi_projects(library: &Path) -> Vec<KimchiProject> {
    let Ok(dir) = std::fs::read_dir(library.join("projects")) else {
        return vec![];
    };
    let mut out: Vec<KimchiProject> = dir
        .flatten()
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path().join("project.json")).ok()?;
            let v: Value = serde_json::from_str(&text).ok()?;
            Some(KimchiProject {
                id: v["id"].as_str()?.to_string(),
                name: v["name"].as_str().unwrap_or("Untitled").to_string(),
                updated: v["updated_at"].as_str().unwrap_or("").to_string(),
                dir: e.path(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    out
}

/// Pick a project by id, by name (case-insensitive), or the most recent one.
pub fn kimchi_project(library: &Path, wanted: Option<&str>) -> Result<KimchiProject> {
    let projects = kimchi_projects(library);
    if projects.is_empty() {
        return Err(format!(
            "No kimchi project in {}: create one in kimchi first.",
            library.display()
        ));
    }
    match wanted {
        None => Ok(projects[0].clone()),
        Some(w) => projects
            .iter()
            .find(|p| p.id == w || p.name.eq_ignore_ascii_case(w))
            .cloned()
            .ok_or_else(|| {
                format!(
                    "No kimchi project `{w}`. Projects: {}",
                    projects
                        .iter()
                        .map(|p| format!("{} ({})", p.name, p.id))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }),
    }
}

/// One audio file to place: where it is, its name and length in seconds.
pub struct Placed {
    pub path: PathBuf,
    pub name: String,
    pub seconds: f64,
}

/// Add audio files to a kimchi project, one new audio track each, starting at `at`
/// seconds. The project file is rewritten atomically; a copy of the previous one stays
/// beside it as `project.json.ryolune-backup`.
pub fn place_on_kimchi(
    project: &KimchiProject,
    files: &[Placed],
    at: f64,
    track_prefix: &str,
) -> Result<Value> {
    let file = project.dir.join("project.json");
    let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let mut doc: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let now = now_rfc3339();
    let mut placed = vec![];
    for (i, f) in files.iter().enumerate() {
        let asset_id = uuid_v4();
        let track_id = uuid_v4();
        let clip_id = uuid_v4();
        doc["assets"]
            .as_array_mut()
            .ok_or("The kimchi project has no asset list")?
            .push(json!({
                "id": asset_id, "name": f.name, "kind": "audio",
                "path": f.path.to_string_lossy(),
                "meta": { "duration": f.seconds, "width": null, "height": null, "fps": null,
                    "has_video": false, "has_audio": true, "video_codec": null,
                    "audio_codec": "pcm_s24le", "size_bytes": std::fs::metadata(&f.path).map_or(0, |m| m.len()) },
                "origin": { "type": "imported" }, "created_at": now,
                "thumbnail": null, "filmstrip": null, "waveform": null, "proxy": null
            }));
        let name = if files.len() == 1 {
            track_prefix.to_string()
        } else {
            format!("{track_prefix} · {}", f.name)
        };
        doc["tracks"]
            .as_array_mut()
            .ok_or("The kimchi project has no track list")?
            .push(json!({
                "id": track_id, "kind": "audio", "name": name,
                "muted": false, "hidden": false, "locked": false,
                "clips": [{ "id": clip_id, "name": f.name, "start": at.max(0.0),
                    "duration": f.seconds.max(1.0 / 60.0), "in_point": 0.0, "speed": 1.0,
                    "content": { "type": "media", "asset_id": asset_id },
                    "volume": 1.0, "fade_in": 0.0, "fade_out": 0.0 }]
            }));
        placed.push(json!({"asset": asset_id, "track": track_id, "clip": clip_id, "file": f.path, "index": i}));
    }
    doc["updated_at"] = json!(now);
    let _ = std::fs::copy(&file, project.dir.join("project.json.ryolune-backup"));
    let out = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    write_file(&file, out.as_bytes())?;
    Ok(json!(placed))
}

/// Leave a hand-off for an app that is open (and would overwrite an edit to its file):
/// `~/.lsuite/handoff/<app>/<id>.json`.
#[allow(dead_code)]
pub fn post(app: &str, manifest: &Value) -> Result<PathBuf> {
    let path = handoff_dir(app).join(format!("{}.json", uuid_v4()));
    let text = serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())?;
    write_file(&path, text.as_bytes())?;
    Ok(path)
}

/// Replace a file atomically, making its folder first.
fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    crate::document::atomic_write(path, |f| {
        use std::io::Write;
        f.write_all(bytes).map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tests_never_reach_the_persons_profile() {
        let sandbox = crate::host::scan::test_sandbox().expect("a cargo test binary");
        assert!(sandbox.starts_with(std::env::temp_dir()));
        if std::env::var_os("RYOLUNE_SETTINGS").is_none() {
            assert!(crate::settings::Settings::path().starts_with(&sandbox));
        }
        if std::env::var_os("KIMCHI_LIBRARY").is_none() {
            assert!(kimchi_library().starts_with(&sandbox));
        }
    }

    #[test]
    fn ids_and_dates_have_the_shapes_other_apps_parse() {
        let id = uuid_v4();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert_ne!(uuid_v4(), id);
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_790_000_000), "2026-09-21T14:13:20Z");
    }

    #[test]
    fn audio_lands_on_a_new_kimchi_track_and_the_old_file_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let project_dir = dir
            .path()
            .join("projects/0b4f6c1e-0000-4000-8000-000000000001");
        std::fs::create_dir_all(&project_dir).unwrap();
        let project = json!({
            "id": "0b4f6c1e-0000-4000-8000-000000000001", "name": "Trailer",
            "created_at": "2026-10-01T10:00:00Z", "updated_at": "2026-10-01T10:00:00Z",
            "settings": {"width": 1920, "height": 1080, "fps": 30.0, "background": "#000000", "sample_rate": 48000},
            "assets": [], "tracks": [{"id": "0b4f6c1e-0000-4000-8000-000000000002", "kind": "video", "name": "Video 1", "clips": []}],
            "markers": []
        });
        std::fs::write(project_dir.join("project.json"), project.to_string()).unwrap();
        let found = kimchi_project(dir.path(), Some("trailer")).unwrap();
        let wav = dir.path().join("mix.wav");
        std::fs::write(&wav, b"RIFF").unwrap();
        place_on_kimchi(
            &found,
            &[Placed {
                path: wav,
                name: "Night Drive".into(),
                seconds: 12.5,
            }],
            2.0,
            "ryolune · Night Drive",
        )
        .unwrap();
        let after: Value = serde_json::from_str(
            &std::fs::read_to_string(project_dir.join("project.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(after["assets"][0]["kind"], "audio");
        assert_eq!(after["assets"][0]["origin"]["type"], "imported");
        assert_eq!(after["tracks"][1]["kind"], "audio");
        assert_eq!(after["tracks"][1]["clips"][0]["start"], 2.0);
        assert_eq!(
            after["tracks"][1]["clips"][0]["content"]["asset_id"],
            after["assets"][0]["id"]
        );
        assert!(project_dir.join("project.json.ryolune-backup").exists());
        assert!(kimchi_project(dir.path(), Some("nope")).is_err());
    }
}
