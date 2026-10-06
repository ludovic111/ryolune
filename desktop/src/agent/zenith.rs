//! zenith, lsuite's agent hub, as a provider ("Zenith · lsuite"). The agents and their
//! sign-ins live in zenith; ryolune drives it through `zenith-cli` (`project.list`,
//! `project.add`, `thread.new`, `thread.send`, `thread.get`, `thread.interrupt`,
//! `provider.list`) and zenith's agent edits the song through ryolune's MCP server, so its
//! edits arrive like any MCP client's: undo steps, in the Changes list and in the chat.
//!
//! Each song has its own folder, `<data dir>/agent-workspaces/<song id>`, registered as a
//! zenith project and holding ryolune's MCP recipe (`.mcp.json`); a conversation's first
//! turn starts a zenith thread whose id ryolune chooses first (so Stop can always interrupt
//! it), and follow-ups and steering go to that thread with `thread.send`.

use super::{bounded, take_steering, Event, Message, Part, Remote, Turn, TEXT_LIMIT};
use ryolune_engine::Result;
use serde_json::{json, Value};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

const CALL_TIMEOUT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(750);
/// How long a sent message may take to show in the thread.
const SHOW_TIMEOUT: Duration = Duration::from_secs(30);

/// `zenith-cli`: `$RYOLUNE_ZENITH_CLI`, the path in Settings, zenith's lsuite discovery
/// entry, then PATH.
pub(crate) fn executable(configured: &str) -> Option<PathBuf> {
    let file = |path: PathBuf| path.is_file().then_some(path);
    if let Some(path) = std::env::var_os("RYOLUNE_ZENITH_CLI").filter(|p| !p.is_empty()) {
        return file(PathBuf::from(path));
    }
    let configured = configured.trim();
    if !configured.is_empty() {
        return file(PathBuf::from(configured));
    }
    ryolune_engine::lsuite::entry("zenith")
        .and_then(|entry| entry["cli"].as_str().map(PathBuf::from))
        .filter(|path| path.is_absolute())
        .and_then(file)
        .or_else(|| {
            let name = format!("zenith-cli{}", std::env::consts::EXE_SUFFIX);
            std::env::var_os("PATH").and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|dir| dir.join(&name))
                    .find(|path| path.is_file())
            })
        })
}

/// One `zenith-cli <command> --json <args>` call, answered as JSON.
pub(crate) fn call(exe: &Path, command: &str, args: &Value) -> Result<Value> {
    let mut child = Command::new(exe);
    child
        .arg(command)
        .arg("--json")
        .arg(args.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    super::cli::group(&mut child);
    let mut child = child
        .spawn()
        .map_err(|e| format!("Could not start zenith-cli ({}): {e}", exe.display()))?;
    let stdout = child.stdout.take().ok_or("zenith-cli has no output")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("zenith-cli has no error output")?;
    let read = |pipe: Box<dyn Read + Send>, limit: u64| {
        std::thread::spawn(move || {
            let mut bytes = vec![];
            let _ = pipe.take(limit).read_to_end(&mut bytes);
            bytes
        })
    };
    let out = read(Box::new(stdout), 16 * 1024 * 1024);
    let err = read(Box::new(stderr), 64 * 1024);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > CALL_TIMEOUT => {
                super::cli::terminate_tree(&mut child);
                return Err(format!(
                    "zenith did not answer {command} in time. Check that zenith is running."
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => {
                super::cli::terminate_tree(&mut child);
                return Err(format!("Could not follow zenith-cli: {e}"));
            }
        }
    };
    let out = out.join().unwrap_or_default();
    let err = err.join().unwrap_or_default();
    if !status.success() {
        let message = String::from_utf8_lossy(&err);
        return Err(format!(
            "zenith {command}: {}",
            bounded(message.trim().trim_start_matches("Error: "), 2000)
        ));
    }
    serde_json::from_slice(&out)
        .map_err(|e| format!("zenith answered {command} with something that is not JSON: {e}"))
}

/// zenith's models, as `provider/model` ids for the model picker.
pub(crate) fn models(configured: &str) -> Result<Vec<super::catalog::Model>> {
    let exe = executable(configured).ok_or("Install zenith to use its lsuite agents.")?;
    models_at(&exe)
}

fn models_at(exe: &Path) -> Result<Vec<super::catalog::Model>> {
    let providers = call(exe, "provider.list", &json!({}))?;
    Ok(providers
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["enabled"] != false)
        .flat_map(|p| {
            let provider = p["provider"].as_str().unwrap_or("").to_string();
            let label = p["name"].as_str().unwrap_or(&provider).to_string();
            p["models"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(move |m| {
                    let model = m["model"].as_str()?;
                    Some(super::catalog::Model {
                        id: format!("{provider}/{model}"),
                        name: format!("{label} · {}", m["name"].as_str().unwrap_or(model)),
                        efforts: vec![],
                    })
                })
                .collect::<Vec<_>>()
        })
        .filter(|m| !m.id.starts_with('/'))
        .collect())
}

/// Whether zenith answers, for the connection check.
pub(crate) fn reachable(configured: &str) -> std::result::Result<(), (&'static str, String)> {
    let exe = executable(configured).ok_or((
        "missingCli",
        "Install zenith, open it once, then check again. ryolune finds it through lsuite.".into(),
    ))?;
    call(&exe, "provider.list", &json!({}))
        .map(|_| ())
        .map_err(|_| {
            (
                "unavailable",
                "zenith did not answer. Open zenith (its server must be running), then check again."
                    .into(),
            )
        })
}

/// `provider/model` picks a zenith provider instance and its model; a bare name is a model.
fn selection(model: &str, args: &mut Value) {
    if let Some((provider, model)) = model.split_once('/') {
        args["provider"] = json!(provider);
        args["model"] = json!(model);
    } else if !model.is_empty() {
        args["model"] = json!(model);
    }
}

/// The song's folder, registered as a zenith project.
pub(crate) fn workspace(song: &str) -> PathBuf {
    ryolune_engine::host::scan::data_dir()
        .join("agent-workspaces")
        .join(crate::conversations::song_key(song))
}

/// The first message of a zenith thread: where it works and how.
fn first_message(turn: &Turn) -> String {
    let (id, name) = &turn.song;
    format!(
        "You are working in ryolune, the music app of lsuite, on the song \"{name}\" (id {id}). Edit it only through ryolune's MCP tools (the `ryolune` server: session_overview, track_add, clip_create and the rest); start with session_overview, and stop if another song is open. Every edit is an undo step the person can revert in ryolune. Do not create files in this folder: the song lives in ryolune.\n\n{}\n\nRequest:\n{}",
        super::system_prompt(&turn.settings),
        super::user_text(&turn.prompt, &turn.session_summary)
    )
}

fn follow_up(turn: &Turn) -> String {
    format!(
        "Request (the song \"{}\" in ryolune):\n{}",
        turn.song.1, turn.prompt
    )
}

/// ryolune's MCP recipe in the song's folder, for agents that read a project's `.mcp.json`;
/// zenith also hands its agents ryolune's server from the lsuite discovery entry.
fn prepare(folder: &Path, turn: &Turn) -> Result<()> {
    std::fs::create_dir_all(folder)
        .map_err(|e| format!("Could not create the zenith workspace: {e}"))?;
    let server = super::clients::Server {
        command: turn.mcp_executable.clone(),
        discovery: turn.discovery.display().to_string(),
    };
    std::fs::write(folder.join(".mcp.json"), server.project_file())
        .map_err(|e| format!("Could not write the MCP recipe for zenith: {e}"))?;
    std::fs::write(
        folder.join("AGENTS.md"),
        format!(
            "# ryolune song \"{}\"\n\nThis folder stands for a song open in ryolune. Work on it only through the `ryolune` MCP server (`{} --live`, configured in .mcp.json); do not create files here.\n",
            turn.song.1, turn.mcp_executable
        ),
    )
    .map_err(|e| format!("Could not write the zenith workspace notes: {e}"))
}

pub(crate) fn run(turn: Turn) -> Result<()> {
    let exe = executable(&turn.settings.agent.zenith_executable).ok_or(
        "zenith was not found. Install zenith (lsuite.xyz) and open it once, or set its zenith-cli in Settings > Agent.",
    )?;
    run_at(&turn, &exe, &workspace(&turn.song.0))
}

fn run_at(turn: &Turn, exe: &Path, folder: &Path) -> Result<()> {
    prepare(folder, turn)?;
    let _ = turn
        .events
        .send(Event::Status("Connecting to zenith…".into()));
    let model = turn.settings.model();
    let (thread, prompt) = match &turn.remote {
        Some(id) => {
            let prompt = follow_up(turn);
            let mut args = json!({"threadId": id, "prompt": prompt});
            selection(&model, &mut args);
            call(exe, "thread.send", &args)?;
            (id.clone(), prompt)
        }
        None => {
            let projects = call(exe, "project.list", &json!({}))?;
            let path = folder.to_string_lossy();
            let existing = projects
                .as_array()
                .into_iter()
                .flatten()
                .find(|p| p["path"].as_str() == Some(path.as_ref()))
                .and_then(|p| p["projectId"].as_str().map(str::to_string));
            let project = match existing {
                Some(id) => id,
                None => call(
                    exe,
                    "project.add",
                    &json!({"path": folder, "title": format!("ryolune · {}", turn.song.1)}),
                )?["projectId"]
                    .as_str()
                    .ok_or("zenith did not return a project id.")?
                    .to_string(),
            };
            // Chosen before the thread exists, so Stop can interrupt it even when the
            // answer to thread.new is lost.
            let id = ryolune_engine::lsuite::uuid_v4();
            let _ = turn.events.send(Event::Remote(Remote {
                provider: "zenith".into(),
                id: id.clone(),
            }));
            let prompt = first_message(turn);
            let mut args = json!({"threadId": id, "projectId": project, "prompt": prompt});
            selection(&model, &mut args);
            call(exe, "thread.new", &args)?;
            (id, prompt)
        }
    };
    let mut expected = prompt.clone();
    let mut sent = Instant::now();
    let mut reply = String::new();
    let mut replies: Vec<String> = vec![];
    let mut steering: Vec<String> = vec![];
    let mut shown_status = String::new();
    let mut status_line = |text: &str| {
        if shown_status != text {
            shown_status = text.to_string();
            let _ = turn.events.send(Event::Status(text.into()));
        }
    };
    loop {
        if turn.cancel.load(Ordering::Acquire) {
            let error = interrupt(exe, &thread).err();
            return done(turn, &prompt, &steering, replies, reply, error, true);
        }
        if let Some(text) = take_steering(&turn.steer) {
            // zenith steers its running turn, or starts the next one when it has finished.
            call(
                exe,
                "thread.send",
                &json!({"threadId": thread, "prompt": text}),
            )?;
            let _ = turn.events.send(Event::Steered);
            if !reply.is_empty() {
                let _ = turn.events.send(Event::TextEnd);
                replies.push(std::mem::take(&mut reply));
            }
            expected = text.clone();
            steering.push(text);
            sent = Instant::now();
        }
        let state = call(
            exe,
            "thread.get",
            &json!({"threadId": thread, "messages": 100}),
        )?;
        let status = state["status"].as_str().unwrap_or("working");
        let timeline = state["timeline"].as_array().cloned().unwrap_or_default();
        let last_user = timeline
            .iter()
            .rposition(|m| m["kind"] == "message" && m["role"] == "user");
        // An acknowledgement can come before the message shows in the thread: never stream
        // or finish with the previous turn's answer.
        if !last_user.is_some_and(|i| timeline[i]["text"].as_str() == Some(expected.as_str())) {
            if sent.elapsed() > SHOW_TIMEOUT {
                return Err(
                    "zenith did not show the message. Open zenith to check this conversation."
                        .into(),
                );
            }
            pause(turn);
            continue;
        }
        let text = timeline
            .iter()
            .skip(last_user.map_or(0, |i| i + 1))
            .filter(|m| m["kind"] == "message" && m["role"] == "assistant")
            .filter_map(|m| m["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        if !text.is_empty() && text != reply {
            reply = bounded(&text, TEXT_LIMIT);
            let _ = turn.events.send(Event::Text {
                text: reply.clone(),
                replace: true,
            });
        }
        match status {
            "approval" => {
                status_line("zenith is waiting for your approval: open the conversation in zenith.")
            }
            "input" => {
                status_line("zenith has a question for you: open the conversation in zenith.")
            }
            "failed" => {
                return Err(state["session"]["error"]
                    .as_str()
                    .unwrap_or("The zenith agent failed. Open zenith for details.")
                    .to_string())
            }
            "ready" | "plan-ready" | "monitoring"
                if sent.elapsed() > Duration::from_secs(1)
                    && (!reply.is_empty() || sent.elapsed() > Duration::from_secs(10)) =>
            {
                break
            }
            _ => status_line("Working in zenith…"),
        }
        pause(turn);
    }
    done(turn, &prompt, &steering, replies, reply, None, false)
}

/// Wait between two looks at the thread, answering Stop and steering at once.
fn pause(turn: &Turn) {
    let until = Instant::now() + POLL;
    while Instant::now() < until {
        if turn.cancel.load(Ordering::Acquire)
            || turn.steer.lock().is_ok_and(|queue| !queue.is_empty())
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Stop zenith's turn and wait until it has stopped.
fn interrupt(exe: &Path, thread: &str) -> Result<()> {
    call(exe, "thread.interrupt", &json!({"threadId": thread}))?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(15) {
        let state = call(
            exe,
            "thread.get",
            &json!({"threadId": thread, "messages": 1}),
        )?;
        if matches!(
            state["status"].as_str(),
            Some("ready" | "failed" | "plan-ready" | "monitoring")
        ) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(
        "zenith has not stopped yet. Open zenith to check the conversation before sending again."
            .into(),
    )
}

fn done(
    turn: &Turn,
    prompt: &str,
    steering: &[String],
    mut replies: Vec<String>,
    reply: String,
    error: Option<String>,
    cancelled: bool,
) -> Result<()> {
    if !reply.is_empty() {
        let _ = turn.events.send(Event::TextEnd);
        replies.push(reply);
    }
    let mut history = turn.history.clone();
    let mut parts = vec![Part::Text(prompt.to_string())];
    parts.extend(steering.iter().cloned().map(Part::Text));
    history.push(Message {
        role: "user",
        parts,
    });
    history.push(Message {
        role: "assistant",
        parts: vec![Part::Text(replies.join("\n\n"))],
    });
    let _ = turn.events.send(Event::Done {
        error,
        cancelled,
        history,
    });
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::mpsc;

    /// A stand-in for zenith-cli: logs each call, answers from `<command>.json`, and plays a
    /// thread that shows the old turn first, then the new one.
    fn mock_cli(dir: &Path) -> PathBuf {
        let exe = dir.join("zenith-cli");
        std::fs::write(
            &exe,
            r##"#!/bin/sh
cd "$(dirname "$0")" || exit 1
printf '%s\n%s\n' "$1" "$3" >> calls
case "$1" in
  thread.new|thread.send) printf '%s' "$3" > submitted.json; printf '{}' ;;
  thread.interrupt) touch interrupted; printf '{}' ;;
  thread.get)
    if [ -f interrupted ]; then
      if [ -f stopping ]; then printf '{"status":"ready"}'; else touch stopping; printf '{"status":"working"}'; fi
    elif [ -f polled ]; then cat turn.json
    else touch polled; cat old.json
    fi ;;
  bad) printf 'not json' ;;
  denied) printf 'Error: Permission denied' >&2; exit 1 ;;
  *) cat "$1.json" ;;
esac
"##,
        )
        .unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
        exe
    }

    fn respond(dir: &Path, name: &str, body: Value) {
        std::fs::write(dir.join(format!("{name}.json")), body.to_string()).unwrap();
    }

    fn submitted(dir: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(dir.join("submitted.json")).unwrap()).unwrap()
    }

    #[test]
    fn models_are_qualified_by_provider_and_failures_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let exe = mock_cli(dir.path());
        respond(
            dir.path(),
            "provider.list",
            json!([
                {"provider":"my-codex","name":"Codex","enabled":true,"models":[{"model":"coding-model","name":"Coding"}]},
                {"provider":"off","enabled":false,"models":[{"model":"hidden"}]}
            ]),
        );
        let models = models_at(&exe).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "my-codex/coding-model");
        assert_eq!(models[0].name, "Codex · Coding");
        assert!(call(&exe, "bad", &json!({}))
            .unwrap_err()
            .contains("not JSON"));
        assert!(call(&exe, "denied", &json!({}))
            .unwrap_err()
            .contains("Permission denied"));
    }

    fn turn_for(dir: &Path, prompt: &str, remote: Option<String>) -> (Turn, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::sync_channel(256);
        let mut settings = ryolune_engine::settings::Settings::default();
        settings.agent.provider = ryolune_engine::settings::Provider::Zenith;
        settings.agent.model = "my-codex/coding-model".into();
        let turn = Turn {
            remote,
            discovery: dir.join("control.json"),
            ..Turn::test(prompt, settings, tx)
        };
        (turn, rx)
    }

    fn thread(user: &str, answer: &str) -> Value {
        json!({"status":"ready","timeline":[
            {"kind":"message","role":"user","text":user},
            {"kind":"message","role":"assistant","text":answer}
        ]})
    }

    #[test]
    fn starts_a_thread_in_the_song_folder_then_follows_up_and_ignores_stale_replies() {
        let dir = tempfile::tempdir().unwrap();
        let exe = mock_cli(dir.path());
        let folder = dir.path().join("agent-workspaces").join("song-1");
        respond(dir.path(), "project.list", json!([]));
        respond(
            dir.path(),
            "project.add",
            json!({"projectId":"zenith-project"}),
        );
        respond(dir.path(), "old", thread("an old request", "stale answer"));
        let (turn, rx) = turn_for(dir.path(), "Add drums", None);
        respond(
            dir.path(),
            "turn",
            thread(&first_message(&turn), "Drums added."),
        );
        run_at(&turn, &exe, &folder).unwrap();
        let events: Vec<Event> = rx.try_iter().collect();
        let remote = events
            .iter()
            .find_map(|e| match e {
                Event::Remote(remote) => Some(remote.id.clone()),
                _ => None,
            })
            .expect("the thread id is known before the thread starts");
        assert!(!events
            .iter()
            .any(|e| matches!(e, Event::Text { text, .. } if text.contains("stale"))));
        let history = events
            .into_iter()
            .find_map(|e| match e {
                Event::Done {
                    history,
                    error: None,
                    cancelled: false,
                } => Some(history),
                _ => None,
            })
            .unwrap();
        assert_eq!(history[1].parts[0], Part::Text("Drums added.".into()));
        let sent = submitted(dir.path());
        assert_eq!(sent["threadId"], remote);
        assert_eq!(sent["projectId"], "zenith-project");
        assert_eq!(sent["provider"], "my-codex");
        assert_eq!(sent["model"], "coding-model");
        assert!(
            sent.get("runtimeMode").is_none(),
            "zenith keeps its approval policy"
        );
        // The folder carries ryolune's MCP recipe, pointing at this window.
        let recipe: Value =
            serde_json::from_str(&std::fs::read_to_string(folder.join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(recipe["mcpServers"]["ryolune"]["args"][0], "--live");
        assert!(recipe["mcpServers"]["ryolune"]["env"]["RYOLUNE_CONTROL"]
            .as_str()
            .unwrap()
            .ends_with("control.json"));

        // A follow-up goes to the same thread with thread.send.
        std::fs::remove_file(dir.path().join("polled")).unwrap();
        let (turn, rx) = turn_for(dir.path(), "Make them louder", Some(remote.clone()));
        respond(
            dir.path(),
            "old",
            thread(&first_message(&turn), "Drums added."),
        );
        respond(dir.path(), "turn", thread(&follow_up(&turn), "Louder now."));
        run_at(&turn, &exe, &folder).unwrap();
        assert!(rx
            .try_iter()
            .any(|e| matches!(e, Event::Done { error: None, .. })));
        let sent = submitted(dir.path());
        assert_eq!(sent["threadId"], remote);
        assert!(
            sent.get("projectId").is_none(),
            "follow-ups use thread.send"
        );
        let calls = std::fs::read_to_string(dir.path().join("calls")).unwrap();
        assert_eq!(calls.lines().filter(|l| *l == "project.add").count(), 1);
        assert_eq!(calls.lines().filter(|l| *l == "thread.new").count(), 1);
        assert_eq!(calls.lines().filter(|l| *l == "thread.send").count(), 1);
    }

    #[test]
    fn steering_goes_to_the_thread_and_stop_interrupts_it() {
        let dir = tempfile::tempdir().unwrap();
        let exe = mock_cli(dir.path());
        let folder = dir.path().join("ws");
        let (turn, rx) = turn_for(dir.path(), "Add drums", Some("thread-9".into()));
        // The thread keeps working on the request; the person steers, then stops.
        respond(
            dir.path(),
            "old",
            json!({"status":"working","timeline":[{"kind":"message","role":"user","text":follow_up(&turn)}]}),
        );
        respond(
            dir.path(),
            "turn",
            json!({"status":"working","timeline":[{"kind":"message","role":"user","text":follow_up(&turn)}]}),
        );
        let steer = turn.steer.clone();
        let cancel = turn.cancel.clone();
        let root = dir.path().to_path_buf();
        let person = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            steer
                .lock()
                .unwrap()
                .push_back(super::super::steering_message("Slower"));
            let started = Instant::now();
            while !std::fs::read_to_string(root.join("calls"))
                .unwrap_or_default()
                .contains("Slower")
                && started.elapsed() < Duration::from_secs(10)
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            cancel.store(true, Ordering::Release);
        });
        run_at(&turn, &exe, &folder).unwrap();
        person.join().unwrap();
        let events: Vec<Event> = rx.try_iter().collect();
        assert!(events.iter().any(|e| matches!(e, Event::Steered)));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Done {
                cancelled: true,
                error: None,
                ..
            }
        )));
        let calls = std::fs::read_to_string(dir.path().join("calls")).unwrap();
        let after = calls.split("thread.interrupt").nth(1).unwrap();
        assert_eq!(
            after.lines().filter(|l| *l == "thread.get").count(),
            2,
            "Stop waits until zenith's turn has stopped"
        );
        let sends: Vec<&str> = calls
            .lines()
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|w| w[0] == "thread.send")
            .map(|w| w[1])
            .collect();
        assert!(sends.last().unwrap().contains("Slower"));
    }
}
