//! Periodic recovery copies, separate from the project chosen by the user.
//! One worker owns file I/O. Generation/revision checks prevent an old job
//! from replacing a newer document or masking edits made during recovery.

use crate::app::Ryolune;
use ryolune_engine::{
    audio::Library,
    document,
    model::Session,
    recovery::{directory, ensure_generated, generated_path, generated_title, list, Snapshot},
    Result,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Default snapshot interval; Settings > General overrides it.
#[cfg(test)]
const INTERVAL: Duration = Duration::from_secs(30);

pub(crate) struct Recovery {
    pub(crate) generation: u64,
    pub(crate) run: u128,
    pub(crate) last_attempt: Instant,
    pub(crate) last_revision: Option<u64>,
    pub(crate) path: Option<PathBuf>,
    pub(crate) latest: Option<(PathBuf, SystemTime)>,
    pub(crate) worker: Option<Worker>,
    pub(crate) open: bool,
    pub(crate) refresh: bool,
    pub(crate) candidates: Vec<Snapshot>,
    pub(crate) selected: Option<PathBuf>,
    pub(crate) error: Option<String>,
}

impl Default for Recovery {
    fn default() -> Self {
        Self {
            generation: 0,
            run: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            last_attempt: Instant::now(),
            last_revision: None,
            path: None,
            latest: None,
            worker: None,
            open: false,
            refresh: false,
            candidates: vec![],
            selected: None,
            error: None,
        }
    }
}

pub(crate) struct Worker {
    generation: u64,
    receiver: mpsc::Receiver<Result<Outcome>>,
}

enum Outcome {
    Saved {
        path: PathBuf,
        revision: u64,
    },
    Listed(Vec<Snapshot>),
    Loaded {
        path: PathBuf,
        session: Box<Session>,
        library: Library,
        revision: u64,
    },
}

impl Recovery {
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn new_document(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.last_attempt = Instant::now();
        self.last_revision = None;
        self.path = None;
        self.latest = None;
        self.open = false;
        self.refresh = false;
        self.selected = None;
        self.error = None;
        // Keep the old worker until it finishes: document changes cannot create
        // another concurrent writer. Its result belongs to the old generation.
    }

    pub(crate) fn status(&self) -> String {
        if let Some(error) = &self.error {
            return format!("Recovery: {error}");
        }
        if self.worker.is_some() {
            return "Recovery: working…".into();
        }
        match &self.latest {
            Some((_, time)) => format!("Recovery snapshot · {}", age(*time)),
            None => "Recovery snapshots · while edited and idle".into(),
        }
    }

    fn due(
        &self,
        now: Instant,
        dirty: bool,
        revision: u64,
        native_plugins: bool,
        interval: Duration,
    ) -> bool {
        self.worker.is_none()
            && (native_plugins || (dirty && self.last_revision != Some(revision)))
            && now.saturating_duration_since(self.last_attempt) >= interval
    }
    /// Listing, writing or opening a snapshot is in progress.
    pub(crate) fn working(&self) -> bool {
        self.worker.is_some()
    }
    pub(crate) fn select(&mut self, path: PathBuf) {
        self.selected = Some(path);
        self.open = false;
    }
    pub(crate) fn close(&mut self) {
        self.open = false;
    }

    fn spawn(&mut self, task: impl FnOnce() -> Result<Outcome> + Send + 'static) {
        debug_assert!(self.worker.is_none());
        let (tx, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let outcome = ryolune_engine::diagnostics::catch("recovery", task)
                .unwrap_or_else(|_| Err("Recovery worker stopped unexpectedly".into()));
            let _ = tx.send(outcome);
        });
        self.worker = Some(Worker {
            generation: self.generation,
            receiver,
        });
    }

    fn poll(&mut self) -> Option<(u64, Result<Outcome>)> {
        let worker = self.worker.as_ref()?;
        let result = match worker.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => Err("Recovery worker disconnected".into()),
        };
        let generation = self.worker.take().unwrap().generation;
        Some((generation, result))
    }
}

impl Ryolune {
    pub(crate) fn open_recovery(&mut self) {
        self.recovery.open = true;
        self.recovery.refresh = true;
    }

    pub(crate) fn restore_recovery(&mut self) {
        let Some(path) = self.recovery.selected.take() else {
            return;
        };
        if self.recovery.worker.is_some() {
            self.error =
                Some("Wait for the recovery worker, then choose the snapshot again.".into());
            return;
        }
        let revision = self.store.revision;
        self.recovery.spawn(move || {
            ensure_generated(&directory(), &path)?;
            let (session, library) = document::load(&path)?;
            Ok(Outcome::Loaded {
                path,
                session: Box::new(session),
                library,
                revision,
            })
        });
    }

    /// Capturing plugin state is an undo step: from a timer it would wipe Redo after the
    /// person undid something, or split a drag (or a batch) in progress into two steps.
    pub(crate) fn background_capture_allowed(&self) -> bool {
        !self.store.can_redo() && !self.store.gesture_active()
    }

    pub(crate) fn poll_recovery(&mut self) {
        if let Some((generation, result)) = self.recovery.poll() {
            if generation == self.recovery.generation {
                match result {
                    Ok(Outcome::Saved { path, revision }) => {
                        self.recovery.last_revision = Some(revision);
                        self.recovery.latest = Some((path, SystemTime::now()));
                        self.recovery.error = None;
                    }
                    Ok(Outcome::Listed(candidates)) => {
                        self.recovery.candidates = candidates;
                        self.recovery.error = None;
                    }
                    Ok(Outcome::Loaded {
                        path,
                        session,
                        library,
                        revision,
                    }) => {
                        if self.store.revision == revision {
                            self.load_document(*session, library, path, None, false);
                            if self.store.revision != revision {
                                self.path = None;
                                self.store.mark_unsaved();
                                self.status =
                                    "Recovered a copy — Save chooses its project file".into();
                            }
                        } else {
                            self.recovery.error = Some("The session changed while recovery was opening. Choose the snapshot again.".into());
                            self.recovery.open = true;
                        }
                    }
                    Err(error) => {
                        self.recovery.error = Some(error);
                    }
                }
            }
        }
        if self.recovery.refresh && self.recovery.worker.is_none() {
            self.recovery.refresh = false;
            self.recovery
                .spawn(|| Ok(Outcome::Listed(list(&directory())?)));
        }
        let idle = self.job.is_none()
            && self.control_job.is_none()
            && self.scan_job.is_none()
            && !self.export.busy()
            && !self.recovery.open
            && self.recorder.is_none()
            && self.record_pending.is_none()
            && self.record_finishing.is_none()
            && !self.midi_recording
            && self.after_take.is_none()
            && self.intent.is_none()
            && !self.interacting;
        let now = Instant::now();
        let interval = Duration::from_secs(u64::from(
            self.settings
                .general
                .recovery_interval_seconds
                .clamp(10, 600),
        ));
        if idle
            && self.recovery.due(
                now,
                self.store.dirty(),
                self.store.revision,
                self.plugins.loaded.values().any(|entry| entry.external),
                interval,
            )
        {
            self.recovery.last_attempt = now;
            let previous_error = self.error.take();
            if self.background_capture_allowed() {
                self.capture_plugin_states();
            }
            if let Some(error) = self.error.take() {
                self.recovery.error = Some(error);
            } else {
                if !self.store.dirty() || self.recovery.last_revision == Some(self.store.revision) {
                    self.error = previous_error;
                    return;
                }
                let mut session = self.store.session().clone();
                session.transport.position_beats = self.position;
                session.view.pixels_per_bar = self.zoom;
                session.view.scroll_bars = self.scroll;
                let library = self.library.clone();
                let revision = self.store.revision;
                let path = self
                    .recovery
                    .path
                    .get_or_insert_with(|| {
                        generated_path(
                            &directory(),
                            self.recovery.run,
                            self.recovery.generation,
                            &session.name,
                        )
                    })
                    .clone();
                self.recovery.spawn(move || {
                    write_snapshot(&path, &session, &library)?;
                    Ok(Outcome::Saved { path, revision })
                });
            }
            self.error = previous_error;
        }
    }
}

fn write_snapshot(path: &Path, session: &Session, library: &Library) -> Result<()> {
    if generated_title(path).is_none() {
        return Err("Invalid generated recovery filename".into());
    }
    fs::create_dir_all(path.parent().ok_or("Recovery path has no parent")?)
        .map_err(|error| error.to_string())?;
    document::save(session, library, path)
}

pub(crate) fn age(time: SystemTime) -> String {
    let seconds = time.elapsed().unwrap_or_default().as_secs();
    if seconds < 60 {
        format!("{seconds}s ago")
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryolune_engine::store;

    fn temp() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ryolune-recovery-test-{}",
            ryolune_engine::control::new_id("case")
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn generated_recovery_writes_preserve_originals_and_latest_copy_loads() {
        let root = temp();
        let original = root.join("song.ryolune");
        let mut session = store::empty();
        document::save(&session, &Library::new(), &original).unwrap();
        let original_bytes = fs::read(&original).unwrap();
        let path = generated_path(&root.join("recovery"), 1234, 1, "My Song / Copy");
        session.name = "Unsaved arrangement".into();
        write_snapshot(&path, &session, &Library::new()).unwrap();
        assert_eq!(fs::read(&original).unwrap(), original_bytes);
        assert_eq!(document::load(&path).unwrap().0.name, "Unsaved arrangement");
        session.name = "Latest arrangement".into();
        write_snapshot(&path, &session, &Library::new()).unwrap();
        assert_eq!(list(&root.join("recovery")).unwrap().len(), 1);
        assert_eq!(document::load(&path).unwrap().0.name, "Latest arrangement");
        let previous_snapshot = fs::read(&path).unwrap();
        session.transport.tempo = f64::NAN;
        assert!(write_snapshot(&path, &session, &Library::new()).is_err());
        assert_eq!(fs::read(&path).unwrap(), previous_snapshot);
        assert_eq!(fs::read(&original).unwrap(), original_bytes);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_eligibility_is_dirty_interval_and_revision_sensitive() {
        let mut recovery = Recovery::default();
        let later = recovery.last_attempt + INTERVAL;
        assert!(!recovery.due(later, false, 1, false, INTERVAL));
        assert!(!recovery.due(recovery.last_attempt, true, 1, false, INTERVAL));
        assert!(recovery.due(later, true, 1, false, INTERVAL));
        recovery.last_revision = Some(1);
        assert!(!recovery.due(later, true, 1, false, INTERVAL));
        assert!(recovery.due(later, true, 2, false, INTERVAL));
        assert!(
            recovery.due(later, false, 1, true, INTERVAL),
            "Native-only edits need a capture even without a new store revision"
        );
        assert!(
            !recovery.due(later, true, 2, false, INTERVAL * 2),
            "a longer configured interval waits"
        );
    }

    #[test]
    fn new_document_keeps_single_worker_and_rejects_old_generation() {
        let mut recovery = Recovery::default();
        let (tx, receiver) = mpsc::sync_channel(1);
        recovery.worker = Some(Worker {
            generation: recovery.generation,
            receiver,
        });
        recovery.new_document();
        assert!(!recovery.due(recovery.last_attempt + INTERVAL, true, 2, true, INTERVAL));
        tx.send(Ok(Outcome::Listed(vec![]))).unwrap();
        let (generation, _) = recovery.poll().unwrap();
        assert_ne!(generation, recovery.generation);
    }

    #[test]
    fn completed_restore_cannot_replace_a_new_document_or_intervening_edit() {
        for new_document in [false, true] {
            let mut app = Ryolune::from_session(store::empty(), None);
            let revision = app.store.revision;
            let mut recovered = store::empty();
            recovered.name = "Obsolete recovery".into();
            let (tx, receiver) = mpsc::sync_channel(1);
            app.recovery.worker = Some(Worker {
                generation: app.recovery.generation,
                receiver,
            });
            tx.send(Ok(Outcome::Loaded {
                path: "/tmp/recovery-snapshot.ryolune".into(),
                session: Box::new(recovered),
                library: Library::new(),
                revision,
            }))
            .unwrap();
            if new_document {
                app.recovery.new_document();
            }
            app.dispatch(ryolune_engine::store::Command::Rename(
                "Current work".into(),
            ));
            app.poll_recovery();
            assert_eq!(app.store.session().name, "Current work");
            assert!(app.path.is_none());
            if !new_document {
                assert!(app.recovery.error.is_some());
            }
        }
    }

    #[test]
    fn restored_copy_is_unsaved_without_a_fake_edit_and_requires_save_on_quit() {
        let mut app = Ryolune::from_session(store::empty(), None);
        let mut recovered = store::empty();
        recovered.name = "Recovered song".into();
        let (tx, receiver) = mpsc::sync_channel(1);
        app.recovery.worker = Some(Worker {
            generation: app.recovery.generation,
            receiver,
        });
        tx.send(Ok(Outcome::Loaded {
            path: "/tmp/recovery-snapshot.ryolune".into(),
            session: Box::new(recovered),
            library: Library::new(),
            revision: app.store.revision,
        }))
        .unwrap();
        app.poll_recovery();
        assert_eq!(app.store.session().name, "Recovered song");
        assert!(app.path.is_none());
        // A snapshot is not a project: it must not be reopened at launch as if it were one.
        assert!(!app
            .settings
            .general
            .recent_sessions
            .iter()
            .any(|p| p.contains("recovery-snapshot")));
        assert!(app
            .settings
            .general
            .last_session
            .as_ref()
            .is_none_or(|p| !p.contains("recovery-snapshot")));
        assert!(app.store.dirty());
        assert!(!app.store.can_undo());
        app.dispatch(ryolune_engine::store::Command::Rename("Edit".into()));
        app.dispatch(ryolune_engine::store::Command::Undo);
        assert!(
            app.store.dirty(),
            "Undo cannot claim a recovered copy was saved"
        );
        app.request(crate::app::Intent::Quit);
        assert!(matches!(app.intent, Some(crate::app::Intent::Quit)));
        assert!(!app.closing);
        app.store.mark_saved(app.store.revision);
        assert!(!app.store.dirty());
    }

    #[test]
    fn the_recovery_timer_leaves_redo_and_gestures_alone() {
        let mut app = Ryolune::from_session(store::empty(), None);
        assert!(app.background_capture_allowed());
        app.dispatch(ryolune_engine::store::Command::Rename("Edit".into()));
        app.dispatch(ryolune_engine::store::Command::Undo);
        assert!(!app.background_capture_allowed(), "Redo would be lost");
        app.dispatch(ryolune_engine::store::Command::Redo);
        app.store.set_gesture(true);
        assert!(!app.background_capture_allowed(), "a drag would be split");
        app.store.set_gesture(false);
        assert!(app.background_capture_allowed());
    }

    #[test]
    fn recovery_lists_only_generated_regular_files_in_its_directory() {
        let root = temp();
        let generated = generated_path(&root, 1234, 1, "Song");
        fs::write(&generated, b"not loaded by directory listing").unwrap();
        fs::write(root.join("my-original.ryolune"), b"original").unwrap();
        assert_eq!(list(&root).unwrap().len(), 1);
        assert!(ensure_generated(&root, &generated).is_ok());
        assert!(ensure_generated(&root, &root.join("my-original.ryolune")).is_err());
        let other = root.join("other");
        fs::create_dir(&other).unwrap();
        assert!(ensure_generated(&other, &generated).is_err());
        #[cfg(unix)]
        {
            let link = generated_path(&root, 1234, 2, "Linked");
            std::os::unix::fs::symlink(&generated, &link).unwrap();
            assert!(ensure_generated(&root, &link).is_err());
            assert_eq!(list(&root).unwrap().len(), 1);
        }
        fs::remove_dir_all(root).unwrap();
    }
}
