//! The window's side of diagnostics and of the first start after an update: the log, crash
//! reports and the running marker are `ryolune_engine::diagnostics` (installed in `main`);
//! here the host says when a previous run did not quit properly, opens What's New once after
//! an update (`general.lastRunVersion`), and logs the errors the window shows.

use crate::app::Ryolune;
use ryolune_engine::release_notes::{self, CURRENT};
use std::path::PathBuf;

/// The What's New sheet: the releases after `since` (after an update), or this version's;
/// `all` lists every release this copy carries.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct WhatsNew {
    pub since: Option<String>,
    pub all: bool,
}

impl WhatsNew {
    /// What the sheet lists, newest first.
    pub fn releases(&self) -> Vec<release_notes::Release> {
        if self.all {
            return release_notes::all();
        }
        let shown = self
            .since
            .as_deref()
            .map(release_notes::since)
            .unwrap_or_default();
        if shown.is_empty() {
            release_notes::find(CURRENT).into_iter().collect()
        } else {
            shown
        }
    }
}

impl Ryolune {
    /// Reports `diagnostics::init` wrote for runs that ended without quitting: say so once,
    /// in the status line (recovery snapshots of an edited song are in File › Recover).
    pub(crate) fn previous_run_ended(&mut self, unclean: &[PathBuf]) {
        if !unclean.is_empty() {
            self.status = "ryolune did not quit properly last time. Settings › Diagnostics has a report; File › Recover Session has snapshots.".into();
        }
    }

    /// Remember the version that runs now, and open What's New when it is new to this
    /// profile (see `release_notes::unseen`). `existing_profile`: the settings file was there
    /// before this start.
    pub(crate) fn note_version(&mut self, existing_profile: bool) {
        let last = self.settings.general.last_run_version.clone();
        if let Some(unseen) = release_notes::unseen(last.as_deref(), existing_profile) {
            log::info!(
                "first start of {CURRENT} (last run: {}); showing What's New",
                last.as_deref().unwrap_or("unknown")
            );
            self.whats_new = Some(WhatsNew {
                since: unseen.since,
                all: false,
            });
        }
        if last.as_deref() != Some(CURRENT) {
            let mut next = self.settings.clone();
            next.general.last_run_version = Some(CURRENT.into());
            if let Err(error) = self.apply_settings(next) {
                log::warn!("could not record the version that ran: {error}");
            }
        }
    }

    /// Debug builds only: `RYOLUNE_CRASH_TEST=main` panics on the interface thread and
    /// `=worker` in a background job, to check the crash and recovered reports.
    pub(crate) fn crash_test(&mut self) {
        if !cfg!(debug_assertions) {
            return;
        }
        match std::env::var("RYOLUNE_CRASH_TEST").as_deref() {
            Ok("main") => panic!("RYOLUNE_CRASH_TEST=main: a test panic on the interface thread"),
            Ok("worker") => {
                std::thread::spawn(|| {
                    let _ = ryolune_engine::diagnostics::catch("crash test", || {
                        panic!("RYOLUNE_CRASH_TEST=worker: a test panic in a background job")
                    });
                });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whats_new_lists_the_releases_since_the_last_run() {
        let current = WhatsNew::default().releases();
        assert_eq!(current.len(), 1);
        assert_eq!(current[0].version, CURRENT);
        let since = WhatsNew {
            since: Some("0.11.0".into()),
            all: false,
        }
        .releases();
        assert!(since.len() >= 2 && since.iter().all(|r| r.version != "0.11.0"));
        let all = WhatsNew {
            since: None,
            all: true,
        }
        .releases();
        assert!(all.len() > since.len());
        assert_eq!(all.last().map(|r| r.version.as_str()), Some("0.2.0"));
    }

    #[test]
    fn a_new_version_opens_whats_new_once() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("RYOLUNE_SETTINGS", dir.path().join("settings.json"));
        let mut app = Ryolune::from_session(ryolune_engine::store::empty(), None);
        app.settings.general.last_run_version = Some("0.11.0".into());
        app.note_version(true);
        assert_eq!(
            app.whats_new,
            Some(WhatsNew {
                since: Some("0.11.0".into()),
                all: false
            })
        );
        assert_eq!(
            app.settings.general.last_run_version.as_deref(),
            Some(CURRENT)
        );
        app.whats_new = None;
        app.note_version(true);
        assert_eq!(app.whats_new, None, "only once");
        // A fresh profile starts quietly.
        let mut fresh = Ryolune::from_session(ryolune_engine::store::empty(), None);
        fresh.note_version(false);
        assert_eq!(fresh.whats_new, None);
        assert_eq!(
            fresh.settings.general.last_run_version.as_deref(),
            Some(CURRENT)
        );
    }
}
