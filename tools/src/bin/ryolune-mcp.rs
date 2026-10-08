//! Model Context Protocol server for ryolune over stdio.
//!
//! Tools are generated from the shared command registry (`track.add` becomes `track_add`).
//! In live mode each call runs inside the open ryolune window, so an agent and a person edit the
//! same session with one undo history. Without the app, the server hosts a session itself.

use ryolune_engine::{control, harness};
use ryolune_tools::Backend;
use serde_json::{json, Value};
use std::{
    io::{BufRead, Read, Write},
    path::PathBuf,
};

const PROTOCOLS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];
const USAGE: &str = "ryolune-mcp — Model Context Protocol server for ryolune (stdio)

USAGE
  ryolune-mcp                 control the running ryolune app; if it is not running, host a
                             session in this process
  ryolune-mcp --live          require the running app
  ryolune-mcp --headless      host a new empty session in this process
  ryolune-mcp --file <path>   host that .ryolune file in this process and save after each change
  --no-context                do not add what the person changed (live) and the song's state
                             to tool results

Register with an MCP client, for example in Claude Code:
  claude mcp add ryolune -- /path/to/ryolune-mcp";

fn main() {
    let input_args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(result) = ryolune_tools::scan_child(&input_args) {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(2);
        }
        return;
    }
    let mut file: Option<PathBuf> = None;
    let (mut live, mut headless, mut with_context) = (false, false, true);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            "--version" | "-V" => {
                println!("ryolune-mcp {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--file" | "-f" => {
                let Some(path) = args.next().filter(|p| !p.starts_with("--")) else {
                    eprintln!("--file needs a path");
                    std::process::exit(2);
                };
                file = Some(PathBuf::from(path));
            }
            "--live" => live = true,
            "--headless" => headless = true,
            "--no-context" => with_context = false,
            _ => {
                eprintln!("Unknown option `{arg}`\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }
    if usize::from(file.is_some()) + usize::from(live) + usize::from(headless) > 1 {
        eprintln!("--file, --live and --headless are exclusive");
        std::process::exit(2);
    }
    let backend = match (file, live, headless) {
        (Some(path), _, _) => Backend::headless(Some(&path), true),
        (None, true, _) => Backend::live(),
        (None, false, true) => Backend::headless(None, false),
        (None, false, false) => Backend::live().or_else(|e| {
            eprintln!("ryolune-mcp: {e}\nryolune-mcp: hosting a session in this process instead");
            Backend::headless(None, false)
        }),
    };
    let backend = match backend {
        Ok(b) => b,
        Err(e) => {
            eprintln!("ryolune-mcp: {e}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "ryolune-mcp: {} mode{}",
        backend.mode(),
        backend
            .path()
            .map(|p| format!(" on {}", p.display()))
            .unwrap_or_default()
    );
    let mut server = Server {
        backend,
        with_context,
        checkpointed: false,
        unchecked: false,
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut input = stdin.lock();
    loop {
        let mut line = String::new();
        match input
            .by_ref()
            .take(control::wire::MAX_LINE as u64)
            .read_line(&mut line)
        {
            Ok(0) | Err(_) => break,
            Ok(_) if line.len() >= control::wire::MAX_LINE => {
                eprintln!("ryolune-mcp: input exceeds 64 MiB");
                break;
            }
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        let Some(reply) = server.handle_line(&line) else {
            continue;
        };
        let mut out = stdout.lock();
        let written = serde_json::to_writer(&mut out, &reply)
            .map_err(std::io::Error::other)
            .and_then(|()| out.write_all(b"\n"))
            .and_then(|()| out.flush());
        if written.is_err() {
            break;
        }
    }
}

struct Server {
    backend: Backend,
    /// Add the live context to tool results (part 3 of lsuite's HARNESS.md).
    with_context: bool,
    /// A checkpoint was taken before this connection's first edit.
    checkpointed: bool,
    /// The song changed since this connection last looked or measured (the finish routine).
    unchecked: bool,
}
impl Server {
    fn handle_line(&mut self, line: &str) -> Option<Value> {
        let frame: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(error(Value::Null, -32700, &format!("Parse error: {e}"))),
        };
        let Some(obj) = frame.as_object() else {
            return Some(error(
                Value::Null,
                -32600,
                "Batch requests are not supported",
            ));
        };
        let id = obj.get("id").cloned();
        if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || obj
                .get("method")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            || id
                .as_ref()
                .is_some_and(|id| !id.is_null() && !id.is_string() && !id.is_number())
        {
            return Some(error(Value::Null, -32600, "Invalid JSON-RPC 2.0 request"));
        }
        let method = obj.get("method").and_then(Value::as_str).unwrap_or("");
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        // Notifications (initialized, cancelled, progress) need no answer.
        let id = id?;
        Some(match self.dispatch(method, &params) {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => error(id, code, &message),
        })
    }
    fn dispatch(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let requested = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let version = if PROTOCOLS.contains(&requested) {
                    requested
                } else {
                    PROTOCOLS[PROTOCOLS.len() - 1]
                };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {
                        "tools": { "listChanged": false },
                        "resources": { "subscribe": false, "listChanged": false },
                        "prompts": { "listChanged": false },
                    },
                    "serverInfo": { "name": "ryolune", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": self.instructions(),
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => {
                if params.get("cursor").is_some_and(|c| !c.is_null()) {
                    return Err((-32602, "This server returned no pagination cursor".into()));
                }
                Ok(json!({ "tools": tools() }))
            }
            "tools/call" => {
                if !params.is_object() {
                    return Err((-32602, "tools/call needs an object of parameters".into()));
                }
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or((-32602, "tools/call needs `name`".to_string()))?;
                let command = name.replacen('_', ".", 1);
                if control::spec(&command).is_none() {
                    return Err((-32602, format!("Unknown tool `{name}`")));
                }
                let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
                Ok(self.call_tool(&command, &arguments))
            }
            "resources/list" => {
                let mut list: Vec<Value> = RESOURCES
                    .iter()
                    .map(|(uri, name, description, _)| json!({
                        "uri": uri, "name": name, "description": description, "mimeType": "application/json"
                    }))
                    .collect();
                list.push(json!({
                    "uri": "ryolune://brief", "name": "Agent brief", "mimeType": "text/markdown",
                    "description": "The expert brief every ryolune agent works from: the trade, the song's model, the quality bar, the finish routine and the skills.",
                }));
                list.push(json!({
                    "uri": "ryolune://skills", "name": "Skills", "mimeType": "application/json",
                    "description": "The playbooks for music jobs, with when to use each.",
                }));
                for skill in harness::skills() {
                    list.push(json!({
                        "uri": format!("ryolune://skills/{}", skill.name),
                        "name": skill.title, "description": skill.when, "mimeType": "text/markdown",
                    }));
                }
                Ok(json!({ "resources": list }))
            }
            "resources/read" => {
                let uri = params
                    .get("uri")
                    .and_then(Value::as_str)
                    .ok_or((-32602, "resources/read needs `uri`".to_string()))?;
                let markdown = |text: String| json!({ "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": text }] });
                if uri == "ryolune://brief" {
                    return Ok(markdown(harness::brief()));
                }
                if let Some(name) = uri.strip_prefix("ryolune://skills/") {
                    let skill = harness::skill(name)
                        .ok_or((-32002, format!("Unknown resource `{uri}`")))?;
                    return Ok(markdown(format!("# {}\n\n{}", skill.title, skill.body)));
                }
                let command = if uri == "ryolune://skills" {
                    "harness.skills"
                } else {
                    RESOURCES
                        .iter()
                        .find(|(u, _, _, _)| *u == uri)
                        .map(|(_, _, _, command)| *command)
                        .ok_or((-32002, format!("Unknown resource `{uri}`")))?
                };
                let value = self
                    .backend
                    .call(command, &Value::Null, false)
                    .map_err(|e| (-32000, e))?;
                Ok(json!({ "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&value).unwrap_or_default(),
                }] }))
            }
            "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
            "prompts/list" => {
                let mut list: Vec<Value> = PROMPTS
                    .iter()
                    .map(|p| json!({
                        "name": p.name, "description": p.description,
                        "arguments": p.arguments.iter().map(|(name, description, required)| json!({
                            "name": name, "description": description, "required": required
                        })).collect::<Vec<_>>()
                    }))
                    .collect();
                for skill in harness::skills() {
                    list.push(json!({
                        "name": skill.name,
                        "title": skill.title,
                        "description": format!("{}. {}", skill.title, skill.when),
                        "arguments": [{
                            "name": "request",
                            "description": "What the person wants, in their words (optional).",
                            "required": false,
                        }],
                    }));
                }
                Ok(json!({ "prompts": list }))
            }
            "prompts/get" => {
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or((-32602, "prompts/get needs `name`".to_string()))?;
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                if let Some(skill) = harness::skill(name).filter(|s| s.name == name) {
                    let request = arg(&arguments, "request", "");
                    let text = format!(
                        "{}Follow this ryolune playbook, then the finish routine of the brief (look, measure, fix, report).\n\n# {}\n\n{}",
                        if request.is_empty() { String::new() } else { format!("Request: {request}\n\n") },
                        skill.title,
                        skill.body
                    );
                    return Ok(json!({
                        "description": skill.when,
                        "messages": [{ "role": "user", "content": { "type": "text", "text": text } }]
                    }));
                }
                let prompt = PROMPTS
                    .iter()
                    .find(|p| p.name == name)
                    .ok_or((-32602, format!("Unknown prompt `{name}`")))?;
                let text = (prompt.render)(&arguments);
                Ok(json!({
                    "description": prompt.description,
                    "messages": [{ "role": "user", "content": { "type": "text", "text": text } }]
                }))
            }
            "completion/complete" => Ok(json!({ "completion": { "values": [] } })),
            _ => Err((-32601, format!("Method not found: {method}"))),
        }
    }
    fn instructions(&self) -> String {
        let mode = match &self.backend {
            Backend::Live(_) => "Live mode: every tool runs inside the open ryolune window. A person may be editing at the same time; you share one undo history, and transport_play is audible.".to_string(),
            Backend::Headless(h, _, _) => match &h.path {
                Some(p) => format!("Headless mode on {}: the file is saved after every change. transport_play is unavailable; use session_bounce to render audio.", p.display()),
                None => "Headless mode: this process hosts a session in memory. Call session_open or session_save with a path to work on files. transport_play is unavailable; use session_bounce to render audio.".into(),
            },
        };
        format!(
            "ryolune-mcp. {mode}\n\
             In live mode ui_screenshot also returns the window as an image. Tool names are the registry's commands with the first dot as an underscore (harness.look is harness_look). Results of edits end with the song's state, a reminder of the finish routine until you look or measure and, in live mode, what the person changed since your last call (also under harnessNotes in the structured result).\n\n{}",
            harness::brief()
        )
    }

    /// One tool call: the command, its image as image content, the live context.
    fn call_tool(&mut self, command: &str, arguments: &Value) -> Value {
        let mutates = control::spec(command).is_some_and(|s| s.mutates);
        // Before this connection's first edit: a checkpoint, so the whole job reverts in one
        // step (harness_revert) and harness_changes lists it.
        if mutates && !self.checkpointed && !command.starts_with("harness.") {
            self.checkpointed = true;
            let _ = self.backend.call(
                "harness.checkpoint",
                &json!({ "label": "Before the MCP agent's first edit" }),
                true,
            );
        }
        // What the person changed since this agent's last command (the window moves the
        // `mcp` mark after each of them).
        let person = if self.with_context
            && matches!(self.backend, Backend::Live(_))
            && !command.starts_with("harness.")
        {
            self.backend
                .call("harness.context", &json!({ "key": "mcp" }), true)
                .ok()
                .and_then(|c| c.get("changedSinceYourLastStep").cloned())
                .and_then(|v| v.as_array().cloned())
                .filter(|v| !v.is_empty())
        } else {
            None
        };
        let mut result = match self.backend.call(command, arguments, true) {
            Ok(result) => result,
            Err(message) => {
                return json!({
                    "content": [{ "type": "text", "text": message }],
                    "isError": true,
                })
            }
        };
        let mut images = vec![];
        if let Some(data) = result
            .get_mut("image")
            .and_then(|image| image.as_object_mut())
            .and_then(|image| image.remove("data"))
            .and_then(|d| d.as_str().map(str::to_string))
        {
            let mime = result["image"]["mimeType"]
                .as_str()
                .unwrap_or("image/png")
                .to_string();
            images.push(json!({ "type": "image", "data": data, "mimeType": mime }));
        }
        if command == "ui.screenshot" {
            if let Some(path) = result.get("path").and_then(Value::as_str) {
                if let Some(data) = png_file(std::path::Path::new(path)) {
                    images.push(json!({ "type": "image", "data": data, "mimeType": "image/png" }));
                }
            }
        }
        if matches!(command, "harness.look" | "harness.measure") {
            self.unchecked = false;
        } else if mutates && !command.starts_with("harness.") {
            self.unchecked = true;
        }
        let mut text = serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string());
        match self.backend.autosave() {
            Ok(Some(path)) => text.push_str(&format!("\n(saved {})", path.display())),
            Ok(None) => {}
            Err(e) => {
                return json!({
                    "content": [{ "type": "text", "text": format!("The command changed the in-memory session but autosave failed: {e}. Retry session_save before closing the server.") }],
                    "isError": true,
                })
            }
        }
        // What the harness adds to a result (HARNESS.md parts 3 and 5). Also kept in the
        // structured result: some clients (Claude Code) show that instead of the text.
        let mut notes = vec![];
        if let Some(changed) = person {
            let lines: Vec<String> = changed
                .iter()
                .filter_map(Value::as_str)
                .map(|c| format!("  - {c}"))
                .collect();
            notes.push(format!(
                "Before this call, the person changed (keep their changes):\n{}",
                lines.join("\n")
            ));
        }
        if self.with_context && mutates && !command.starts_with("harness.") {
            if let Ok(context) =
                self.backend
                    .call("harness.context", &json!({ "key": "mcp-state" }), true)
            {
                let line = |k: &str| context[k].as_str().unwrap_or("").to_string();
                notes.push(format!(
                    "Song now: {} · {} tracks · playhead {}",
                    line("song"),
                    context["tracks"].as_array().map_or(0, Vec::len)
                        + context["moreTracks"].as_u64().unwrap_or(0) as usize,
                    line("playhead"),
                ));
            }
        }
        if self.with_context && self.unchecked && mutates {
            notes.push(
                "Not checked yet: before you report, run the finish routine (harness_look over what you changed, harness_measure when levels matter).".into(),
            );
        }
        for note in &notes {
            text.push_str("\n\n");
            text.push_str(note);
        }
        let mut content = vec![json!({ "type": "text", "text": text })];
        content.extend(images);
        let mut reply = json!({ "content": content, "isError": false });
        if let Some(object) = result.as_object_mut() {
            if !notes.is_empty() {
                object.insert("harnessNotes".into(), json!(notes));
            }
            reply["structuredContent"] = result;
        }
        reply
    }
}

/// A PNG file's bytes as base64, when it is one and not too large to send.
fn png_file(path: &std::path::Path) -> Option<String> {
    use base64::Engine;
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > 8 * 1024 * 1024 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    bytes
        .starts_with(b"\x89PNG")
        .then(|| base64::engine::general_purpose::STANDARD.encode(&bytes))
}

/// Registry-backed resources: uri, name, description, command.
const RESOURCES: [(&str, &str, &str, &str); 9] = [
    (
        "ryolune://session/overview",
        "Song overview",
        "Everything about the song in one compact answer: tracks, plugins, clips, sections, mix problems and history.",
        "session.overview",
    ),
    (
        "ryolune://session",
        "Open session",
        "The complete session document as JSON.",
        "session.get",
    ),
    (
        "ryolune://session/info",
        "Session summary",
        "Name, file, transport, counts and history state.",
        "session.info",
    ),
    (
        "ryolune://session/inspect",
        "Arrangement and mixer",
        "Tracks, clip summaries, strips and automation without plugin state.",
        "session.inspect",
    ),
    (
        "ryolune://catalog",
        "Catalog",
        "Built-in instruments, effects and loops.",
        "session.catalog",
    ),
    (
        "ryolune://plugins",
        "Installed plugins",
        "The first page of scanned plugins.",
        "plugin.list",
    ),
    (
        "ryolune://presets",
        "Presets",
        "Factory and user plugin presets.",
        "preset.list",
    ),
    (
        "ryolune://settings",
        "Preferences",
        "ryolune settings with secrets masked.",
        "settings.get",
    ),
    (
        "ryolune://app",
        "Application",
        "Version, paths and mode.",
        "app.info",
    ),
];

struct Prompt {
    name: &'static str,
    description: &'static str,
    arguments: &'static [(&'static str, &'static str, bool)],
    render: fn(&Value) -> String,
}
fn arg<'a>(arguments: &'a Value, key: &str, default: &'a str) -> &'a str {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(default)
}
const PROMPTS: [Prompt; 3] = [
    Prompt {
        name: "compose",
        description:
            "Write a short arrangement from a style, length and key using the stock instruments.",
        arguments: &[
            (
                "style",
                "Genre or mood, for example lo-fi hip hop or cinematic pad",
                true,
            ),
            ("bars", "Length in bars (default 8)", false),
            ("key", "Song key, for example A minor", false),
        ],
        render: |a| {
            format!(
            "Compose {bars} bars of {style} in {key}. Start with session_overview and session_catalog. Set the tempo and key with transport_setTempo and transport_setKey, add one instrument track per part with track_add (choose stock instruments that fit), then write each part with clip_create and a full notes array in one call. Keep every note inside its clip, use velocities between 60 and 110 for dynamics, and set sends or inserts with strip_setSendLevel and strip_setPlugin for depth. Finish with session_overview and describe what you made in a few sentences.",
            bars = arg(a, "bars", "8"), style = arg(a, "style", "a warm, simple groove"), key = arg(a, "key", "the current key")
        )
        },
    },
    Prompt {
        name: "mix-review",
        description: "Inspect the mix and propose or apply balanced levels, panning and effects.",
        arguments: &[(
            "apply",
            "true to apply the changes, otherwise only propose them",
            false,
        )],
        render: |a| {
            format!(
            "Review this session's mix: call session_overview (it lists every track's plugins, sends, fader in dB and problems), then strip_parameters where a plugin needs a closer look. Consider level balance (track_setVolume, 0.75 is unity), panning width (track_setPan), the reverb and delay sends, and the master chain. {} Keep changes small and explain each one.",
            if arg(a, "apply", "false") == "true" { "Apply the improvements with the strip and track commands, one undo step each." } else { "Do not change anything yet; list the concrete commands you would run." }
        )
        },
    },
    Prompt {
        name: "see-the-window",
        description:
            "Take a screenshot of the running app and describe what the person is looking at.",
        arguments: &[],
        render: |_| {
            "Call ui_state, then ui_screenshot and look at the image file it returns. Describe the arrangement, the selected track or region, any open panels and anything that looks wrong.".into()
        },
    },
];

fn tools() -> Vec<Value> {
    control::COMMANDS
        .iter()
        .map(|spec| {
            let destructive = spec.mutates
                && [
                    "remove", "delete", "new", "open", "quit", "reset", "restore", "setNotes",
                    "clear",
                ]
                .iter()
                .any(|w| spec.name.contains(w));
            json!({
                "name": spec.name.replacen('.', "_", 1),
                "title": spec.name,
                "description": spec.doc,
                "inputSchema": control::schema(spec),
                "annotations": {
                    "readOnlyHint": !spec.mutates,
                    "destructiveHint": destructive,
                    "idempotentHint": !spec.mutates,
                    "openWorldHint": false,
                },
            })
        })
        .collect()
}
fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
