//! Updates from GitHub Releases. The check and the download run on a worker thread and the
//! interface only reads their results between frames; nothing here touches the audio callback.
//!
//! A release is published by `.github/workflows/release.yml` from a `vX.Y.Z` tag. It carries one
//! asset per platform, a `SHA256SUMS` file and, when the workflow holds the signing key, a
//! `SHA256SUMS.sig` Ed25519 signature. The app compares the latest tag with its own version,
//! checks that every URL belongs to this repository's releases, verifies the signature against
//! the public key compiled into `assets/update-signing.pub`, downloads its asset, verifies the
//! checksum, checks the new binaries report the expected version, swaps the installed copy in
//! place and relaunches. Set `RYOLUNE_PRETEND_VERSION=0.0.1` to exercise the flow against a real
//! release.

use crate::app::Ryolune;
use crate::control::LiveWait;
use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use ryolune_engine::Result;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

pub const REPO: &str = "ludovic111/ryolune";
const API: &str = "https://api.github.com/repos";
const CHECKSUMS: &str = "SHA256SUMS";
const SIGNATURE: &str = "SHA256SUMS.sig";
const SIGNATURE_PREFIX: &str = "ryolune-ed25519";
const MAX_ASSET: u64 = 512 * 1024 * 1024;
/// Hex public key; empty when no release key pair has been generated yet.
const PUBLIC_KEY_HEX: &str = include_str!("../assets/update-signing.pub");

/// The compiled-in public key, when one is configured.
pub fn public_key() -> Option<[u8; 32]> {
    let hex: String = PUBLIC_KEY_HEX
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    if hex.len() != 64 {
        return None;
    }
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(key)
}
/// Check `SHA256SUMS.sig` against the built-in key.
pub fn verify_signature(message: &[u8], signature_text: &str) -> Result<()> {
    let key = public_key().ok_or("This build has no release signing key")?;
    let line = signature_text
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with(SIGNATURE_PREFIX))
        .ok_or("The release signature has an unknown format")?;
    let encoded = line[SIGNATURE_PREFIX.len()..].trim();
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|e| format!("The release signature is not valid base64: {e}"))?;
    let signature = Signature::from_slice(&bytes)
        .map_err(|_| "The release signature has the wrong length".to_string())?;
    let verifying = VerifyingKey::from_bytes(&key)
        .map_err(|_| "The built-in release key is invalid".to_string())?;
    verifying
        .verify(message, &signature)
        .map_err(|_| "The release signature does not match SHA256SUMS; nothing was changed".into())
}
fn read_secret_key(path: &Path) -> Result<SigningKey> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let hex: String = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    if hex.len() != 64 {
        return Err("The secret key file must hold 64 hex characters".into());
    }
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| "The secret key is not hex".to_string())?;
    }
    Ok(SigningKey::from_bytes(&key))
}
/// Create a signing key pair: the secret goes to `path` (0600), the public key is returned.
pub fn write_keypair(path: &Path) -> Result<String> {
    let signing = SigningKey::generate(&mut rand_core::OsRng);
    if path.exists() {
        return Err(format!(
            "{} exists; refusing to overwrite a signing key",
            path.display()
        ));
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    writeln!(
        file,
        "# ryolune release signing secret key. Keep private.\n{}",
        hex(&signing.to_bytes())
    )
    .map_err(|e| e.to_string())?;
    Ok(hex(signing.verifying_key().as_bytes()))
}
/// Sign a file with the secret key, writing `<file>.sig`.
pub fn sign_file(key: &Path, file: &Path) -> Result<PathBuf> {
    let signing = read_secret_key(key)?;
    let message = fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let signature = signing.sign(&message);
    let text = format!(
        "{SIGNATURE_PREFIX} {}\n",
        STANDARD.encode(signature.to_bytes())
    );
    let out = PathBuf::from(format!("{}.sig", file.display()));
    fs::write(&out, text).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(out)
}
/// Verify `<file>.sig` with the built-in public key.
pub fn verify_file(file: &Path) -> Result<()> {
    let message = fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let sig_path = PathBuf::from(format!("{}.sig", file.display()));
    let signature =
        fs::read_to_string(&sig_path).map_err(|e| format!("{}: {e}", sig_path.display()))?;
    verify_signature(&message, &signature)
}
/// Only this repository's release assets are ever downloaded.
fn trusted_url(url: &str) -> Result<()> {
    let prefix = format!("https://github.com/{REPO}/releases/download/");
    if url.starts_with(&prefix) {
        Ok(())
    } else {
        Err(format!(
            "Refusing to download from an unexpected location: {url}"
        ))
    }
}

/// The running version, or the one `RYOLUNE_PRETEND_VERSION` asks us to pretend we are.
pub fn current_version() -> String {
    std::env::var("RYOLUNE_PRETEND_VERSION")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

/// The release asset built for this platform, or `None` where no release is built.
pub fn asset_name() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("ryolune-macos-arm64.zip")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("ryolune-macos-x86_64.zip")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("ryolune-linux-x86_64.zip")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("ryolune-windows-x86_64.zip")
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub tag: String,
    pub notes: String,
    pub asset: String,
    pub url: String,
    pub size: u64,
    pub checksums_url: Option<String>,
    pub signature_url: Option<String>,
    pub sha256: Option<String>,
}

pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let major = parts.next()??;
    let minor = parts.next().unwrap_or(Some(0))?;
    let patch = parts.next().unwrap_or(Some(0))?;
    Some((major, minor, patch))
}

pub fn newer(latest: &str, current: &str) -> bool {
    // A pre-release comes before its release: 0.9.0-rc.1 updates to 0.9.0.
    let pre = |v: &str| v.trim().split('+').next().unwrap_or("").contains('-');
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c || (l == c && pre(current) && !pre(latest)),
        _ => false,
    }
}

/// Read the release this platform can install out of a GitHub `releases/latest` document.
pub fn find(json: &Value, asset: &str) -> Result<Option<Release>> {
    let tag = json
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or("The release has no tag")?;
    let assets = json
        .get("assets")
        .and_then(Value::as_array)
        .ok_or("The release lists no assets")?;
    let by_name = |name: &str| {
        assets
            .iter()
            .find(|a| a.get("name").and_then(Value::as_str) == Some(name))
    };
    let Some(mine) = by_name(asset) else {
        return Ok(None);
    };
    let url = mine
        .get("browser_download_url")
        .and_then(Value::as_str)
        .ok_or("The asset has no download URL")?;
    trusted_url(url)?;
    let checksums_url = by_name(CHECKSUMS)
        .and_then(|a| a.get("browser_download_url"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let signature_url = by_name(SIGNATURE)
        .and_then(|a| a.get("browser_download_url"))
        .and_then(Value::as_str)
        .map(str::to_string);
    for extra in checksums_url.iter().chain(signature_url.iter()) {
        trusted_url(extra)?;
    }
    Ok(Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        tag: tag.to_string(),
        notes: json
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        asset: asset.to_string(),
        url: url.to_string(),
        size: mine.get("size").and_then(Value::as_u64).unwrap_or(0),
        checksums_url,
        signature_url,
        sha256: None,
    }))
}

/// The hex digest for `name` in a `sha256sum` style listing.
pub fn parse_checksum(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

fn hex(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// A stalled connection must not keep the window "checking" for ten minutes, and a slow
/// line must not fail a large download halfway: connecting and the first reply are bounded
/// tightly, the body of a download gets an hour.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(3600)))
        .user_agent(format!("ryolune/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}
/// The release check reads a few kilobytes: a minute is plenty.
fn check_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .timeout_connect(Some(Duration::from_secs(15)))
        .user_agent(format!("ryolune/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn get_text(agent: &ureq::Agent, url: &str) -> Result<String> {
    agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Could not reach GitHub: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Could not read the reply from GitHub: {e}"))
}

/// Ask GitHub for the latest release and return it when it is newer than this build.
pub fn check() -> Result<Option<Release>> {
    let Some(asset) = asset_name() else {
        return Ok(None);
    };
    let agent = check_agent();
    let url = format!("{API}/{REPO}/releases/latest");
    let text = match agent
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(mut response) => response
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("Could not read the reply from GitHub: {e}"))?,
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(e) => return Err(format!("Could not reach GitHub: {e}")),
    };
    let json: Value =
        serde_json::from_str(&text).map_err(|e| format!("GitHub sent an unexpected reply: {e}"))?;
    let Some(mut release) = find(&json, asset)? else {
        return Ok(None);
    };
    if !newer(&release.version, &current_version()) {
        return Ok(None);
    }
    let sums_url = release
        .checksums_url
        .clone()
        .ok_or_else(|| format!("Release {} has no {CHECKSUMS} file", release.tag))?;
    let sums = get_text(&agent, &sums_url)?;
    if public_key().is_some() {
        let sig_url = release.signature_url.clone().ok_or_else(|| {
            format!(
                "Release {} is not signed ({SIGNATURE} missing); refusing to install it",
                release.tag
            )
        })?;
        let signature = get_text(&agent, &sig_url)?;
        verify_signature(sums.as_bytes(), &signature)?;
    }
    release.sha256 = Some(
        parse_checksum(&sums, asset)
            .ok_or_else(|| format!("{CHECKSUMS} in release {} lacks {asset}", release.tag))?,
    );
    Ok(Some(release))
}

fn download(agent: &ureq::Agent, release: &Release, to: &Path) -> Result<()> {
    trusted_url(&release.url)?;
    let mut response = agent
        .get(&release.url)
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let mut reader = response.body_mut().with_config().limit(MAX_ASSET).reader();
    let mut file = fs::File::create(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])
            .map_err(|e| format!("{}: {e}", to.display()))?;
    }
    file.sync_all().map_err(|e| e.to_string())?;
    let got = hex(&hasher.finalize());
    match &release.sha256 {
        Some(expected) if *expected == got => Ok(()),
        Some(_) => Err(format!(
            "The download of {} does not match its published checksum; nothing was changed",
            release.asset
        )),
        None => Err("The release has no checksum for this file; nothing was changed".into()),
    }
}

/// Where this process runs from, resolved through symlinks.
fn current_exe() -> Result<PathBuf> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    fs::canonicalize(&exe).map_err(|e| format!("{}: {e}", exe.display()))
}

#[cfg(target_os = "macos")]
fn bundle_of(exe: &Path) -> Result<PathBuf> {
    let bundle = exe
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|b| b.extension().is_some_and(|e| e == "app"))
        .ok_or_else(|| {
            format!(
                "Updates apply to an installed ryolune.app; this copy runs from {}",
                exe.display()
            )
        })?;
    Ok(bundle.to_path_buf())
}

#[cfg(target_os = "macos")]
fn previous_bundle(bundle: &Path) -> PathBuf {
    bundle.with_file_name(".ryolune-previous.app")
}

#[cfg(target_os = "macos")]
fn run(cmd: &mut std::process::Command, what: &str) -> Result<()> {
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{what}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

// Portable archives contain exactly these three root files. The macOS application is
// already replaced as one signed bundle, including its companions; it shares the version
// check below.
#[cfg_attr(target_os = "macos", allow(dead_code))]
mod companions {
    use super::*;
    use std::io::{Seek, SeekFrom};
    use std::process::{Command, Stdio};
    use std::time::Instant;

    const BACKUP_PREFIX: &str = ".ryolune-update-backup-";
    const MARKER: &str = "installed-version";
    const MAX_EXPANDED: u64 = 1024 * 1024 * 1024;

    fn names(windows: bool) -> [&'static str; 3] {
        if windows {
            ["ryolune.exe", "ryolune-cli.exe", "ryolune-mcp.exe"]
        } else {
            ["ryolune", "ryolune-cli", "ryolune-mcp"]
        }
    }

    /// Validate every entry before writing anything. Exact names also exclude paths,
    /// device names, extra payloads, links and duplicate entries on either platform.
    fn unpack(archive: &Path, destination: &Path, names: &[&str; 3]) -> Result<()> {
        let file = fs::File::open(archive).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("Invalid update ZIP: {e}"))?;
        if zip.len() != names.len() {
            return Err(
                "The update ZIP must contain exactly the desktop, CLI and MCP binaries".into(),
            );
        }
        let mut seen = std::collections::HashSet::new();
        let mut expanded = 0u64;
        for index in 0..zip.len() {
            let entry = zip.by_index(index).map_err(|e| e.to_string())?;
            let file_type = entry.unix_mode().unwrap_or(0) & 0o170000;
            if !names.contains(&entry.name())
                || !seen.insert(entry.name().to_owned())
                || !matches!(file_type, 0 | 0o100000)
                || entry.is_dir()
                || entry.size() == 0
                || entry.size() > MAX_ASSET
            {
                return Err(format!(
                    "The update ZIP contains an invalid binary: {}",
                    entry.name()
                ));
            }
            expanded = expanded
                .checked_add(entry.size())
                .ok_or("Update ZIP is too large")?;
            if expanded > MAX_EXPANDED {
                return Err("The expanded update ZIP is too large".into());
            }
        }
        fs::create_dir(destination).map_err(|e| e.to_string())?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
            let path = destination.join(entry.name());
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| e.to_string())?;
            // Reading to EOF verifies the ZIP CRC; the declared and actual size must agree.
            let declared = entry.size();
            let copied = std::io::copy(&mut entry.by_ref().take(MAX_ASSET + 1), &mut output)
                .map_err(|e| format!("Could not extract {}: {e}", path.display()))?;
            if copied != declared {
                return Err("The update ZIP has an inconsistent binary size".into());
            }
            output.sync_all().map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// Only run this after the whole archive has passed the published SHA-256 check.
    /// A file-backed stdout avoids pipe deadlocks; malformed tools have a fixed deadline.
    pub(super) fn verify_version(
        binary: &Path,
        name: &str,
        version: &str,
        timeout: Duration,
    ) -> Result<()> {
        let mut output = tempfile::tempfile().map_err(|e| e.to_string())?;
        let mut child = Command::new(binary)
            .arg("--version")
            .current_dir(binary.parent().ok_or("The staged binary has no folder")?)
            .env_remove("RYOLUNE_PRETEND_VERSION")
            .stdin(Stdio::null())
            .stdout(output.try_clone().map_err(|e| e.to_string())?)
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not verify {name}: {e}"))?;
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() < timeout => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                result => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(match result {
                        Err(e) => format!("Could not verify {name}: {e}"),
                        _ => format!("{name} did not answer --version in time"),
                    });
                }
            }
        };
        output.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let mut text = String::new();
        output
            .take(1025)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        let expected = format!("{} {version}", name.trim_end_matches(".exe"));
        if !status.success() || text.len() > 1024 || text.trim() != expected {
            return Err(format!(
                "{name} does not report the expected version {version}; nothing was changed"
            ));
        }
        Ok(())
    }

    /// Move existing files aside before publishing any replacement. A failure restores
    /// every old file, including the original absence of a companion. A failed rollback
    /// retains its backup directory for manual recovery rather than deleting evidence.
    fn replace_with(
        executable: &Path,
        staged: &Path,
        names: &[&str; 3],
        version: &str,
        mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
    ) -> Result<PathBuf> {
        let parent = executable.parent().ok_or("The executable has no folder")?;
        let targets = [
            executable.to_path_buf(),
            parent.join(names[1]),
            parent.join(names[2]),
        ];
        if targets[0] == targets[1] || targets[0] == targets[2] {
            return Err("The desktop executable has a companion's filename".into());
        }
        for target in &targets {
            match fs::symlink_metadata(target) {
                Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => {
                    return Err(format!(
                        "Refusing to replace a non-regular file: {}",
                        target.display()
                    ))
                }
            }
        }
        let backup = tempfile::Builder::new()
            .prefix(BACKUP_PREFIX)
            .tempdir_in(parent)
            .map_err(|e| format!("Could not create update backup: {e}"))?
            .keep();
        let mut moved = [false; 3];
        let mut published = [false; 3];
        let result = (|| -> Result<()> {
            for (index, target) in targets.iter().enumerate() {
                if target.exists() {
                    rename(target, &backup.join(names[index])).map_err(|e| e.to_string())?;
                    moved[index] = true;
                }
            }
            for (index, target) in targets.iter().enumerate() {
                rename(&staged.join(names[index]), target).map_err(|e| e.to_string())?;
                published[index] = true;
            }
            fs::write(backup.join(MARKER), version).map_err(|e| e.to_string())?;
            Ok(())
        })();
        if let Err(error) = result {
            let mut rollback_errors = Vec::new();
            for index in (0..3).rev() {
                if published[index] {
                    if let Err(e) = fs::remove_file(&targets[index]) {
                        rollback_errors.push(e.to_string());
                        continue;
                    }
                }
                if moved[index] {
                    if let Err(e) = fs::rename(backup.join(names[index]), &targets[index]) {
                        rollback_errors.push(e.to_string());
                    }
                }
            }
            if rollback_errors.is_empty() {
                let _ = fs::remove_dir_all(&backup);
                return Err(format!(
                    "Update failed; previous files were restored: {error}"
                ));
            }
            return Err(format!("Update failed: {error}. Some files could not be restored ({}). Previous files remain in {}", rollback_errors.join("; "), backup.display()));
        }
        Ok(backup)
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn install(agent: &ureq::Agent, release: &Release, executable: &Path) -> Result<()> {
        let parent = executable.parent().ok_or("The executable has no folder")?;
        let staging = tempfile::Builder::new()
            .prefix(".ryolune-update-stage-")
            .tempdir_in(parent)
            .map_err(|e| e.to_string())?;
        let archive = staging.path().join("update.zip");
        download(agent, release, &archive)?;
        let names = names(cfg!(windows));
        let unpacked = staging.path().join("binaries");
        unpack(&archive, &unpacked, &names)?;
        for name in names {
            verify_version(
                &unpacked.join(name),
                name,
                &release.version,
                Duration::from_secs(5),
            )?;
        }
        replace_with(
            executable,
            &unpacked,
            &names,
            &release.version,
            |from, to| fs::rename(from, to),
        )?;
        Ok(())
    }

    pub(super) fn cleanup_backups(parent: &Path, running_version: &str) {
        let Ok(entries) = fs::read_dir(parent) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry
                .file_name()
                .to_string_lossy()
                .starts_with(BACKUP_PREFIX)
                || !entry
                    .file_type()
                    .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
            {
                continue;
            }
            let Ok(children) = fs::read_dir(&path) else {
                continue;
            };
            let children: std::io::Result<Vec<_>> = children.collect();
            let Ok(children) = children else { continue };
            // Only clean a completed update belonging to this running release, and only
            // its known regular files. Interrupted transactions remain recoverable.
            if children.iter().any(|child| {
                let name = child.file_name();
                let name = name.to_string_lossy();
                (name != MARKER
                    && !names(false).contains(&name.as_ref())
                    && !names(true).contains(&name.as_ref()))
                    || !child
                        .file_type()
                        .is_ok_and(|kind| kind.is_file() && !kind.is_symlink())
            }) || fs::read_to_string(path.join(MARKER)).ok().as_deref() != Some(running_version)
            {
                continue;
            }
            let mut removed = true;
            for child in children.iter().filter(|child| child.file_name() != MARKER) {
                removed &= fs::remove_file(child.path()).is_ok();
            }
            if removed {
                let _ = fs::remove_file(path.join(MARKER));
                let _ = fs::remove_dir(path);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use zip::write::SimpleFileOptions;

        fn archive(path: &Path, entries: &[(&str, bool)]) {
            let mut writer = zip::ZipWriter::new(fs::File::create(path).unwrap());
            for (name, symlink) in entries {
                if *symlink {
                    writer
                        .add_symlink(*name, "outside", SimpleFileOptions::default())
                        .unwrap();
                } else {
                    writer
                        .start_file(*name, SimpleFileOptions::default())
                        .unwrap();
                    writer.write_all(b"binary").unwrap();
                }
            }
            writer.finish().unwrap();
        }

        #[test]
        fn archives_require_only_three_regular_root_binaries() {
            let dir = tempfile::tempdir().unwrap();
            let zip = dir.path().join("update.zip");
            let valid = [
                ("ryolune", false),
                ("ryolune-cli", false),
                ("ryolune-mcp", false),
            ];
            archive(&zip, &valid);
            unpack(&zip, &dir.path().join("good"), &names(false)).unwrap();
            assert_eq!(
                fs::read(dir.path().join("good/ryolune-mcp")).unwrap(),
                b"binary"
            );
            for bad in [
                vec![("ryolune", false)],
                vec![("../ryolune", false), valid[1], valid[2]],
                vec![("/ryolune", false), valid[1], valid[2]],
                vec![("nested\\ryolune", false), valid[1], valid[2]],
                vec![("ryolune", true), valid[1], valid[2]],
                vec![("ryolune", false), valid[1], ("extra", false)],
            ] {
                archive(&zip, &bad);
                let destination = dir.path().join("rejected");
                assert!(
                    unpack(&zip, &destination, &names(false)).is_err(),
                    "{bad:?}"
                );
                assert!(!destination.exists());
            }
        }

        #[test]
        fn every_forward_failure_restores_all_existing_files_and_absences() {
            for missing_companion in [false, true] {
                let count = if missing_companion { 5 } else { 6 };
                for failure in 0..count {
                    let dir = tempfile::tempdir().unwrap();
                    let staged = dir.path().join("stage");
                    fs::create_dir(&staged).unwrap();
                    for name in names(false) {
                        if !missing_companion || name != "ryolune-cli" {
                            fs::write(dir.path().join(name), format!("old {name}")).unwrap();
                        }
                        fs::write(staged.join(name), format!("new {name}")).unwrap();
                    }
                    fs::write(dir.path().join("song.ryolune"), "original song").unwrap();
                    let mut call = 0;
                    let result = replace_with(
                        &dir.path().join("ryolune"),
                        &staged,
                        &names(false),
                        "0.2.0",
                        |from, to| {
                            let fail = call == failure;
                            call += 1;
                            if fail {
                                Err(std::io::Error::other("injected rename failure"))
                            } else {
                                fs::rename(from, to)
                            }
                        },
                    );
                    assert!(result.unwrap_err().contains("previous files were restored"));
                    for name in names(false) {
                        if missing_companion && name == "ryolune-cli" {
                            assert!(!dir.path().join(name).exists());
                        } else {
                            assert_eq!(
                                fs::read_to_string(dir.path().join(name)).unwrap(),
                                format!("old {name}")
                            );
                        }
                    }
                    assert_eq!(
                        fs::read_to_string(dir.path().join("song.ryolune")).unwrap(),
                        "original song"
                    );
                }
            }
        }

        #[test]
        fn successful_update_keeps_backup_until_new_version_starts() {
            let dir = tempfile::tempdir().unwrap();
            let staged = dir.path().join("stage");
            fs::create_dir(&staged).unwrap();
            for name in names(false) {
                fs::write(staged.join(name), "new").unwrap();
                fs::write(dir.path().join(name), "old").unwrap();
            }
            let backup = replace_with(
                &dir.path().join("ryolune"),
                &staged,
                &names(false),
                "0.2.0",
                |from, to| fs::rename(from, to),
            )
            .unwrap();
            for name in names(false) {
                assert_eq!(fs::read_to_string(dir.path().join(name)).unwrap(), "new");
                assert_eq!(fs::read_to_string(backup.join(name)).unwrap(), "old");
            }
            cleanup_backups(dir.path(), "0.1.0");
            assert!(backup.exists());
            cleanup_backups(dir.path(), "0.2.0");
            assert!(!backup.exists());
        }

        #[test]
        fn a_failed_rollback_keeps_the_original_binaries_for_recovery() {
            let dir = tempfile::tempdir().unwrap();
            let staged = dir.path().join("stage");
            fs::create_dir(&staged).unwrap();
            for name in names(false) {
                fs::write(staged.join(name), "new").unwrap();
                fs::write(dir.path().join(name), "old").unwrap();
            }
            let desktop = dir.path().join("ryolune");
            let mut call = 0;
            let result = replace_with(&desktop, &staged, &names(false), "0.2.0", |from, to| {
                call += 1;
                if call == 4 {
                    // Another process obstructs the original location after all backups
                    // are safe, making even a real rollback rename fail.
                    fs::create_dir(&desktop)?;
                    fs::write(desktop.join("unexpected"), "preserve")?;
                    Err(std::io::Error::other("injected publish failure"))
                } else {
                    fs::rename(from, to)
                }
            });
            assert!(result.unwrap_err().contains("Previous files remain in"));
            cleanup_backups(dir.path(), "0.2.0");
            let backup = fs::read_dir(dir.path())
                .unwrap()
                .flatten()
                .find(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(BACKUP_PREFIX)
                })
                .unwrap()
                .path();
            assert_eq!(fs::read_to_string(backup.join("ryolune")).unwrap(), "old");
            assert_eq!(
                fs::read_to_string(dir.path().join("ryolune-cli")).unwrap(),
                "old"
            );
            assert_eq!(
                fs::read_to_string(dir.path().join("ryolune-mcp")).unwrap(),
                "old"
            );
            assert_eq!(
                fs::read_to_string(desktop.join("unexpected")).unwrap(),
                "preserve"
            );
        }

        #[cfg(unix)]
        #[test]
        fn symlink_targets_and_unfinished_backups_are_preserved() {
            use std::os::unix::fs::symlink;
            let dir = tempfile::tempdir().unwrap();
            let original = dir.path().join("original");
            fs::write(&original, "keep").unwrap();
            symlink(&original, dir.path().join("ryolune-cli")).unwrap();
            assert!(replace_with(
                &dir.path().join("ryolune"),
                dir.path(),
                &names(false),
                "0.2.0",
                |from, to| fs::rename(from, to)
            )
            .is_err());
            let backup = dir.path().join(format!("{BACKUP_PREFIX}interrupted"));
            fs::create_dir(&backup).unwrap();
            fs::write(backup.join("ryolune"), "keep backup").unwrap();
            cleanup_backups(dir.path(), "0.2.0");
            assert!(backup.exists());
            fs::write(backup.join(MARKER), "0.2.0").unwrap();
            symlink(&original, backup.join("ryolune-cli")).unwrap();
            cleanup_backups(dir.path(), "0.2.0");
            assert!(backup.exists());
            assert_eq!(fs::read_to_string(original).unwrap(), "keep");
        }

        #[cfg(unix)]
        #[test]
        fn version_verification_requires_exact_binary_identity_and_is_bounded() {
            use std::os::unix::fs::PermissionsExt;
            let dir = tempfile::tempdir().unwrap();
            let binary = dir.path().join("ryolune-cli");
            fs::write(&binary, "#!/bin/sh\nprintf 'ryolune-cli 0.2.0\\n'\n").unwrap();
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
            verify_version(&binary, "ryolune-cli", "0.2.0", Duration::from_secs(1)).unwrap();
            assert!(
                verify_version(&binary, "ryolune-mcp", "0.2.0", Duration::from_secs(1)).is_err()
            );
            assert!(
                verify_version(&binary, "ryolune-cli", "0.3.1", Duration::from_secs(1)).is_err()
            );
            fs::write(&binary, "#!/bin/sh\nwhile :; do :; done\n").unwrap();
            let started = Instant::now();
            assert!(
                verify_version(&binary, "ryolune-cli", "0.2.0", Duration::from_millis(30))
                    .unwrap_err()
                    .contains("in time")
            );
            assert!(started.elapsed() < Duration::from_secs(2));
        }
    }
}

/// Download, verify and swap the installed copy. Returns what to launch afterwards. The old
/// copy stays in place until the new one is fully unpacked and verified.
pub fn install(release: &Release) -> Result<PathBuf> {
    let exe = current_exe()?;
    let agent = agent();
    #[cfg(target_os = "macos")]
    {
        let bundle = bundle_of(&exe)?;
        let parent = bundle.parent().ok_or("The app has no parent folder")?;
        let staging = parent.join(format!(".ryolune-update-{}", std::process::id()));
        let _ = fs::remove_dir_all(&staging);
        fs::create_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
        let result = (|| -> Result<()> {
            let zip = staging.join(&release.asset);
            download(&agent, release, &zip)?;
            let unpacked = staging.join("unpacked");
            run(
                std::process::Command::new("ditto")
                    .args(["-x", "-k"])
                    .arg(&zip)
                    .arg(&unpacked),
                "Could not unpack the update",
            )?;
            let fresh = fs::read_dir(&unpacked)
                .map_err(|e| e.to_string())?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .find(|p| p.extension().is_some_and(|e| e == "app"))
                .ok_or("The update archive holds no application")?;
            if ["ryolune", "ryolune-cli", "ryolune-mcp"]
                .iter()
                .any(|binary| !fresh.join("Contents/MacOS").join(binary).is_file())
            {
                return Err("The update archive is incomplete".into());
            }
            run(
                std::process::Command::new("codesign")
                    .args(["--verify", "--deep", "--strict"])
                    .arg(&fresh),
                "The downloaded app failed signature verification",
            )?;
            for binary in ["ryolune", "ryolune-cli", "ryolune-mcp"] {
                companions::verify_version(
                    &fresh.join("Contents/MacOS").join(binary),
                    binary,
                    &release.version,
                    Duration::from_secs(8),
                )?;
            }
            let previous = previous_bundle(&bundle);
            let _ = fs::remove_dir_all(&previous);
            fs::rename(&bundle, &previous)
                .map_err(|e| format!("Could not replace {}: {e}", bundle.display()))?;
            if let Err(e) = fs::rename(&fresh, &bundle) {
                let _ = fs::rename(&previous, &bundle);
                return Err(format!("Could not install into {}: {e}", bundle.display()));
            }
            Ok(())
        })();
        let _ = fs::remove_dir_all(&staging);
        result?;
        Ok(bundle)
    }
    #[cfg(not(target_os = "macos"))]
    {
        companions::install(&agent, release, &exe)?;
        Ok(exe)
    }
}

/// Remove what a previous update left behind. Called once when the window starts (never from
/// the scanner's child processes, which run while an old copy may still be open); errors are
/// ignored because the old copy may still be shutting down.
pub fn cleanup() {
    let Ok(exe) = current_exe() else {
        return;
    };
    #[cfg(target_os = "macos")]
    if let Ok(bundle) = bundle_of(&exe) {
        let _ = fs::remove_dir_all(previous_bundle(&bundle));
        if let Some(parent) = bundle.parent() {
            remove_abandoned_staging(parent, std::time::SystemTime::now());
        }
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(dir) = exe.parent() {
        companions::cleanup_backups(dir, env!("CARGO_PKG_VERSION"));
    }
}

/// A download interrupted by quitting leaves its `.ryolune-update-<pid>` folder beside the app.
/// One more than a day old belongs to no install still running. Only macOS stages beside the
/// app; the other platforms keep companion backups, which `companions::cleanup_backups` owns.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn remove_abandoned_staging(parent: &Path, now: std::time::SystemTime) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(".ryolune-update-") {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > Duration::from_secs(24 * 3600));
        if old {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// JSON form of a release for the registry.
pub fn release_value(release: &Release) -> Value {
    json!({
        "version": release.version,
        "tag": release.tag,
        "asset": release.asset,
        "size": release.size,
        "signed": release.signature_url.is_some(),
        "notes": release.notes,
    })
}

/// What Relaunch starts: the freshly installed copy, or this one again when nothing was
/// installed (the app bundle on macOS, so it opens as an app and not in a terminal).
pub fn relaunch_target(installed: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(target) = installed {
        return Ok(target);
    }
    let exe = current_exe()?;
    #[cfg(target_os = "macos")]
    if let Ok(bundle) = bundle_of(&exe) {
        return Ok(bundle);
    }
    Ok(exe)
}

/// Start `target`. The caller closes this copy.
pub fn relaunch(target: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = if target.extension().is_some_and(|e| e == "app") {
        let mut c = std::process::Command::new("open");
        c.arg("-n").arg(target);
        c
    } else {
        std::process::Command::new(target)
    };
    #[cfg(not(target_os = "macos"))]
    let mut cmd = std::process::Command::new(target);
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not relaunch {}: {e}", target.display()))
}

#[derive(Default)]
pub(crate) struct Updates {
    pub checking: Option<mpsc::Receiver<Result<Option<Release>>>>,
    pub manual: bool,
    pub available: Option<Release>,
    pub installing: Option<mpsc::Receiver<Result<PathBuf>>>,
    pub installed: Option<PathBuf>,
    pub show: bool,
    /// Checks again every [`RECHECK`] while open (off for `--no-update-check`,
    /// RYOLUNE_NO_UPDATE and capture runs; Settings › Updates can turn checks off too).
    pub periodic: bool,
    /// When the last check started.
    pub last_check: Option<std::time::Instant>,
}
impl Updates {
    pub fn busy(&self) -> bool {
        self.checking.is_some() || self.installing.is_some()
    }
    /// A background check is due: periodic checks are on, nothing is known or running yet,
    /// and the last check is [`RECHECK`] old (or there was none, when the start check was off
    /// and turned on later).
    pub fn due(&self, enabled: bool, now: std::time::Instant) -> bool {
        self.periodic
            && enabled
            && !self.busy()
            && self.available.is_none()
            && self.installed.is_none()
            && self
                .last_check
                .is_none_or(|at| now.duration_since(at) >= RECHECK)
    }
}

/// How often a window that stays open asks GitHub again.
pub const RECHECK: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

impl Ryolune {
    pub(crate) fn check_for_updates(&mut self, manual: bool) {
        if self.updates.busy() {
            return;
        }
        if self.updates.available.is_some() && manual {
            self.updates.show = true;
            return;
        }
        self.updates.manual = manual;
        self.updates.last_check = Some(std::time::Instant::now());
        let (tx, rx) = mpsc::sync_channel(1);
        self.updates.checking = Some(rx);
        if manual {
            self.status = "Checking for updates…".into();
        }
        std::thread::spawn(move || {
            let _ = tx.send(check());
        });
    }
    pub(crate) fn install_update(&mut self) {
        let Some(release) = self.updates.available.clone() else {
            return;
        };
        if self.updates.busy() || self.updates.installed.is_some() {
            return;
        }
        let (tx, rx) = mpsc::sync_channel(1);
        self.updates.installing = Some(rx);
        self.status = format!("Downloading ryolune {}…", release.version);
        std::thread::spawn(move || {
            let _ = tx.send(install(&release));
        });
    }
    pub(crate) fn poll_updates(&mut self) {
        if self.updates.due(
            self.settings.general.check_updates_on_start,
            std::time::Instant::now(),
        ) {
            if self.updates.last_check.is_some() {
                log::info!("checking for updates again (every six hours while open)");
            }
            self.check_for_updates(false);
        }
        if let Some(result) = self
            .updates
            .checking
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(v) => Some(v),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("Update check stopped".into())),
                Err(_) => None,
            })
        {
            self.updates.checking = None;
            let live = match &result {
                Ok(release) => Ok(json!({
                    "current": current_version(),
                    "available": release.as_ref().map(release_value),
                })),
                Err(e) => Err(e.clone()),
            };
            match &result {
                Ok(Some(release)) => {
                    log::info!("update check: ryolune {} is available", release.version)
                }
                Ok(None) => log::info!("update check: ryolune {} is up to date", current_version()),
                Err(e) => log::warn!("update check failed: {e}"),
            }
            match result {
                Ok(Some(release)) => {
                    self.status = format!("ryolune {} is available", release.version);
                    self.updates.available = Some(release);
                    if self.settings.general.install_updates_automatically {
                        self.install_update();
                    } else {
                        self.updates.show = true;
                    }
                }
                Ok(None) if self.updates.manual => {
                    self.status = format!("ryolune {} is up to date", current_version());
                }
                Ok(None) => {}
                Err(e) if self.updates.manual => self.error = Some(e),
                Err(_) => {}
            }
            self.finish_live(|wait| matches!(wait, LiveWait::UpdateCheck), live);
        }
        if let Some(result) = self
            .updates
            .installing
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(v) => Some(v),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("Update stopped".into())),
                Err(_) => None,
            })
        {
            self.updates.installing = None;
            let live = match &result {
                Ok(target) => Ok(
                    json!({ "installed": target, "relaunch": "confirm in the window or quit and reopen" }),
                ),
                Err(e) => Err(e.clone()),
            };
            match &result {
                Ok(target) => log::info!("update installed at {}", target.display()),
                Err(e) => log::warn!("update install failed: {e}"),
            }
            match result {
                Ok(target) => {
                    self.status = "Update installed: restart ryolune to use it".into();
                    self.updates.installed = Some(target);
                    self.updates.show = true;
                }
                Err(e) => {
                    self.status = "Update failed".into();
                    self.updates.show = false;
                    self.error = Some(e);
                }
            }
            self.finish_live(|wait| matches!(wait, LiveWait::UpdateInstall), live);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_open_window_checks_again_every_six_hours() {
        let start = std::time::Instant::now();
        let mut updates = super::Updates::default();
        assert!(!updates.due(true, start), "off without periodic checks");
        updates.periodic = true;
        assert!(
            !updates.due(false, start),
            "off when Settings turns checks off"
        );
        assert!(updates.due(true, start), "never checked: check now");
        updates.last_check = Some(start);
        assert!(!updates.due(true, start + std::time::Duration::from_secs(60)));
        assert!(updates.due(true, start + super::RECHECK));
        updates.installed = Some(std::path::PathBuf::from("/x"));
        assert!(
            !updates.due(true, start + super::RECHECK),
            "an installed update waits for a restart"
        );
    }

    #[test]
    fn relaunch_without_an_update_starts_this_copy() {
        let installed = PathBuf::from("/Applications/ryolune.app");
        assert_eq!(relaunch_target(Some(installed.clone())).unwrap(), installed);
        let this = relaunch_target(None).unwrap();
        assert!(this.exists(), "{}", this.display());
    }

    #[test]
    fn abandoned_update_folders_are_removed_and_fresh_ones_kept() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join(".ryolune-update-4242");
        let other = dir.path().join("ryolune.app");
        fs::create_dir_all(&staging).unwrap();
        fs::create_dir_all(&other).unwrap();
        let now = std::time::SystemTime::now();
        remove_abandoned_staging(dir.path(), now);
        assert!(staging.exists(), "a running install keeps its folder");
        remove_abandoned_staging(dir.path(), now + Duration::from_secs(2 * 24 * 3600));
        assert!(!staging.exists());
        assert!(other.exists());
    }

    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.10"), Some((0, 10, 0)));
        assert_eq!(parse_version("v1.2.3-beta.1"), Some((1, 2, 3)));
        assert_eq!(parse_version("nightly"), None);
        assert!(newer("v0.2.0", "0.1.9"));
        assert!(newer("v0.10.0", "0.9.0"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("garbage", "0.1.0"));
        // A release candidate is followed by its release, never the other way round.
        assert!(newer("v0.9.0", "0.9.0-rc.1"));
        assert!(!newer("v0.9.0-rc.2", "0.9.0"));
        assert!(!newer("v0.9.0-rc.1", "0.9.0-rc.1"));
    }

    #[test]
    fn release_document_yields_this_platforms_asset() {
        let json: Value = serde_json::json!({
            "tag_name": "v0.2.0",
            "body": "Notes",
            "assets": [
                {"name": "SHA256SUMS", "browser_download_url": "https://github.com/ludovic111/ryolune/releases/download/v0.2.0/SHA256SUMS", "size": 300},
                {"name": "ryolune-macos-arm64.zip", "browser_download_url": "https://github.com/ludovic111/ryolune/releases/download/v0.2.0/ryolune-macos-arm64.zip", "size": 10},
            ]
        });
        let r = find(&json, "ryolune-macos-arm64.zip").unwrap().unwrap();
        assert_eq!(r.version, "0.2.0");
        assert_eq!(
            r.url,
            "https://github.com/ludovic111/ryolune/releases/download/v0.2.0/ryolune-macos-arm64.zip"
        );
        assert_eq!(
            r.checksums_url.as_deref(),
            Some("https://github.com/ludovic111/ryolune/releases/download/v0.2.0/SHA256SUMS")
        );
        assert!(r.signature_url.is_none());
        assert_eq!(r.notes, "Notes");
        assert!(find(&json, "ryolune-linux-x86_64").unwrap().is_none());
        assert!(find(&serde_json::json!({}), "x").is_err());
    }

    #[test]
    fn checksum_listing_is_parsed() {
        let sums = format!(
            "{}  ryolune-macos-arm64.zip\n{} *ryolune-windows-x86_64.exe\n",
            "a".repeat(64),
            "B".repeat(64)
        );
        assert_eq!(
            parse_checksum(&sums, "ryolune-macos-arm64.zip").as_deref(),
            Some("a".repeat(64).as_str())
        );
        assert_eq!(
            parse_checksum(&sums, "ryolune-windows-x86_64.exe").as_deref(),
            Some("b".repeat(64).as_str())
        );
        assert_eq!(parse_checksum(&sums, "other"), None);
        assert_eq!(
            parse_checksum(&format!("{}  other", "g".repeat(64)), "other"),
            None
        );
        assert_eq!(
            hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn this_platform_has_a_release_asset() {
        assert!(asset_name().is_some());
    }

    #[test]
    fn release_urls_must_belong_to_this_repository() {
        let json: Value = serde_json::json!({
            "tag_name": "v9.9.9",
            "assets": [
                {"name": "ryolune-macos-arm64.zip", "browser_download_url": "https://evil.example/a.zip", "size": 10},
            ]
        });
        assert!(find(&json, "ryolune-macos-arm64.zip")
            .unwrap_err()
            .contains("unexpected location"));
        let json: Value = serde_json::json!({
            "tag_name": "v9.9.9",
            "assets": [
                {"name": "ryolune-macos-arm64.zip", "browser_download_url": "https://github.com/ludovic111/ryolune/releases/download/v9.9.9/ryolune-macos-arm64.zip", "size": 10},
                {"name": "SHA256SUMS.sig", "browser_download_url": "https://github.com/ludovic111/ryolune/releases/download/v9.9.9/SHA256SUMS.sig", "size": 10},
            ]
        });
        let release = find(&json, "ryolune-macos-arm64.zip").unwrap().unwrap();
        assert!(release.signature_url.is_some());
    }

    #[test]
    fn signatures_round_trip_and_tampering_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("keys/release.key");
        let public = write_keypair(&key).unwrap();
        assert_eq!(public.len(), 64);
        assert!(write_keypair(&key).is_err(), "keys are never overwritten");
        let sums = dir.path().join("SHA256SUMS");
        fs::write(&sums, "abc  ryolune-macos-arm64.zip\n").unwrap();
        let sig = sign_file(&key, &sums).unwrap();
        let text = fs::read_to_string(&sig).unwrap();
        assert!(text.starts_with(SIGNATURE_PREFIX));
        let signing = read_secret_key(&key).unwrap();
        let verifying = signing.verifying_key();
        let line = text.lines().next().unwrap();
        let bytes = STANDARD
            .decode(line[SIGNATURE_PREFIX.len()..].trim())
            .unwrap();
        let signature = Signature::from_slice(&bytes).unwrap();
        assert!(verifying
            .verify(b"abc  ryolune-macos-arm64.zip\n", &signature)
            .is_ok());
        assert!(verifying
            .verify(b"abc  ryolune-macos-arm64.zip\n tampered", &signature)
            .is_err());
        if public_key().is_none() {
            assert!(verify_signature(b"x", &text)
                .unwrap_err()
                .contains("no release signing key"));
        }
    }
}
