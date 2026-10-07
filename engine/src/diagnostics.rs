//! Logs and crash reports, kept in the data directory (`host::scan::data_dir`):
//!
//! * `logs/ryolune.log` is this run's log; `ryolune.1.log` the previous run's, up to
//!   `ryolune.3.log`. A log that passes [`MAX_LOG`] while running rolls over too, so the folder
//!   stays bounded. Every line is also echoed to stderr, as `eprintln!` did before.
//! * `crashes/crash-<time>-<pid>.txt`: a panic that nothing caught, written by the panic hook
//!   with the message, the thread, a backtrace, the system and the last lines of the log.
//!   `recovered-<time>-<pid>.txt`: a panic a background job caught ([`catch`]); ryolune kept
//!   running, but it is a bug worth reporting. `unclean-<time>-<pid>.txt`: a run that ended
//!   without quitting (a crash outside Rust, a forced quit, the computer stopping), noticed
//!   on the next start from the `logs/running-<pid>.json` marker it left behind.
//!
//! The window installs all of this ([`init`]) and removes its marker on a clean quit
//! ([`clean_exit`]). The CLI and MCP server only read the files (`app.logs`,
//! `app.crashReports`, `app.diagnostics`). Nothing leaves the computer: reports are files the
//! person reads, copies or attaches to an issue themselves. Secrets are never written: the
//! settings' API keys are masked in every line ([`set_secrets`]), and the audio callback never
//! logs.

use serde::{Deserialize, Serialize};
use std::{
    cell::Cell,
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex, OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};

/// The log's file name, without `.log`.
pub const LOG_NAME: &str = "ryolune";
/// A log file rolls over past this size.
pub const MAX_LOG: u64 = 8 << 20;
/// Older logs kept: `ryolune.1.log` … `ryolune.3.log`.
const KEEP_LOGS: usize = 3;
/// Crash reports kept; the oldest go first.
const KEEP_REPORTS: usize = 25;
/// Log lines kept in memory for crash reports.
const RECENT_LINES: usize = 400;
/// Reports one run writes at most, so a panic that repeats cannot fill the disk.
const REPORTS_PER_RUN: usize = 10;
/// The repository issues are filed in.
pub const ISSUES_URL: &str = "https://github.com/ludovic111/ryolune/issues/new";

pub fn logs_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("logs")
}
pub fn crashes_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("crashes")
}
fn log_path(dir: &Path, n: usize) -> PathBuf {
    if n == 0 {
        dir.join(format!("{LOG_NAME}.log"))
    } else {
        dir.join(format!("{LOG_NAME}.{n}.log"))
    }
}
fn marker_path(logs: &Path, pid: u32) -> PathBuf {
    logs.join(format!("running-{pid}.json"))
}

struct State {
    logs: PathBuf,
}
static STATE: OnceLock<State> = OnceLock::new();
/// The open log file and its size.
static FILE: Mutex<Option<(File, u64)>> = Mutex::new(None);
/// The last lines written, for crash reports.
static RECENT: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
/// Text never written to the log: the API keys in the settings.
static SECRETS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Secrets that do not come from the settings (the lsuite account token): `set_secrets`
/// keeps them.
static KEPT: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Reports this run wrote, and what they were about.
static WRITTEN: AtomicUsize = AtomicUsize::new(0);
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

thread_local! {
    /// The job [`catch`] is running on this thread: a panic in it is recovered, not a crash.
    static GUARD: Cell<Option<&'static str>> = const { Cell::new(None) };
}

/// What [`init`] found about the previous run.
#[derive(Debug, Clone, Default)]
pub struct Started {
    /// Reports written for runs that ended without quitting.
    pub unclean: Vec<PathBuf>,
}

/// Start logging to `<data>/logs/ryolune.log` (and stderr), install the panic hook and mark
/// this process as running, after turning what a previous run left behind into reports.
/// Call it once, first thing in the window process; later calls do nothing.
pub fn init(data_dir: &Path) -> Started {
    let logs = logs_dir(data_dir);
    let _ = fs::create_dir_all(&logs);
    let _ = fs::create_dir_all(crashes_dir(data_dir));
    if STATE.get().is_some() {
        return Started::default();
    }
    let started = Started {
        unclean: collect_unclean(data_dir),
    };
    rotate(&logs);
    if let Ok(file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path(&logs, 0))
    {
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        *lock(&FILE) = Some((file, len));
    }
    let level = level_from_env();
    if log::set_boxed_logger(Box::new(Logger { level })).is_err() {
        return started;
    }
    log::set_max_level(level.max(log::LevelFilter::Warn));
    let _ = STATE.set(State { logs: logs.clone() });
    install_panic_hook(data_dir.to_path_buf());
    let marker = Marker {
        pid: std::process::id(),
        version: env!("CARGO_PKG_VERSION").into(),
        started: crate::lsuite::now_rfc3339(),
    };
    if let Ok(json) = serde_json::to_vec_pretty(&marker) {
        let _ = fs::write(marker_path(&logs, marker.pid), json);
    }
    log::info!(
        "ryolune {} ({}) on {} ({}), pid {}",
        env!("CARGO_PKG_VERSION"),
        build(),
        os_name(),
        std::env::consts::ARCH,
        std::process::id()
    );
    for report in &started.unclean {
        log::warn!(
            "the previous run did not quit properly; report: {}",
            report.display()
        );
    }
    started
}

/// Note a clean quit: removes this process's running marker.
pub fn clean_exit() {
    if let Some(state) = STATE.get() {
        if fs::remove_file(marker_path(&state.logs, std::process::id())).is_ok() {
            log::info!("ryolune quit");
        }
    }
}

/// The settings' secrets, masked in every log line from now on.
pub fn set_secrets(secrets: Vec<String>) {
    let kept = lock(&KEPT).clone();
    *lock(&SECRETS) = secrets
        .into_iter()
        .chain(kept)
        .map(|s| s.trim().to_string())
        .filter(|s| s.len() >= 6)
        .collect();
}

/// Mask one more secret in every log line from now on (the lsuite account token).
pub fn add_secret(secret: &str) {
    let secret = secret.trim().to_string();
    if secret.len() < 6 {
        return;
    }
    let mut kept = lock(&KEPT);
    if !kept.contains(&secret) {
        kept.push(secret.clone());
        lock(&SECRETS).push(secret);
    }
}

/// A warning for the log when the window logs, else for stderr (the CLI, the MCP server).
pub fn warn(message: &str) {
    if STATE.get().is_some() {
        log::warn!("{message}");
    } else {
        eprintln!("{message}");
    }
}

/// `catch_unwind` for a background job: a panic in `work` is written as a `recovered`
/// report (the job failed, ryolune kept running) instead of a crash.
pub fn catch<R>(what: &'static str, work: impl FnOnce() -> R) -> std::thread::Result<R> {
    let previous = GUARD.with(|g| g.replace(Some(what)));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
    GUARD.with(|g| g.set(previous));
    if result.is_err() {
        log::error!("{what} panicked; the job was stopped and ryolune kept running");
    }
    result
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn level_from_env() -> log::LevelFilter {
    std::env::var("RYOLUNE_LOG")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(log::LevelFilter::Info)
}

fn build() -> &'static str {
    if cfg!(debug_assertions) {
        "debug build"
    } else {
        "release build"
    }
}

// ---- the log -------------------------------------------------------------------

struct Logger {
    /// The level for ryolune's own crates; other crates (GPUI, the audio stack) log warnings
    /// and errors only.
    level: log::LevelFilter,
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let ours = metadata.target().starts_with("ryolune");
        metadata.level()
            <= if ours {
                self.level
            } else {
                log::LevelFilter::Warn
            }
    }
    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} {}: {}",
            timestamp(),
            record.level(),
            record.target(),
            record.args()
        );
        write_line(&line, true);
    }
    fn flush(&self) {
        if let Some((file, _)) = lock(&FILE).as_mut() {
            let _ = file.flush();
        }
    }
}

/// Mask, echo to stderr, keep in memory and append to the file. The panic hook passes
/// `wait = false` so a panic inside the logger cannot deadlock it.
fn write_line(line: &str, wait: bool) {
    let secrets = if wait {
        Some(lock(&SECRETS))
    } else {
        SECRETS.try_lock().ok()
    };
    // A secret list that cannot be read could leave a key unmasked: drop the line.
    let Some(secrets) = secrets else { return };
    let line = mask(line, &secrets);
    drop(secrets);
    let _ = writeln!(io::stderr(), "{line}");
    if let Some(mut recent) = if wait {
        Some(lock(&RECENT))
    } else {
        RECENT.try_lock().ok()
    } {
        for l in line.lines().filter(|l| !l.trim().is_empty()) {
            if recent.len() == RECENT_LINES {
                recent.pop_front();
            }
            recent.push_back(l.to_string());
        }
    }
    let Some(state) = STATE.get() else { return };
    let Some(mut file) = (if wait {
        Some(lock(&FILE))
    } else {
        FILE.try_lock().ok()
    }) else {
        return;
    };
    let bytes = line.len() as u64 + 1;
    if file.as_ref().is_some_and(|(_, len)| len + bytes > MAX_LOG) {
        *file = None;
        rotate(&state.logs);
        if let Ok(f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path(&state.logs, 0))
        {
            *file = Some((f, 0));
        }
    }
    if let Some((f, len)) = file.as_mut() {
        if writeln!(f, "{line}").is_ok() {
            *len += bytes;
        }
    }
}

fn mask(line: &str, secrets: &[String]) -> String {
    let mut out = line.to_string();
    for secret in secrets {
        if out.contains(secret.as_str()) {
            out = out.replace(secret.as_str(), "••••");
        }
    }
    out
}

/// `2026-10-06T12:00:00.123Z`.
fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let base = crate::lsuite::rfc3339(now.as_secs() as i64);
    format!("{}.{:03}Z", base.trim_end_matches('Z'), now.subsec_millis())
}

/// `20261006-120000`, for file names.
fn file_stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    crate::lsuite::rfc3339(secs as i64)
        .trim_end_matches('Z')
        .replace(['-', ':'], "")
        .replace('T', "-")
}

/// `ryolune.log` → `ryolune.1.log` → … → dropped after [`KEEP_LOGS`].
fn rotate(dir: &Path) {
    let _ = fs::remove_file(log_path(dir, KEEP_LOGS));
    for n in (0..KEEP_LOGS).rev() {
        let from = log_path(dir, n);
        if from.exists() {
            let _ = fs::rename(&from, log_path(dir, n + 1));
        }
    }
}

/// The last `n` lines this run logged (from memory).
pub fn recent_lines(n: usize) -> Vec<String> {
    let recent = lock(&RECENT);
    recent
        .iter()
        .skip(recent.len().saturating_sub(n))
        .cloned()
        .collect()
}

/// The last `n` lines of a file, without reading all of a big one.
pub fn tail(path: &Path, n: usize) -> io::Result<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub((n as u64).saturating_mul(400).max(64 << 10));
    f.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() {
        // Probably cut in half.
        lines.remove(0);
    }
    Ok(lines[lines.len().saturating_sub(n)..]
        .iter()
        .map(|l| l.to_string())
        .collect())
}

/// This process's log file, when it logs to one.
pub fn current_log() -> Option<PathBuf> {
    STATE.get().map(|state| log_path(&state.logs, 0))
}

/// The log files, newest first: `(path, bytes)`.
pub fn log_files(data_dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut out: Vec<(String, PathBuf, u64)> = fs::read_dir(logs_dir(data_dir))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let meta = e.metadata().ok()?;
            let name = path.file_name()?.to_str()?.to_string();
            (meta.is_file() && name.ends_with(".log")).then_some((name, path, meta.len()))
        })
        .collect();
    // ryolune.log, then ryolune.1.log, ryolune.2.log…: newest first by construction.
    out.sort_by_key(|(name, _, _)| {
        let n = name
            .trim_end_matches(".log")
            .rsplit('.')
            .next()
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(0);
        (n, name.clone())
    });
    out.into_iter().map(|(_, path, len)| (path, len)).collect()
}

// ---- crashes -------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    pid: u32,
    version: String,
    started: String,
}

/// Markers left by processes that are gone: a report for each (unless the panic hook already
/// wrote one for that process), and the marker is removed.
fn collect_unclean(data_dir: &Path) -> Vec<PathBuf> {
    let logs = logs_dir(data_dir);
    let mut reports = vec![];
    let Ok(entries) = fs::read_dir(&logs) else {
        return reports;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(pid) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("running-")?.strip_suffix(".json"))
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == std::process::id() || pid_alive(pid) {
            continue;
        }
        let marker: Option<Marker> = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        let _ = fs::remove_file(&path);
        if let Some(existing) = report_for_pid(data_dir, pid) {
            reports.push(existing);
            continue;
        }
        // That run's log is still `ryolune.log`: this run rotates it afterwards.
        let log = tail(&log_path(&logs, 0), 200).unwrap_or_default();
        let started = marker.as_ref().map_or_else(
            || "unknown".to_string(),
            |m| format!("{} (ryolune {})", m.started, m.version),
        );
        let last = log
            .iter()
            .rev()
            .map(|l| last_words(l))
            .find(|l| !l.is_empty())
            .unwrap_or_else(|| "nothing in its log".into());
        let body = format!(
            "ryolune did not quit properly\n\nLast in its log: {last}\n\nThe process (pid {pid}, started {started}) ended without quitting: a crash outside Rust (an audio driver, a plugin, the system), a forced quit, or the computer stopping. Recovery snapshots of an edited session are in File › Recover Session.\n\n{}\n\n---- last lines of its log ----\n{}\n",
            system_summary(),
            log.join("\n")
        );
        if let Some(report) = write_report(data_dir, "unclean", pid, &body) {
            reports.push(report);
        }
    }
    reports
}

/// A log line without its timestamp (`2026-…Z WARN target: message` → `WARN target: message`).
fn last_words(line: &str) -> String {
    let l = line.trim();
    let l = match l.split_once(char::is_whitespace) {
        Some((first, rest)) if first.len() > 18 && first.as_bytes().get(4) == Some(&b'-') => {
            rest.trim_start()
        }
        _ => l,
    };
    l.chars().take(240).collect()
}

fn report_for_pid(data_dir: &Path, pid: u32) -> Option<PathBuf> {
    let suffix = format!("-{pid}.txt");
    fs::read_dir(crashes_dir(data_dir))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("crash-") && n.ends_with(&suffix))
        })
}

#[cfg(target_os = "linux")]
fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}
#[cfg(all(unix, not(target_os = "linux")))]
fn pid_alive(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
}
#[cfg(not(any(unix, windows)))]
fn pid_alive(_pid: u32) -> bool {
    false
}

fn write_report(data_dir: &Path, kind: &str, pid: u32, body: &str) -> Option<PathBuf> {
    let dir = crashes_dir(data_dir);
    fs::create_dir_all(&dir).ok()?;
    let mut path = dir.join(format!("{kind}-{}-{pid}.txt", file_stamp()));
    // Two reports in one second from one process (a repeated job panic).
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{kind}-{}-{pid}-{n}.txt", file_stamp()));
        n += 1;
    }
    fs::write(&path, body).ok()?;
    prune_reports(&dir);
    Some(path)
}

fn prune_reports(dir: &Path) {
    let mut all: Vec<(SystemTime, PathBuf)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if all.len() > KEEP_REPORTS {
        all.sort();
        for (_, path) in &all[..all.len() - KEEP_REPORTS] {
            let _ = fs::remove_file(path);
        }
    }
}

/// Whether this run may write another report about `key` (the place and the message).
fn may_write(key: &str) -> bool {
    let Ok(mut seen) = SEEN.try_lock() else {
        return false;
    };
    if seen.iter().any(|k| k == key) || WRITTEN.load(Ordering::Relaxed) >= REPORTS_PER_RUN {
        return false;
    }
    seen.push(key.to_string());
    WRITTEN.fetch_add(1, Ordering::Relaxed);
    true
}

fn payload_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".into())
}

/// The text of a panic report. `job` is the background job that caught it, if one did.
fn panic_report(
    thread: &str,
    location: &str,
    message: &str,
    job: Option<&str>,
    backtrace: &str,
    log: &[String],
) -> String {
    let (title, outcome) = match job {
        Some(job) => (
            "ryolune recovered from a panic",
            format!("The panic stopped one background job ({job}); ryolune kept running. It is still a bug worth reporting."),
        ),
        None => (
            "ryolune crash report",
            "Nothing caught this panic: ryolune or one of its threads stopped.".to_string(),
        ),
    };
    format!(
        "{title}\n\nPanic on thread '{thread}' at {location}:\n{message}\n\n{outcome}\n\n{}\n\n---- backtrace ----\n{backtrace}\n\n---- last lines of the log ----\n{}\n",
        system_summary(),
        log.join("\n")
    )
}

fn install_panic_hook(data_dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let job = GUARD.try_with(|g| g.get()).ok().flatten();
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("unnamed").to_string();
        let message = payload_text(info.payload());
        let location = info.location().map_or_else(
            || "unknown".to_string(),
            |l| format!("{}:{}:{}", l.file(), l.line(), l.column()),
        );
        write_line(
            &format!(
                "{} ERROR ryolune::panic: panic on thread '{thread}' at {location}: {message}",
                timestamp()
            ),
            false,
        );
        if may_write(&format!("{location} {message}")) {
            let backtrace = std::backtrace::Backtrace::force_capture().to_string();
            let log = RECENT
                .try_lock()
                .map(|recent| {
                    recent
                        .iter()
                        .skip(recent.len().saturating_sub(150))
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let body = panic_report(&thread, &location, &message, job, &backtrace, &log);
            let body = match SECRETS.try_lock() {
                Ok(secrets) => mask(&body, &secrets),
                Err(_) => body,
            };
            let kind = if job.is_some() { "recovered" } else { "crash" };
            if let Some(path) = write_report(&data_dir, kind, std::process::id(), &body) {
                write_line(
                    &format!(
                        "{} ERROR ryolune::panic: report written to {}",
                        timestamp(),
                        path.display()
                    ),
                    false,
                );
            }
        }
        previous(info);
    }));
}

/// Version, build, system, architecture: the lines every report starts with.
pub fn system_summary() -> String {
    format!(
        "ryolune {} ({})\nSystem: {} ({} {})\nTime: {}\nExecutable: {}",
        env!("CARGO_PKG_VERSION"),
        build(),
        os_name(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        crate::lsuite::now_rfc3339(),
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    )
}

/// A readable system name and version (`macOS 15.1`, `Ubuntu 24.04.1 LTS`, `Windows 10.0.26100`).
pub fn os_name() -> String {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(system_name).clone()
}

#[cfg(target_os = "macos")]
fn system_name() -> String {
    let version = std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    format!("macOS {}", version.unwrap_or_default())
        .trim()
        .to_string()
}
#[cfg(target_os = "linux")]
fn system_name() -> String {
    let pretty = fs::read_to_string("/etc/os-release").ok().and_then(|t| {
        t.lines().find_map(|l| {
            l.strip_prefix("PRETTY_NAME=")
                .map(|v| v.trim_matches('"').to_string())
        })
    });
    let session = std::env::var("XDG_SESSION_TYPE")
        .ok()
        .filter(|s| !s.is_empty());
    match (pretty, session) {
        (Some(p), Some(s)) => format!("{p}, {s}"),
        (Some(p), None) => p,
        (None, _) => "Linux".into(),
    }
}
#[cfg(windows)]
fn system_name() -> String {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("cmd")
        .args(["/C", "ver"])
        .creation_flags(0x0800_0000)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "Windows".into())
}
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn system_name() -> String {
    std::env::consts::OS.to_string()
}

// ---- listing -------------------------------------------------------------------

/// One crash report, as `app.crashReports` lists them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// The file name (`crash-20261006-101500-1234.txt`).
    pub id: String,
    pub path: PathBuf,
    /// `crash` (a panic nothing caught), `recovered` (a background job's panic, ryolune kept
    /// running) or `unclean` (the process ended without quitting).
    pub kind: String,
    /// When it was written, RFC 3339 in UTC.
    pub at: String,
    /// The line that says what happened.
    pub summary: String,
}

/// Reports, newest first.
pub fn reports(data_dir: &Path) -> Vec<Report> {
    let mut out: Vec<(SystemTime, Report)> = fs::read_dir(crashes_dir(data_dir))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let id = path.file_name()?.to_str()?.to_string();
            if !id.ends_with(".txt") {
                return None;
            }
            let kind = id.split('-').next()?.to_string();
            let modified = e.metadata().ok()?.modified().ok()?;
            let secs = modified
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let text = tail_text(&path);
            Some((
                modified,
                Report {
                    summary: summarize(&text),
                    id,
                    path,
                    kind,
                    at: crate::lsuite::rfc3339(secs as i64),
                },
            ))
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.id.cmp(&a.1.id)));
    out.into_iter().map(|(_, r)| r).collect()
}

/// The start of a report, enough to summarize it.
fn tail_text(path: &Path) -> String {
    use std::io::Read;
    let mut text = String::new();
    if let Ok(f) = File::open(path) {
        let _ = f.take(4096).read_to_string(&mut text);
    }
    text
}

fn summarize(text: &str) -> String {
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = lines.next().unwrap_or_default();
    if first.starts_with("ryolune crash report") || first.starts_with("ryolune recovered") {
        // "Panic on thread … at file:line:" then the message.
        let _where = lines.next();
        return lines
            .next()
            .unwrap_or("A panic")
            .chars()
            .take(240)
            .collect();
    }
    lines
        .next()
        .map(|l| l.chars().take(240).collect())
        .unwrap_or_else(|| first.to_string())
}

/// One report's text, by file name (`id` must name a report in the crashes folder).
pub fn read_report(data_dir: &Path, id: &str) -> Result<String, String> {
    if id.contains(['/', '\\']) || id.starts_with('.') || !id.ends_with(".txt") {
        return Err(format!("`{id}` is not a crash report name."));
    }
    fs::read_to_string(crashes_dir(data_dir).join(id))
        .map_err(|_| format!("No crash report named `{id}`. app.crashReports lists them."))
}

/// Delete every crash report; returns how many.
pub fn clear_reports(data_dir: &Path) -> usize {
    let mut n = 0;
    for entry in fs::read_dir(crashes_dir(data_dir))
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "txt") && fs::remove_file(&path).is_ok() {
            n += 1;
        }
    }
    n
}

/// A new GitHub issue (the bug report form) with the version, the system and the last
/// crash's summary filled in. Opening it sends nothing: the person reads, edits and submits.
pub fn issue_url(data_dir: &Path) -> String {
    let mut url = format!(
        "{ISSUES_URL}?template=bug_report.yml&version={}&system={}",
        url_encode(env!("CARGO_PKG_VERSION")),
        url_encode(&format!("{} ({})", os_name(), std::env::consts::ARCH)),
    );
    if let Some(last) = reports(data_dir).into_iter().next() {
        url.push_str("&crash=");
        url.push_str(&url_encode(&format!(
            "Last report ({}, {}): {}",
            last.kind, last.at, last.summary
        )));
    }
    url
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ryolune-diagnostics-{name}-{}-{}",
            std::process::id(),
            crate::control::new_id("t")
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn logs_roll_over_and_keep_three() {
        let dir = scratch("rotate");
        for i in 0..6 {
            fs::write(log_path(&dir, 0), format!("run {i}")).unwrap();
            rotate(&dir);
        }
        assert!(!log_path(&dir, 0).exists());
        assert_eq!(fs::read_to_string(log_path(&dir, 1)).unwrap(), "run 5");
        assert_eq!(fs::read_to_string(log_path(&dir, 3)).unwrap(), "run 3");
        assert!(!log_path(&dir, 4).exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn tail_reads_the_last_lines() {
        let dir = scratch("tail");
        let p = dir.join("a.log");
        fs::write(
            &p,
            (0..1000).map(|i| format!("line {i}\n")).collect::<String>(),
        )
        .unwrap();
        assert_eq!(tail(&p, 2).unwrap(), vec!["line 998", "line 999"]);
        assert!(tail(&dir.join("missing.log"), 2).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn log_files_list_this_run_first() {
        let data = scratch("files");
        let logs = logs_dir(&data);
        fs::create_dir_all(&logs).unwrap();
        for n in [2, 0, 1] {
            fs::write(log_path(&logs, n), "x").unwrap();
        }
        fs::write(logs.join("running-1.json"), "{}").unwrap();
        let names: Vec<String> = log_files(&data)
            .into_iter()
            .map(|(p, _)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["ryolune.log", "ryolune.1.log", "ryolune.2.log"]);
        let _ = fs::remove_dir_all(data);
    }

    #[test]
    fn a_marker_left_by_a_dead_process_becomes_a_report() {
        let data = scratch("unclean");
        let logs = logs_dir(&data);
        fs::create_dir_all(&logs).unwrap();
        fs::write(
            log_path(&logs, 0),
            "2026-10-06T10:00:00.000Z INFO  ryolune: starting\n2026-10-06T10:00:01.000Z WARN  ryolune::app: last words\n",
        )
        .unwrap();
        // No process has this pid.
        let pid = u32::MAX - 1;
        let marker = Marker {
            pid,
            version: "0.13.0".into(),
            started: "2026-10-06T10:00:00Z".into(),
        };
        fs::write(
            marker_path(&logs, pid),
            serde_json::to_vec(&marker).unwrap(),
        )
        .unwrap();
        // A live process (this one) is left alone.
        fs::write(marker_path(&logs, std::process::id()), "{}").unwrap();
        let found = collect_unclean(&data);
        assert_eq!(found.len(), 1);
        let text = fs::read_to_string(&found[0]).unwrap();
        assert!(
            text.contains("did not quit properly") && text.contains("last words"),
            "{text}"
        );
        assert_eq!(
            summarize(&text),
            "Last in its log: WARN  ryolune::app: last words"
        );
        assert!(!marker_path(&logs, pid).exists());
        assert!(marker_path(&logs, std::process::id()).exists());
        let listed = reports(&data);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].kind, "unclean");
        assert!(listed[0].id.ends_with(&format!("-{pid}.txt")));
        assert!(read_report(&data, &listed[0].id).is_ok());
        assert!(read_report(&data, "../settings.json").is_err());
        assert!(read_report(&data, "missing.txt").is_err());
        assert!(issue_url(&data).contains("&crash=Last%20report%20%28unclean"));
        assert_eq!(clear_reports(&data), 1);
        assert!(reports(&data).is_empty());
        let _ = fs::remove_dir_all(data);
    }

    #[test]
    fn panic_reports_say_whether_ryolune_kept_running() {
        let crash = panic_report(
            "main",
            "src/a.rs:1:2",
            "index out of bounds",
            None,
            "bt",
            &["a line".into()],
        );
        assert_eq!(summarize(&crash), "index out of bounds");
        assert!(crash.starts_with("ryolune crash report"));
        assert!(crash.contains("a line") && crash.contains("---- backtrace ----\nbt"));
        let recovered = panic_report("w", "src/b.rs:3:4", "boom", Some("plugin scan"), "", &[]);
        assert!(recovered.starts_with("ryolune recovered from a panic"));
        assert!(recovered.contains("(plugin scan); ryolune kept running"));
        assert_eq!(summarize(&recovered), "boom");
    }

    #[test]
    fn secrets_are_masked_and_issue_links_are_encoded() {
        let secrets = vec!["sk-test-123456".to_string()];
        assert_eq!(
            mask("key sk-test-123456 refused", &secrets),
            "key •••• refused"
        );
        assert_eq!(url_encode("a b&c/é"), "a%20b%26c%2F%C3%A9");
        let url = issue_url(&scratch("issue"));
        assert!(url.starts_with(
            "https://github.com/ludovic111/ryolune/issues/new?template=bug_report.yml&version="
        ));
        assert!(!url.contains("&crash="));
        assert_eq!(
            last_words("2026-10-06T10:00:01.000Z WARN x: y"),
            "WARN x: y"
        );
    }

    #[test]
    fn catch_returns_the_panic_and_restores_the_guard() {
        let result = catch("test job", || -> u8 { panic!("boom") });
        assert!(result.is_err());
        assert_eq!(GUARD.with(|g| g.get()), None);
        assert_eq!(catch("ok", || 3).unwrap(), 3);
    }
}
