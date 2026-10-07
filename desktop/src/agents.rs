//! The agent panel: a conversation with the built-in agent, the tool calls it makes as it
//! works, and the revertable log of every command an agent ran against this window,
//! whether it came from the panel, an MCP client or the CLI. Layout follows the agent
//! panel in `design/ryolune Arrangement.dc.html`; settings live in Settings > Agent.

use crate::{
    agent::{self, Role, Runtime, Turn},
    app::Ryolune,
    control::Reply,
};
use ryolune_engine::{control, model::Session, store::Command, Result};
use serde_json::{json, Value};
use std::{collections::VecDeque, time::Instant};

const HISTORY_LIMIT: usize = 64;
const DETAIL_LIMIT: usize = 24_000;

#[derive(Default)]
pub(crate) struct AgentPanel {
    pub open: bool,
    /// 0 = conversation, 1 = changes.
    pub tab: usize,
    pub prompt: String,
    pub(crate) runtime: Runtime,
    /// The song's saved conversations and project memory (`crate::conversations`).
    pub(crate) conversations: crate::conversations::Conversations,
    history: VecDeque<Activity>,
    sequence: u64,
    last_request: Option<Instant>,
    /// The latest turn of the built-in agent: its checkpoint and what it changed, for Revert
    /// turn (lsuite's HARNESS.md part 6).
    pub(crate) turn: Option<TurnRecord>,
}

/// One built-in agent turn, revertable as a whole.
#[derive(Clone, Debug, Default)]
pub(crate) struct TurnRecord {
    /// The checkpoint taken before its first edit.
    pub before: String,
    /// The checkpoint of its result, taken when it is reverted (Redo turn returns to it).
    pub after: Option<String>,
    pub prompt: String,
    /// Its changes in plain words, from when it ended.
    pub changes: Vec<String>,
    pub running: bool,
    pub reverted: bool,
}

/// A tool result as the chat keeps it: a picture's base64 is left out (it is in the PNG file
/// at `image.path` and was sent to the model).
fn without_image_data(result: &Result<Value>) -> Result<Value> {
    let mut result = result.clone();
    if let Ok(value) = &mut result {
        if let Some(image) = value.get_mut("image").and_then(Value::as_object_mut) {
            image.remove("data");
        }
    }
    result
}

struct Activity {
    sequence: u64,
    title: String,
    /// The command as the CLI would spell it.
    detail: String,
    output: String,
    /// The track the command touched, for its swatch.
    #[allow(dead_code)]
    track: Option<String>,
    succeeded: bool,
    running: bool,
    depth_before: usize,
    depth_after: usize,
}

impl Activity {
    fn mutated(&self) -> bool {
        self.depth_after > self.depth_before
    }
    fn applied(&self, undo_depth: usize) -> bool {
        undo_depth >= self.depth_after
    }
}

/// One entry of the Changes log, as `agent.changes` reports it.
pub(crate) struct ChangeRow<'a> {
    pub sequence: u64,
    pub title: &'a str,
    /// The command as the CLI would spell it.
    pub detail: &'a str,
    pub output: &'a str,
    pub succeeded: bool,
    pub running: bool,
    /// It changed the document, so it can be undone and redone.
    pub mutated: bool,
    /// Its edit is in the document now (not undone).
    pub applied: bool,
}

struct Connection {
    port: Option<u16>,
    discovery: std::path::PathBuf,
}

impl AgentPanel {
    /// Cheap digest of everything `status_json`, `transcript_json` and `changes_json` report,
    /// so the window only serialises the conversation when it actually changed.
    pub(crate) fn fingerprint(&self, depth: usize) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let r = &self.runtime;
        (
            r.running(),
            &r.status,
            &r.last_error,
            r.tokens,
            r.turns,
            r.edits(),
        )
            .hash(&mut h);
        r.elapsed().as_secs().hash(&mut h);
        (r.first_id, r.transcript.len()).hash(&mut h);
        let c = &self.conversations;
        (&c.thread, c.memory.len(), c.storage_error()).hash(&mut h);
        for entry in r.transcript.iter().rev().take(100) {
            (&entry.text, entry.streaming).hash(&mut h);
            if let Some(tool) = &entry.tool {
                (&tool.name, tool.result.as_ref().map(|r| r.is_ok())).hash(&mut h);
            }
        }
        for e in &self.history {
            (
                e.sequence,
                e.succeeded,
                e.running,
                e.applied(depth),
                e.output.len(),
            )
                .hash(&mut h);
        }
        h.finish()
    }

    pub(crate) fn changes_json(&self, depth: usize) -> Value {
        json!(self
            .history
            .iter()
            .map(|entry| json!({
                "sequence": entry.sequence, "title": entry.title, "detail": entry.detail,
                "output": entry.output, "succeeded": entry.succeeded, "running": entry.running,
                "mutated": entry.mutated(), "applied": entry.applied(depth)
            }))
            .collect::<Vec<_>>())
    }

    /// The Changes log as the window draws it, newest first, borrowed rather than
    /// serialised: `changes_json` would copy every output on each frame.
    pub(crate) fn change_rows(&self, depth: usize) -> impl Iterator<Item = ChangeRow<'_>> {
        self.history.iter().map(move |entry| ChangeRow {
            sequence: entry.sequence,
            title: &entry.title,
            detail: &entry.detail,
            output: &entry.output,
            succeeded: entry.succeeded,
            running: entry.running,
            mutated: entry.mutated(),
            applied: entry.applied(depth),
        })
    }
    pub(crate) fn change_count(&self) -> usize {
        self.history.len()
    }
}

impl Ryolune {
    fn connection(&self) -> Connection {
        Connection {
            port: self.control.as_ref().map(control::wire::Server::port),
            discovery: self
                .control
                .as_ref()
                .map_or_else(control::wire::discovery_path, |server| {
                    server.path().to_path_buf()
                }),
        }
    }
    pub(crate) fn discovery_path(&self) -> std::path::PathBuf {
        self.connection().discovery
    }

    /// Start a turn with the configured provider from the prompt in the panel; used by the
    /// Send button and by `agent.send` from the registry.
    pub(crate) fn start_agent_task_now(&mut self) -> Result<()> {
        if self.agents.runtime.running() {
            return Err("An agent task is already running; stop it first.".into());
        }
        let prompt = self.agents.prompt.trim().to_string();
        if prompt.is_empty() {
            return Err("Type a request first.".into());
        }
        let provider = self.settings.agent.provider;
        let needs_bridge = matches!(
            provider,
            ryolune_engine::settings::Provider::Codex
                | ryolune_engine::settings::Provider::Claude
                | ryolune_engine::settings::Provider::Zenith
        );
        let connection = self.connection();
        if needs_bridge && !self.settings.control.enable_bridge {
            return Err("The local connection is disabled. Enable it in Settings > Control, or choose an API provider in Settings > Agent.".into());
        }
        if needs_bridge && connection.port.is_none() {
            self.bridge_wanted = true;
            return Err(
                "The local bridge is starting for the CLI provider; send again in a moment.".into(),
            );
        }
        let settings = self.settings.clone();
        // The overview without plugin parameters and with a few clips per track: enough to
        // start oriented; the agent asks session_overview for more.
        let summary = control::call(
            self,
            "session.overview",
            &json!({"maxClips": 2, "parameters": false}),
            false,
        )
        .unwrap_or(Value::Null);
        self.conversation_started(&prompt);
        let history = self.agents.runtime.history.clone();
        let mcp = agent::cli::companion("ryolune-mcp");
        // The song's project memory goes ahead of the request, for every provider.
        let memory = agent::memory_prefix(&self.agents.conversations.memory);
        let request = format!("{memory}{prompt}");
        let song = (
            self.store.session().id.clone(),
            self.store.session().name.clone(),
        );
        let remote = self
            .agents
            .runtime
            .remote
            .as_ref()
            .filter(|r| r.provider == provider.key())
            .map(|r| r.id.clone());
        if provider == ryolune_engine::settings::Provider::Zenith {
            // zenith hands its agents ryolune's MCP server from the lsuite entry.
            self.publish_discovery(true);
        }
        self.agents
            .runtime
            .start(prompt, memory, move |cancel, events, steer| Turn {
                prompt: request,
                history,
                settings,
                session_summary: summary,
                discovery: connection.discovery,
                mcp_executable: mcp,
                cancel,
                events,
                steer,
                song,
                remote,
            })?;
        // One undo for the whole turn: a checkpoint before its first edit, and the agent's
        // mark so its live context reports only what the person changes meanwhile.
        let label: String = format!(
            "Agent: {}",
            self.agents
                .runtime
                .transcript
                .last()
                .map_or("", |e| e.text.as_str())
        )
        .chars()
        .take(80)
        .collect();
        let checkpoint = self.store.checkpoint(&label);
        self.store.set_mark("agent");
        self.agents.turn = Some(TurnRecord {
            before: checkpoint.id,
            prompt: label,
            running: true,
            ..TurnRecord::default()
        });
        self.agents.prompt.clear();
        self.agents.tab = 0;
        self.save_conversation(false);
        Ok(())
    }

    /// The turn's changes, from its checkpoint to now.
    fn turn_changes(&self, before: &str) -> Vec<String> {
        self.store
            .find_checkpoint(Some(before))
            .map(|cp| {
                ryolune_engine::harness::changes::describe(&cp.session, self.store.session(), 40)
            })
            .unwrap_or_default()
    }

    /// `agent.revertTurn`: the whole last turn undone in one step, or redone.
    pub(crate) fn revert_turn(&mut self, redo: bool) -> Result<Value> {
        if self.agents.runtime.running() {
            return Err("Stop the agent before reverting its turn".into());
        }
        let mut turn = self
            .agents
            .turn
            .clone()
            .ok_or("The agent has not made a turn in this song yet")?;
        let target = if redo {
            if !turn.reverted {
                return Err("The last turn is not reverted".into());
            }
            turn.after.clone().ok_or("Nothing to redo")?
        } else {
            if turn.reverted {
                return Err(
                    "The last turn is already reverted: agent.revertTurn redo=true brings it back"
                        .into(),
                );
            }
            let after = self.store.checkpoint(&format!("After {}", turn.prompt));
            turn.after = Some(after.id);
            turn.before.clone()
        };
        let result = control::call(
            self,
            "harness.revert",
            &json!({ "checkpoint": target }),
            false,
        )?;
        turn.reverted = !redo;
        self.agents.turn = Some(turn);
        self.status = if redo {
            "Agent turn restored".into()
        } else {
            "Agent turn reverted".into()
        };
        Ok(
            json!({ "reverted": !redo, "undone": result["undone"], "turn": self.agents.turn_json() }),
        )
    }

    /// `agent.steer`: the text joins the running task at its next step.
    pub(crate) fn steer_agent(&mut self, text: &str) -> Result<()> {
        self.agents.runtime.steer(text)?;
        self.save_conversation(false);
        Ok(())
    }

    /// Execute the tool calls the worker asked for and keep the transcript moving.
    pub(crate) fn run_agent_tools(&mut self) {
        self.follow_song();
        let was_running = self.agents.runtime.running();
        let calls = self.agents.runtime.poll();
        if was_running && !self.agents.runtime.running() {
            if let Some(before) = self.agents.turn.as_ref().map(|t| t.before.clone()) {
                let changes = self.turn_changes(&before);
                if let Some(turn) = &mut self.agents.turn {
                    turn.changes = changes;
                    turn.running = false;
                }
            }
            // The run ended: keep the conversation as it ended.
            self.agents.conversations.thread.updated_at = ryolune_engine::lsuite::now_rfc3339();
            self.save_conversation(false);
            self.follow_song();
        }
        for call in calls {
            if self.song_switching() {
                let _ = call.reply.send(Err(
                    "Another song was opened; this request was stopped.".into()
                ));
                continue;
            }
            let method = call.name.replacen('_', ".", 1);
            let result = self.run_control_command(&method, &call.args, true, "Agent");
            let running = result
                .as_ref()
                .is_ok_and(|value| value["status"] == "running");
            if running {
                if let Err(Reply::Channel(sender)) = self.attach_reply(Reply::Channel(call.reply)) {
                    let _ = sender.send(result);
                }
            } else {
                let _ = call.reply.send(result);
            }
        }
    }

    pub(crate) fn revert_activity(&mut self, sequence: u64) {
        let Some(target) = self
            .agents
            .history
            .iter()
            .find(|e| e.sequence == sequence)
            .map(|e| e.depth_before)
        else {
            return;
        };
        while self.store.undo_depth() > target && self.store.can_undo() {
            self.dispatch(Command::Undo);
        }
    }

    pub(crate) fn redo_activity(&mut self, sequence: u64) {
        let Some(target) = self
            .agents
            .history
            .iter()
            .find(|e| e.sequence == sequence)
            .map(|e| e.depth_after)
        else {
            return;
        };
        while self.store.undo_depth() < target && self.store.can_redo() {
            self.dispatch(Command::Redo);
        }
    }

    pub(crate) fn record_agent_activity(
        &mut self,
        method: &str,
        params: &Value,
        source: &str,
        _revision_before: u64,
        depth_before: usize,
        result: &Result<Value>,
    ) {
        // Observing the agent must not change its activity or recursively capture
        // previous transcripts in the next transcript.
        if matches!(
            method,
            "agent.status"
                | "agent.transcript"
                | "agent.providers"
                | "agent.conversations"
                | "agent.memory"
                | "agent.steer"
        ) {
            return;
        }
        // The window's own requests are the person at the keyboard, not the agent, and a
        // query changes nothing: neither belongs in the Changes list. Commands the registry
        // does not know (live-only ones) are kept, as they can act on the window.
        if source == "Interface" {
            return;
        }
        // The agent saw the song as it is now: its next live context reports only what
        // changes after this (the person's edits while it thinks).
        self.store
            .set_mark(if source == "Agent" { "agent" } else { "mcp" });
        let recorded = control::spec(method).is_none_or(|spec| spec.mutates);
        let depth_after = self.store.undo_depth();
        self.agents.sequence += 1;
        self.agents.last_request = Some(Instant::now());
        let mutated = depth_after > depth_before;
        let running = result
            .as_ref()
            .is_ok_and(|value| value["status"] == "running");
        let sequence = self.agents.sequence;
        if self.agents.runtime.running() && !running && matches!(source, "Agent" | "MCP / agent") {
            if mutated {
                self.agents.runtime.note_edit();
            }
            if source == "Agent" {
                self.agents
                    .runtime
                    .attach_result(method, &without_image_data(result), sequence);
            } else if !matches!(method, "harness.context" | "harness.checkpoint") {
                // A CLI provider working through the bridge: show the call in the chat too,
                // but not ryolune-mcp's bookkeeping (its context per call and the checkpoint
                // before its first edit; the turn has its own), as the built-in agent's live
                // context is not shown either.
                self.agents.runtime.transcript.push(agent::Entry {
                    role: Role::Tool,
                    text: String::new(),
                    tool: Some(agent::ToolRecord {
                        name: method.into(),
                        args: params.clone(),
                        result: Some(without_image_data(result)),
                        sequence: Some(sequence),
                    }),
                    streaming: false,
                });
                self.agents.runtime.scroll_to_end = true;
            }
        }
        if !recorded {
            return;
        }
        let (title, track) = describe(method, params, self.store.session());
        self.agents.history.push_front(Activity {
            sequence,
            title,
            detail: cli_form(method, params),
            output: bounded(match result {
                Ok(value) => pretty(value),
                Err(message) => message.clone(),
            }),
            track,
            succeeded: result.is_ok(),
            running,
            depth_before: depth_before.min(depth_after),
            depth_after,
        });
        self.agents.history.truncate(HISTORY_LIMIT);
    }

    /// A new document has no agent history and nothing to revert; it shows its own saved
    /// conversations.
    pub(crate) fn reset_agent_history(&mut self) {
        self.agents.history.clear();
        self.agents.turn = None;
        self.follow_song();
    }
}

impl AgentPanel {
    pub(crate) fn set_prompt(&mut self, prompt: &str) {
        self.prompt = prompt.to_string();
    }
    pub(crate) fn status_json(&self, settings: &ryolune_engine::settings::Settings) -> Value {
        json!({
            "provider": settings.agent.provider.key(),
            "model": settings.model(),
            "reasoningEffort": settings.agent.reasoning_effort,
            "running": self.runtime.running(),
            "status": self.runtime.status,
            "reply": self.runtime.last_reply,
            "error": self.runtime.last_error,
            "turns": self.runtime.turns,
            "changes": self.history.len(),
            "edits": self.runtime.edits(),
            "elapsedSeconds": self.runtime.elapsed().as_secs(),
            "tokens": { "input": self.runtime.tokens.0, "output": self.runtime.tokens.1 },
            "conversation": {
                "id": self.conversations.thread.id,
                "title": self.conversations.thread.title,
            },
            "steeringPending": self.runtime.pending_steering(),
            "storageError": self.conversations.storage_error(),
            "turn": self.turn_json(),
        })
    }
    /// The last turn for `agent.status` and Revert turn.
    pub(crate) fn turn_json(&self) -> Value {
        match &self.turn {
            Some(t) => json!({
                "checkpoint": t.before, "request": t.prompt, "changes": t.changes,
                "running": t.running, "reverted": t.reverted,
            }),
            None => Value::Null,
        }
    }
    pub(crate) fn transcript_json(&self, limit: usize) -> Value {
        let first = self.runtime.first_id;
        let entries: Vec<Value> = self
            .runtime
            .transcript
            .iter()
            .enumerate()
            .rev()
            .take(limit)
            .map(|(index, entry)| {
                let mut value = json!({
                    "id": first + index as u64,
                    "role": match entry.role {
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                        Role::Notice => "notice",
                    },
                    "text": entry.text,
                    "streaming": entry.streaming,
                });
                if let Some(tool) = &entry.tool {
                    value["tool"] = json!({
                        "name": tool.name,
                        "args": tool.args,
                        "ok": tool.result.as_ref().is_none_or(|r| r.is_ok()),
                        "result": match &tool.result {
                            Some(Ok(v)) => v.clone(),
                            Some(Err(e)) => json!({ "error": e }),
                            None => Value::Null,
                        },
                    });
                }
                value
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        json!({ "entries": entries, "changes": self.history.len() })
    }
    /// Empty the open conversation (it keeps its id, its title starts over).
    pub(crate) fn clear_transcript(&mut self) {
        self.runtime.clear();
        self.conversations.thread.title = crate::conversations::NEW_TITLE.into();
    }
    pub(crate) fn runner_busy(&self) -> bool {
        self.runtime.running()
    }
    pub(crate) fn stop_runner(&mut self) {
        self.runtime.stop();
    }
    #[cfg(test)]
    pub(crate) fn mock_running_task(&mut self) -> impl FnOnce() + use<> {
        self.runtime.mock_task()
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// "setSendLevel" → "set send level".
fn camel_words(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_uppercase() {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn brief(value: &Value) -> String {
    match value {
        Value::String(s) => {
            let s: String = s.chars().take(28).collect();
            format!("“{s}”")
        }
        Value::Number(n) => n
            .as_f64()
            .map(|f| {
                format!("{f:.2}")
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            })
            .unwrap_or_else(|| n.to_string()),
        Value::Bool(b) => b.to_string(),
        Value::Array(items) => format!("{} items", items.len()),
        Value::Object(_) => "{…}".into(),
        Value::Null => "null".into(),
    }
}

/// A one-line human title for a log entry and the track it touched.
fn describe(method: &str, params: &Value, session: &Session) -> (String, Option<String>) {
    if method == "session.batch" {
        // One change, as it is one undo step: say how much it did and what kind of thing.
        let commands: Vec<&str> = params["commands"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| entry["command"].as_str())
            .collect();
        let mut kinds: Vec<&str> = vec![];
        for command in &commands {
            if !kinds.contains(command) {
                kinds.push(command);
            }
        }
        let shown = kinds.iter().take(3).copied().collect::<Vec<_>>().join(", ");
        let more = if kinds.len() > 3 { ", …" } else { "" };
        let plural = if commands.len() == 1 { "" } else { "s" };
        return (
            format!("Batch · {} command{plural} · {shown}{more}", commands.len()),
            None,
        );
    }
    let (object, action) = method.split_once('.').unwrap_or((method, ""));
    let clip = params["clipId"]
        .as_str()
        .and_then(|id| session.clips.iter().find(|c| c.id == id));
    let track_id = params["trackId"]
        .as_str()
        .map(str::to_string)
        .or_else(|| clip.map(|c| c.track_id.clone()));
    let track = track_id.and_then(|id| {
        session
            .tracks
            .iter()
            .position(|t| t.id == id)
            .map(|i| (i, &session.tracks[i]))
    });
    let track_id = track.map(|(_, t)| t.id.clone());
    let subject = match object {
        "clip" | "note" => clip.map(|c| c.name.clone()),
        _ => None,
    }
    .or_else(|| track.map(|(_, t)| t.name.clone()));
    let value = params.as_object().and_then(|map| {
        map.get("name")
            .or_else(|| {
                map.iter()
                    .find(|(k, _)| !k.ends_with("Id") && *k != "notes")
                    .map(|(_, v)| v)
            })
            .map(brief)
    });
    let mut title = format!("{} {}", capitalize(object), camel_words(action));
    // A rename already shows its result as the value; do not repeat the name.
    if let Some(s) = subject.filter(|s| value.as_deref() != Some(&format!("“{s}”"))) {
        title.push_str(&format!(" · {s}"));
    }
    if let Some(v) = value {
        title.push_str(&format!(" · {v}"));
    }
    (title, track_id)
}

/// The CLI spelling of a request, for the log and the clipboard.
fn cli_form(method: &str, params: &Value) -> String {
    let mut line = format!("ryolune-cli {method}");
    let mut complex = serde_json::Map::new();
    if let Some(map) = params.as_object() {
        for (k, v) in map {
            match v {
                Value::String(s) => {
                    if s.chars().any(char::is_whitespace) || s.is_empty() {
                        line.push_str(&format!(" --{k} \"{}\"", s.replace('"', "\\\"")));
                    } else {
                        line.push_str(&format!(" --{k} {s}"));
                    }
                }
                Value::Number(_) | Value::Bool(_) => line.push_str(&format!(" --{k} {v}")),
                _ => {
                    complex.insert(k.clone(), v.clone());
                }
            }
        }
    }
    if !complex.is_empty() {
        line.push_str(&format!(" --params '{}'", Value::Object(complex)));
    }
    line
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn bounded(text: String) -> String {
    if text.len() <= DETAIL_LIMIT {
        text
    } else {
        let mut end = DETAIL_LIMIT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!(
            "{}\n… Display truncated; query with the CLI for the full response.",
            &text[..end]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryolune_engine::store;

    #[test]
    fn stopping_stays_busy_until_worker_cleanup_has_completed() {
        let mut panel = AgentPanel::default();
        let complete = panel.mock_running_task();
        panel.stop_runner();
        panel.runtime.poll();
        assert!(panel.runner_busy());
        complete();
        panel.runtime.poll();
        assert!(!panel.runner_busy());
        assert!(panel.runtime.status.starts_with("Stopped"));
    }

    #[test]
    fn activity_records_success_errors_and_shared_history() {
        let mut app = Ryolune::from_session(store::demo(), None);
        let before = app.store.revision;
        let depth = app.store.undo_depth();
        let params = json!({"name":"Agent session"});
        let result = control::call(&mut app, "session.rename", &params, true);
        app.record_agent_activity("session.rename", &params, "test", before, depth, &result);
        assert!(app.agents.history[0].succeeded);
        assert!(app.agents.history[0].mutated());
        assert_eq!(app.store.session().name, "Agent session");
        control::call(&mut app, "history.undo", &json!({}), true).unwrap();
        assert_ne!(app.store.session().name, "Agent session");
        let before = app.store.revision;
        let depth = app.store.undo_depth();
        let result = control::call(
            &mut app,
            "track.remove",
            &json!({"trackId":"missing"}),
            true,
        );
        app.record_agent_activity("track.remove", &json!({}), "test", before, depth, &result);
        assert!(!app.agents.history[0].succeeded);
        assert!(!app.agents.history[0].mutated());
        assert_eq!(app.store.revision, before);
        for _ in 0..HISTORY_LIMIT + 1 {
            app.record_agent_activity(
                "transport.stop",
                &json!({}),
                "test",
                before,
                depth,
                &Ok(json!({})),
            );
        }
        assert_eq!(app.agents.history.len(), HISTORY_LIMIT);
    }

    #[test]
    fn tool_results_reach_the_chat_and_bridge_calls_are_shown_while_working() {
        let mut app = Ryolune::from_session(store::demo(), None);
        let complete = app.agents.mock_running_task();
        app.agents.runtime.transcript.push(agent::Entry {
            role: Role::Tool,
            text: String::new(),
            tool: Some(agent::ToolRecord {
                name: "session.rename".into(),
                args: json!({"name":"From the agent"}),
                result: None,
                sequence: None,
            }),
            streaming: true,
        });
        let params = json!({"name":"From the agent"});
        let result = app
            .run_control_command("session.rename", &params, true, "Agent")
            .unwrap();
        assert_eq!(result["name"], "From the agent");
        let tool = app.agents.runtime.transcript[0].tool.clone().unwrap();
        assert!(tool.result.as_ref().unwrap().is_ok());
        assert!(tool.sequence.is_some());
        assert_eq!(app.agents.runtime.edits(), 1);
        let activity_count = app.agents.history.len();
        for method in ["agent.status", "agent.transcript", "agent.providers"] {
            app.run_control_command(method, &json!({}), false, "CLI")
                .unwrap();
        }
        assert_eq!(app.agents.history.len(), activity_count);
        app.run_control_command("session.info", &json!({}), false, "CLI")
            .unwrap();
        assert_eq!(app.agents.runtime.transcript.len(), 1);
        assert_eq!(app.agents.runtime.edits(), 1);
        app.run_control_command("session.info", &json!({}), true, "MCP / agent")
            .unwrap();
        assert_eq!(app.agents.runtime.transcript.len(), 2);
        assert_eq!(
            app.agents.runtime.transcript[1].tool.as_ref().unwrap().name,
            "session.info"
        );
        // ryolune-mcp's bookkeeping stays out of the chat.
        app.run_control_command(
            "harness.checkpoint",
            &json!({"label": "Before the MCP agent's first edit"}),
            true,
            "MCP / agent",
        )
        .unwrap();
        app.run_control_command(
            "harness.context",
            &json!({"key": "mcp"}),
            true,
            "MCP / agent",
        )
        .unwrap();
        assert_eq!(app.agents.runtime.transcript.len(), 2);
        complete();
        app.agents.runtime.poll();
        assert!(!app.agents.runner_busy());
    }

    #[test]
    fn changes_list_skips_the_window_and_queries() {
        let mut app = Ryolune::from_session(store::demo(), None);
        let complete = app.agents.mock_running_task();
        let count = app.agents.history.len();
        // The interface's own reads and edits are the person, not the agent.
        for method in ["plugin.list", "session.info"] {
            app.run_control_command(method, &json!({}), false, "Interface")
                .unwrap();
        }
        app.run_control_command(
            "session.rename",
            &json!({"name":"Typed by hand"}),
            false,
            "Interface",
        )
        .unwrap();
        // A query from the CLI or the agent changes nothing either.
        app.run_control_command("session.info", &json!({}), false, "CLI")
            .unwrap();
        app.run_control_command("track.list", &json!({}), true, "MCP / agent")
            .unwrap();
        assert_eq!(app.agents.history.len(), count);
        // The query still reaches the agent's transcript.
        assert_eq!(app.agents.runtime.transcript.len(), 1);
        app.run_control_command(
            "session.rename",
            &json!({"name":"By the agent"}),
            false,
            "CLI",
        )
        .unwrap();
        assert_eq!(app.agents.history.len(), count + 1);
        assert!(app.agents.history[0].mutated());
        complete();
        app.agents.runtime.poll();
    }

    #[test]
    fn agent_permissions_are_enforced_on_agent_requests_only() {
        let mut app = Ryolune::from_session(store::demo(), None);
        app.settings.agent.permissions.transport = false;
        let denied = app
            .run_control_command("transport.stop", &json!({}), true, "Agent")
            .unwrap_err();
        assert!(denied.contains("not allowed for agents"));
        assert!(app
            .run_control_command("transport.stop", &json!({}), false, "CLI")
            .is_ok());
    }

    #[test]
    fn revert_and_redo_walk_the_undo_stack_to_the_entry() {
        let mut app = Ryolune::from_session(store::demo(), None);
        let original = app.store.session().name.clone();
        for name in ["First", "Second"] {
            let before = app.store.revision;
            let depth = app.store.undo_depth();
            let params = json!({ "name": name });
            let result = control::call(&mut app, "session.rename", &params, true);
            app.record_agent_activity("session.rename", &params, "test", before, depth, &result);
        }
        let first = app.agents.history[1].sequence;
        let second = app.agents.history[0].sequence;
        app.revert_activity(first);
        assert_eq!(app.store.session().name, original);
        assert!(!app.agents.history[1].applied(app.store.undo_depth()));
        assert!(!app.agents.history[0].applied(app.store.undo_depth()));
        app.redo_activity(second);
        assert_eq!(app.store.session().name, "Second");
        assert!(app.agents.history[0].applied(app.store.undo_depth()));
    }

    #[test]
    fn titles_and_cli_form_name_the_track_and_the_value() {
        let session = store::demo();
        let track = &session.tracks[1];
        let params = json!({ "trackId": track.id, "name": "Lead Vox" });
        let (title, touched) = describe("track.rename", &params, &session);
        assert_eq!(title, format!("Track rename · {} · “Lead Vox”", track.name));
        let (title, _) = describe(
            "track.rename",
            &json!({ "trackId": track.id, "name": track.name }),
            &session,
        );
        assert_eq!(title, format!("Track rename · “{}”", track.name));
        assert_eq!(touched.as_deref(), Some(track.id.as_str()));
        let cli = cli_form("track.rename", &params);
        assert!(cli.starts_with("ryolune-cli track.rename "));
        assert!(cli.contains(&format!("--trackId {}", track.id)));
        assert!(cli.contains("--name \"Lead Vox\""));
        let (title, _) = describe("session.info", &json!({}), &session);
        assert_eq!(title, "Session info");
        assert!(cli_form("clip.setNotes", &json!({"clipId":"c","notes":[]})).contains("--params"));
    }

    #[test]
    fn large_activity_is_bounded_on_a_utf8_boundary() {
        let output = bounded("🎹".repeat(DETAIL_LIMIT));
        assert!(output.len() < DETAIL_LIMIT + 100);
        assert!(output.contains("Display truncated"));
    }
}
