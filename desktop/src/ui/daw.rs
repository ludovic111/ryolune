//! The `Daw` entity owns the host (`Ryolune`): the document store, audio, plugins, the
//! control bridge and the agent. Every view holds an `Entity<Daw>`, reads the host from it
//! and changes it through [`Daw::run`], which goes through the shared command registry like
//! the CLI, MCP and the agent do. Nothing here draws.

use crate::app::Ryolune;
use gpui::{Context, Task};
use ryolune_engine::Result;
use serde_json::{json, Value};
use std::time::Duration;

/// Ticks while something moves (playback, recording, a worker): one per display frame.
const BUSY: Duration = Duration::from_millis(16);
/// Ticks while idle: the bridge, the agent and recovery still need the interface thread.
const IDLE: Duration = Duration::from_millis(100);

pub struct Daw {
    pub app: Ryolune,
    /// The last state views were told about; a tick notifies only when it changed.
    fingerprint: u64,
    /// The error last written to the log, so each one is logged once.
    logged_error: Option<String>,
    _tick: Option<Task<()>>,
    _wake: Option<Task<()>>,
}

/// What a tick compares to decide whether the window must redraw.
fn fingerprint(app: &Ryolune) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    app.store.revision.hash(&mut h);
    app.store.undo_depth().hash(&mut h);
    app.store.dirty().hash(&mut h);
    app.position.to_bits().hash(&mut h);
    (app.playing, app.record_enabled, app.musical_typing).hash(&mut h);
    (app.zoom.to_bits(), app.scroll.to_bits()).hash(&mut h);
    (&app.status, &app.error, app.intent.is_some()).hash(&mut h);
    (
        app.job.is_some(),
        app.control_job.is_some(),
        app.scan_job.is_some(),
    )
        .hash(&mut h);
    (
        app.show_mixer,
        app.show_controllers,
        app.show_tempo,
        app.show_palette,
        app.show_help,
        app.show_automation,
        app.settings_ui.open,
        app.settings_ui.section,
        app.agents.open,
        app.tool,
    )
        .hash(&mut h);
    app.agents.fingerprint(app.store.undo_depth()).hash(&mut h);
    app.plugins.windows.len().hash(&mut h);
    // A plugin's own window closed by the person: its key goes dark.
    app.plugins
        .windows
        .values()
        .filter(|w| w.native.as_ref().is_some_and(|n| n.is_open()))
        .count()
        .hash(&mut h);
    app.plugins.loaded.len().hash(&mut h);
    app.catalog.len().hash(&mut h);
    app.library.len().hash(&mut h);
    (app.export.open, app.export.busy(), app.recovery.open).hash(&mut h);
    app.recovery.candidates.len().hash(&mut h);
    (
        app.updates.available.is_some(),
        app.updates.installed.is_some(),
        app.updates.busy(),
        app.updates.show,
    )
        .hash(&mut h);
    app.settings_ui.job.is_some().hash(&mut h);
    app.settings_ui.notice.as_ref().map(|n| &n.0).hash(&mut h);
    app.settings_ui.error.hash(&mut h);
    app.whats_new.hash(&mut h);
    if let Some(device) = &app.device {
        // Meters move without anything else changing; a coarse step is enough to redraw.
        for peak in device.telemetry.peaks() {
            ((peak * 200.0) as i32).hash(&mut h);
        }
        for peak in device.telemetry.track_peaks() {
            ((peak * 200.0) as i32).hash(&mut h);
        }
        ((device.telemetry.load() * 100.0) as i32).hash(&mut h);
    }
    h.finish()
}

impl Daw {
    pub fn new(app: Ryolune) -> Self {
        Self {
            app,
            fingerprint: 0,
            logged_error: None,
            _tick: None,
            _wake: None,
        }
    }

    /// Start the interface thread's work: a tick per frame while busy, ten a second while
    /// idle, and an immediate pass whenever another thread wakes the window.
    pub fn start(
        &mut self,
        wake: futures::channel::mpsc::UnboundedReceiver<()>,
        cx: &mut Context<Self>,
    ) {
        self._tick = Some(cx.spawn(async move |this, cx| loop {
            let Ok(busy) = this.update(cx, |daw, cx| {
                daw.tick(cx);
                daw.app.busy()
            }) else {
                break;
            };
            cx.background_executor()
                .timer(if busy { BUSY } else { IDLE })
                .await;
        }));
        self._wake = Some(cx.spawn(async move |this, cx| {
            use futures::StreamExt;
            let mut wake = wake;
            while wake.next().await.is_some() {
                // Several wakes queued behind a busy frame are one pass.
                while wake.try_recv().is_ok() {}
                if this
                    .update(cx, |daw, cx| {
                        daw.app.serve();
                        daw.changed(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    pub fn tick(&mut self, cx: &mut Context<Self>) {
        self.app.tick();
        if self.app.error != self.logged_error {
            if let Some(error) = &self.app.error {
                log::warn!("error shown: {error}");
            }
            self.logged_error = self.app.error.clone();
        }
        if self.app.closing {
            self.app.publish_discovery(false);
            self.app.shutdown_audio();
            ryolune_engine::diagnostics::clean_exit();
            cx.quit();
            return;
        }
        self.changed(cx);
    }

    /// Tell the views when anything they show changed.
    pub fn changed(&mut self, cx: &mut Context<Self>) {
        let now = fingerprint(&self.app);
        if now != self.fingerprint {
            self.fingerprint = now;
            cx.notify();
        }
    }

    /// Run a registry command from the window. A failure shows in the error dialog; use
    /// [`Daw::request`] when the caller shows its own error.
    pub fn run(&mut self, method: &str, params: Value, cx: &mut Context<Self>) -> Option<Value> {
        match self.request(method, params, cx) {
            Ok(value) => Some(value),
            Err(error) => {
                self.app.error = Some(error);
                cx.notify();
                None
            }
        }
    }

    /// Run a registry command and hand the error back.
    pub fn request(
        &mut self,
        method: &str,
        params: Value,
        cx: &mut Context<Self>,
    ) -> Result<Value> {
        let result = self
            .app
            .run_control_command(method, &params, false, "Interface");
        self.changed(cx);
        cx.notify();
        result
    }

    /// `run` with no parameters.
    pub fn fire(&mut self, method: &str, cx: &mut Context<Self>) -> Option<Value> {
        self.run(method, json!({}), cx)
    }

    /// Start or end a continuous edit (a drag, a dial): every command in between is one
    /// undo step.
    pub fn gesture(&mut self, active: bool) {
        self.app.store.set_gesture(active);
        self.app.interacting = active;
    }
}
