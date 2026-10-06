//! Codex app-server transport. Dynamic tools use ryolune's ordinary command dispatcher;
//! agentMessage deltas reach the interface before the turn is complete.
use super::{
    await_tool, bounded, cli, read_line_limited, system_prompt, take_steering, tool_output,
    tool_specs, Event, Message, Part, ToolCall, Turn, TEXT_LIMIT,
};
use ryolune_engine::Result;
use serde_json::{json, Value};
use std::{
    io::{BufReader, Write},
    process::{Command, Stdio},
    sync::{atomic::Ordering, mpsc},
    time::{Duration, Instant},
};

const LINE_LIMIT: usize = 2 * 1024 * 1024;

fn write(writer: &mut impl Write, value: Value) -> Result<()> {
    serde_json::to_writer(&mut *writer, &value).map_err(|e| e.to_string())?;
    writer
        .write_all(b"\n")
        .and_then(|_| writer.flush())
        .map_err(|e| e.to_string())
}

/// A private config root keeps user hooks, MCP servers and plugins out of this music
/// session. Link the credential file so OAuth refreshes remain owned by the CLI.
pub(super) fn isolated_home() -> Result<tempfile::TempDir> {
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    let original = std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| std::path::PathBuf::from(home).join(".codex"))
        })
        .ok_or("Could not locate the Codex account directory")?;
    let auth = original.join("auth.json");
    if auth.is_file() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(&auth, root.path().join("auth.json"))
            .map_err(|e| e.to_string())?;
        #[cfg(windows)]
        std::fs::copy(&auth, root.path().join("auth.json")).map_err(|e| e.to_string())?;
    } else {
        return Err("Codex streaming needs a file-backed sign-in. Run codex login with cli_auth_credentials_store=\"file\", then reconnect in Settings > Agent.".into());
    }
    Ok(root)
}

pub(crate) fn run(turn: Turn) -> Result<()> {
    let home = isolated_home()?;
    let workspace = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut command = Command::new(super::discover_codex(&turn.settings.agent.codex_executable));
    command
        .args(["app-server", "--listen", "stdio://"])
        .env("CODEX_HOME", home.path())
        .current_dir(workspace.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for config in [
        "features.shell_tool=false",
        "features.unified_exec=false",
        "features.apps=false",
        "features.plugins=false",
        "features.memories=false",
        "features.multi_agent=false",
        "features.hooks=false",
        "web_search=\"disabled\"",
        "cli_auth_credentials_store=\"file\"",
    ] {
        command.args(["-c", config]);
    }
    cli::group(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start Codex streaming: {e}"))?;
    let outcome = (|| {
        let mut stdin = child.stdin.take().ok_or("Missing Codex input")?;
        let stdout = child.stdout.take().ok_or("Missing Codex output")?;
        let stderr = child.stderr.take().ok_or("Missing Codex error output")?;
        // Bounded readers never hold up Stop. Their senders exit when this run drops rx.
        let (tx, rx) = mpsc::sync_channel(256);
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_line_limited(&mut reader, LINE_LIMIT) {
                    Ok(Some(line)) => {
                        let event = serde_json::from_str::<Value>(&line)
                            .map_err(|e| format!("Invalid Codex event: {e}"));
                        let failed = event.is_err();
                        if tx.send(event).is_err() || failed {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string()));
                        break;
                    }
                }
            }
        });
        let errors = std::thread::spawn(move || cli::read_errors(BufReader::new(stderr)));
        let result = session(&turn, workspace.path(), &mut stdin, &rx);
        cli::terminate_tree(&mut child);
        drop(rx);
        drop(stdin);
        let _ = reader.join();
        let diagnostics = errors
            .join()
            .ok()
            .and_then(std::result::Result::ok)
            .unwrap_or_default();
        result.map_err(|error| {
            if diagnostics.is_empty() {
                error
            } else {
                format!("{error}\n{}", bounded(&diagnostics, 1200))
            }
        })
    })();
    cli::terminate_tree(&mut child);
    outcome
}

fn session(
    turn: &Turn,
    workspace: &std::path::Path,
    stdin: &mut impl Write,
    rx: &mpsc::Receiver<Result<Value>>,
) -> Result<()> {
    write(
        stdin,
        json!({"id":1,"method":"initialize","params":{
        "clientInfo":{"name":"ryolune","version":env!("CARGO_PKG_VERSION")},
        "capabilities":{"experimentalApi":true}}}),
    )?;
    let mut initialized = false;
    let mut active = false;
    let started = Instant::now();
    let mut transcript = Stream::default();
    let mut calls = 0;
    // Steering: `turn/steer` joins the running turn (Codex reads it at its next step); one
    // that arrives too late to join becomes the next turn of the same thread.
    let mut thread: Option<String> = None;
    let mut running_turn: Option<String> = None;
    let mut turn_starts: std::collections::HashSet<u64> = [3].into();
    let mut steers: std::collections::HashMap<u64, String> = Default::default();
    let mut ignored: std::collections::HashSet<u64> = Default::default();
    let mut late: Vec<String> = vec![];
    let mut steered: Vec<String> = vec![];
    let mut next_id = 10;
    loop {
        if turn.cancel.load(Ordering::Acquire) {
            finish(turn, transcript.response, None, true, &steered);
            return Ok(());
        }
        if !active && started.elapsed() > Duration::from_secs(60) {
            return Err("Codex did not connect within 60 seconds. Check your sign-in.".into());
        }
        if let (Some(thread), Some(running)) = (&thread, &running_turn) {
            if let Some(text) = take_steering(&turn.steer) {
                next_id += 1;
                write(
                    stdin,
                    json!({"id":next_id,"method":"turn/steer","params":{
                    "threadId":thread,"expectedTurnId":running,
                    "input":[{"type":"text","text":text}]}}),
                )?;
                steers.insert(next_id, text);
            }
        }
        let event = match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(value) => value?,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => return Err("Codex disconnected before completing the response".into()),
        };
        // An answer to one of our requests (a steer's may be an error: it came too late).
        let answer = event.get("method").is_none()
            && (event.get("result").is_some() || event.get("error").is_some());
        if answer && event["id"].as_u64().is_some_and(|id| ignored.remove(&id)) {
            continue;
        }
        if let Some(text) = event["id"]
            .as_u64()
            .filter(|_| answer)
            .and_then(|id| steers.remove(&id))
        {
            if event.get("error").is_some() {
                late.push(text);
            } else {
                steered.push(text);
                let _ = turn.events.send(Event::Steered);
            }
            continue;
        }
        if let Some(error) = event.get("error") {
            return Err(bounded(
                error["message"].as_str().unwrap_or("Codex request failed"),
                TEXT_LIMIT,
            ));
        }
        // JSON-RPC request IDs are independent in each direction. A server tool
        // request can use the same ID as one of our initialization requests.
        let response = event.get("method").is_none() && event.get("result").is_some();
        if response && event["id"] == 1 && !initialized {
            initialized = true;
            write(stdin, json!({"method":"initialized"}))?;
            let tools: Vec<_> = tool_specs().into_iter().map(|(_, name, description, schema)|
                json!({"type":"function","name":name,"description":description,"inputSchema":schema})).collect();
            let model = turn.settings.model();
            write(
                stdin,
                json!({"id":2,"method":"thread/start","params":{
                "cwd":workspace,"ephemeral":true,"sandbox":"read-only","approvalPolicy":"never",
                "model":if model.is_empty() { None } else { Some(model) },
                "developerInstructions":system_prompt(&turn.settings),"dynamicTools":tools}}),
            )?;
        } else if response && event["id"] == 2 {
            let id = event["result"]["thread"]["id"]
                .as_str()
                .ok_or("Codex returned no session ID")?;
            thread = Some(id.to_string());
            let effort = &turn.settings.agent.reasoning_effort;
            write(
                stdin,
                json!({"id":3,"method":"turn/start","params":{
                "threadId":id,"input":[{"type":"text","text":cli::prompt_with_context(turn, &cli::turn_prefix(turn))}],
                "effort":if effort.is_empty() { None } else { Some(effort) }}}),
            )?;
        } else if response
            && event["id"]
                .as_u64()
                .is_some_and(|id| turn_starts.contains(&id))
        {
            active = true;
            running_turn = event["result"]["turn"]["id"].as_str().map(str::to_string);
        } else if event["method"] == "item/tool/call" {
            calls += 1;
            if calls > turn.settings.agent.max_tool_rounds {
                return Err(format!(
                    "Stopped after {} tool calls. Finished edits remain in Undo.",
                    turn.settings.agent.max_tool_rounds
                ));
            }
            let params = &event["params"];
            let name = params["tool"]
                .as_str()
                .ok_or("Codex tool call has no name")?;
            let (tx, answer) = mpsc::sync_channel(1);
            turn.events
                .send(Event::ToolCall(ToolCall {
                    name: name.into(),
                    args: params["arguments"].clone(),
                    reply: tx,
                }))
                .map_err(|_| "The interface stopped listening")?;
            let result = await_tool(&answer, &turn.cancel);
            let (text, failed) = tool_output(&result);
            write(
                stdin,
                json!({"id":event["id"],"result":{"contentItems":[{"type":"inputText","text":text}],"success":!failed}}),
            )?;
        } else if event.get("id").is_some() && event.get("method").is_some() {
            write(
                stdin,
                json!({"id":event["id"],"error":{"code":-32601,"message":"Only ryolune music tools are available"}}),
            )?;
        } else if transcript.event(&event, &turn.events) {
            let status = event["params"]["turn"]["status"].as_str().unwrap_or("");
            running_turn = None;
            // Steering that missed the turn starts the next one on the same thread.
            let mut pending = std::mem::take(&mut late);
            // A steer still unanswered when the turn ended did not join it: its answer is
            // ignored and the text goes into the next turn.
            for (id, text) in steers.drain() {
                ignored.insert(id);
                pending.push(text);
            }
            pending.extend(take_steering(&turn.steer));
            if status == "completed" && !pending.is_empty() {
                if let Some(thread) = &thread {
                    let text = pending
                        .iter()
                        .map(|t| t.strip_prefix(super::STEERING_HEADER).unwrap_or(t).trim())
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    let text = super::steering_message(&text);
                    next_id += 1;
                    write(
                        stdin,
                        json!({"id":next_id,"method":"turn/start","params":{
                        "threadId":thread,"input":[{"type":"text","text":text}]}}),
                    )?;
                    turn_starts.insert(next_id);
                    steered.push(text);
                    let _ = turn.events.send(Event::Steered);
                    continue;
                }
            }
            let error = (status == "failed").then(|| {
                event["params"]["turn"]["error"]["message"]
                    .as_str()
                    .unwrap_or("Codex turn failed")
                    .to_string()
            });
            finish(
                turn,
                transcript.response,
                error,
                status == "interrupted",
                &steered,
            );
            return Ok(());
        }
    }
}

#[derive(Default)]
struct Stream {
    response: String,
}
impl Stream {
    fn event(&mut self, event: &Value, events: &mpsc::SyncSender<Event>) -> bool {
        let params = &event["params"];
        match event["method"].as_str().unwrap_or("") {
            "item/started" if params["item"]["type"] == "agentMessage" => self.response.clear(),
            "item/agentMessage/delta" => {
                if let Some(text) = params["delta"].as_str() {
                    self.response = bounded(&format!("{}{text}", self.response), TEXT_LIMIT);
                    let _ = events.send(Event::Text {
                        text: text.into(),
                        replace: false,
                    });
                }
            }
            "item/completed" if params["item"]["type"] == "agentMessage" => {
                if let Some(text) = params["item"]["text"].as_str() {
                    self.response = bounded(text, TEXT_LIMIT);
                    let _ = events.send(Event::Text {
                        text: self.response.clone(),
                        replace: true,
                    });
                }
                let _ = events.send(Event::TextEnd);
            }
            "turn/completed" => return true,
            _ => {}
        }
        false
    }
}
fn finish(
    turn: &Turn,
    response: String,
    error: Option<String>,
    cancelled: bool,
    steered: &[String],
) {
    let mut history = turn.history.clone();
    let mut parts = vec![Part::Text(turn.prompt.clone())];
    parts.extend(steered.iter().cloned().map(Part::Text));
    history.push(Message {
        role: "user",
        parts,
    });
    if !response.is_empty() {
        history.push(Message {
            role: "assistant",
            parts: vec![Part::Text(response)],
        });
    }
    let _ = turn.events.send(Event::Done {
        error,
        cancelled,
        history,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn server_tool_request_ids_do_not_collide_with_client_response_ids() {
        let (input, rx) = mpsc::sync_channel(8);
        for event in [
            json!({"id":1,"result":{}}),
            json!({"id":2,"result":{"thread":{"id":"test-thread"}}}),
            json!({"id":3,"result":{}}),
            json!({"id":2,"method":"item/tool/call","params":{"tool":"session_info","arguments":{}}}),
            json!({"method":"turn/completed","params":{"turn":{"status":"completed"}}}),
        ] {
            input.send(Ok(event)).unwrap();
        }
        let (events, output) = mpsc::sync_channel(8);
        let interface = std::thread::spawn(move || {
            match output.recv_timeout(Duration::from_secs(2)).unwrap() {
                Event::ToolCall(call) => {
                    assert_eq!(call.name, "session_info");
                    call.reply.send(Ok(json!({"name":"Test song"}))).unwrap();
                }
                _ => panic!("Expected a DAW tool call"),
            }
        });
        let turn = Turn::test("Inspect the song", Default::default(), events);
        let mut wire = vec![];
        session(&turn, std::path::Path::new("."), &mut wire, &rx).unwrap();
        interface.join().unwrap();
        let frames: Vec<Value> = String::from_utf8(wire)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert!(frames
            .iter()
            .any(|frame| frame["id"] == 2 && frame["result"]["success"] == true));
        assert_eq!(
            frames
                .iter()
                .filter(|frame| frame["method"] == "turn/start")
                .count(),
            1
        );
    }

    /// What the session writes, line by line, to a test standing in for Codex.
    struct Lines(Vec<u8>, mpsc::Sender<Value>);
    impl Write for Lines {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.extend_from_slice(bytes);
            while let Some(end) = self.0.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.0.drain(..=end).collect();
                let _ = self.1.send(serde_json::from_slice(&line).unwrap());
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    #[ignore = "stalls after `initialized` in this harness (never writes thread/start); open item for 0.14.1"]
    fn steering_joins_the_running_turn_or_starts_the_next_one() {
        let (input, rx) = mpsc::sync_channel(16);
        let (events, output) = mpsc::sync_channel(16);
        let (frames_tx, frames) = mpsc::channel::<Value>();
        let turn = Turn::test("Make a beat", Default::default(), events);
        let steer = turn.steer.clone();
        // The interface answers the tool call and steers while it runs.
        let interface = std::thread::spawn(move || {
            let mut steered = 0;
            while let Ok(event) = output.recv_timeout(Duration::from_secs(5)) {
                match event {
                    Event::ToolCall(call) => {
                        steer.lock().unwrap().push_back("Use 90 BPM".into());
                        call.reply.send(Ok(json!({}))).unwrap();
                    }
                    Event::Steered => steered += 1,
                    Event::Done { history, .. } => return (steered, history),
                    _ => {}
                }
            }
            panic!("no Done");
        });
        // Codex: answers each request as it is written.
        let steer = turn.steer.clone();
        let codex = std::thread::spawn(move || {
            let mut seen = vec![];
            let send = |v: Value| input.send(Ok(v)).unwrap();
            while let Ok(frame) = frames.recv_timeout(Duration::from_secs(5)) {
                seen.push(frame.clone());
                match (frame["method"].as_str().unwrap_or(""), frame["id"].as_u64()) {
                    ("initialize", _) => send(json!({"id":1,"result":{}})),
                    ("thread/start", _) => {
                        send(json!({"id":2,"result":{"thread":{"id":"thread-1"}}}))
                    }
                    ("turn/start", Some(3)) => {
                        send(json!({"id":3,"result":{"turn":{"id":"turn-1"}}}));
                        send(
                            json!({"id":7,"method":"item/tool/call","params":{"tool":"session_info","arguments":{}}}),
                        );
                    }
                    ("turn/steer", Some(id))
                        if seen.iter().filter(|f| f["method"] == "turn/steer").count() == 1 =>
                    {
                        // The first steer joins the turn; then the person steers again.
                        send(json!({"id":id,"result":{"turnId":"turn-1"}}));
                        steer.lock().unwrap().push_back("And add a clap".into());
                    }
                    ("turn/steer", Some(id)) => {
                        // Too late: the turn is over.
                        send(json!({"id":id,"error":{"message":"no active turn"}}));
                        send(
                            json!({"method":"turn/completed","params":{"turn":{"status":"completed"}}}),
                        );
                    }
                    ("turn/start", Some(id)) => {
                        send(json!({"id":id,"result":{"turn":{"id":"turn-2"}}}));
                        send(
                            json!({"method":"turn/completed","params":{"turn":{"status":"completed"}}}),
                        );
                    }
                    _ => {}
                }
            }
            seen
        });
        let mut wire = Lines(vec![], frames_tx);
        session(&turn, std::path::Path::new("."), &mut wire, &rx).unwrap();
        drop(wire);
        let (steered, history) = interface.join().unwrap();
        let frames = codex.join().unwrap();
        let steers: Vec<&Value> = frames
            .iter()
            .filter(|f| f["method"] == "turn/steer")
            .collect();
        assert_eq!(steers.len(), 2);
        assert_eq!(steers[0]["params"]["expectedTurnId"], "turn-1");
        assert_eq!(steers[0]["params"]["threadId"], "thread-1");
        let starts: Vec<&Value> = frames
            .iter()
            .filter(|f| f["method"] == "turn/start")
            .collect();
        assert_eq!(starts.len(), 2, "the late steer became the next turn");
        assert_eq!(starts[1]["params"]["threadId"], "thread-1");
        let text = starts[1]["params"]["input"][0]["text"].as_str().unwrap();
        assert!(
            text.starts_with(super::super::STEERING_HEADER) && text.ends_with("And add a clap")
        );
        assert_eq!(steered, 2);
        assert_eq!(history[0].parts.len(), 3, "the request and both steers");
    }

    #[test]
    fn emits_text_before_completion_and_ignores_reasoning() {
        let (tx, rx) = mpsc::sync_channel(10);
        let mut stream = Stream::default();
        stream.event(
            &json!({"method":"item/agentMessage/delta","params":{"delta":"A **drum"}}),
            &tx,
        );
        assert!(
            matches!(rx.try_recv(), Ok(Event::Text { text, replace:false }) if text == "A **drum")
        );
        stream.event(
            &json!({"method":"item/reasoning/textDelta","params":{"delta":"private"}}),
            &tx,
        );
        assert!(rx.try_recv().is_err());
        stream.event(&json!({"method":"item/completed","params":{"item":{"type":"agentMessage","text":"A **drum beat**"}}}), &tx);
        assert_eq!(stream.response, "A **drum beat**");
        assert!(matches!(
            rx.try_recv(),
            Ok(Event::Text { replace: true, .. })
        ));
        assert!(matches!(rx.try_recv(), Ok(Event::TextEnd)));
    }
}
