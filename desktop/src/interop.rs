//! The window's side of working with other apps: the first-run setup, the recent songs and
//! the song an import or a recent file is about to replace the open one with. The formats
//! and the commands live in the engine (`interop/`, `control_interop.rs`); the sheets are
//! `ui/dialogs/onboarding.rs`, `ui/dialogs/recent.rs` and the export sheet's two app modes.

use crate::app::{Intent, Ryolune};
use ryolune_engine::{audio::Library, model::Session};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Default)]
pub(crate) struct Interop {
    /// The first-run setup shows: on the first start, or from Help › Set Up ryolune….
    pub show_onboarding: bool,
    /// The recent songs sheet (File › Open Recent…).
    pub show_recent: bool,
    /// A recent song to open once the unsaved-changes question is answered.
    pub pending_open: Option<PathBuf>,
    /// A `session.importFrom` request waiting for the same answer.
    pub pending_import: Option<Value>,
    /// The setup's answers while it shows: the app (index in `interop::apps::APPS`) and AI.
    pub from: Option<usize>,
    pub ai: bool,
}

impl Ryolune {
    /// Show the first-run setup with the answers given last time.
    pub(crate) fn show_onboarding(&mut self) {
        let onboarding = &self.settings.onboarding;
        self.interop.from = ryolune_engine::interop::apps::APPS
            .iter()
            .position(|a| a.id == onboarding.coming_from);
        self.interop.ai = onboarding.ai.unwrap_or(false);
        self.interop.show_onboarding = true;
    }

    /// Install a song brought from another app as the open document: no file yet, unsaved.
    pub(crate) fn adopt_imported(&mut self, session: Session, library: Library) {
        let revision = self.store.revision;
        self.load_document(session, library, PathBuf::new(), None, false);
        if self.store.revision != revision {
            self.path = None;
            self.session_file = None;
            self.position = 0.0;
            self.scroll = 0.0;
            self.store.mark_unsaved();
            self.status = "Song imported — Save chooses where it goes".into();
        }
    }

    /// Open a recent song, asking first about unsaved changes.
    pub(crate) fn open_recent(&mut self, path: PathBuf) {
        self.interop.show_recent = false;
        self.interop.pending_open = Some(path);
        self.request(Intent::OpenRecent);
    }

    /// Run a `session.importFrom` the export sheet prepared, asking first about unsaved
    /// changes; the sheet shows the report when the job ends.
    pub(crate) fn import_from_app(&mut self, params: Value) {
        self.interop.pending_import = Some(params);
        self.request(Intent::ImportFrom);
    }

    /// The unsaved-changes question was answered (or not needed): open the recent song or
    /// run the import.
    pub(crate) fn execute_interop(&mut self, intent: Intent) {
        match intent {
            Intent::OpenRecent => {
                if let Some(path) = self.interop.pending_open.take() {
                    self.load_path(path);
                }
            }
            Intent::ImportFrom => {
                if let Some(params) = self.interop.pending_import.take() {
                    self.export.awaiting = Some("session.importFrom".into());
                    let result =
                        self.run_control_command("session.importFrom", &params, false, "File menu");
                    if !result
                        .as_ref()
                        .is_ok_and(|value| value["status"] == "running")
                    {
                        self.export.completed("session.importFrom", &result);
                    }
                }
            }
            _ => {}
        }
    }
}
