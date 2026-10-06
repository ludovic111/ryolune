//! Commands for people coming from other music apps: the formats and apps ryolune works
//! with (`session.formats`), opening a song from another app (`session.importFrom`) and
//! writing one for it (`session.exportTo`), the first-run setup (`app.onboarding`,
//! `app.finishOnboarding`) and the recent songs (`app.recent`, `app.openRecent`). The
//! formats themselves are in `interop/`.

use crate::{
    control::{self, edit, opt, query, req, Args, Host, Kind, Spec},
    host as plugin_host,
    interop::{self, apps},
    settings::{self, Provider},
    Result,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const SPECS: &[Spec] = &[
    query("session.formats", "What ryolune opens from and writes for other music apps: DAWproject (Bitwig Studio, Studio One, Cubase…), MIDI files, audio files and stems, each with what survives the trip; and every app ryolune knows (Ableton Live, Logic Pro, FL Studio, Bitwig Studio, REAPER, Cubase, Studio One, Pro Tools, GarageBand) with the formats it exchanges, how to bring a song over and take it back, and whether it is installed on this computer.", &[
        opt("app", Kind::String, "Only this app: ableton, logic, fl, bitwig, reaper, cubase, studioone, protools or garageband."),
    ]),
    edit("session.importFrom", "Open a song from another app as a new, unsaved song that replaces the open one (unsaved changes are discarded): a DAWproject (.dawproject, with tracks, buses, clips, notes, audio, the mix, markers, tempo and installed plugins), a MIDI file (an instrument track per channel, with its tempo), or audio files (one audio track per file from bar 1, for stems). Returns a report of what came across, what changed and what was left out. Runs as a job in the app.", &[
        opt("path", Kind::String, "The file to open (.dawproject, .mid, or an audio file)."),
        opt("paths", Kind::Array, "Several audio files (stems), each on its own track."),
    ]),
    edit("session.exportTo", "Write the song for another app: dawproject (Bitwig Studio, Studio One, Cubase), midi, audio (the mix), stems (a new folder, one WAV per track) or package (a new folder with the MIDI file and one WAV per track, for apps without DAWproject). Files are replaced atomically and folders must be new. Returns a report of what the format could not carry. Runs as a job in the app.", &[
        req("path", Kind::String, "Destination file, or the new folder for stems and package."),
        opt("format", Kind::String, "dawproject, midi, audio, stems or package. Default: the app's best, else from the extension (.dawproject, .mid, .wav…; none makes a package)."),
        opt("app", Kind::String, "The app it is for (see session.formats); picks its best format."),
    ]),
    query("app.onboarding", "The first-run setup: whether it was done, the app the person came from and the steps to bring a song from it, whether they want AI features, the agent providers ready to use, the music apps found on this computer, and the steps the setup walks through.", &[]),
    edit("app.finishOnboarding", "Finish (or skip) the first-run setup with the person's choices: the app they come from (it decides which steps the import shows first), whether they want AI features, and the agent provider to use. Saved in settings.onboarding. Only a person can do this, from the window or the CLI.", &[
        req("comingFrom", Kind::String, "App id from session.formats (ableton, logic, fl, bitwig, reaper, cubase, studioone, protools, garageband), or none."),
        req("ai", Kind::Boolean, "Whether they want AI features (the agent and sound generation)."),
        opt("agentProvider", Kind::String, "Agent provider to select when ai is true (agent.providers): codex, claude, anthropic, openai, gemini…"),
        opt("skipped", Kind::Boolean, "They skipped the setup; the choices given still apply."),
    ]),
    query("app.recent", "Songs opened recently, newest first: name, folder, path, and whether the file is still there.", &[]),
    edit("app.openRecent", "Open a recent song, replacing the open one (unsaved changes are discarded). Give its index in app.recent or its path.", &[
        opt("index", Kind::Integer, "Position in app.recent, 0 for the most recent."),
        opt("path", Kind::String, "A path from app.recent."),
    ]),
];

pub fn serves(name: &str) -> bool {
    SPECS.iter().any(|s| s.name == name)
}

/// Agent permission checks for these commands (Settings › Agent). `None` when allowed.
pub fn denied_for_agent(name: &str, permissions: &settings::Permissions) -> Option<String> {
    let deny = |what: &str, setting: &str| {
        Some(format!(
            "{name} is not allowed for agents: {what} is off in Settings > Agent ({setting})."
        ))
    };
    match name {
        "app.finishOnboarding" => Some(format!(
            "{name} is not allowed for agents: the first-run setup is the person's to answer."
        )),
        "session.importFrom" | "app.openRecent" if !permissions.replace_session => {
            deny("replacing the session", "replaceSession")
        }
        "session.importFrom" | "session.exportTo" if !permissions.file_operations => {
            deny("file operations", "fileOperations")
        }
        _ => None,
    }
}

pub(crate) fn call(host: &mut dyn Host, name: &str, a: &Args, agent: bool) -> Result<Value> {
    match name {
        "session.formats" => formats(a.opt_str("app")),
        "session.importFrom" => {
            let paths = import_paths(a)?;
            let catalog = plugin_host::scan::installed();
            let imported = interop::import(&paths, &catalog)?;
            let report = imported.report.clone();
            host.adopt_session(imported.session, imported.library)?;
            Ok(json!({ "report": report, "session": control::info(host) }))
        }
        "session.exportTo" => {
            let path = PathBuf::from(a.str("path")?);
            let format = interop::export_format(&path, a.opt_str("format"), a.opt_str("app"))?;
            control::protect_session_file(host.path(), &path)?;
            let mut session = host.store().session().clone();
            session.transport.playing = false;
            let mut value = interop::export(&session, host.library(), &path, format)?;
            value["format"] = json!(format);
            Ok(value)
        }
        "app.onboarding" => Ok(onboarding(&host.settings())),
        "app.finishOnboarding" => {
            if agent {
                return Err(
                    "app.finishOnboarding is not allowed for agents: the first-run setup is the person's to answer."
                        .into(),
                );
            }
            let mut settings = host.settings();
            finish(
                &mut settings,
                a.str("comingFrom")?,
                a.bool("ai")?,
                a.opt_str("agentProvider"),
            )?;
            host.update_settings(settings.clone())?;
            Ok(onboarding(&settings))
        }
        "app.recent" => Ok(json!({ "recent": recent(&host.settings()) })),
        "app.openRecent" => {
            let path = recent_path(&host.settings(), a.opt_int("index"), a.opt_str("path"))?;
            host.open(&path)?;
            Ok(control::info(host))
        }
        _ => Err(format!("Unknown command `{name}`")),
    }
}

fn import_paths(a: &Args) -> Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = a.opt_str("path").map(PathBuf::from).into_iter().collect();
    if let Some(list) = a.get("paths").and_then(Value::as_array) {
        for item in list {
            paths.push(PathBuf::from(
                item.as_str().ok_or("paths holds file paths, as text")?,
            ));
        }
    }
    if paths.is_empty() {
        return Err("Give the file to open as path (or several audio files as paths)".into());
    }
    Ok(paths)
}

/// The formats, and the apps with whether each is installed here.
pub fn formats(app: Option<&str>) -> Result<Value> {
    let apps: Vec<Value> = match app {
        Some(id) => vec![app_json(apps::find(id).ok_or_else(|| apps::unknown(id))?)],
        None => apps::APPS.iter().map(app_json).collect(),
    };
    Ok(json!({
        "formats": interop::FORMATS,
        "apps": apps,
    }))
}
fn app_json(app: &apps::App) -> Value {
    let mut value = json!(app);
    value["installed"] = json!(apps::installed(app));
    value
}

/// Record the setup's answers in `settings`.
pub fn finish(
    settings: &mut settings::Settings,
    coming_from: &str,
    ai: bool,
    provider: Option<&str>,
) -> Result<()> {
    let coming_from = coming_from.trim();
    let coming_from = match coming_from {
        "" | "none" => "none".to_string(),
        id => apps::find(id)
            .map(|app| app.id.to_string())
            .ok_or_else(|| apps::unknown(id))?,
    };
    if let Some(key) = provider.map(str::trim).filter(|k| !k.is_empty()) {
        let provider = Provider::parse(key).ok_or_else(|| {
            format!(
                "Unknown agent provider `{key}`: use one of {}",
                Provider::ALL
                    .iter()
                    .map(|p| p.key())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        if ai {
            settings.set("agent.provider", json!(provider.key()))?;
        }
    }
    settings.onboarding = settings::Onboarding {
        completed: env!("CARGO_PKG_VERSION").into(),
        coming_from,
        ai: Some(ai),
    };
    settings.validate()
}

/// The setup's state and what it walks through.
pub fn onboarding(settings: &settings::Settings) -> Value {
    let o = &settings.onboarding;
    let from = apps::find(&o.coming_from);
    let installed: Vec<&str> = apps::APPS
        .iter()
        .filter(|app| apps::installed(app))
        .map(|app| app.id)
        .collect();
    let providers: Vec<Value> = Provider::ALL
        .iter()
        .map(|p| {
            let ready = match p.hosted() {
                Some(hosted) => !hosted.needs_key || settings.api_key(*p).is_some(),
                None => p.is_cli() || settings.api_key(*p).is_some(),
            };
            json!({ "id": p.key(), "name": p.label(), "ready": ready })
        })
        .collect();
    json!({
        "done": o.is_done(),
        "completed": o.completed,
        "comingFrom": o.coming_from,
        "ai": o.ai,
        "agentProvider": settings.agent.provider.key(),
        "bring": from.map(|app| app.bring),
        "installedApps": installed,
        "providers": providers,
        "steps": [
            { "id": "comingFrom", "title": "Where are you coming from?", "does": "Pick the app you used (session.formats lists them); its steps for bringing a song over show first in File › Import from Another App…." },
            { "id": "ai", "title": "AI features", "does": "Say whether you want the agent and sound generation. Nothing is hidden either way; it is remembered in settings.onboarding.ai." },
            { "id": "provider", "title": "Connect a provider", "does": "Choose the agent provider (agent.providers) and add its key in Settings › Agent, or use an installed Codex or Claude Code." },
            { "id": "audio", "title": "Check your sound", "does": "See which output device plays (audio.status) and change it in Settings › Audio (audio.setOutput)." },
            { "id": "start", "title": "Start", "does": "Start from the demo song (session.new demo=true), an empty song (session.new), or a song from another app (session.importFrom)." },
        ],
    })
}

/// The recent songs, newest first.
pub fn recent(settings: &settings::Settings) -> Vec<Value> {
    settings
        .general
        .recent_sessions
        .iter()
        .map(|text| {
            let path = Path::new(text);
            json!({
                "path": text,
                "name": path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| text.clone()),
                "folder": path.parent().map(|p| p.display().to_string()),
                "exists": path.is_file(),
            })
        })
        .collect()
}

/// The file `app.openRecent` opens: by index or by path, and only one in the list.
pub fn recent_path(
    settings: &settings::Settings,
    index: Option<i64>,
    path: Option<&str>,
) -> Result<PathBuf> {
    let list = &settings.general.recent_sessions;
    let chosen = match (index, path) {
        (Some(index), _) => usize::try_from(index)
            .ok()
            .and_then(|i| list.get(i))
            .ok_or_else(|| {
                format!(
                    "No recent song {index}: app.recent lists {}",
                    interop::count(list.len(), "song", "songs")
                )
            })?
            .clone(),
        (None, Some(path)) => list
            .iter()
            .find(|p| p.as_str() == path)
            .cloned()
            .ok_or_else(|| format!("{path} is not in the recent songs (app.recent)"))?,
        (None, None) => list.first().cloned().ok_or("No song was opened recently")?,
    };
    let chosen = PathBuf::from(chosen);
    if !chosen.is_file() {
        return Err(format!(
            "{} is no longer there: it was moved or deleted",
            chosen.display()
        ));
    }
    Ok(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finishing_the_setup_records_the_answers_and_the_provider() {
        let mut settings = settings::Settings::default();
        assert!(!settings.onboarding.is_done());
        finish(&mut settings, "Bitwig Studio", true, Some("anthropic")).unwrap();
        assert!(settings.onboarding.is_done());
        assert_eq!(settings.onboarding.coming_from, "bitwig");
        assert_eq!(settings.onboarding.ai, Some(true));
        assert_eq!(settings.agent.provider, Provider::Anthropic);
        let mut other = settings::Settings::default();
        finish(&mut other, "none", false, Some("openai")).unwrap();
        assert_eq!(other.agent.provider, Provider::Codex, "no AI, no provider change");
        assert!(finish(&mut other, "protools2", false, None).is_err());
        assert!(finish(&mut other, "logic", true, Some("nobody")).is_err());
        let state = onboarding(&settings);
        assert_eq!(state["done"], true);
        assert!(state["bring"].as_str().unwrap().contains("DAWproject"));
        assert_eq!(state["steps"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn agents_may_not_answer_the_setup() {
        let permissions = settings::Settings::default().agent.permissions;
        assert!(denied_for_agent("app.finishOnboarding", &permissions).is_some());
        let mut closed = permissions.clone();
        closed.file_operations = false;
        closed.replace_session = false;
        assert!(denied_for_agent("session.exportTo", &closed).is_some());
        assert!(denied_for_agent("session.importFrom", &closed).is_some());
        assert!(denied_for_agent("app.openRecent", &closed).is_some());
        assert!(denied_for_agent("app.recent", &closed).is_none());
    }
}
