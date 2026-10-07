//! Live control: the window serves the shared command registry over the loopback socket, so
//! `ryolune-cli` and `ryolune-mcp` edit the same session a person is looking at. Requests are
//! answered on the interface thread between frames; nothing here touches the audio callback.

use crate::app::{Intent, Ryolune};
use ryolune_engine::{
    audio::{self, Library},
    control::{self, wire, Headless, Host},
    control_app, control_generate, document, midi, recovery, render,
    session_file::SessionFileLock,
    settings::Settings,
    store::{Command, Store},
    Result,
};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
    time::Instant,
};

const TOOLS: [&str; 3] = ["pointer", "pencil", "scissors"];
const BROWSER_TABS: [&str; 4] = ["instruments", "loops", "plugins", "files"];
/// ryolune's support page on lsuite.xyz, which redirects to the donation page (GitHub
/// Sponsors), so it can change without a new release.
const SUPPORT_URL: &str = "https://lsuite.xyz/ryolune/support";

/// Who is waiting for a deferred command: a bridge client or the built-in agent.
pub(crate) enum Reply {
    Wire(wire::Request),
    Channel(mpsc::SyncSender<Result<Value>>),
}
impl Reply {
    pub(crate) fn respond(self, result: Result<Value>) {
        match self {
            Reply::Wire(request) => request.respond(result),
            Reply::Channel(sender) => {
                let _ = sender.send(result);
            }
        }
    }
}

pub(crate) struct ControlJob {
    receiver: mpsc::Receiver<Result<(Value, Headless, Option<SessionFileLock>)>>,
    reply: Option<Reply>,
    method: String,
    params: Value,
    source: String,
    revision: u64,
    undo_depth: usize,
}

/// Interface work that completes on a later frame: a screenshot, an update check or install.
pub(crate) enum LiveWait {
    /// Work on another thread (the network, an offline render) that answers when it is done.
    Worker(mpsc::Receiver<Result<Value>>),
    Screenshot {
        path: PathBuf,
        requested: bool,
    },
    UpdateCheck,
    UpdateInstall,
    /// A sound arriving from a generation service, or a kept one decoded, to place in the
    /// song on the interface thread when it is ready.
    Generation(mpsc::Receiver<Result<GenerationDone>>),
}
/// What a generation worker hands back to the interface thread.
pub(crate) struct GenerationDone {
    generated: control_generate::Generated,
    buffer: Arc<ryolune_engine::audio::AudioBuffer>,
    place: bool,
    as_instrument: bool,
    track_id: Option<String>,
    start_bar: Option<f64>,
    root_note: u8,
    agent: bool,
}
pub(crate) struct LiveJob {
    pub wait: LiveWait,
    pub reply: Option<Reply>,
    method: String,
    params: Value,
    source: String,
    revision: u64,
    undo_depth: usize,
    started: Instant,
}

impl Ryolune {
    pub(crate) fn start_control(&mut self) {
        let wake = self.wake.clone();
        match wire::Server::start(move || wake()) {
            Ok(server) => self.control = Some(server),
            Err(e) => self.status = format!("Live control unavailable: {e}"),
        }
    }
    /// Answer every queued request once per frame. Requests run with no gesture in progress,
    /// so each command is its own undo step and a drag that spans them is split around them;
    /// an idle server leaves the gesture untouched so a drag stays one undo step.
    pub(crate) fn serve_control(&mut self, gesture: bool) {
        let pending = self
            .control
            .as_ref()
            .map(wire::Server::drain)
            .unwrap_or_default();
        if pending.is_empty() {
            return;
        }
        self.store.set_gesture(false);
        for request in pending {
            let result = self.run_control_command(
                &request.method,
                &request.params,
                request.agent,
                if request.agent { "MCP / agent" } else { "CLI" },
            );
            let running = result
                .as_ref()
                .is_ok_and(|value| value["status"] == "running");
            if running {
                if let Err(Reply::Wire(request)) = self.attach_reply(Reply::Wire(request)) {
                    request.respond(result);
                }
            } else {
                request.respond(result);
            }
        }
        self.store.set_gesture(gesture);
    }
    /// Hand a waiting party to the job the last command started. Returns the reply when
    /// nothing is pending so the caller can answer immediately.
    pub(crate) fn attach_reply(&mut self, reply: Reply) -> std::result::Result<(), Reply> {
        // Only the job this command started: a file job started earlier (a dropped MIDI file
        // imports with nobody waiting) must not take the answer of an unrelated command.
        if std::mem::take(&mut self.attach_control) {
            if let Some(job) = self.control_job.as_mut().filter(|job| job.reply.is_none()) {
                job.reply = Some(reply);
                return Ok(());
            }
        }
        if let Some(index) = self.attach_live.take() {
            if let Some(job) = self
                .live_jobs
                .get_mut(index)
                .filter(|job| job.reply.is_none())
            {
                job.reply = Some(reply);
                return Ok(());
            }
        }
        Err(reply)
    }
    pub(crate) fn run_control_command(
        &mut self,
        method: &str,
        params: &Value,
        agent: bool,
        source: &str,
    ) -> Result<Value> {
        // Shared lsuite names (`app.version`, `export.audio`…) run their ryolune command.
        let method = control::canonical(method);
        if method == "session.batch" && !self.batching {
            return self.run_batch(params, agent, source);
        }
        let before = self.store.revision;
        let depth_before = self.store.undo_depth();
        self.attach_live = None;
        self.attach_control = false;
        let result = (|| {
            control::validate_request(method, params)?;
            if method.starts_with("take.") && method != "take.list" && self.agents.runtime.running()
            {
                return Err("Stop the agent before switching or saving creative takes".into());
            }
            if agent {
                if matches!(method, "agent.configure" | "agent.openClient")
                    || (matches!(method, "settings.set" | "settings.reset")
                        && params["path"].as_str().is_none_or(|p| {
                            p.trim() == "agent"
                                || p.trim().starts_with("agent.")
                                || p.trim() == "control"
                                || p.trim().starts_with("control.")
                                || p.trim() == "generation"
                                || p.trim().starts_with("generation.")
                        }))
                {
                    return Err("Agent connections and permissions must be changed by the person in Settings".into());
                }
                if method == "app.reportProblem" {
                    return Err("Only a person can report a problem: app.reportProblem opens a GitHub issue for them to read and submit. app.diagnostics returns what a report needs.".into());
                }
                // Project memory goes ahead of every later request, like the standing
                // instructions in Settings, and deleting a conversation loses it for good:
                // both are the person's.
                if matches!(method, "agent.setMemory" | "agent.deleteConversation") {
                    return Err(format!(
                        "{method} is not available to agents: project memory and saved conversations are changed by the person, in the agent panel or with ryolune-cli."
                    ));
                }
                if let Some(denied) = control_app::denied_for_agent_request(
                    method,
                    params,
                    &self.settings.agent.permissions,
                ) {
                    return Err(denied);
                }
            }
            if matches!(
                method,
                "plugin.build"
                    | "plugin.publishLocal"
                    | "plugin.toolchain"
                    | "plugin.remove"
                    | "plugin.install"
                    | "plugin.new"
            ) {
                // A compiler or a scan: minutes of work that must not hold the window. The
                // plugin list is taken again (and rebuilt plugins reload) when it is done.
                let mut scratch = Headless::new();
                let (method_owned, params_owned) = (method.to_string(), params.clone());
                self.plugin_job = Some(method.to_string());
                self.status = match method {
                    "plugin.build" => "Building the plugin…".into(),
                    "plugin.publishLocal" => "Building and installing the plugin…".into(),
                    _ => self.status.clone(),
                };
                return Ok(self.start_worker(method, params, source, move || {
                    control::call(&mut scratch, &method_owned, &params_owned, agent)
                }));
            }
            if ryolune_engine::control_account::serves(method) {
                // lsuite AI: the account server (or the browser) answers on a worker.
                return self.start_account(method, params, source);
            }
            if matches!(
                method,
                "session.new" | "session.open" | "session.importFrom" | "app.openRecent"
            ) {
                self.can_replace_document()?;
            }
            if method == "app.openRecent" {
                // A recent song opens like any other: on a worker, with its file lock.
                let path = ryolune_engine::control_interop::recent_path(
                    &self.settings,
                    params["index"].as_i64(),
                    params["path"].as_str(),
                )?;
                return self.run_control_command(
                    "session.open",
                    &json!({ "path": path }),
                    agent,
                    source,
                );
            }
            if let Some(job) = self.control_job.as_ref().filter(|_| {
                control::COMMANDS
                    .iter()
                    .any(|s| s.name == method && s.mutates)
            }) {
                return Err(format!(
                    "{} is still running; retry {method} when it finishes.",
                    job.method
                ));
            }
            if matches!(
                method,
                "session.open"
                    | "session.save"
                    | "session.bounce"
                    | "session.importAudio"
                    | "session.importMidi"
                    | "session.exportMidi"
                    | "session.exportAudio"
                    | "session.exportStems"
                    | "session.scoreCut"
                    | "session.importFrom"
                    | "session.exportTo"
                    | "export.toKimchi"
                    | "plugin.scan"
            ) {
                self.available()?;
                // Preparing a graph is routine and may be discarded safely. File operations
                // cannot interrupt a take or another operation.
                self.guarded(Ryolune::stop)?;
                if matches!(
                    method,
                    "session.save"
                        | "session.bounce"
                        | "session.exportAudio"
                        | "session.exportStems"
                        | "session.exportTo"
                        | "export.toKimchi"
                ) {
                    self.guarded(Ryolune::capture_plugin_states)?;
                }
                let mut session = self.store.session().clone();
                session.transport.position_beats = self.position;
                session.view.pixels_per_bar = self.zoom;
                session.view.scroll_bars = self.scroll;
                let mut host = Headless {
                    store: Store::new(session)?,
                    library: self.library.clone(),
                    path: self.path.clone(),
                    position: self.position,
                    clipboard: None,
                    lane_width: self.lane_width,
                };
                let revision = self.store.revision;
                let (method_owned, mut params_owned) = (method.to_string(), params.clone());
                let ownership = if matches!(method, "session.open" | "session.save") {
                    let path = params
                        .get("path")
                        .and_then(Value::as_str)
                        .map(PathBuf::from)
                        .or_else(|| {
                            if method == "session.save" {
                                self.path.clone()
                            } else {
                                None
                            }
                        })
                        .ok_or("The session has no file yet: pass `path`.")?;
                    let ownership =
                        SessionFileLock::acquire_or_reuse(&path, self.session_file.as_ref())?;
                    if !params_owned.is_object() {
                        params_owned = json!({});
                    }
                    params_owned["path"] = json!(ownership.path());
                    Some(ownership)
                } else {
                    None
                };
                let (tx, rx) = mpsc::sync_channel(1);
                std::thread::spawn(move || {
                    let outcome = ryolune_engine::diagnostics::catch("file operation", || {
                        let result = control::call(&mut host, &method_owned, &params_owned, agent)?;
                        Ok((result, host, ownership))
                    })
                    .unwrap_or_else(|_| {
                        Err("Agent file operation failed; the open document is intact.".into())
                    });
                    let _ = tx.send(outcome);
                });
                self.attach_control = true;
                self.control_job = Some(ControlJob {
                    receiver: rx,
                    reply: None,
                    method: method.into(),
                    params: params.clone(),
                    source: source.into(),
                    revision,
                    undo_depth: self.store.undo_depth(),
                });
                self.status = format!("Running {method}…");
                return Ok(json!({"status":"running", "command":method}));
            }
            if matches!(method, "generate.audio" | "generate.place") {
                return self.start_generation(method, params, agent, source);
            }
            if matches!(method, "harness.look" | "harness.measure") {
                // An offline render of the song: seconds of work that must not hold the
                // interface. It renders a copy of the song with its plugins' current state.
                self.guarded(Ryolune::capture_plugin_states)?;
                let mut scratch = Headless {
                    store: Store::new(self.store.session().clone())?,
                    library: self.library.clone(),
                    path: self.path.clone(),
                    position: self.position,
                    clipboard: None,
                    lane_width: self.lane_width,
                };
                let (method_owned, params_owned) = (method.to_string(), params.clone());
                return Ok(self.start_worker(method, params, source, move || {
                    control::call(&mut scratch, &method_owned, &params_owned, agent)
                }));
            }
            if method == "rhythm.preview" {
                // The render runs on a scratch document that has no file: check the window's.
                if let Some(path) = params.get("path").and_then(Value::as_str) {
                    control::protect_session_file(self.path.as_deref(), Path::new(path))?;
                }
                // An offline render: seconds of work that must not hold the interface, and that
                // needs nothing from the open document but its meter and the tempo at the
                // playhead.
                let mut scratch = Headless::new();
                let mut transport = self.store.session().transport.clone();
                transport.tempo = self.store.session().tempo_map().bpm(self.position);
                scratch.store.dispatch(Command::SetTransport(transport))?;
                let params_owned = params.clone();
                return Ok(self.start_worker(method, params, source, move || {
                    control::call(&mut scratch, "rhythm.preview", &params_owned, false)
                }));
            }
            if source != "Interface" && !self.batching {
                self.store.set_gesture(false);
            }
            let mut result = control::call(self, method, params, agent)?;
            if matches!(method, "transport.record" | "transport.stop") {
                result["pending"] =
                    json!(self.record_pending.is_some() || self.record_finishing.is_some());
            }
            Ok(result)
        })();
        // The entries of a batch share one undo step, so they are one change: `run_batch`
        // records it. A change per entry would offer Reverts that each undo the whole batch.
        if !self.batching {
            self.record_agent_activity(method, params, source, before, depth_before, &result);
        }
        result
    }
    /// `session.batch` in the window: every entry passes the same permission and busy checks
    /// as a command sent on its own, and the whole list lands as one undo step.
    fn run_batch(&mut self, params: &Value, agent: bool, source: &str) -> Result<Value> {
        use ryolune_engine::control_edit;
        control::validate_request("session.batch", params)?;
        let entries = control_edit::batch_entries(params)?;
        let atomic = params["atomic"].as_bool().unwrap_or(true);
        let (before, depth_before) = (self.store.revision, self.store.undo_depth());
        self.store.set_gesture(true);
        self.batching = true;
        let mut results = Vec::with_capacity(entries.len());
        let mut failure = None;
        for (index, (command, params)) in entries.iter().enumerate() {
            match self.run_control_command(command, params, agent, source) {
                Ok(value) => results.push(value),
                Err(error) => {
                    failure = Some((index, command.as_str(), error));
                    break;
                }
            }
        }
        self.batching = false;
        let outcome = match failure {
            Some((index, command, error)) => {
                let rolled_back = atomic && self.store.cancel_gesture();
                Err(control_edit::batch_error(
                    index,
                    command,
                    &error,
                    rolled_back,
                ))
            }
            None => Ok(control_edit::batch_reply(results)),
        };
        self.store.set_gesture(false);
        self.record_agent_activity(
            "session.batch",
            params,
            source,
            before,
            depth_before,
            &outcome,
        );
        outcome
    }
    pub(crate) fn poll_control_job(&mut self) {
        let outcome = self
            .control_job
            .as_ref()
            .and_then(|job| match job.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Agent worker stopped before completing the operation.".into(),
                )),
                Err(mpsc::TryRecvError::Empty) => None,
            });
        let Some(outcome) = outcome else { return };
        let mut job = self.control_job.take().unwrap();
        let result = outcome.and_then(|(mut value, host, ownership)| {
            match job.method.as_str() {
                "session.open" => {
                    self.loaded(
                        host.store.session().clone(),
                        host.library,
                        host.path.unwrap(),
                        ownership,
                    );
                    value = control::call(self, "session.info", &json!({}), false)?;
                }
                "session.save" => {
                    self.saved(
                        host.path.unwrap(),
                        job.revision,
                        ownership.expect("save acquired ownership"),
                    );
                    value["dirty"] = json!(self.store.dirty());
                }
                "session.importAudio" | "session.importMidi" | "session.scoreCut" => {
                    let prior = self.store.snapshot();
                    let next = host.store.session();
                    let mut commands = vec![];
                    for track in &next.tracks {
                        if !prior.tracks.iter().any(|t| t.id == track.id) {
                            commands.push(Command::AddTrack(track.clone()));
                        }
                    }
                    for source in next.sources.values() {
                        if !prior.sources.contains_key(&source.id) {
                            commands.push(Command::PutSource(source.clone()));
                        }
                    }
                    for clip in &next.clips {
                        if !prior.clips.iter().any(|c| c.id == clip.id) {
                            commands.push(Command::PutClip(clip.clone()));
                        }
                    }
                    for (track, strip) in &next.strips {
                        if !prior.strips.contains_key(track) {
                            commands.push(Command::SetStrip {
                                track: track.clone(),
                                strip: strip.clone(),
                            });
                        }
                    }
                    if job.method == "session.scoreCut" {
                        for marker in &next.markers {
                            if !prior.markers.iter().any(|m| m.id == marker.id) {
                                commands.push(Command::PutMarker(marker.clone()));
                            }
                        }
                    }
                    if matches!(
                        job.method.as_str(),
                        "session.importMidi" | "session.scoreCut"
                    ) {
                        commands.push(Command::SetTransport(next.transport.clone()));
                        if next.tempo_changes != prior.tempo_changes {
                            commands.push(Command::SetTempoChanges(next.tempo_changes.clone()));
                        }
                    }
                    self.try_dispatch(Command::Batch(commands))?;
                    self.library = host.library;
                }
                "session.importFrom" => {
                    self.stop();
                    self.adopt_imported(host.store.session().clone(), host.library);
                    value["session"] = control::call(self, "session.info", &json!({}), false)?;
                }
                "plugin.scan" => {
                    value["reloaded"] = json!(self.adopt_catalog());
                }
                _ => {}
            }
            self.status = format!("{} complete", job.method);
            Ok(value)
        });
        self.export.completed(&job.method, &result);
        self.record_agent_activity(
            &job.method,
            &job.params,
            &job.source,
            job.revision,
            job.undo_depth,
            &result,
        );
        if let Err(error) = &result {
            self.error = Some(error.clone());
        }
        if let Some(reply) = job.reply.take() {
            reply.respond(result);
        }
    }
    /// Start a deferred interface job and mark it for the reply of the current command.
    fn start_live(&mut self, wait: LiveWait, method: &str, params: &Value, source: &str) -> Value {
        self.live_jobs.push(LiveJob {
            wait,
            reply: None,
            method: method.into(),
            params: params.clone(),
            source: source.into(),
            revision: self.store.revision,
            undo_depth: self.store.undo_depth(),
            started: Instant::now(),
        });
        self.attach_live = Some(self.live_jobs.len() - 1);
        json!({ "status": "running", "command": method })
    }
    /// Run `work` off the interface thread as a live job: the caller is told "running" and
    /// gets the result when it arrives, and the window never waits on the network.
    pub(crate) fn start_worker(
        &mut self,
        method: &str,
        params: &Value,
        source: &str,
        work: impl FnOnce() -> Result<Value> + Send + 'static,
    ) -> Value {
        let (tx, rx) = mpsc::sync_channel(1);
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let result = ryolune_engine::diagnostics::catch("background job", work)
                .unwrap_or_else(|_| Err("The background job failed".into()));
            let _ = tx.send(result);
            wake();
        });
        self.start_live(LiveWait::Worker(rx), method, params, source)
    }
    /// Generate a sound, or decode a kept one, on a worker; it is placed in the song when it
    /// arrives (`poll_workers`), in one undo step.
    fn start_generation(
        &mut self,
        method: &str,
        params: &Value,
        agent: bool,
        source: &str,
    ) -> Result<Value> {
        let (tx, rx) = mpsc::sync_channel(1);
        let wake = self.wake.clone();
        let send = move |result: Result<GenerationDone>| {
            let _ = tx.send(result);
            wake();
        };
        if method == "generate.audio" {
            let request =
                control_generate::request_for(&self.settings, self.store.session(), params)?;
            let settings = self.settings.clone();
            self.status = format!(
                "Generating “{}” with {}…",
                request.name,
                request.service.label()
            );
            std::thread::spawn(move || {
                let work = || -> Result<GenerationDone> {
                    let sound = crate::generate::fetch(&settings, &request)?;
                    let generated = control_generate::keep(
                        &sound.bytes,
                        &sound.extension,
                        &request,
                        &sound.model,
                    )?;
                    let as_instrument = request.kind == control_generate::GenKind::Instrument;
                    let buffer = control_generate::decoded(&generated, as_instrument)
                        .map_err(|e| format!("The service's audio could not be read: {e}"))?;
                    Ok(GenerationDone {
                        generated,
                        buffer: Arc::new(buffer),
                        place: request.place,
                        as_instrument,
                        track_id: request.track_id,
                        start_bar: request.start_bar,
                        root_note: request.root_note,
                        agent,
                    })
                };
                send(
                    ryolune_engine::diagnostics::catch("sound generation", work)
                        .unwrap_or_else(|_| Err("The generation stopped unexpectedly".into())),
                );
            });
        } else {
            let generated = control_generate::find(params["id"].as_str().unwrap_or(""))?;
            let as_instrument = match params["as"].as_str() {
                Some("audio") => false,
                Some("instrument") => true,
                Some(other) => return Err(format!("as is audio or instrument, not `{other}`")),
                None => generated.kind == control_generate::GenKind::Instrument,
            };
            let root_note = match params["rootNote"].as_i64() {
                Some(n) if (0..=127).contains(&n) => n as u8,
                Some(_) => return Err("rootNote must be between 0 and 127".into()),
                None => generated.root_note,
            };
            let track_id = params["trackId"].as_str().map(str::to_string);
            let start_bar = params["startBar"].as_f64();
            std::thread::spawn(move || {
                let buffer = control_generate::decoded(&generated, as_instrument);
                send(buffer.map(|buffer| GenerationDone {
                    generated,
                    buffer: Arc::new(buffer),
                    place: true,
                    as_instrument,
                    track_id,
                    start_bar,
                    root_note,
                    agent,
                }));
            });
        }
        Ok(self.start_live(LiveWait::Generation(rx), method, params, source))
    }
    /// Put a finished generation in the song, on the interface thread.
    fn finish_generation(&mut self, done: GenerationDone) -> Result<Value> {
        let mut out = json!({ "generated": done.generated });
        if done.place {
            if let Some(job) = &self.control_job {
                return Err(format!(
                    "“{}” is ready but {} is still running: place it with generate.place id={}",
                    done.generated.name, job.method, done.generated.id
                ));
            }
            out["placed"] = control_generate::place(
                self,
                done.buffer,
                &done.generated.name,
                done.as_instrument,
                done.track_id.as_deref(),
                done.start_bar,
                done.root_note,
                done.agent,
            )?;
        }
        self.status = format!("“{}” is ready", done.generated.name);
        Ok(out)
    }
    /// Answer the worker jobs that have finished.
    pub(crate) fn poll_workers(&mut self) {
        let mut index = 0;
        while index < self.live_jobs.len() {
            let outcome = match &self.live_jobs[index].wait {
                LiveWait::Worker(receiver) => match receiver.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err("The background job stopped before it finished".into()))
                    }
                    Err(mpsc::TryRecvError::Empty) => None,
                },
                LiveWait::Generation(receiver) => match receiver.try_recv() {
                    Ok(Ok(done)) => Some(self.finish_generation(done)),
                    Ok(Err(error)) => Some(Err(error)),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err("The generation stopped before it finished".into()))
                    }
                    Err(mpsc::TryRecvError::Empty) => None,
                },
                _ => None,
            };
            let Some(result) = outcome else {
                index += 1;
                continue;
            };
            let job = self.live_jobs.remove(index);
            if self.attach_live.is_some_and(|waiting| waiting >= index) {
                self.attach_live = None;
            }
            if job.method.starts_with("account.") {
                self.account_finished(&job.method, &result);
            }
            if job.method.starts_with("plugin.") {
                self.plugin_finished(&job.method, &result);
            }
            self.record_agent_activity(
                &job.method,
                &job.params,
                &job.source,
                job.revision,
                job.undo_depth,
                &result,
            );
            if let Some(reply) = job.reply {
                reply.respond(result);
            }
        }
    }
    /// A window capture a live job asked for and the window has not taken yet. Marks it
    /// taken, so each request is captured once.
    pub(crate) fn take_capture_request(&mut self) -> bool {
        let mut wanted = false;
        for job in &mut self.live_jobs {
            if let LiveWait::Screenshot { requested, .. } = &mut job.wait {
                if !*requested {
                    *requested = true;
                    wanted = true;
                }
            }
        }
        wanted
    }
    /// Time out jobs nobody can finish.
    pub(crate) fn poll_live_jobs(&mut self) {
        self.poll_workers();
        let expired: Vec<usize> = self
            .live_jobs
            .iter()
            .enumerate()
            .filter(|(_, job)| job.started.elapsed().as_secs() > 600)
            .map(|(i, _)| i)
            .collect();
        for index in expired.into_iter().rev() {
            let job = self.live_jobs.remove(index);
            let result = Err(format!("{} did not complete in time", job.method));
            self.record_agent_activity(
                &job.method,
                &job.params,
                &job.source,
                job.revision,
                job.undo_depth,
                &result,
            );
            if let Some(reply) = job.reply {
                reply.respond(result);
            }
        }
    }
    /// Complete every pending live job of one kind with the same result.
    pub(crate) fn finish_live(&mut self, pick: impl Fn(&LiveWait) -> bool, result: Result<Value>) {
        let (done, rest): (Vec<LiveJob>, Vec<LiveJob>) = std::mem::take(&mut self.live_jobs)
            .into_iter()
            .partition(|job| pick(&job.wait));
        self.live_jobs = rest;
        for job in done {
            self.record_agent_activity(
                &job.method,
                &job.params,
                &job.source,
                job.revision,
                job.undo_depth,
                &result,
            );
            if let Some(reply) = job.reply {
                reply.respond(result.clone());
            }
        }
    }
    /// A finished window capture: write it where the live job asked.
    pub(crate) fn deliver_screenshot(&mut self, image: &image::RgbaImage) -> bool {
        let Some(index) = self
            .live_jobs
            .iter()
            .position(|job| matches!(job.wait, LiveWait::Screenshot { .. }))
        else {
            return false;
        };
        let path = match &self.live_jobs[index].wait {
            LiveWait::Screenshot { path, .. } => path.clone(),
            _ => unreachable!(),
        };
        let result = (|| {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            image.save(&path).map_err(|e| e.to_string())?;
            Ok(json!({
                "path": path,
                "width": image.width(),
                "height": image.height(),
                "bytes": std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            }))
        })();
        let target = path.clone();
        self.finish_live(
            move |wait| matches!(wait, LiveWait::Screenshot { path, .. } if *path == target),
            result,
        );
        true
    }
    fn audio_status(&self) -> Value {
        let (peaks, cpu) = self.device.as_ref().map_or(([0.0; 4], 0.0), |d| {
            (d.telemetry.peaks(), d.telemetry.load())
        });
        json!({
            "device": self.device.as_ref().map(|d| d.device_name.clone()),
            "sampleRate": self.device.as_ref().map(|d| d.sample_rate),
            "bufferFrames": self.device.as_ref().map(|d| d.telemetry.output_frames.load(std::sync::atomic::Ordering::Relaxed)),
            "cpuLoad": cpu,
            "masterPeak": [peaks[0], peaks[1]],
            "selectedTrackPeak": [peaks[2], peaks[3]],
            "midiInput": self.midi.as_ref().map(|m| m.port_name.clone()),
            "outputDevice": self.output_device,
            "inputDevice": self.input_device,
            "playing": self.playing,
            "recording": self.midi_recording || self.recorder.is_some(),
            "recordEnabled": self.record_enabled,
            "musicalTyping": self.musical_typing,
            "monitoring": self.monitoring_status(),
        })
    }
    fn monitoring_status(&self) -> Value {
        use ryolune_engine::device::Monitoring;
        use std::sync::atomic::Ordering::Relaxed;
        let (state, input, rate, reason) = match &self.monitoring {
            Monitoring::Off => ("off", None, None, None),
            Monitoring::On { input_name, input_rate } => {
                ("on", Some(input_name.clone()), Some(*input_rate), None)
            }
            Monitoring::FeedbackRisk { input_name } => (
                "blocked",
                Some(input_name.clone()),
                None,
                Some("The built-in microphone would feed back through the built-in speakers. Use headphones, or allow it with audio.allowSpeakerMonitoring.".to_string()),
            ),
            Monitoring::Failed(error) => ("failed", None, None, Some(error.clone())),
        };
        let mut value = json!({
            "state": state, "inputDevice": input, "inputRate": rate, "reason": reason,
            "speakersAllowed": self.monitor_speakers_ok,
        });
        if let Some(d) = self.device.as_ref().filter(|_| state == "on") {
            let t = &d.telemetry;
            value["inputFrames"] = json!(t.input_frames.load(Relaxed));
            value["outputFrames"] = json!(t.output_frames.load(Relaxed));
            value["ringFrames"] = json!(t.monitor_fill.load(Relaxed));
            value["latencyMs"] = json!(t
                .monitor_latency_ms(d.sample_rate)
                .map(|ms| (ms * 10.0).round() / 10.0));
            value["drops"] = json!(t.monitor_drops.load(Relaxed));
            value["underruns"] = json!(t.monitor_underruns.load(Relaxed));
        }
        value
    }
    fn ui_status(&self) -> Value {
        let session = self.store.session();
        json!({
            "frontendReady": self.frontend_ready,
            "agentPanel": self.agents.open,
            "automation": self.show_automation,
            "settings": self.settings_ui.open,
            "settingsSection": crate::settings::SECTION_KEYS[self.settings_ui.section.min(crate::settings::SECTION_KEYS.len() - 1)],
            "help": self.show_help,
            "whatsNew": self.whats_new.is_some(),
            "mixer": self.show_mixer,
            "controllers": self.show_controllers,
            "tempo": self.show_tempo,
            "palette": self.show_palette,
            "plugins": self.show_plugins.map(|p| ["stock", "installed", "formats", "build"][p.min(3)]),
            "pluginJob": self.plugin_job,
            "tool": TOOLS[self.tool.min(2)],
            "musicalTyping": self.musical_typing,
            "pluginWindows": self.plugins.windows.keys().cloned().collect::<Vec<_>>(),
            "status": self.status,
            "error": self.error,
            "playing": self.playing,
            "recordEnabled": self.record_enabled,
            "pixelsPerBar": self.zoom,
            "scrollBar": self.scroll,
            "browserTab": session.view.browser_tab,
            "browserSelection": session.view.browser_selection,
            "selectedTrackId": session.view.selected_track_id,
            "selectedClipId": session.view.selected_clip_id,
            "busy": self.job.is_some() || self.control_job.is_some(),
            "prompt": self.intent.map(|intent| match intent {
                crate::app::Intent::New => "new",
                crate::app::Intent::Open => "open",
                crate::app::Intent::Recover => "recover",
                crate::app::Intent::Demo => "demo",
                crate::app::Intent::Quit => "quit",
                crate::app::Intent::Relaunch => "relaunch",
                crate::app::Intent::OpenRecent => "openRecent",
                crate::app::Intent::ImportFrom => "importFrom",
            }),
            "recoveredTake": self.unplaced_recording.is_some(),
            "monitorBlocked": matches!(
                self.monitoring,
                ryolune_engine::device::Monitoring::FeedbackRisk { .. }
            ),
            "heldNotes": self.typing_down,
        })
    }
    /// `ui.state`: what the window shows, named so an agent can act on it without a screenshot.
    fn ui_state(&self) -> Value {
        let session = self.store.session();
        let view = &session.view;
        let track_name = |id: &str| {
            if ryolune_engine::model::is_bus(id) {
                Some(ryolune_engine::model::bus_name(id).to_string())
            } else {
                session
                    .tracks
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| t.name.clone())
            }
        };
        let clip = |id: &Option<String>| {
            id.as_ref().and_then(|id| {
                session.clips.iter().find(|c| &c.id == id).map(|c| {
                    json!({"id": c.id, "name": c.name, "trackId": c.track_id,
                        "track": track_name(&c.track_id), "bars": [c.start_bar, c.start_bar + c.length_bars]})
                })
            })
        };
        let mut windows: Vec<Value> = self
            .plugins
            .windows
            .iter()
            .map(|(key, window)| {
                let found = crate::plugins::find_insert(session, key);
                let slot = found.as_ref().and_then(|(strip, _, synth)| {
                    (!synth)
                        .then(|| {
                            session
                                .strips
                                .get(strip)
                                .and_then(|s| s.inserts.iter().position(|i| &i.id == key))
                        })
                        .flatten()
                });
                json!({
                    "id": key,
                    "trackId": found.as_ref().map(|f| &f.0),
                    "track": found.as_ref().and_then(|f| track_name(&f.0)),
                    "slot": slot,
                    "plugin": found.as_ref().map(|f| &f.1.name),
                    "nativeEditor": window.native.is_some(),
                })
            })
            .collect();
        windows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        let visible_bars = self.lane_width / self.zoom.max(1.0) as f64;
        let interface = &self.settings.interface;
        json!({
            "panels": {
                "agent": self.agents.open,
                "mixer": self.show_mixer,
                "automation": self.show_automation,
                "controllers": self.show_controllers,
                "tempo": self.show_tempo,
                "palette": self.show_palette,
                "help": self.show_help,
                "settings": if self.settings_ui.open {
                    json!(crate::settings::SECTION_KEYS[self.settings_ui.section.min(crate::settings::SECTION_KEYS.len() - 1)])
                } else {
                    json!(false)
                },
                "export": self.export.is_open(),
                "recovery": self.recovery.is_open(),
                "whatsNew": self.whats_new.is_some(),
            },
            "prompt": self.intent.map(|intent| match intent {
                crate::app::Intent::New => "new",
                crate::app::Intent::Open => "open",
                crate::app::Intent::Recover => "recover",
                crate::app::Intent::Demo => "demo",
                crate::app::Intent::Quit => "quit",
                crate::app::Intent::Relaunch => "relaunch",
                crate::app::Intent::OpenRecent => "openRecent",
                crate::app::Intent::ImportFrom => "importFrom",
            }),
            "pluginWindows": windows,
            "editor": {
                "clip": clip(&view.editor_clip_id),
                "mode": view.editor_mode,
                "lowPitch": view.editor_low_pitch,
                "shownInstead": if self.show_mixer { Some("mixer") } else { None },
            },
            "arrangement": {
                "pixelsPerBar": self.zoom,
                "firstBar": self.scroll,
                "visibleBars": [self.scroll, self.scroll + visible_bars],
                "laneWidth": self.lane_width,
                "tool": TOOLS[self.tool.min(2)],
                "followPlayhead": view.follow_playhead,
            },
            "browser": { "tab": view.browser_tab, "selection": view.browser_selection },
            "selection": {
                "track": view.selected_track_id.as_ref().map(|id| json!({"id": id, "name": track_name(id)})),
                "clip": clip(&view.selected_clip_id),
                "noteId": view.selected_note_id,
            },
            "theme": { "appearance": interface.appearance, "mode": interface.mode, "scale": interface.scale },
            "musicalTyping": self.musical_typing,
            "heldNotes": self.typing_down,
            "status": self.status,
            "error": self.error,
            "busy": self.job.is_some() || self.control_job.is_some(),
            "recoveredTake": self.unplaced_recording.is_some(),
            "frontendReady": self.frontend_ready,
        })
    }
    fn available(&self) -> Result<()> {
        if (self.job.is_some() && !self.preparing) || self.control_job.is_some() {
            return Err("ryolune is busy with a file operation; retry in a moment".into());
        }
        if self.midi_recording
            || self.recorder.is_some()
            || self.record_pending.is_some()
            || self.record_finishing.is_some()
        {
            return Err("A recording is in progress; stop the transport first".into());
        }
        Ok(())
    }
    fn can_replace_document(&self) -> Result<()> {
        if self.unplaced_recording.is_some() {
            return Err("Save the recovered recording before replacing this session".into());
        }
        Ok(())
    }
    /// Run an interface action and report the error it would have shown, leaving any message
    /// the person was already reading in place.
    pub(crate) fn guarded(&mut self, action: impl FnOnce(&mut Self)) -> Result<()> {
        let shown = self.error.take();
        action(self);
        let result = self.error.take().map_or(Ok(()), Err);
        self.error = shown;
        result
    }
}

impl Host for Ryolune {
    fn store(&self) -> &Store {
        &self.store
    }
    fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }
    fn library(&self) -> &Library {
        &self.library
    }
    fn library_mut(&mut self) -> &mut Library {
        &mut self.library
    }
    fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
    fn mode(&self) -> &'static str {
        "live"
    }
    fn playing(&self) -> bool {
        self.playing
    }
    fn recording(&self) -> bool {
        self.midi_recording || self.recorder.is_some()
    }
    fn record(&mut self) -> Result<()> {
        self.available()?;
        if !self.store.session().tracks.iter().any(|track| track.armed) {
            return Err("Arm an audio or MIDI track before recording.".into());
        }
        if self.store.session().transport.cycle {
            return Err("Disable cycle before recording a linear take.".into());
        }
        if self.sync_needed || self.synced_revision != Some(self.store.revision) {
            return Err("Audio is still updating; retry in a moment.".into());
        }
        self.record_enabled = true;
        if self.playing {
            self.guarded(Ryolune::start_recording)
        } else {
            self.guarded(Ryolune::play)
        }
    }
    fn loaded_editor(
        &mut self,
        insert: &ryolune_engine::model::Insert,
    ) -> Option<&mut dyn ryolune_engine::plugin::Editor> {
        self.plugins
            .loaded
            .get_mut(&insert.id)
            .filter(|entry| {
                !entry.retiring
                    && entry.plugin_id == insert.plugin_id()
                    && entry.blob == insert.blob
            })
            .map(|entry| entry.editor.as_mut() as &mut dyn ryolune_engine::plugin::Editor)
    }
    fn plugin_failures(&self) -> Vec<(String, String)> {
        self.plugins
            .failed
            .iter()
            .map(|(key, why)| (key.clone(), why.clone()))
            .collect()
    }
    fn position(&self) -> f64 {
        self.position
    }
    fn dispatch(&mut self, command: Command) -> Result<bool> {
        self.try_dispatch(command)
    }
    fn play(&mut self) -> Result<()> {
        if self.playing {
            return Ok(());
        }
        if self.sync_needed || self.synced_revision != Some(self.store.revision) {
            return Err("Audio is still updating after the last edit; retry in a moment".into());
        }
        self.guarded(Ryolune::play)?;
        if self.playing {
            Ok(())
        } else {
            Err("Playback did not start".into())
        }
    }
    fn stop(&mut self) -> Result<()> {
        self.guarded(Ryolune::stop)
    }
    fn locate(&mut self, beats: f64) -> Result<()> {
        self.guarded(|app| Ryolune::locate(app, beats))
    }
    fn new_session(&mut self, demo: bool) -> Result<()> {
        self.available()?;
        self.can_replace_document()?;
        Ryolune::stop(self);
        self.guarded(|app| app.execute(if demo { Intent::Demo } else { Intent::New }))
    }
    fn open(&mut self, path: &Path) -> Result<()> {
        self.available()?;
        self.can_replace_document()?;
        let ownership = SessionFileLock::acquire_or_reuse(path, self.session_file.as_ref())?;
        let path = ownership.path().to_path_buf();
        Ryolune::stop(self);
        let (session, library) = document::load(&path)?;
        self.guarded(|app| app.loaded(session, library, path, Some(ownership)))
    }
    fn adopt_session(
        &mut self,
        session: ryolune_engine::model::Session,
        library: Library,
    ) -> Result<()> {
        self.available()?;
        self.can_replace_document()?;
        Ryolune::stop(self);
        self.guarded(|app| app.adopt_imported(session, library))
    }
    fn save(&mut self, path: Option<&Path>) -> Result<PathBuf> {
        self.available()?;
        Ryolune::stop(self);
        self.guarded(Ryolune::capture_plugin_states)?;
        let mut path = path
            .map(Path::to_path_buf)
            .or_else(|| self.path.clone())
            .ok_or("The session has no file yet: pass `path`.")?;
        if path.extension().is_none() {
            path.set_extension(ryolune_engine::document::EXTENSION);
        }
        let ownership = SessionFileLock::acquire_or_reuse(&path, self.session_file.as_ref())?;
        let path = ownership.path().to_path_buf();
        let mut session = (*self.store.snapshot()).clone();
        session.transport.position_beats = self.position;
        session.view.pixels_per_bar = self.zoom;
        session.view.scroll_bars = self.scroll;
        let revision = self.store.revision;
        document::save(&session, &self.library, &path)?;
        self.saved(path.clone(), revision, ownership);
        Ok(path)
    }
    fn bounce(&mut self, path: &Path) -> Result<()> {
        self.available()?;
        Ryolune::stop(self);
        self.guarded(Ryolune::capture_plugin_states)?;
        let session = self.store.snapshot();
        let mut library = self.library.clone();
        audio::prepare_sources(&session, &mut library)?;
        render::bounce(&session, &library, path, 48000)?;
        self.status = "WAV export complete".into();
        Ok(())
    }
    fn view_state(&self) -> (f32, f64) {
        (self.zoom, self.scroll)
    }
    fn view_changed(&mut self) {
        let view = &self.store.session().view;
        self.zoom = view.pixels_per_bar.clamp(12.0, 480.0);
        self.scroll = view.scroll_bars.max(0.0);
        self.browser_tab = BROWSER_TABS
            .iter()
            .position(|tab| *tab == view.browser_tab)
            .unwrap_or(0);
    }
    fn lane_width(&self) -> f64 {
        self.lane_width
    }
    fn set_lane_width(&mut self, pixels: f64) {
        self.lane_width = pixels;
    }
    fn clipboard(&self) -> Option<&ryolune_engine::model::Clip> {
        self.clipboard.as_ref()
    }
    fn set_clipboard(&mut self, clip: Option<ryolune_engine::model::Clip>) {
        self.clipboard = clip;
    }
    fn capture_states(&mut self) -> Result<()> {
        self.guarded(Ryolune::capture_plugin_states)
    }
    fn settings(&self) -> Settings {
        self.settings.clone()
    }
    fn update_settings(&mut self, settings: Settings) -> Result<()> {
        self.apply_settings(settings)
    }
    fn live(&mut self, action: &str, params: &Value) -> Result<Value> {
        let source = "live";
        match action {
            "audio.status" => Ok(self.audio_status()),
            "audio.allowSpeakerMonitoring" => {
                self.monitor_speakers_ok = params["allow"].as_bool().unwrap_or(false);
                self.poll_input();
                Ok(self.audio_status())
            }
            "audio.setOutput" | "audio.setInput" => {
                let name = params["name"].as_str().map(str::to_string);
                if let Some(name) = &name {
                    let known = if action == "audio.setOutput" {
                        ryolune_engine::device::output_devices()
                    } else {
                        ryolune_engine::device::input_devices()
                    };
                    if !known.contains(name) {
                        return Err(format!("Unknown device `{name}`. See audio.devices."));
                    }
                }
                let mut settings = self.settings.clone();
                if action == "audio.setOutput" {
                    settings.audio.output_device = name;
                } else {
                    settings.audio.input_device = name;
                }
                self.apply_settings(settings)?;
                Ok(self.audio_status())
            }
            "audio.setMidiInput" => {
                let port = params["port"].as_str().map(str::to_string);
                if let Some(port) = &port {
                    if !midi::ports().contains(port) {
                        return Err(format!("Unknown MIDI port `{port}`. See audio.devices."));
                    }
                }
                let mut settings = self.settings.clone();
                settings.audio.midi_input = port;
                self.apply_settings(settings)?;
                Ok(self.audio_status())
            }
            "audio.reconnect" => {
                self.connect();
                Ok(json!({ "reconnecting": true }))
            }
            "note.preview" => {
                let track = params["trackId"]
                    .as_str()
                    .ok_or("note.preview needs `trackId`")?;
                if !self
                    .store
                    .session()
                    .tracks
                    .iter()
                    .any(|t| t.id == track && t.kind == "midi")
                {
                    return Err("Preview needs an instrument track".into());
                }
                let pitch = params["pitch"]
                    .as_i64()
                    .filter(|p| (0..=127).contains(p))
                    .ok_or("pitch must be 0-127")?;
                let velocity = params["velocity"].as_i64().unwrap_or(100);
                if !(1..=127).contains(&velocity) {
                    return Err("velocity must be 1-127".into());
                }
                let track = track.to_string();
                self.preview(&track, pitch as u8, velocity as u8);
                Ok(json!({ "trackId": track, "pitch": pitch, "velocity": velocity }))
            }
            "note.hold" => {
                let pitch = params["pitch"]
                    .as_i64()
                    .filter(|p| (0..=127).contains(p))
                    .ok_or("pitch must be 0-127")? as u8;
                let velocity = params["velocity"].as_i64().unwrap_or(100);
                if !(1..=127).contains(&velocity) {
                    return Err("velocity must be 1-127".into());
                }
                let on = params["on"].as_bool().ok_or("note.hold needs `on`")?;
                if on {
                    if self.midi_route.load(std::sync::atomic::Ordering::Relaxed)
                        == ryolune_engine::midi::UNROUTED
                    {
                        return Err("Select an instrument track before holding a note".into());
                    }
                    if !self.typing_down.contains(&pitch) {
                        self.typing_down.push(pitch);
                        self.live_note(true, pitch, velocity as u8);
                    }
                } else {
                    self.typing_down.retain(|p| *p != pitch);
                    self.live_note(false, pitch, 0);
                }
                Ok(json!({ "pitch": pitch, "on": on, "held": self.typing_down }))
            }
            "note.releaseAll" => {
                self.release_typing();
                Ok(json!({ "held": [] }))
            }
            "transport.punch" => {
                let enabled = params["enabled"]
                    .as_bool()
                    .ok_or("transport.punch needs `enabled`")?;
                if self.record_enabled != enabled {
                    self.record_enabled = enabled;
                    if self.playing {
                        // Only this punch's own failure answers the request: an error the
                        // window was already showing is not about it.
                        self.guarded(|app| {
                            if enabled {
                                app.start_recording();
                            } else {
                                app.finish_recording();
                            }
                        })?;
                    }
                }
                Ok(
                    json!({ "recordEnabled": self.record_enabled, "playing": self.playing,
                    "recording": self.midi_recording || self.recorder.is_some() }),
                )
            }
            "ui.closePluginWindow" => {
                let id = params["id"]
                    .as_str()
                    .ok_or("ui.closePluginWindow needs `id`")?;
                if !self.plugins.windows.contains_key(id) {
                    return Err(format!(
                        "No plugin window `{id}`; see ui.status pluginWindows"
                    ));
                }
                self.close_plugin_window(id);
                Ok(self.ui_status())
            }
            "ui.dismissError" => {
                let dismissed = self.error.take();
                Ok(json!({ "dismissed": dismissed }))
            }
            "app.confirm" => {
                let Some(intent) = self.intent.take() else {
                    return Err("The window is not asking anything right now".into());
                };
                match params["choice"].as_str().unwrap_or("") {
                    "cancel" => {}
                    "discard" => self.execute(intent),
                    "save" => {
                        self.after_save = Some(intent);
                        self.save(false);
                    }
                    other => {
                        self.intent = Some(intent);
                        return Err(format!(
                            "choice must be save, discard or cancel, not `{other}`"
                        ));
                    }
                }
                Ok(self.ui_status())
            }
            "app.openGuide" => match params["guide"].as_str().unwrap_or("") {
                "plugins" => {
                    let url =
                        "https://github.com/ludovic111/ryolune/blob/main/docs/NATIVE_PLUGINS.md";
                    crate::settings::reveal(std::path::Path::new(url));
                    Ok(json!({ "opened": url }))
                }
                "support" => {
                    crate::settings::reveal(std::path::Path::new(SUPPORT_URL));
                    Ok(json!({ "opened": SUPPORT_URL }))
                }
                other => match ryolune_engine::settings::Service::parse(other) {
                    Some(service) => {
                        crate::settings::reveal(std::path::Path::new(service.help_url()));
                        Ok(json!({ "opened": service.help_url() }))
                    }
                    None => Err(format!(
                        "Unknown guide `{other}`. Guides: plugins, support, elevenlabs, stability, fal, custom."
                    )),
                },
            },
            "app.relaunch" => {
                self.request(crate::app::Intent::Relaunch);
                Ok(json!({ "prompt": self.intent.is_some() }))
            }
            "app.reportProblem" => {
                let url = ryolune_engine::diagnostics::issue_url(
                    &ryolune_engine::host::scan::data_dir(),
                );
                crate::settings::reveal(Path::new(&url));
                Ok(json!({ "opened": url }))
            }
            "session.saveRecoveredTake" => {
                let path = PathBuf::from(params["path"].as_str().unwrap_or(""));
                if path
                    .extension()
                    .is_none_or(|e| !e.eq_ignore_ascii_case("wav"))
                {
                    return Err("The recovered take is written as .wav".into());
                }
                if self.recovered_recording_write.is_some() {
                    return Err("The recovered take is already being saved".into());
                }
                let buffer = self
                    .unplaced_recording
                    .clone()
                    .ok_or("There is no recovered take to save")?;
                ryolune_engine::device::preserve_recording(&buffer, &path)?;
                self.unplaced_recording = None;
                self.status = format!("Recovered take saved to {}", path.display());
                Ok(json!({ "path": path }))
            }
            "agent.changes" => Ok(self.agents.changes_json(self.store.undo_depth())),
            "agent.revertTurn" => self.revert_turn(params["redo"].as_bool().unwrap_or(false)),
            "agent.revert" => {
                let sequence = params["sequence"]
                    .as_u64()
                    .ok_or("agent.revert needs `sequence`")?;
                if self.agents.runtime.running() {
                    return Err("Stop the agent before stepping through its changes".into());
                }
                if !self
                    .agents
                    .changes_json(0)
                    .as_array()
                    .is_some_and(|list| list.iter().any(|c| c["sequence"] == sequence))
                {
                    return Err(format!("No agent change {sequence}; see agent.changes"));
                }
                if params["redo"].as_bool().unwrap_or(false) {
                    self.redo_activity(sequence);
                } else {
                    self.revert_activity(sequence);
                }
                Ok(self.agents.changes_json(self.store.undo_depth()))
            }
            "ui.screenshot" => {
                let path = match params["path"].as_str() {
                    Some(p) if !p.trim().is_empty() => PathBuf::from(p),
                    _ => control_app::default_screenshot_path(),
                };
                if path
                    .extension()
                    .is_none_or(|e| !e.eq_ignore_ascii_case("png"))
                {
                    return Err("Screenshots are written as .png".into());
                }
                if self
                    .live_jobs
                    .iter()
                    .any(|j| matches!(j.wait, LiveWait::Screenshot { .. }))
                {
                    return Err("A screenshot is already being captured; retry in a moment".into());
                }
                let mut result = self.start_live(
                    LiveWait::Screenshot {
                        path: path.clone(),
                        requested: false,
                    },
                    action,
                    params,
                    source,
                );
                result["path"] = json!(path);
                Ok(result)
            }
            "ui.showPanel" => {
                let panel = params["panel"].as_str().unwrap_or("");
                let visible = params["visible"].as_bool().unwrap_or(true);
                match panel {
                    "agent" => self.agents.open = visible,
                    "automation" => self.show_automation = visible,
                    "settings" => {
                        let section = params["section"].as_str().map(|key| {
                            crate::settings::SECTION_KEYS.iter().position(|candidate| *candidate == key)
                                .ok_or_else(|| format!("Unknown settings section `{key}`"))
                        }).transpose()?;
                        if visible {
                            self.open_settings(section);
                        } else {
                            self.settings_ui.open = false;
                        }
                    }
                    "help" => self.show_help = visible,
                    "mixer" => self.show_mixer = visible,
                    "controllers" => self.show_controllers = visible,
                    "tempo" => self.show_tempo = visible,
                    "palette" => self.show_palette = visible,
                    "export" => {
                        if visible {
                            self.open_export_dialog();
                        } else {
                            self.export.close();
                        }
                    }
                    "recovery" => {
                        if visible {
                            self.open_recovery();
                        } else {
                            self.recovery.close();
                        }
                    }
                    "whatsNew" => {
                        self.whats_new = visible.then(crate::diagnostics::WhatsNew::default);
                    }
                    "diagnostics" => {
                        if visible {
                            let section = crate::settings::SECTION_KEYS
                                .iter()
                                .position(|key| *key == "diagnostics");
                            self.open_settings(section);
                        } else {
                            self.settings_ui.open = false;
                        }
                    }
                    "plugins" => {
                        const PARTS: [&str; 4] = ["stock", "installed", "formats", "build"];
                        let part = match params["section"].as_str() {
                            Some(key) => PARTS.iter().position(|p| *p == key).ok_or_else(|| {
                                format!("Unknown Plugins part `{key}`: stock, installed, formats or build")
                            })?,
                            None => self.show_plugins.unwrap_or(1),
                        };
                        self.show_plugins = visible.then_some(part);
                    }
                    "master" | "bus-a" | "bus-b" => {
                        self.try_dispatch(Command::Select {
                            track: Some(panel.into()),
                            clip: None,
                            note: None,
                        })?;
                    }
                    other => {
                        return Err(format!(
                            "Unknown panel `{other}`. Panels: agent, automation, mixer, controllers, tempo, palette, settings, plugins, help, export, recovery, whatsNew, diagnostics, master, bus-a, bus-b."
                        ))
                    }
                }
                Ok(self.ui_status())
            }
            "ui.openPluginWindow" => {
                let track = params["trackId"]
                    .as_str()
                    .ok_or("ui.openPluginWindow needs `trackId`")?;
                let slot = params["slot"].as_u64().map(|s| s as usize);
                if slot.is_some_and(|s| s >= ryolune_engine::model::MAX_INSERTS) {
                    return Err("Insert slot must be 0-7".into());
                }
                let insert = control::selected_plugin(self.store.session(), track, slot)?;
                let key = insert.id.clone();
                self.open_plugin_window(&key);
                if params["native"].as_bool().unwrap_or(false) {
                    if !self
                        .plugins
                        .loaded
                        .get(&key)
                        .is_some_and(|l| l.editor.has_gui())
                    {
                        return Err(
                            "This plugin has no native editor; its parameter panel is open".into(),
                        );
                    }
                    self.toggle_native_window_public(&key);
                }
                Ok(json!({ "opened": key, "plugin": insert.name }))
            }
            "ui.closePluginWindows" => {
                let keys: Vec<String> = self.plugins.windows.keys().cloned().collect();
                for key in &keys {
                    self.close_plugin_window(key);
                }
                Ok(json!({ "closed": keys.len() }))
            }
            "ui.musicalTyping" => {
                let enabled = params["enabled"]
                    .as_bool()
                    .ok_or("enabled must be true or false")?;
                if enabled != self.musical_typing {
                    self.toggle_musical_typing();
                }
                Ok(json!({ "musicalTyping": self.musical_typing }))
            }
            "ui.setTool" => {
                self.tool = match params["tool"].as_str().unwrap_or("") {
                    "pointer" => 0,
                    "pencil" => 1,
                    "scissors" => 2,
                    other => {
                        return Err(format!(
                            "Unknown tool `{other}`: pointer, pencil or scissors"
                        ))
                    }
                };
                Ok(self.ui_status())
            }
            "ui.status" => Ok(self.ui_status()),
            "ui.state" => Ok(self.ui_state()),
            "app.status" => Ok(json!({
                "device": self.device.as_ref().map(|d| d.device_name.clone()),
                "sampleRate": self.device.as_ref().map(|d| d.sample_rate),
                "bridgePort": self.control.as_ref().map(|s| s.port()),
                "updateAvailable": self.updates.available.as_ref().map(|r| r.version.clone()),
                "updateInstalled": self.updates.installed.as_ref().map(|p| p.display().to_string()),
                "agentProvider": self.settings.agent.provider.key(),
                "dirty": self.store.dirty(),
            })),
            "app.checkUpdates" => {
                if self.updates.busy() {
                    return Err("An update check or install is already running".into());
                }
                if let Some(release) = &self.updates.available {
                    return Ok(
                        json!({ "available": release_json(release), "current": crate::update::current_version() }),
                    );
                }
                self.check_for_updates(true);
                Ok(self.start_live(LiveWait::UpdateCheck, action, params, source))
            }
            "app.installUpdate" => {
                if self.updates.installed.is_some() {
                    return Err("An update is installed; relaunch ryolune to use it".into());
                }
                if self.updates.busy() {
                    return Err("An update check or install is already running".into());
                }
                if self.updates.available.is_none() {
                    return Err("No update is known; run app.checkUpdates first".into());
                }
                self.install_update();
                Ok(self.start_live(LiveWait::UpdateInstall, action, params, source))
            }
            "app.quit" => {
                if params["discard"].as_bool().unwrap_or(false) {
                    self.intent = None;
                    self.store.mark_saved(self.store.revision);
                    self.execute(Intent::Quit);
                } else {
                    self.request(Intent::Quit);
                }
                Ok(json!({ "quitting": true, "prompted": self.intent.is_some() }))
            }
            "session.restoreSnapshot" => {
                let path = PathBuf::from(
                    params["path"]
                        .as_str()
                        .ok_or("session.restoreSnapshot needs `path`")?,
                );
                recovery::ensure_generated(&recovery::directory(), &path)?;
                self.can_replace_document()?;
                self.recovery.select(path);
                self.request(Intent::Recover);
                Ok(json!({ "status": "requested", "prompted": self.intent.is_some() }))
            }
            "agent.configure" => {
                if self.agents.runtime.running() {
                    return Err("Stop the agent before changing its model".into());
                }
                let mut next = self.settings.clone();
                next.agent.provider = serde_json::from_value(params["provider"].clone())
                    .map_err(|e| format!("Invalid provider: {e}"))?;
                next.agent.model = params["model"]
                    .as_str()
                    .ok_or("Missing model")?
                    .trim()
                    .into();
                next.agent.reasoning_effort = params["reasoningEffort"]
                    .as_str()
                    .ok_or("Missing effort")?
                    .into();
                self.apply_settings(next)?;
                Ok(self.agents.status_json(&self.settings))
            }
            "agent.status" => Ok(self.agents.status_json(&self.settings)),
            "agent.providers" => Ok(crate::agent::providers_json(&self.settings)),
            "agent.mcp" => Ok(crate::agent::clients::clients(
                &crate::agent::clients::Server::current(&self.discovery_path()),
                self.settings.control.enable_bridge,
            )),
            "agent.openClient" => {
                let client = params["client"].as_str().unwrap_or("");
                let server = crate::agent::clients::Server::current(&self.discovery_path());
                let link = crate::agent::clients::link(&server, client).ok_or_else(|| {
                    format!("{client} has no install link: copy its configuration from agent.mcp")
                })?;
                crate::settings::reveal(Path::new(&link));
                Ok(json!({ "opened": client }))
            }
            "agent.models" => {
                let settings = self.settings.clone();
                Ok(self.start_worker(action, params, source, move || {
                    serde_json::to_value(crate::agent::catalog::discover(&settings))
                        .map_err(|e| e.to_string())
                }))
            }
            "agent.connection" => {
                let settings = self.settings.clone();
                Ok(self.start_worker(action, params, source, move || {
                    serde_json::to_value(crate::agent::connection::check(&settings))
                        .map_err(|e| e.to_string())
                }))
            }
            "agent.send" => {
                let prompt = params["prompt"].as_str().unwrap_or("").trim().to_string();
                if prompt.is_empty() {
                    return Err("agent.send needs a non-empty `prompt`".into());
                }
                self.agents.set_prompt(&prompt);
                self.agents.open = true;
                self.start_agent_task_now()?;
                Ok(self.agents.status_json(&self.settings))
            }
            "agent.stop" => {
                self.agents.stop_runner();
                Ok(self.agents.status_json(&self.settings))
            }
            "agent.transcript" => {
                let limit = params["limit"].as_u64().unwrap_or(40).clamp(1, 500) as usize;
                Ok(self.agents.transcript_json(limit))
            }
            "agent.clear" => {
                if self.agents.runner_busy() {
                    return Err("Stop the running task before clearing the conversation".into());
                }
                self.follow_song();
                self.agents.clear_transcript();
                self.save_conversation(false);
                Ok(json!({ "cleared": true }))
            }
            "agent.conversations" => {
                self.follow_song();
                Ok(self.conversations_json())
            }
            "agent.newConversation" => self.new_conversation(),
            "agent.selectConversation" => self.select_conversation(params["id"].as_str().unwrap_or("")),
            "agent.renameConversation" => self.rename_conversation(
                params["id"].as_str(),
                params["title"].as_str().unwrap_or(""),
            ),
            "agent.deleteConversation" => self.delete_conversation(params["id"].as_str().unwrap_or("")),
            "agent.memory" => {
                self.follow_song();
                Ok(self.memory_json())
            }
            "agent.setMemory" => self.set_memory(params["text"].as_str().unwrap_or("")),
            "agent.steer" => {
                self.steer_agent(params["text"].as_str().unwrap_or(""))?;
                Ok(self.agents.status_json(&self.settings))
            }
            other => Err(format!("{other} is not available in this window")),
        }
    }
}

fn release_json(release: &crate::update::Release) -> Value {
    json!({
        "version": release.version,
        "tag": release.tag,
        "asset": release.asset,
        "size": release.size,
        "signed": release.signature_url.is_some(),
        "notes": release.notes,
    })
}

#[cfg(test)]
mod tests {
    /// Between an edit and the next reconcile the loaded instance is out of date; commands
    /// must then read the document through a fresh instance, never the stale one.
    #[test]
    fn a_loaded_editor_serves_only_its_own_plugin_and_state() {
        let mut app = Ryolune::from_session(store::empty(), None);
        app.reconcile_plugins();
        let space = app.store.session().strips["bus-a"].inserts[0].clone();
        assert!(Host::loaded_editor(&mut app, &space).is_some());
        let mut other_state = space.clone();
        other_state.blob = "pending".into();
        assert!(Host::loaded_editor(&mut app, &other_state).is_none());
        let mut other_plugin = space.clone();
        other_plugin.plugin = "stock:Echo".into();
        assert!(Host::loaded_editor(&mut app, &other_plugin).is_none());
    }

    #[test]
    fn a_reply_waits_only_on_the_job_its_own_command_started() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = Ryolune::from_session(store::empty(), None);
        // A save nobody waits on, like a dropped MIDI file importing.
        let started = app
            .run_control_command(
                "session.save",
                &json!({"path": dir.path().join("song.ryolune")}),
                false,
                "Interface",
            )
            .unwrap();
        assert_eq!(started["status"], "running");
        app.run_control_command("session.info", &json!({}), false, "CLI")
            .unwrap();
        let (tx, _rx) = mpsc::sync_channel(1);
        assert!(
            app.attach_reply(Reply::Channel(tx)).is_err(),
            "session.info must be answered now, not with the save's result"
        );
        while app.control_job.is_some() {
            app.poll_control_job();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn punch_answers_for_itself_not_for_an_error_already_on_screen() {
        let mut app = Ryolune::from_session(ryolune_engine::store::empty(), None);
        app.error = Some("Audio device disconnected".into());
        app.playing = true;
        app.record_enabled = true;
        let reply = app
            .run_control_command("transport.punch", &json!({"enabled": true}), false, "CLI")
            .expect("nothing changed, so nothing failed");
        assert_eq!(reply["recordEnabled"], true);
        assert_eq!(app.error.as_deref(), Some("Audio device disconnected"));
        app.playing = false;
    }

    use super::*;
    use ryolune_engine::{audio::AudioBuffer, model::*, store};
    use std::time::{Duration, Instant};

    fn finish(app: &mut Ryolune) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while app.control_job.is_some() {
            app.poll_control_job();
            assert!(Instant::now() < deadline, "control worker stalled");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(app.error.is_none(), "{:?}", app.error);
    }

    #[test]
    fn session_lock_filemode_probe() {
        let Some(path) = std::env::var_os("RYOLUNE_TEST_LOCK_TARGET") else {
            return;
        };
        let denied = std::env::var_os("RYOLUNE_TEST_LOCK_DENIED").is_some();
        let result = ryolune_tools::Backend::headless(Some(Path::new(&path)), false);
        if denied {
            assert!(result
                .err()
                .is_some_and(|error| error.contains("already in use")));
        } else {
            assert!(
                result.is_ok(),
                "file mode must be available after the window releases ownership"
            );
        }
    }

    fn probe_file_mode(path: &Path, denied: bool) {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "control::tests::session_lock_filemode_probe",
                "--nocapture",
            ])
            .env("RYOLUNE_TEST_LOCK_TARGET", path)
            .env_remove("RYOLUNE_TEST_LOCK_DENIED");
        if denied {
            command.env("RYOLUNE_TEST_LOCK_DENIED", "1");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn finish_native(app: &mut Ryolune) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while app.job.is_some() {
            app.poll();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn window_file_ownership_survives_native_save_and_async_path_transitions() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.ryolune");
        let b = dir.path().join("b.ryolune");
        let c = dir.path().join("c.ryolune");
        document::save(&store::empty(), &Library::new(), &a).unwrap();
        document::save(&store::empty(), &Library::new(), &b).unwrap();
        let mut app = Ryolune::from_session(store::empty(), None);
        app.load_path(a.clone());
        finish_native(&mut app);
        assert!(app.error.is_none(), "{:?}", app.error);
        probe_file_mode(&a, true);
        assert!(
            document::load(&a).is_ok(),
            "read-only validation remains allowed"
        );
        app.try_dispatch(Command::Rename("Window owns A".into()))
            .unwrap();
        Ryolune::save(&mut app, false);
        finish_native(&mut app);
        assert!(app.error.is_none(), "{:?}", app.error);
        probe_file_mode(&a, true);
        let other = ryolune_tools::Backend::headless(Some(&b), false).unwrap();
        for method in ["session.open", "session.save"] {
            assert!(app
                .run_control_command(method, &json!({"path":b}), false, "test")
                .unwrap_err()
                .contains("already in use"));
            assert_eq!(app.store.session().name, "Window owns A");
            assert!(app.control_job.is_none());
            probe_file_mode(&a, true);
        }
        app.run_control_command("session.save", &json!({"path":c}), false, "test")
            .unwrap();
        probe_file_mode(&c, true);
        probe_file_mode(&a, true);
        finish(&mut app);
        probe_file_mode(&a, false);
        probe_file_mode(&c, true);
        drop(other);
        app.run_control_command("session.open", &json!({"path":b}), false, "test")
            .unwrap();
        finish(&mut app);
        probe_file_mode(&c, false);
        probe_file_mode(&b, true);
        app.execute(Intent::New);
        probe_file_mode(&b, false);
    }

    #[test]
    fn failed_window_load_keeps_its_previous_lease_and_document() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.ryolune");
        let corrupt = dir.path().join("corrupt.ryolune");
        document::save(&store::empty(), &Library::new(), &a).unwrap();
        std::fs::write(&corrupt, b"invalid session").unwrap();
        let mut app = Ryolune::from_session(store::empty(), None);
        Host::open(&mut app, &a).unwrap();
        let before = app.store.snapshot();
        app.run_control_command("session.open", &json!({"path":corrupt}), false, "test")
            .unwrap();
        while app.control_job.is_some() {
            app.poll_control_job();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(app.error.take().is_some());
        assert_eq!(json!(app.store.session()), json!(&*before));
        probe_file_mode(&a, true);
        assert!(SessionFileLock::acquire(&corrupt).is_ok());
        let directory = dir.path().join("cannot-replace-directory.ryolune");
        std::fs::create_dir(&directory).unwrap();
        app.run_control_command("session.save", &json!({"path":directory}), false, "test")
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while app.control_job.is_some() {
            app.poll_control_job();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(app.error.take().is_some());
        assert!(directory.is_dir());
        assert!(
            SessionFileLock::acquire(&directory).is_ok(),
            "failed save releases destination lease"
        );
        probe_file_mode(&a, true);
        drop(app);
        probe_file_mode(&a, false);
    }

    #[test]
    fn agent_file_work_is_deferred_validated_and_preserves_undo() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("take.wav");
        let buffer = AudioBuffer::new(
            48000,
            (0..4800)
                .map(|i| {
                    let v = (i as f32 * 0.03).sin() * 0.2;
                    [v, v]
                })
                .collect(),
        )
        .unwrap();
        std::fs::write(&input, audio::encode_wav(&buffer).unwrap()).unwrap();
        let mut app = Ryolune::from_session(store::empty(), None);
        app.preparing = false;
        app.sync_needed = false;
        let before = app.store.session().clips.len();
        let result = app
            .run_control_command(
                "session.importAudio",
                &json!({"path":input,"startBar":0}),
                true,
                "test",
            )
            .unwrap();
        assert_eq!(result["status"], "running");
        assert_eq!(app.store.session().clips.len(), before);
        assert!(app
            .try_dispatch(Command::Rename("concurrent change".into()))
            .is_err());
        finish(&mut app);
        assert_eq!(app.store.session().clips.len(), before + 1);
        assert!(app.store.session().clips.last().unwrap().agent);
        assert!(!app.library.is_empty());
        app.try_dispatch(Command::Undo).unwrap();
        assert_eq!(app.store.session().clips.len(), before);
        app.try_dispatch(Command::Redo).unwrap();

        let project = temp.path().join("song.ryolune");
        app.run_control_command("session.save", &json!({"path":project}), true, "test")
            .unwrap();
        finish(&mut app);
        assert!(!app.store.dirty());
        let (saved, library) = document::load(&project).unwrap();
        assert_eq!(saved.clips.len(), before + 1);
        assert!(!library.is_empty());
        let wav = temp.path().join("mix.wav");
        app.run_control_command("session.bounce", &json!({"path":wav}), true, "test")
            .unwrap();
        finish(&mut app);
        let mix = control::decode_file(&wav).unwrap();
        assert!(mix.frames.iter().any(|f| f[0].abs() > 0.001));
        assert!(mix.frames.iter().all(|f| f.iter().all(|v| v.is_finite())));
    }

    #[test]
    fn malformed_agent_file_command_does_not_start_work() {
        let mut app = Ryolune::from_session(store::empty(), None);
        assert!(app
            .run_control_command("session.save", &json!({"path":3}), true, "test")
            .is_err());
        assert!(app.control_job.is_none());
        assert!(!app.store.dirty());
    }

    #[test]
    fn a_groove_preview_is_a_job_that_answers_later_and_leaves_the_song_alone() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("groove.wav");
        let mut app = Ryolune::from_session(store::demo(), None);
        app.preparing = false;
        app.sync_needed = false;
        let revision = app.store.revision;
        let tracks = app.store.session().tracks.len();
        let lanes = json!([{"steps":16,"pulses":4,"rotation":0,"pitch":36,"velocity":110}]);
        let result = app
            .run_control_command(
                "rhythm.preview",
                &json!({"lanes":lanes,"bars":1,"path":path}),
                false,
                "test",
            )
            .unwrap();
        assert_eq!(result["status"], "running");
        let (tx, rx) = mpsc::sync_channel(1);
        assert!(app.attach_reply(Reply::Channel(tx)).is_ok());
        // Unlike a file job, a preview does not lock the document while it renders.
        app.try_dispatch(Command::Rename("still editable".into()))
            .unwrap();
        let answer = loop {
            app.poll_workers();
            if let Ok(answer) = rx.try_recv() {
                break answer.unwrap();
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert!(app.live_jobs.is_empty());
        assert_eq!(answer["bars"], 1);
        let decoded = audio::decode(std::fs::read(&path).unwrap(), Some("wav")).unwrap();
        assert!(
            decoded.frames.iter().flatten().any(|v| v.abs() > 0.01),
            "the kick is audible"
        );
        assert_eq!(
            app.store.session().tracks.len(),
            tracks,
            "nothing was created"
        );
        assert_eq!(app.store.revision, revision + 1, "only the rename happened");
        // Bad input is refused by the job, not by a panic.
        app.run_control_command(
            "rhythm.preview",
            &json!({"lanes":lanes,"bars":9}),
            false,
            "test",
        )
        .unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        assert!(app.attach_reply(Reply::Channel(tx)).is_ok());
        let refused = loop {
            app.poll_workers();
            if let Ok(answer) = rx.try_recv() {
                break answer;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert!(refused.unwrap_err().contains("1 to 4 bars"));
    }

    #[test]
    fn the_window_describes_itself_and_the_overview_carries_it() {
        let mut app = Ryolune::from_session(store::demo(), None);
        app.preparing = false;
        app.sync_needed = false;
        app.run_control_command(
            "ui.showPanel",
            &json!({"panel":"mixer","visible":true}),
            true,
            "MCP / agent",
        )
        .unwrap();
        app.run_control_command(
            "ui.showPanel",
            &json!({"panel":"settings","section":"audio"}),
            false,
            "test",
        )
        .unwrap();
        app.run_control_command("ui.setTool", &json!({"tool":"scissors"}), true, "test")
            .unwrap();
        let state = app
            .run_control_command("ui.state", &json!({}), true, "MCP / agent")
            .unwrap();
        assert_eq!(state["panels"]["mixer"], true, "{state}");
        assert_eq!(state["panels"]["settings"], "audio");
        assert_eq!(state["arrangement"]["tool"], "scissors");
        assert_eq!(state["editor"]["shownInstead"], "mixer");
        assert_eq!(state["selection"]["track"]["name"], "Bass");
        assert_eq!(state["editor"]["clip"]["name"], "Bass verse");
        assert!(state["theme"]["appearance"].is_string());
        let overview = app
            .run_control_command(
                "session.overview",
                &json!({"parameters": false}),
                true,
                "MCP / agent",
            )
            .unwrap();
        assert_eq!(overview["song"]["mode"], "live");
        assert_eq!(overview["window"]["panels"]["mixer"], true);
        // Names work through the window too, agent permissions included.
        let solo = app
            .run_control_command(
                "track.setSolo",
                &json!({"trackId":"Keys","solo":true}),
                true,
                "MCP / agent",
            )
            .unwrap();
        assert_eq!(solo["solo"], true);
    }

    #[test]
    fn a_batch_is_one_change_because_it_is_one_undo_step() {
        let mut app = Ryolune::from_session(store::demo(), None);
        app.preparing = false;
        app.sync_needed = false;
        let clips = app.store.session().clips.len();
        let batch = json!({"commands":[
            {"command":"clip.create","params":{"trackId":"bass","startBar":40,"lengthBars":1}},
            {"command":"clip.create","params":{"trackId":"bass","startBar":41,"lengthBars":1}},
            {"command":"track.setVolume","params":{"trackId":"bass","volume":0.5}},
        ]});
        app.run_control_command("session.batch", &batch, true, "MCP / agent")
            .unwrap();
        assert_eq!(app.store.session().clips.len(), clips + 2);
        let changes = app.agents.changes_json(app.store.undo_depth());
        let list = changes.as_array().unwrap();
        assert_eq!(list.len(), 1, "{changes}");
        assert!(list[0]["title"]
            .as_str()
            .unwrap()
            .starts_with("Batch · 3 commands · clip.create"));
        // Reverting that one change takes the whole batch back, which is what it says.
        let sequence = list[0]["sequence"].clone();
        app.run_control_command("agent.revert", &json!({"sequence":sequence}), false, "test")
            .unwrap();
        assert_eq!(app.store.session().clips.len(), clips);
        // A batch that fails and rolls back is still one entry, marked as not having succeeded.
        let bad = json!({"commands":[
            {"command":"clip.create","params":{"trackId":"bass","startBar":50,"lengthBars":1}},
            {"command":"clip.remove","params":{"clipId":"no-such-clip"}},
        ]});
        assert!(app
            .run_control_command("session.batch", &bad, true, "MCP / agent")
            .is_err());
        assert_eq!(app.store.session().clips.len(), clips);
        let after = app.agents.changes_json(app.store.undo_depth());
        let after = after.as_array().unwrap();
        // The revert above is itself on the list; the failed batch added exactly one more.
        let failed: Vec<_> = after
            .iter()
            .filter(|c| c["title"].as_str().unwrap().starts_with("Batch"))
            .collect();
        assert_eq!(failed.len(), 2);
        let outcomes: Vec<bool> = failed
            .iter()
            .map(|c| c["succeeded"].as_bool().unwrap())
            .collect();
        assert!(
            outcomes.contains(&true) && outcomes.contains(&false),
            "{outcomes:?}"
        );
        assert!(after
            .iter()
            .all(|c| !c["title"].as_str().unwrap().starts_with("Clip")));
    }

    #[test]
    fn midi_take_blocks_nested_timing_edits_and_undo() {
        let mut app = Ryolune::from_session(store::empty(), None);
        app.midi_recording = true;
        let mut t: Transport = app.store.session().transport.clone();
        t.tempo = 99.0;
        assert!(app
            .try_dispatch(Command::Batch(vec![Command::SetTransport(t)]))
            .is_err());
        assert!(app.try_dispatch(Command::Undo).is_err());
        assert_eq!(app.store.session().transport.tempo, 120.0);
        assert!(app
            .try_dispatch(Command::Rename("Take session".into()))
            .is_ok());
    }

    #[test]
    fn diagnostics_are_readable_and_reporting_stays_with_the_person() {
        let mut app = Ryolune::from_session(store::empty(), None);
        let refused = app
            .run_control_command("app.reportProblem", &json!({}), true, "MCP / agent")
            .unwrap_err();
        assert!(refused.contains("Only a person"), "{refused}");
        let diagnostics = app
            .run_control_command("app.diagnostics", &json!({}), true, "MCP / agent")
            .unwrap();
        assert_eq!(diagnostics["version"], env!("CARGO_PKG_VERSION"));
        assert!(diagnostics["paths"]["crashes"].is_string(), "{diagnostics}");
        assert!(!diagnostics.to_string().contains("ApiKey"));
        app.settings.agent.permissions.file_operations = false;
        assert!(app
            .run_control_command("app.clearCrashReports", &json!({}), true, "MCP / agent")
            .unwrap_err()
            .contains("fileOperations"));
        // The lsuite name for Relaunch keeps Relaunch's permission.
        app.settings.agent.permissions.app_control = false;
        assert!(app
            .run_control_command("app.restart", &json!({}), true, "MCP / agent")
            .unwrap_err()
            .contains("appControl"));
        let notes = app
            .run_control_command("app.whatsNew", &json!({"since": "0.12.0"}), true, "test")
            .unwrap();
        assert_eq!(notes["releases"][0]["version"], env!("CARGO_PKG_VERSION"));
        app.run_control_command("ui.showPanel", &json!({"panel": "whatsNew"}), true, "test")
            .unwrap();
        assert!(app.whats_new.is_some());
        let state = app
            .run_control_command("ui.state", &json!({}), true, "test")
            .unwrap();
        assert_eq!(state["panels"]["whatsNew"], true);
        app.run_control_command(
            "ui.showPanel",
            &json!({"panel": "diagnostics"}),
            true,
            "test",
        )
        .unwrap();
        assert!(app.settings_ui.open);
        assert_eq!(
            crate::settings::SECTION_KEYS[app.settings_ui.section],
            "diagnostics"
        );
    }
}
