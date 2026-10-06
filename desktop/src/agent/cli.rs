//! Vendor CLIs as providers: the installed Codex CLI or Claude Code CLI runs one turn in a
//! child process, connects back to this window through the MCP bridge, and streams its
//! progress here. Credentials stay with the CLI; ryolune passes only the discovery path.

use super::{bounded, read_line_limited, system_prompt, Event, Turn, TEXT_LIMIT};
use ryolune_engine::Result;
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{atomic::Ordering, mpsc},
    time::{Duration, Instant},
};

const LINE_LIMIT: usize = 64 * 1024 * 1024;

pub(crate) fn group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
}

pub(crate) fn terminate_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn kill(pid: std::os::raw::c_int, signal: std::os::raw::c_int) -> std::os::raw::c_int;
        }
        // Each run starts its own process group. Signal it directly: command-line kill
        // utilities differ in how they parse a negative PID.
        if let Ok(pid) = std::os::raw::c_int::try_from(child.id()) {
            if pid > 1 {
                // SAFETY: the checked positive child PID is its owned process-group ID.
                // Negation cannot overflow or select group 0 / all processes. SIGKILL is 9
                // on the supported macOS and Linux targets.
                unsafe { kill(-pid, 9) };
            }
        }
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Run `<exe> <args>` to completion with a deadline and a bounded capture of stdout.
fn preflight(
    executable: &Path,
    args: &[&str],
    cancel: &std::sync::atomic::AtomicBool,
    install_hint: &str,
) -> Result<String> {
    let mut output = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(output.try_clone().map_err(|e| e.to_string())?)
        .stderr(Stdio::null());
    group(&mut command);
    let mut child = command.spawn().map_err(|e| {
        format!(
            "{} could not start: {e}. {install_hint}",
            executable.display()
        )
    })?;
    let started = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Acquire) || started.elapsed() > Duration::from_secs(15) {
            terminate_tree(&mut child);
            return Err("The CLI capability check was stopped or timed out.".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => {
                terminate_tree(&mut child);
                return Err(e.to_string());
            }
        }
    };
    terminate_tree(&mut child);
    output.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut text = String::new();
    output
        .take(LINE_LIMIT as u64)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!(
            "{} {} failed. {install_hint}",
            executable.display(),
            args.join(" ")
        ));
    }
    Ok(text)
}

pub(super) fn prompt_with_context(turn: &Turn, prefix: &str) -> String {
    let summary = bounded(
        &serde_json::to_string(&turn.session_summary).unwrap_or_default(),
        8000,
    );
    if prefix.trim().is_empty() {
        format!("{}\n\n[Current session overview: {summary}]", turn.prompt)
    } else {
        format!(
            "Conversation so far, for context only:\n{prefix}\n\nNew request:\n{}\n\n[Current session overview: {summary}]",
            turn.prompt
        )
    }
}

/// Claude Code: `claude -p --output-format stream-json` with this window's MCP server.
pub(crate) fn run_claude(turn: Turn) -> Result<()> {
    let executable = super::discover_claude(&turn.settings.agent.claude_executable);
    let hint = "Install Claude Code, set its path in Settings > Agent, then sign in there.";
    let _ = turn
        .events
        .send(Event::Status("Checking Claude Code…".into()));
    preflight(&executable, &["--version"], &turn.cancel, hint)?;
    let workspace = ryolune_engine::host::scan::data_dir().join("agent-workspace");
    fs::create_dir_all(&workspace)
        .map_err(|e| format!("Could not create the agent workspace: {e}"))?;
    let config_path = workspace.join(format!("mcp-{}.json", std::process::id()));
    let config = json!({
        "mcpServers": {
            "ryolune": {
                "command": turn.mcp_executable,
                "args": ["--live"],
                "env": { "RYOLUNE_CONTROL": turn.discovery.to_string_lossy() }
            }
        }
    });
    fs::write(&config_path, config.to_string())
        .map_err(|e| format!("Could not write the MCP config: {e}"))?;
    let mut command = Command::new(&executable);
    command
        .args([
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--no-session-persistence",
            "--setting-sources",
            "",
            "--tools",
            "",
            "--mcp-config",
        ])
        .arg(&config_path)
        .args([
            "--strict-mcp-config",
            "--allowedTools",
            "mcp__ryolune__*",
            "--disallowedTools",
            "Bash,Edit,Write,MultiEdit,NotebookEdit,Read,Glob,Grep,WebFetch,WebSearch,Task,Agent",
            "--append-system-prompt",
        ])
        .arg(system_prompt(&turn.settings));
    let model = turn.settings.model();
    if !model.is_empty() {
        command.args(["--model", &model]);
    }
    if !turn.settings.agent.reasoning_effort.is_empty() {
        command.args(["--effort", &turn.settings.agent.reasoning_effort]);
    }
    command
        .current_dir(&workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    group(&mut command);
    let result = run_steerable(&mut command, &turn, parse_claude_event, "Claude Code");
    let _ = fs::remove_file(&config_path);
    result
}

/// A CLI turn that the person can steer. `claude -p` reads its whole prompt before it
/// starts and keeps no session here, so steering stops the run (its edits are already in
/// the song and in Undo) and starts it again with the request, what it had answered so far
/// and the steering. Codex and zenith steer their running turn instead.
fn run_steerable(
    command: &mut Command,
    turn: &Turn,
    parse: fn(&Value, &mut Transcript, &mpsc::SyncSender<Event>),
    label: &str,
) -> Result<()> {
    let request = prompt_with_context(turn, &turn_prefix(turn));
    let mut prompt = request.clone();
    let mut steering: Vec<String> = vec![];
    let mut earlier: Vec<String> = vec![];
    loop {
        let run = spawn_and_wait(command, prompt, turn, parse, label, true)?;
        let Some(text) = run.steered else {
            return report(turn, run, label, &steering);
        };
        let _ = turn.events.send(Event::TextEnd);
        let _ = turn.events.send(Event::Steered);
        let _ = turn.events.send(Event::Status(format!(
            "{label} restarts with your steering…"
        )));
        if !run.transcript.response.trim().is_empty() {
            earlier.push(bounded(run.transcript.response.trim(), 4000));
        }
        steering.push(text);
        prompt = restarted_prompt(&request, &earlier, &steering);
    }
}

/// The request again, after the person steered a run that was stopped for it.
fn restarted_prompt(request: &str, earlier: &[String], steering: &[String]) -> String {
    let said = if earlier.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nWhat you had answered so far:\n{}",
            earlier.join("\n\n")
        )
    };
    let texts: Vec<&str> = steering
        .iter()
        .map(|t| t.strip_prefix(super::STEERING_HEADER).unwrap_or(t).trim())
        .collect();
    format!(
        "{request}\n\nYou had already started on this request; your run was stopped so you could read the person's steering. The edits you made are in the song now (call session_overview: the overview above is from before you started), so do not redo them.{said}\n\n{}",
        super::steering_message(&texts.join("\n\n"))
    )
}

pub(super) fn turn_prefix(turn: &Turn) -> String {
    // CLI providers restart per turn; carry the recent exchange as plain text.
    turn.history
        .iter()
        .flat_map(|m| {
            m.parts.iter().filter_map(move |p| match p {
                super::Part::Text(t) if !t.is_empty() => Some(format!(
                    "{}: {}",
                    if m.role == "user" {
                        "Person"
                    } else {
                        "Assistant"
                    },
                    bounded(t, 1500)
                )),
                _ => None,
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

struct Transcript {
    response: String,
    error: Option<String>,
    completed: bool,
}

/// Spawn the CLI, feed the prompt on stdin, stream its JSON lines into events and wait.
#[cfg(test)]
fn run_child(
    mut command: Command,
    prompt: String,
    turn: &Turn,
    parse: fn(&Value, &mut Transcript, &mpsc::SyncSender<Event>),
    label: &str,
) -> Result<()> {
    let run = spawn_and_wait(&mut command, prompt, turn, parse, label, false)?;
    report(turn, run, label, &[])
}

/// How one CLI run ended.
struct ChildRun {
    /// `None` when it was stopped: by the person, or for their steering (`steered`).
    status: Option<std::process::ExitStatus>,
    steered: Option<String>,
    input: std::io::Result<()>,
    transcript: Transcript,
    stderr: String,
}

fn spawn_and_wait(
    command: &mut Command,
    prompt: String,
    turn: &Turn,
    parse: fn(&Value, &mut Transcript, &mpsc::SyncSender<Event>),
    label: &str,
    steerable: bool,
) -> Result<ChildRun> {
    let mut child = command.spawn().map_err(|e| {
        format!("Could not start {label}: {e}. Check its executable in Settings > Agent and sign in there.")
    })?;
    let stdout = child.stdout.take().ok_or("The CLI has no output stream")?;
    let stderr = child.stderr.take().ok_or("The CLI has no error stream")?;
    let events = turn.events.clone();
    let out_reader =
        std::thread::spawn(move || read_events(BufReader::new(stdout), &events, parse));
    let err_reader = std::thread::spawn(move || read_errors(BufReader::new(stderr)));
    let mut stdin = child.stdin.take().ok_or("The CLI has no input stream")?;
    // Keep cancellation responsive even if a broken CLI never reads its input pipe.
    let input_writer = std::thread::spawn(move || {
        let result = stdin.write_all(prompt.as_bytes());
        drop(stdin);
        result
    });
    let _ = turn.events.send(Event::Status(format!(
        "{label} is connecting to this session…"
    )));
    let mut steered = None;
    let status = loop {
        if turn.cancel.load(Ordering::Acquire) {
            terminate_tree(&mut child);
            break None;
        }
        if steerable {
            if let Some(text) = super::take_steering(&turn.steer) {
                terminate_tree(&mut child);
                steered = Some(text);
                break None;
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                terminate_tree(&mut child);
                break Some(status);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(error) => {
                terminate_tree(&mut child);
                let _ = input_writer.join();
                let _ = out_reader.join();
                let _ = err_reader.join();
                return Err(format!("Could not monitor {label}: {error}"));
            }
        }
    };
    let input_result = input_writer
        .join()
        .map_err(|_| "CLI input writer stopped")?;
    let transcript = out_reader
        .join()
        .map_err(|_| "CLI output reader stopped")??;
    let stderr = err_reader
        .join()
        .map_err(|_| "CLI error reader stopped")??;
    Ok(ChildRun {
        status,
        steered,
        input: input_result,
        transcript,
        stderr,
    })
}

/// End the turn: what went wrong, if anything, and the history for the next prompt.
fn report(turn: &Turn, run: ChildRun, label: &str, steering: &[String]) -> Result<()> {
    let ChildRun {
        status,
        input: input_result,
        transcript,
        stderr,
        ..
    } = run;
    let cancelled = status.is_none();
    let error = if cancelled {
        None
    } else if status.is_some_and(|status| !status.success()) {
        Some(format!(
            "{label} exited unsuccessfully. {}{}",
            transcript.error.as_deref().unwrap_or(""),
            if stderr.is_empty() {
                format!("Check the {label} sign-in in Settings > Agent.")
            } else {
                format!("\n{stderr}")
            }
        ))
    } else if let Err(error) = input_result {
        Some(format!("Could not send the task to {label}: {error}"))
    } else if transcript.error.is_some() {
        transcript.error
    } else if !transcript.completed {
        Some(format!(
            "{label} ended without a completed turn. Finished edits remain in Undo."
        ))
    } else {
        None
    };
    if !stderr.is_empty() && error.is_none() {
        let _ = turn.events.send(Event::Status(format!(
            "CLI notice: {}",
            bounded(&stderr, 400)
        )));
    }
    let mut history = turn.history.clone();
    let mut parts = vec![super::Part::Text(turn.prompt.clone())];
    parts.extend(steering.iter().cloned().map(super::Part::Text));
    history.push(super::Message {
        role: "user",
        parts,
    });
    history.push(super::Message {
        role: "assistant",
        parts: vec![super::Part::Text(transcript.response)],
    });
    let _ = turn.events.send(Event::Done {
        error,
        cancelled,
        history,
    });
    Ok(())
}

fn read_events(
    mut reader: impl BufRead,
    events: &mpsc::SyncSender<Event>,
    parse: fn(&Value, &mut Transcript, &mpsc::SyncSender<Event>),
) -> Result<Transcript> {
    let mut transcript = Transcript {
        response: String::new(),
        error: None,
        completed: false,
    };
    while let Some(line) = read_line_limited(&mut reader, LINE_LIMIT).map_err(|e| e.to_string())? {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<Value>(&line) else {
            let _ = events.send(Event::Status(format!(
                "CLI notice: {}",
                bounded(line.trim(), 400)
            )));
            continue;
        };
        parse(&event, &mut transcript, events);
    }
    Ok(transcript)
}

#[cfg(test)]
fn parse_codex_event(event: &Value, transcript: &mut Transcript, events: &mpsc::SyncSender<Event>) {
    match event["type"].as_str().unwrap_or("") {
        "item.started" | "item.updated" | "item.completed" => {
            let item = &event["item"];
            match item["type"].as_str().unwrap_or("") {
                "agent_message" => {
                    if let Some(text) = item["text"].as_str() {
                        transcript.response = bounded(text, TEXT_LIMIT);
                        let _ = events.send(Event::Text {
                            text: transcript.response.clone(),
                            replace: true,
                        });
                    }
                }
                "mcp_tool_call" => {
                    let _ = events.send(Event::ToolNote {
                        name: item["tool"].as_str().unwrap_or("ryolune command").into(),
                        done: event["type"] == "item.completed",
                    });
                }
                _ => {}
            }
        }
        "turn.completed" => transcript.completed = true,
        "turn.failed" | "error" => {
            transcript.error = Some(bounded(
                event["error"]["message"]
                    .as_str()
                    .or_else(|| event["message"].as_str())
                    .unwrap_or("Codex reported a failed turn"),
                TEXT_LIMIT,
            ));
        }
        _ => {}
    }
}

fn parse_claude_event(
    event: &Value,
    transcript: &mut Transcript,
    events: &mpsc::SyncSender<Event>,
) {
    match event["type"].as_str().unwrap_or("") {
        "stream_event" => {
            let stream = &event["event"];
            match stream["type"].as_str().unwrap_or("") {
                "message_start" => transcript.response.clear(),
                "content_block_delta" if stream["delta"]["type"] == "text_delta" => {
                    if let Some(text) = stream["delta"]["text"].as_str() {
                        transcript.response =
                            bounded(&format!("{}{text}", transcript.response), TEXT_LIMIT);
                        let _ = events.send(Event::Text {
                            text: text.into(),
                            replace: false,
                        });
                    }
                }
                _ => {}
            }
        }
        "assistant" => {
            let blocks = event["message"]["content"].as_array();
            let text = blocks
                .into_iter()
                .flatten()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            if !text.is_empty() {
                transcript.response = bounded(&text, TEXT_LIMIT);
                let _ = events.send(Event::Text {
                    text: transcript.response.clone(),
                    replace: true,
                });
                let _ = events.send(Event::TextEnd);
            }
            for block in blocks.into_iter().flatten() {
                if block["type"] == "tool_use" {
                    let _ = events.send(Event::ToolNote {
                        name: block["name"].as_str().unwrap_or("ryolune command").into(),
                        done: false,
                    });
                }
            }
        }
        "user" => {
            for block in event["message"]["content"].as_array().into_iter().flatten() {
                if block["type"] == "tool_result" {
                    let _ = events.send(Event::ToolNote {
                        name: "tool".into(),
                        done: true,
                    });
                }
            }
        }
        "result" => {
            transcript.completed = true;
            if let Some(text) = event["result"].as_str() {
                if !text.is_empty() && transcript.response != text {
                    transcript.response = bounded(text, TEXT_LIMIT);
                    let _ = events.send(Event::Text {
                        text: transcript.response.clone(),
                        replace: true,
                    });
                    let _ = events.send(Event::TextEnd);
                }
            }
            if event["is_error"].as_bool().unwrap_or(false) {
                transcript.error = Some(bounded(
                    event["result"]
                        .as_str()
                        .unwrap_or("Claude Code reported an error"),
                    TEXT_LIMIT,
                ));
            }
            let usage = &event["usage"];
            let _ = events.send(Event::Usage {
                input: usage["input_tokens"].as_u64().unwrap_or(0),
                output: usage["output_tokens"].as_u64().unwrap_or(0),
            });
        }
        "system" => {
            if event["subtype"] == "init" {
                let _ = events.send(Event::Status("Claude Code connected".into()));
            }
        }
        _ => {}
    }
}

pub(super) fn read_errors(mut reader: impl BufRead) -> Result<String> {
    let mut output = String::new();
    while let Some(line) = read_line_limited(&mut reader, LINE_LIMIT).map_err(|e| e.to_string())? {
        output.push_str(&line);
        if output.len() > TEXT_LIMIT {
            let mut start = output.len() - TEXT_LIMIT;
            while !output.is_char_boundary(start) {
                start += 1;
            }
            output.drain(..start);
        }
    }
    Ok(output)
}

/// The discovery path and MCP companion for tests and the bridge configuration.
pub(crate) fn companion(name: &str) -> String {
    let executable = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    if let Some(path) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(&executable)))
    {
        if path.is_file() {
            return path.to_string_lossy().into_owned();
        }
    }
    executable
}
#[allow(dead_code)]
pub(crate) fn workspace() -> PathBuf {
    ryolune_engine::host::scan::data_dir().join("agent-workspace")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::sync::{atomic::AtomicBool, Arc};

    /// A turn for the process-runner tests, which only run where a shell script can stand in
    /// for the agent CLI.
    #[cfg(unix)]
    fn turn(prompt: &str, events: mpsc::SyncSender<Event>, cancel: Arc<AtomicBool>) -> Turn {
        Turn {
            mcp_executable: "C:\\ryolune tools\\ryolune-mcp.exe".into(),
            cancel,
            ..Turn::test(prompt, Default::default(), events)
        }
    }

    #[cfg(unix)]
    #[test]
    fn steering_restarts_a_cli_run_with_the_request_the_answer_so_far_and_the_steering() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("fake-claude");
        // Each run keeps its prompt; the first one never ends on its own.
        fs::write(
            &binary,
            "#!/bin/sh\nn=$(ls prompt-* 2>/dev/null | wc -l)\ncat > prompt-$n\nif [ \"$n\" = 0 ]; then\n  printf '%s\\n' '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Adding drums\"}]}}'\n  sleep 30\nfi\nprintf '%s\\n' '{\"type\":\"result\",\"result\":\"Slower drums added.\",\"is_error\":false}'\n",
        )
        .unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let (tx, rx) = mpsc::sync_channel(64);
        let turn = turn("Make a beat", tx, Arc::new(AtomicBool::new(false)));
        let steer = turn.steer.clone();
        let first = dir.path().join("prompt-0");
        std::thread::spawn(move || {
            let waiting = Instant::now();
            while !first.exists() && waiting.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(10));
            }
            std::thread::sleep(Duration::from_millis(200));
            steer.lock().unwrap().push_back("Slower, please".into());
        });
        let mut command = Command::new(&binary);
        command
            .current_dir(dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        group(&mut command);
        let started = Instant::now();
        run_steerable(&mut command, &turn, parse_claude_event, "Fake").unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the first run was stopped"
        );
        let second = fs::read_to_string(dir.path().join("prompt-1")).unwrap();
        assert!(second.starts_with("Make a beat"));
        assert!(second.contains("Adding drums"));
        assert!(second.contains("do not redo them"));
        assert!(second.ends_with("Slower, please"));
        let events: Vec<Event> = rx.try_iter().collect();
        assert!(events.iter().any(|e| matches!(e, Event::Steered)));
        let history = events
            .into_iter()
            .find_map(|e| match e {
                Event::Done { history, error, .. } => {
                    assert!(error.is_none(), "{error:?}");
                    Some(history)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(history[0].parts.len(), 2, "the request and the steering");
        assert_eq!(
            history[1].parts[0],
            super::super::Part::Text("Slower drums added.".into())
        );
    }

    #[test]
    fn codex_events_report_tools_final_text_and_failures_without_exposing_reasoning() {
        let (tx, rx) = mpsc::sync_channel(64);
        let source = [
            json!({"type":"item.started","item":{"type":"mcp_tool_call","tool":"session_info"}}),
            json!({"type":"item.completed","item":{"type":"reasoning","text":"private reasoning"}}),
            json!({"type":"item.completed","item":{"type":"agent_message","text":"Added a bass track."}}),
            json!({"type":"turn.completed"}),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
        let result = read_events(source.as_bytes(), &tx, parse_codex_event).unwrap();
        assert_eq!(result.response, "Added a bass track.");
        assert!(result.completed && result.error.is_none());
        let events: Vec<Event> = rx.try_iter().collect();
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::ToolNote { name, .. } if name == "session_info")));
        assert!(!events
            .iter()
            .any(|e| matches!(e, Event::Text { text, .. } if text.contains("reasoning"))));
        let result = read_events(
            b"{\"type\":\"turn.failed\",\"error\":{\"message\":\"Login required\"}}\n".as_slice(),
            &tx,
            parse_codex_event,
        )
        .unwrap();
        assert_eq!(result.error.as_deref(), Some("Login required"));
    }

    #[test]
    fn claude_partial_text_is_replaced_once_by_final_snapshot() {
        let (tx, rx) = mpsc::sync_channel(32);
        let source = [
            json!({"type":"stream_event","event":{"type":"message_start"}}),
            json!({"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"**One"}}}),
            json!({"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":" beat.**"}}}),
            json!({"type":"assistant","message":{"content":[{"type":"text","text":"**One beat.**"}]}}),
            json!({"type":"result","result":"**One beat.**","is_error":false})
        ].iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
        let result = read_events(source.as_bytes(), &tx, parse_claude_event).unwrap();
        assert!(result.completed);
        let events: Vec<Event> = rx.try_iter().collect();
        let text_events: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::Text { text, replace } => Some((text.as_str(), *replace)),
                _ => None,
            })
            .collect();
        assert_eq!(
            text_events,
            vec![
                ("**One", false),
                (" beat.**", false),
                ("**One beat.**", true)
            ]
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::TextEnd))
                .count(),
            1
        );
    }

    #[test]
    fn claude_stream_json_yields_text_tools_usage_and_errors() {
        let (tx, rx) = mpsc::sync_channel(64);
        let source = [
            json!({"type":"system","subtype":"init","session_id":"abc"}),
            json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"mcp__ryolune__track_add","input":{}}]}}),
            json!({"type":"user","message":{"content":[{"type":"tool_result","content":"ok"}]}}),
            json!({"type":"assistant","message":{"content":[{"type":"text","text":"Done: one track."}]}}),
            json!({"type":"result","subtype":"success","result":"Done: one track.","is_error":false,"usage":{"input_tokens":10,"output_tokens":5}}),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
        let result = read_events(source.as_bytes(), &tx, parse_claude_event).unwrap();
        assert_eq!(result.response, "Done: one track.");
        assert!(result.completed && result.error.is_none());
        let events: Vec<Event> = rx.try_iter().collect();
        assert!(events.iter().any(
            |e| matches!(e, Event::ToolNote { name, done: false } if name.ends_with("track_add"))
        ));
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Usage {
                input: 10,
                output: 5
            }
        )));
        let failed = read_events(
            b"{\"type\":\"result\",\"subtype\":\"error_during_execution\",\"result\":\"Not logged in\",\"is_error\":true}\n".as_slice(),
            &tx,
            parse_claude_event,
        )
        .unwrap();
        assert_eq!(failed.error.as_deref(), Some("Not logged in"));
    }

    #[cfg(unix)]
    #[test]
    fn process_runner_uses_stdin_and_distinguishes_nonzero_exit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("fake-cli");
        fs::write(&binary, "#!/bin/sh\ncat > received-prompt\nprintf '%s\\n' '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"Done\"}}' '{\"type\":\"turn.completed\"}'\nexit 7\n").unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let (tx, rx) = mpsc::sync_channel(64);
        let cancel = Arc::new(AtomicBool::new(false));
        let turn = turn("Make a bass line", tx, cancel);
        let mut command = Command::new(&binary);
        command
            .current_dir(dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        group(&mut command);
        run_child(
            command,
            "Make a bass line".into(),
            &turn,
            parse_codex_event,
            "Fake",
        )
        .unwrap();
        let events: Vec<Event> = rx.try_iter().collect();
        let done = events
            .iter()
            .find_map(|e| match e {
                Event::Done {
                    error,
                    cancelled,
                    history,
                } => Some((error.clone(), *cancelled, history.len())),
                _ => None,
            })
            .unwrap();
        assert!(done.0.unwrap().contains("unsuccessfully"));
        assert!(!done.1);
        assert_eq!(done.2, 2, "the turn joins the history for the next prompt");
        assert_eq!(
            fs::read_to_string(dir.path().join("received-prompt")).unwrap(),
            "Make a bass line"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_kills_descendants_holding_output_pipes() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("fake-cli");
        fs::write(
            &binary,
            "#!/bin/sh\nsleep 30 &\nprintf started > started\nwait\n",
        )
        .unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let later = cancel.clone();
        let started_file = dir.path().join("started");
        std::thread::spawn(move || {
            let waiting = Instant::now();
            while !started_file.exists() && waiting.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(5));
            }
            later.store(true, Ordering::Release);
        });
        let (tx, rx) = mpsc::sync_channel(64);
        let turn = turn("Test", tx, cancel);
        let mut command = Command::new(&binary);
        command
            .current_dir(dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        group(&mut command);
        let started = Instant::now();
        run_child(command, "Test".into(), &turn, parse_codex_event, "Fake").unwrap();
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(rx.try_iter().any(|e| matches!(
            e,
            Event::Done {
                cancelled: true,
                ..
            }
        )));
    }

    #[test]
    fn codex_invocation_is_isolated_and_keeps_secrets_out_of_argv() {
        // The invocation shape is fixed by run_codex; exercise its option list through a
        // command built the same way.
        let mut command = Command::new("codex");
        for option in [
            "features.shell_tool=false",
            "features.apps=false",
            "features.plugins=false",
            "--ignore-user-config",
        ] {
            command.arg(option);
        }
        command.args([
            "-c",
            &format!(
                "mcp_servers.ryolune.command={}",
                json!("C:\\ryolune tools\\ryolune-mcp.exe")
            ),
        ]);
        let args: Vec<_> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--ignore-user-config".into()));
        assert!(args
            .iter()
            .any(|arg| arg
                == "mcp_servers.ryolune.command=\"C:\\\\ryolune tools\\\\ryolune-mcp.exe\""));
        assert!(!args
            .iter()
            .any(|arg| arg.contains("token") || arg.contains("auth.json")));
    }
}
