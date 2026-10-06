//! The Settings window: every preference ryolune keeps, edited in place and applied at once.
//! The document of record is `engine::settings::Settings`; the window holds the live copy
//! the rest of the app reads and writes it back through `apply_settings`, which is also
//! what `settings.set` from the CLI, MCP and agents ends up calling.

use crate::app::Ryolune;
use ryolune_engine::{settings::Settings, Result};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

pub(crate) const SECTIONS: [&str; 10] = [
    "General",
    "Audio & MIDI",
    "Interface",
    "Agent",
    "Plugins",
    "Control",
    "Updates",
    "About",
    "Generation",
    "Diagnostics",
];
/// Settings sections by index. Generation came last (0.12), then Diagnostics, so earlier
/// indices keep their meaning; the window orders them itself.
pub(crate) const SECTION_KEYS: [&str; 10] = [
    "general",
    "audio",
    "interface",
    "agent",
    "plugins",
    "control",
    "updates",
    "about",
    "generation",
    "diagnostics",
];

#[derive(Default)]
pub(crate) struct SettingsWindow {
    pub open: bool,
    pub section: usize,
    pub notice: Option<(String, Instant)>,
    pub error: Option<String>,
    pub(crate) job: Option<CliJob>,
}
/// A vendor CLI run for sign-in or status checks; output shows in the pane.
pub(crate) struct CliJob {
    pub label: String,
    receiver: mpsc::Receiver<Result<String>>,
}

impl Ryolune {
    pub(crate) fn open_settings(&mut self, section: Option<usize>) {
        self.settings_ui.open = true;
        if let Some(section) = section {
            self.settings_ui.section = section.min(SECTIONS.len() - 1);
        }
    }

    /// Apply a new document: side effects for what changed, then persist. Errors leave the
    /// previous settings in force.
    pub(crate) fn apply_settings(&mut self, next: Settings) -> Result<()> {
        next.validate()?;
        let previous = self.settings.clone();
        next.save()?;
        self.settings = next.clone();
        ryolune_engine::diagnostics::set_secrets(next.secrets());
        if previous.audio.output_device != next.audio.output_device
            || previous.audio.buffer_frames != next.audio.buffer_frames
        {
            self.output_device = next.audio.output_device.clone();
            self.connect();
        }
        if previous.audio.input_device != next.audio.input_device {
            self.input_device = next.audio.input_device.clone();
        }
        if previous.audio.midi_input != next.audio.midi_input {
            match &next.audio.midi_input {
                Some(port) => self.connect_midi(Some(port.clone())),
                None => {
                    self.midi = None;
                    self.midi_port = None;
                }
            }
        }
        if previous.control.enable_bridge != next.control.enable_bridge {
            if next.control.enable_bridge {
                self.bridge_wanted = true;
            } else if self.control.take().is_some() {
                self.agents.stop_runner();
                self.status = "Local agent bridge disabled".into();
            }
        }
        Ok(())
    }

    /// Start the local bridge once it is wanted again (Settings > Control turned back on, or
    /// a CLI agent provider that needs it). Runs every tick, whichever interface is drawn.
    pub(crate) fn poll_bridge(&mut self) {
        self.poll_settings_job();
        if self.bridge_wanted && self.control.is_none() {
            self.bridge_wanted = false;
            if self.settings.control.enable_bridge {
                self.start_control();
            }
        }
    }

    /// Sign in to the Codex or Claude Code account the agent uses: the vendor CLI opens the
    /// browser and the job ends when the sign-in does (Settings > Agent shows the outcome).
    pub(crate) fn start_sign_in(&mut self) -> Result<()> {
        if self.settings_ui.job.is_some() {
            return Err("A sign-in is already running".into());
        }
        let agent = &self.settings.agent;
        let (exe, args): (PathBuf, &[&str]) = match agent.provider {
            ryolune_engine::settings::Provider::Codex => (
                crate::agent::discover_codex(&agent.codex_executable),
                &["login"],
            ),
            ryolune_engine::settings::Provider::Claude => (
                crate::agent::discover_claude(&agent.claude_executable),
                &["auth", "login"],
            ),
            _ => return Err("Sign-in is available for Codex and Claude Code".into()),
        };
        self.settings_ui.error = None;
        self.settings_ui.notice = None;
        self.start_settings_job("Sign-in".into(), exe, args);
        Ok(())
    }

    pub(crate) fn start_settings_job(&mut self, label: String, exe: PathBuf, args: &[&str]) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (tx, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = run_cli(&exe, &args, Duration::from_secs(240));
            let _ = tx.send(result);
        });
        self.settings_ui.job = Some(CliJob { label, receiver });
    }

    pub(crate) fn poll_settings_job(&mut self) {
        let Some(job) = &self.settings_ui.job else {
            return;
        };
        let outcome = match job.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("The command stopped unexpectedly".into()),
        };
        let label = job.label.clone();
        self.settings_ui.job = None;
        match outcome {
            Ok(output) => {
                self.settings_ui.error = None;
                self.settings_ui.notice =
                    Some((format!("{label}: {}", one_line(&output)), Instant::now()));
            }
            Err(e) => self.settings_ui.error = Some(format!("{label}: {}", one_line(&e))),
        }
    }
}

/// Run a vendor CLI to completion with a deadline, returning its combined output.
pub(crate) fn run_cli(exe: &Path, args: &[String], timeout: Duration) -> Result<String> {
    let mut command = Command::new(exe);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::agent::cli::group(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start {}: {e}", exe.display()))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out = std::thread::spawn(move || read_all(stdout));
    let err = std::thread::spawn(move || read_all(stderr));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Ok(None) => {
                crate::agent::cli::terminate_tree(&mut child);
                return Err("Timed out waiting for the command; finish the sign-in in the browser and check again".into());
            }
            Err(e) => {
                crate::agent::cli::terminate_tree(&mut child);
                return Err(e.to_string());
            }
        }
    };
    crate::agent::cli::terminate_tree(&mut child);
    let output = format!(
        "{}\n{}",
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default()
    )
    .trim()
    .to_string();
    if status.success() {
        Ok(if output.is_empty() {
            "done".into()
        } else {
            output
        })
    } else {
        Err(if output.is_empty() {
            format!("exited with {status}")
        } else {
            output
        })
    }
}
fn read_all(stream: Option<impl std::io::Read>) -> String {
    let mut bytes = Vec::new();
    if let Some(mut stream) = stream {
        let mut buffer = [0; 4096];
        while let Ok(count) = stream.read(&mut buffer) {
            if count == 0 {
                break;
            }
            let keep = count.min((64 * 1024_usize).saturating_sub(bytes.len()));
            bytes.extend_from_slice(&buffer[..keep]);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
fn one_line(text: &str) -> String {
    let line: String = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    if line.chars().count() > 160 {
        line.chars().take(157).collect::<String>() + "…"
    } else {
        line
    }
}
/// Show a folder in the system file manager.
pub(crate) fn reveal(path: &Path) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = Command::new(program).arg(path).spawn();
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn a_wanted_bridge_starts_from_the_tick_and_only_when_enabled() {
        // Never touch the person's own discovery file from a test.
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("RYOLUNE_CONTROL", dir.path().join("control.json"));
        let mut app = crate::app::Ryolune::from_session(ryolune_engine::store::empty(), None);
        app.control = None;
        app.settings.control.enable_bridge = false;
        app.bridge_wanted = true;
        app.poll_bridge();
        assert!(app.control.is_none(), "a disabled bridge stays off");
        assert!(!app.bridge_wanted);
        app.settings.control.enable_bridge = true;
        app.bridge_wanted = true;
        app.poll_bridge();
        assert!(
            app.control.is_some(),
            "turning the bridge back on starts it"
        );
        app.control = None;
    }

    #[test]
    fn cli_output_capture_is_bounded_but_drains_the_pipe() {
        let bytes = vec![b'x'; 200_000];
        let mut stream = std::io::Cursor::new(bytes);
        assert_eq!(read_all(Some(&mut stream)).len(), 64 * 1024);
        assert_eq!(stream.position(), 200_000);
    }

    #[cfg(unix)]
    #[test]
    fn cli_timeout_terminates_the_owned_process_group() {
        let started = Instant::now();
        let result = run_cli(
            Path::new("/bin/sh"),
            &["-c".into(), "sleep 30 & wait".into()],
            Duration::from_millis(100),
        );
        assert!(result.unwrap_err().contains("Timed out"));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn successful_cli_exit_does_not_wait_on_descendants_holding_stdout() {
        let started = Instant::now();
        let result = run_cli(
            Path::new("/bin/sh"),
            &["-c".into(), "sleep 30 & printf connected".into()],
            Duration::from_secs(2),
        );
        assert_eq!(result.unwrap(), "connected");
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
