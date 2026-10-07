//! Plugins a person gets by asking their agent (lsuite's PLUGINS.md): the crate an agent
//! writes, its build, and the bundle that ryolune loads without a restart.
//!
//! - Sources live in `~/.lsuite/plugins-src/ryolune/<name>/` (`plugin.new`), a crate made from
//!   the SDK template. Its `Cargo.toml` takes `ryolune-plugin` from GitHub by tag
//!   (`v<version>`), so the crate builds anywhere; `.cargo/config.toml` replaces that source
//!   with the SDK this ryolune carries (`sdk/src`, built in), written once to
//!   `plugins-src/ryolune/.sdk/<version>/`, with a seeded `Cargo.lock`. A plugin therefore
//!   builds with exactly the ABI of the app that asked for it, and the SDK is never fetched.
//! - `plugin.build` runs `cargo build --release` and turns the compiler's JSON into
//!   `{file, line, column, message}`. Builds share one target folder
//!   (`plugins-src/ryolune/.target`), so the second plugin compiles in seconds.
//! - `plugin.publishLocal` makes the bundle `~/.lsuite/plugins/ryolune/<id>/`: `plugin.toml`
//!   and the library, under a new file name per build (`lib<crate>.<stamp>.so`) because an
//!   operating system hands back the library it already loaded for a path it has seen. The
//!   previous library is retired (renamed `.retired`, deleted when it can be). The window
//!   rescans and reloads the inserts that use the plugin.

use crate::{
    control::{edit, opt, query, req, Args, Host, Kind, Spec},
    host::{native, scan},
    lsuite, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::Instant,
};

pub const APP: &str = "ryolune";
pub const SDK_GIT: &str = "https://github.com/ludovic111/ryolune";
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The largest file `plugin.writeSource` writes.
const SOURCE_LIMIT: usize = 1024 * 1024;

/// The SDK this ryolune was built with, written out for plugin builds.
const SDK_FILES: &[(&str, &str)] = &[
    ("src/lib.rs", include_str!("../../sdk/src/lib.rs")),
    ("src/plugin.rs", include_str!("../../sdk/src/plugin.rs")),
    ("src/ffi.rs", include_str!("../../sdk/src/ffi.rs")),
    ("src/dsp.rs", include_str!("../../sdk/src/dsp.rs")),
    ("src/testing.rs", include_str!("../../sdk/src/testing.rs")),
];

pub fn sdk_tag() -> String {
    format!("v{VERSION}")
}
/// Installed lsuite plugin bundles: `~/.lsuite/plugins/ryolune`.
pub fn installed_dir() -> PathBuf {
    lsuite::home().join("plugins").join(APP)
}
/// Plugin sources an agent writes: `~/.lsuite/plugins-src/ryolune`.
pub fn sources_dir() -> PathBuf {
    lsuite::home().join("plugins-src").join(APP)
}
fn sdk_dir() -> PathBuf {
    sources_dir().join(".sdk").join(VERSION)
}
fn target_dir() -> PathBuf {
    std::env::var_os("RYOLUNE_PLUGIN_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| sources_dir().join(".target"))
}

// ---------------------------------------------------------------------------------------
// Bundles

/// `plugin.toml`, the manifest of a plugin bundle.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub app: String,
    pub kind: String,
    pub abi: u32,
    pub description: String,
    pub authors: Vec<String>,
    pub library: Libraries,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Libraries {
    pub macos: String,
    pub linux: String,
    pub windows: String,
}
impl Libraries {
    pub fn this_platform(&self) -> &str {
        if cfg!(target_os = "macos") {
            &self.macos
        } else if cfg!(windows) {
            &self.windows
        } else {
            &self.linux
        }
    }
    fn set_this_platform(&mut self, file: String) {
        if cfg!(target_os = "macos") {
            self.macos = file
        } else if cfg!(windows) {
            self.windows = file
        } else {
            self.linux = file
        }
    }
}

pub fn read_manifest(dir: &Path) -> Result<Manifest> {
    let path = dir.join("plugin.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let manifest: Manifest =
        toml::from_str(&text).map_err(|e| format!("{} is not valid: {e}", path.display()))?;
    if manifest.id.trim().is_empty() || manifest.name.trim().is_empty() {
        return Err(format!("{} needs an id and a name", path.display()));
    }
    if !manifest.app.is_empty() && manifest.app != APP {
        return Err(format!(
            "{} is a plugin for {}, not ryolune",
            manifest.name, manifest.app
        ));
    }
    Ok(manifest)
}

/// The bundle a scanned library belongs to, when it sits beside a `plugin.toml`.
pub fn bundle_of(library: &Path) -> Option<(PathBuf, Manifest)> {
    let dir = library.parent()?;
    read_manifest(dir).ok().map(|m| (dir.to_path_buf(), m))
}

/// Whether a descriptor path is a plugin the person installed themselves (an lsuite bundle
/// or a library in ryolune's own plugin folder), which `plugin.remove` may delete.
pub fn removable(path: &Path) -> bool {
    path.starts_with(installed_dir()) || path.starts_with(scan::data_dir().join("plugins"))
}

/// Copy a built bundle (a folder with `plugin.toml` and its library) into the plugin folder.
pub fn install_bundle(source: &Path) -> Result<Value> {
    let manifest = read_manifest(source)?;
    let file = manifest.library.this_platform().to_string();
    if file.is_empty() {
        return Err(format!(
            "{} has no library for this platform in plugin.toml [library]",
            manifest.name
        ));
    }
    let library = source.join(&file);
    if !library.is_file() {
        return Err(format!(
            "{} is missing; build the plugin first",
            library.display()
        ));
    }
    install_library(&manifest, &library)
}

fn safe_id(id: &str) -> Result<String> {
    let ok = !id.is_empty()
        && id.len() <= 120
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        && !id.starts_with('.');
    ok.then(|| id.to_string())
        .ok_or_else(|| format!("`{id}` is not a plugin id: use reverse-DNS such as com.you.plugin"))
}

/// Put a library into its bundle under a new name, retire the previous one, write the
/// manifest. Returns the bundle folder and the library path.
fn install_library(manifest: &Manifest, library: &Path) -> Result<Value> {
    let id = safe_id(&manifest.id)?;
    let bundle = installed_dir().join(&id);
    std::fs::create_dir_all(&bundle).map_err(|e| e.to_string())?;
    let extension = native::library_extension();
    let stem = library
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("plugin")
        .split('.')
        .next()
        .unwrap_or("plugin")
        .to_string();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let file = format!("{stem}.{stamp}.{extension}");
    let target = bundle.join(&file);
    let staged = bundle.join(format!("{file}.part"));
    std::fs::copy(library, &staged).map_err(|e| format!("Could not copy the library: {e}"))?;
    // Retire the libraries of earlier builds: a running song may still have one loaded.
    for entry in std::fs::read_dir(&bundle).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with(".retired") {
            let _ = std::fs::remove_file(&path);
        } else if path.extension().is_some_and(|e| e == extension) {
            let retired = bundle.join(format!("{name}.retired"));
            if std::fs::rename(&path, &retired).is_ok() {
                let _ = std::fs::remove_file(&retired);
            }
        }
    }
    std::fs::rename(&staged, &target).map_err(|e| {
        let _ = std::fs::remove_file(&staged);
        e.to_string()
    })?;
    let mut written = manifest.clone();
    written.app = APP.into();
    written.library.set_this_platform(file);
    let text = toml::to_string_pretty(&written).map_err(|e| e.to_string())?;
    std::fs::write(bundle.join("plugin.toml"), text).map_err(|e| e.to_string())?;
    Ok(json!({ "bundle": bundle, "library": target, "id": id, "pluginId": format!("native:{id}") }))
}

/// Scan again so the new bundle is in the plugin list (the window then reloads its inserts).
pub fn rescan() -> Result<Value> {
    let cache = scan::scan_all(|_| {});
    scan::store_cache(&cache)?;
    let errors: Vec<Value> = cache
        .entries
        .iter()
        .filter(|e| e.error.is_some() && Path::new(&e.path).starts_with(installed_dir()))
        .map(|e| json!({ "path": e.path, "error": e.error }))
        .collect();
    Ok(json!({ "pluginCount": scan::installed().len(), "errors": errors }))
}

// ---------------------------------------------------------------------------------------
// The toolchain

fn cargo_candidates() -> Vec<PathBuf> {
    let exe = std::env::consts::EXE_SUFFIX;
    let mut found = vec![];
    if let Some(path) = std::env::var_os("CARGO") {
        found.push(PathBuf::from(path));
    }
    if let Some(paths) = std::env::var_os("PATH") {
        found.extend(std::env::split_paths(&paths).map(|d| d.join(format!("cargo{exe}"))));
    }
    if let Some(home) = std::env::var_os("CARGO_HOME") {
        found.push(PathBuf::from(home).join("bin").join(format!("cargo{exe}")));
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        found.push(
            PathBuf::from(home)
                .join(".cargo/bin")
                .join(format!("cargo{exe}")),
        );
    }
    found.push(PathBuf::from("/opt/homebrew/bin/cargo"));
    found.push(PathBuf::from("/usr/local/bin/cargo"));
    found
}

/// The `cargo` a plugin is built with: the environment's, else rustup's usual place (an app
/// started from the Finder has a bare PATH).
pub fn cargo() -> Option<PathBuf> {
    cargo_candidates().into_iter().find(|p| p.is_file())
}

fn version_of(program: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = program.parent() {
        command.env("PATH", with_path(dir));
    }
    let out = command.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn with_path(dir: &Path) -> std::ffi::OsString {
    let mut paths = vec![dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).unwrap_or_default()
}

pub fn install_hint() -> &'static str {
    if cfg!(windows) {
        "Install Rust with rustup: download and run rustup-init.exe from https://rustup.rs, then press Check again."
    } else {
        "Install Rust with rustup: in Terminal, run curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh (it asks before changing anything), then press Check again."
    }
}

/// `{cargo, rustc, version, ok, installHint}`.
pub fn toolchain() -> Value {
    let cargo = cargo();
    let cargo_version = cargo.as_deref().and_then(|c| version_of(c, &["--version"]));
    let rustc = cargo.as_deref().and_then(|c| {
        let path = c.with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX));
        path.is_file().then_some(path)
    });
    let rustc_version = rustc.as_deref().and_then(|r| version_of(r, &["--version"]));
    let ok = cargo_version.is_some() && rustc_version.is_some();
    json!({
        "cargo": cargo,
        "rustc": rustc,
        "version": rustc_version.clone().or(cargo_version.clone()),
        "cargoVersion": cargo_version,
        "ok": ok,
        "installHint": if ok { Value::Null } else { json!(install_hint()) },
        "installUrl": "https://rustup.rs",
    })
}

// ---------------------------------------------------------------------------------------
// Crates

/// `Warm Drive 2` → `warm-drive-2`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// A crate's folder from its name (`Warm Drive` or `warm-drive`); it must exist.
pub fn crate_dir(name: &str) -> Result<PathBuf> {
    let slug = slug(name);
    if slug.is_empty() {
        return Err("Name the plugin, as plugin.new made it".into());
    }
    let dir = sources_dir().join(&slug);
    if !dir.join("Cargo.toml").is_file() {
        return Err(format!(
            "No plugin source called `{slug}` in {}. Start one with plugin.new.",
            sources_dir().display()
        ));
    }
    Ok(dir)
}

/// Write the built-in SDK where plugin builds find it (once per version).
fn ensure_sdk() -> Result<PathBuf> {
    let root = sdk_dir();
    let dir = root.join("ryolune-plugin");
    let cargo = format!(
        "[package]\nname = \"ryolune-plugin\"\nversion = \"{VERSION}\"\nedition = \"2021\"\nrust-version = \"1.88\"\nlicense = \"MIT\"\ndescription = \"ryolune native plugin SDK (the copy built into ryolune {VERSION})\"\n\n[dependencies]\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\n"
    );
    let mut files: Vec<(&str, &str)> = SDK_FILES.to_vec();
    files.push(("Cargo.toml", &cargo));
    files.push((".cargo-checksum.json", "{\"files\":{},\"package\":null}"));
    for (file, text) in files {
        let path = dir.join(file);
        if std::fs::read_to_string(&path).is_ok_and(|t| t == text) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
    }
    Ok(root)
}

fn cargo_config(sdk: &Path) -> String {
    let tag = sdk_tag();
    // TOML basic strings: a Windows path's backslashes are escaped.
    let sdk = sdk.display().to_string().replace('\\', "\\\\");
    format!(
        "# Written by ryolune: build against the SDK built into ryolune {VERSION} instead of\n# fetching it from GitHub. Delete this file to use the published SDK.\n[source.\"git+{SDK_GIT}?tag={tag}\"]\ngit = \"{SDK_GIT}\"\ntag = \"{tag}\"\nreplace-with = \"ryolune-sdk\"\n\n[source.ryolune-sdk]\ndirectory = \"{sdk}\"\n"
    )
}

fn seeded_lock() -> String {
    let tag = sdk_tag();
    format!(
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"ryolune-plugin\"\nversion = \"{VERSION}\"\nsource = \"git+{SDK_GIT}?tag={tag}#0000000000000000000000000000000000000000\"\n"
    )
}

/// A new crate: its name, its files (path, text) and its plugin id.
pub type CrateFiles = (String, Vec<(String, String)>, String);

/// The files of a new plugin crate: `Cargo.toml` (the SDK from GitHub by tag), the source
/// from the SDK template, its `plugin.toml`, a README, and the local SDK override.
pub fn crate_files(name: &str, instrument: bool, vendor: &str) -> Result<CrateFiles> {
    let crate_name = slug(name);
    if crate_name.is_empty() || name.len() > 60 || name.chars().any(|c| c.is_control() || c == '"')
    {
        return Err(
            "The plugin name needs letters or digits, at most 60 characters, no quotes".into(),
        );
    }
    if vendor.len() > 60 || vendor.chars().any(|c| c.is_control() || c == '"') {
        return Err("The vendor is at most 60 characters, no quotes".into());
    }
    let ty: String = crate_name
        .split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect();
    let ty = if ty.starts_with(|c: char| c.is_ascii_digit()) {
        format!("P{ty}")
    } else {
        ty
    };
    let id = format!(
        "com.{}.{}",
        slug(vendor).replace('-', ""),
        crate_name.replace('-', "")
    );
    let tag = sdk_tag();
    let cargo = format!(
        "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"cdylib\", \"rlib\"]\n\n[dependencies]\nryolune-plugin = {{ git = \"{SDK_GIT}\", tag = \"{tag}\" }}\n\n# Its own workspace: never part of a project around it.\n[workspace]\n"
    );
    let body = template(instrument)
        .replace("__TYPE__", &ty)
        .replace("__ID__", &id)
        .replace("__NAME__", name)
        .replace("__VENDOR__", vendor);
    let lib_name = crate_name.replace('-', "_");
    let manifest = Manifest {
        id: id.clone(),
        name: name.to_string(),
        version: "0.1.0".into(),
        app: APP.into(),
        kind: if instrument { "instrument" } else { "effect" }.into(),
        abi: ryolune_plugin::ABI_VERSION,
        description: if instrument {
            "A small polyphonic synth."
        } else {
            "Soft saturation with a blend control."
        }
        .into(),
        authors: vec![vendor.to_string()],
        library: Libraries {
            macos: format!("lib{lib_name}.dylib"),
            linux: format!("lib{lib_name}.so"),
            windows: format!("{lib_name}.dll"),
        },
    };
    let readme = format!(
        "# {name}\n\nA ryolune plugin (lsuite). Ask your agent, or by hand:\n\n    cargo test              # runs the plugin through the real plugin ABI\n    ryolune-cli plugin.build name={crate_name}\n    ryolune-cli plugin.publishLocal name={crate_name}   # installs it into ryolune, no restart\n\n`process` runs on the audio thread: no allocation, locks, files or logging there.\nParameters are stored in the song by ryolune, so they undo, save and automate for free.\nGuide: `ryolune-cli plugin.guide`, or {SDK_GIT}/blob/main/docs/NATIVE_PLUGINS.md\n"
    );
    let files = vec![
        ("Cargo.toml".to_string(), cargo),
        ("src/lib.rs".to_string(), body),
        (
            "plugin.toml".to_string(),
            toml::to_string_pretty(&manifest).map_err(|e| e.to_string())?,
        ),
        ("README.md".to_string(), readme),
        (".gitignore".to_string(), "/target\n".to_string()),
        ("Cargo.lock".to_string(), seeded_lock()),
    ];
    Ok((crate_name, files, id))
}

/// Write a new crate at `path` (which must not exist), with the local SDK override.
pub fn scaffold(path: &Path, name: &str, instrument: bool, vendor: &str) -> Result<Value> {
    if path.exists() {
        return Err(format!(
            "{} already exists; choose a new directory or name",
            path.display()
        ));
    }
    let (crate_name, files, id) = crate_files(name, instrument, vendor)?;
    let sdk = ensure_sdk()?;
    std::fs::create_dir_all(path.join("src")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(path.join(".cargo")).map_err(|e| e.to_string())?;
    let mut written = vec![];
    for (file, text) in &files {
        std::fs::write(path.join(file), text).map_err(|e| e.to_string())?;
        written.push(file.clone());
    }
    std::fs::write(path.join(".cargo/config.toml"), cargo_config(&sdk))
        .map_err(|e| e.to_string())?;
    written.push(".cargo/config.toml".into());
    Ok(json!({
        "name": crate_name,
        "path": path,
        "crate": crate_name,
        "kind": if instrument { "instrument" } else { "effect" },
        "pluginId": format!("native:{id}"),
        "files": written,
        "sdk": { "git": SDK_GIT, "tag": sdk_tag(), "localOverride": sdk },
        "next": ["plugin.writeSource (src/lib.rs)", "plugin.build", "plugin.publishLocal"],
    }))
}

/// `plugin.new`: a crate in the sources folder.
pub fn new_plugin(name: &str, kind: &str, vendor: &str) -> Result<Value> {
    if !["effect", "instrument"].contains(&kind) {
        return Err("kind must be effect or instrument".into());
    }
    let dir = sources_dir().join(slug(name));
    scaffold(&dir, name, kind == "instrument", vendor)
}

/// `plugin.writeSource`: one file inside the crate, never outside it.
pub fn write_source(name: &str, path: &str, contents: &str) -> Result<Value> {
    let dir = crate_dir(name)?;
    let relative = Path::new(path.trim());
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!(
            "`{path}` is not inside the plugin crate: give a path such as src/lib.rs"
        ));
    }
    if relative.starts_with("target") || relative.starts_with(".cargo") {
        return Err("target/ and .cargo/ are written by the build, not by hand".into());
    }
    if contents.len() > SOURCE_LIMIT {
        return Err("A source file is at most 1 MB".into());
    }
    let target = dir.join(relative);
    // A symlink inside the crate must not lead out of it.
    if let Ok(real) = target.canonicalize() {
        let root = dir.canonicalize().map_err(|e| e.to_string())?;
        if !real.starts_with(&root) {
            return Err(format!("`{path}` leads outside the plugin crate"));
        }
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    crate::document::atomic_write(&target, |f| {
        use std::io::Write;
        f.write_all(contents.as_bytes()).map_err(|e| e.to_string())
    })?;
    Ok(json!({ "written": target, "bytes": contents.len() }))
}

/// One compiler message, as an agent fixes it.
fn diagnostic(message: &Value, dir: &Path) -> Value {
    let spans = message["spans"].as_array().cloned().unwrap_or_default();
    let primary = spans
        .iter()
        .find(|s| s["is_primary"] == true)
        .or_else(|| spans.first());
    let file = primary
        .and_then(|s| s["file_name"].as_str())
        .map(|f| {
            Path::new(f)
                .strip_prefix(dir)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| f.to_string())
                .replace('\\', "/")
        })
        .unwrap_or_default();
    json!({
        "file": file,
        "line": primary.and_then(|s| s["line_start"].as_u64()).unwrap_or(0),
        "column": primary.and_then(|s| s["column_start"].as_u64()).unwrap_or(0),
        "message": message["message"].as_str().unwrap_or(""),
        "code": message["code"]["code"].as_str(),
        "rendered": message["rendered"].as_str().unwrap_or("").chars().take(4000).collect::<String>(),
    })
}

/// `cargo build --release` in the crate. `{ok, errors: [{file, line, column, message,
/// rendered}], warnings, library, seconds}`; never the whole log.
pub fn build(name: &str) -> Result<Value> {
    let dir = crate_dir(name)?;
    let sdk = ensure_sdk()?;
    // An older ryolune may have written the override: point it at this one's SDK.
    let config = dir.join(".cargo/config.toml");
    if config.is_file() {
        std::fs::write(&config, cargo_config(&sdk)).map_err(|e| e.to_string())?;
    }
    let cargo = cargo().ok_or_else(|| format!("Rust is not installed. {}", install_hint()))?;
    let started = Instant::now();
    let mut command = Command::new(&cargo);
    command
        .args(["build", "--release", "--message-format=json"])
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", target_dir())
        .env("CARGO_TERM_COLOR", "never")
        .env_remove("RUSTFLAGS")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(bin) = cargo.parent() {
        command.env("PATH", with_path(bin));
    }
    let output = command
        .output()
        .map_err(|e| format!("Could not start cargo: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut errors = vec![];
    let mut warnings = 0;
    let mut library: Option<PathBuf> = None;
    for line in stdout.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match event["reason"].as_str() {
            Some("compiler-message") => {
                let message = &event["message"];
                match message["level"].as_str() {
                    Some("error") | Some("error: internal compiler error") => {
                        if errors.len() < 50 {
                            errors.push(diagnostic(message, &dir));
                        }
                    }
                    Some("warning") => warnings += 1,
                    _ => {}
                }
            }
            Some("compiler-artifact")
                if event["target"]["kind"]
                    .as_array()
                    .is_some_and(|k| k.iter().any(|v| v == "cdylib")) =>
            {
                library = event["filenames"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(PathBuf::from)
                    .find(|p| {
                        p.extension()
                            .is_some_and(|e| e == native::library_extension())
                    });
            }
            _ => {}
        }
    }
    let ok = output.status.success() && library.is_some();
    if !ok && errors.is_empty() {
        // Cargo itself failed (a manifest, the network): its last lines say why.
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = stderr
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("Compiling"))
            .collect();
        let start = tail.len().saturating_sub(12);
        errors.push(json!({
            "file": "Cargo.toml", "line": 0, "column": 0,
            "message": tail[start..].join("\n"),
            "rendered": "",
        }));
    }
    Ok(json!({
        "ok": ok,
        "name": slug(name),
        "crate": dir,
        "errors": errors,
        "warnings": warnings,
        "library": library,
        "seconds": (started.elapsed().as_secs_f64() * 10.0).round() / 10.0,
    }))
}

/// `plugin.publishLocal`: build, make the bundle, install it and scan, so the plugin is in
/// the list now. A failed build answers with its errors and installs nothing.
pub fn publish_local(name: &str) -> Result<Value> {
    let built = build(name)?;
    if built["ok"] != true {
        return Ok(json!({ "ok": false, "build": built }));
    }
    let dir = crate_dir(name)?;
    let manifest = read_manifest(&dir)?;
    let library = PathBuf::from(built["library"].as_str().unwrap_or(""));
    // The library must load and export the id its manifest names before it is installed.
    let exported = native::inspect(&library)?;
    let wanted = format!("native:{}", manifest.id);
    if !exported.iter().any(|d| d.id == wanted) {
        let ids: Vec<&str> = exported.iter().map(|d| d.id.as_str()).collect();
        return Err(format!(
            "plugin.toml says id {} but the library exports {}. Make them the same (Info::effect / Info::instrument in src/lib.rs).",
            manifest.id,
            ids.join(", ")
        ));
    }
    let installed = install_library(&manifest, &library)?;
    let scanned = rescan()?;
    Ok(json!({
        "ok": true,
        "pluginId": wanted,
        "plugins": exported.iter().map(|d| json!({"id": d.id, "name": d.name, "kind": if d.instrument {"instrument"} else {"effect"}})).collect::<Vec<_>>(),
        "bundle": installed["bundle"],
        "library": installed["library"],
        "warnings": built["warnings"],
        "seconds": built["seconds"],
        "scan": scanned,
        "next": "Load it on a track (strip.insertPlugin / track.setInstrument) and listen.",
    }))
}

/// `plugin.remove`: an installed lsuite plugin (its bundle) or a library in ryolune's own
/// plugin folder. Stock plugins and other formats can only be disabled.
pub fn remove(descriptor: &crate::plugin::Descriptor) -> Result<Value> {
    let path = PathBuf::from(&descriptor.path);
    if descriptor.format != crate::plugin::Format::Native || !removable(&path) {
        return Err(format!(
            "{} is not an lsuite plugin you installed: stock plugins and {} plugins can only be turned off (plugin.disable).",
            descriptor.name,
            descriptor.format.prefix()
        ));
    }
    let target = if path.starts_with(installed_dir()) {
        path.parent().map(Path::to_path_buf).unwrap_or(path.clone())
    } else {
        path.clone()
    };
    let removed = if target.is_dir() {
        std::fs::remove_dir_all(&target)
    } else {
        std::fs::remove_file(&target)
    };
    if let Err(e) = removed {
        // Windows keeps a loaded library: retire it so the next scan forgets the plugin.
        let retired = path.with_extension(format!("{}.retired", native::library_extension()));
        std::fs::rename(&path, &retired)
            .map_err(|_| format!("Could not remove {}: {e}", target.display()))?;
    }
    let scanned = rescan()?;
    Ok(json!({ "removed": descriptor.id, "path": target, "scan": scanned }))
}

// ---------------------------------------------------------------------------------------
// The guide

fn template(instrument: bool) -> &'static str {
    if instrument {
        crate::control_plugins::INSTRUMENT_TEMPLATE
    } else {
        crate::control_plugins::EFFECT_TEMPLATE
    }
}

/// What `ryolune_plugin::prelude` brings into scope, read from the SDK this ryolune carries.
pub fn prelude_items() -> Vec<String> {
    let lib = SDK_FILES[0].1;
    let Some(start) = lib.find("pub mod prelude {") else {
        return vec![];
    };
    let body = &lib[start..];
    let body = &body[..body.find("\n}").unwrap_or(body.len())];
    let mut items = vec![];
    for chunk in body.split("pub use ").skip(1) {
        let chunk = chunk.split(';').next().unwrap_or("");
        let names = chunk
            .rsplit_once('{')
            .map_or(chunk, |(_, inside)| inside.trim_end_matches('}'));
        for name in names.split(',') {
            let name = name.trim().trim_end_matches('}').trim();
            let name = name.rsplit("::").next().unwrap_or(name);
            if !name.is_empty() && !items.iter().any(|i| i == name) {
                items.push(name.to_string());
            }
        }
    }
    items
}

/// `plugin.guide`: everything an agent needs to write a ryolune plugin, from the SDK this
/// ryolune carries (a test keeps it in step).
pub fn guide() -> String {
    let prelude = prelude_items().join(", ");
    let abi = ryolune_plugin::ABI_VERSION;
    let base = ryolune_plugin::BASE_ABI_VERSION;
    let block = ryolune_plugin::MAX_BLOCK;
    let effect = template(false)
        .replace("__TYPE__", "WarmDrive")
        .replace("__ID__", "com.you.warmdrive")
        .replace("__NAME__", "Warm Drive")
        .replace("__VENDOR__", "You");
    format!(
        r#"# Writing a ryolune plugin

A ryolune plugin is a Rust crate built on the SDK `ryolune-plugin` ({SDK_GIT}, tag `{tag}`,
plugin ABI {abi}; ABI {base} plugins still load). It exports one or more instruments or effects
that ryolune loads like its own: parameters are document state (they undo, save and automate
for free), a crash in a plugin call stops that plugin, not ryolune.

## The recipe

1. `plugin.guide` (this) and `plugin.toolchain`. When `ok` is false, Rust is missing: tell the
   person `installHint`; never install it without their consent.
2. `plugin.new {{name, kind}}`: kind `effect` (transforms audio) or `instrument` (plays notes).
   It writes a working crate in `~/.lsuite/plugins-src/ryolune/<name>/` from the template below
   and returns its files.
3. Write the code: `plugin.writeSource {{name, path, contents}}` (whole files, inside the crate
   only), or your own file tools in that folder. Usually only `src/lib.rs`; keep `plugin.toml`'s
   `id` equal to the id in `Info`.
4. `plugin.build {{name}}` until `ok` is true. Fix from `errors`: `{{file, line, column,
   message, rendered}}`.
5. `plugin.publishLocal {{name}}`: it is installed and scanned, no restart. Then try it: load it
   (`strip.insertPlugin` for an effect, `track.setInstrument` for an instrument, with its
   `pluginId`), play or render a bar (`session.bounce` or `transport.play`), and listen.

## Kinds

- **effect** (`Info::effect(id, name, vendor, category)`): `process` transforms the stereo
  buffer in place. Categories file it in the browser: Dynamics, EQ & Filter, Distortion,
  Modulation, Space & Time, Pitch, Utility, or your own word.
- **instrument** (`Info::instrument(id, name, vendor)`): `process` receives silence and adds its
  output; notes arrive as events (`process_events` with ABI {abi}: notes, controllers, pitch bend,
  pressure, and parameter changes at their frame).

## The SDK

`use ryolune_plugin::{{export_plugins, prelude::*}};` brings in: {prelude}.

- `trait Plugin`: `const INFO: Info`, `fn params() -> Vec<ParamSpec>`, `fn new(sample_rate: f64)`,
  `fn set_param(&mut self, index: usize, value: f64)`, `fn process(&mut self, audio: &mut [[f32; 2]],
  notes: &[NoteEvent], ctx: &ProcessContext)`; optional `reset`, `latency`, `process_events`,
  `save` / `load`, `tail_seconds`.
- Parameters: `param(name, min, max, default, unit)`, `hz(name, min, max, default)`,
  `choice(name, labels, default)`, `switch(name, default)`. Values arrive in plain units through
  `set_param`, in the order `params()` lists them, always inside their range.
- DSP: `Smoother` (click-free changes), `Biquad` (shelves, peaking, low/high/all-pass), `Svf`,
  `Delay` (a stereo line with fractional reads), `db_to_gain`, `db`, `coef`.
- `export_plugins!(TypeA, TypeB);` exports them (ABI {abi}, and ABI {base} for older hosts).
- Tests: `ryolune_plugin::testing::Bench` runs a plugin the way the host does (`set`, `process`,
  `sine`, `note`, `play`, `peak`, `assert_sane`); `cargo test` in the crate.

## Rules

- `process` runs on the audio thread: no allocation, locks, file or network I/O, logging or
  panics in it. Make buffers in `new`. Blocks are at most {block} frames.
- Smooth what a person turns (`Smoother`): a jump clicks. Keep output finite and near unity gain.
- An `id` is forever (songs store `native:<id>`): reverse-DNS, `com.<you>.<plugin>`.
- No dependencies beyond the SDK unless they are pure Rust and never allocate in `process`.

## plugin.toml (the bundle manifest)

```toml
id = "com.you.warmdrive"      # the same as Info
name = "Warm Drive"
version = "0.1.0"
app = "ryolune"
kind = "effect"               # or "instrument"
abi = {abi}
description = "One line people read in the Plugins window."
authors = ["You"]

[library]                     # publishLocal fills these in
macos = "libwarm_drive.dylib"
linux = "libwarm_drive.so"
windows = "warm_drive.dll"
```

## Example (the effect template)

```rust
{effect}```
"#,
        tag = sdk_tag(),
    )
}

// ---------------------------------------------------------------------------------------
// Commands

pub const SPECS: &[Spec] = &[
    query("plugin.info", "One plugin, as the Plugins window shows it: name, kind, format, vendor, version, description, whether it is on, where it came from (its bundle and plugin.toml for lsuite plugins) and its parameters.", &[
        req("id", Kind::String, "Plugin id from plugin.list (stock:Space, native:com.you.drive, clap:…), or its name."),
    ]),
    edit("plugin.enable", "Turn a plugin back on: it shows in the browser and agents can load it again.", &[
        req("id", Kind::String, "Plugin id from plugin.list."),
    ]),
    edit("plugin.disable", "Turn a plugin off without deleting it (a setting): it leaves the browser and agents cannot load it; songs that use it still play it.", &[
        req("id", Kind::String, "Plugin id from plugin.list."),
    ]),
    edit("plugin.remove", "Delete an lsuite plugin you installed (its bundle in ~/.lsuite/plugins/ryolune) and rescan. Stock plugins and CLAP, VST3 or Audio Unit plugins can only be disabled.", &[
        req("id", Kind::String, "Plugin id from plugin.list."),
    ]),
    query("plugin.guide", "How to write a ryolune plugin in Rust, for an agent: the SDK, the kinds, plugin.toml, the rules of the audio thread, an example and the recipe (plugin.toolchain, plugin.new, plugin.writeSource, plugin.build, plugin.publishLocal). Markdown, made from the SDK this ryolune carries.", &[]),
    query("plugin.toolchain", "Whether Rust is installed to build plugins: cargo and rustc paths, the version, ok, and how to install it (rustup) when it is missing.", &[]),
    edit("plugin.new", "Start a plugin crate from the SDK template in ~/.lsuite/plugins-src/ryolune/<name>/: a working effect or instrument, its plugin.toml and tests. Returns its path and files.", &[
        req("name", Kind::String, "Plugin display name, for example Warm Drive."),
        req("kind", Kind::String, "effect or instrument."),
        opt("vendor", Kind::String, "Your name or label, default the person's lsuite name or My Studio."),
    ]),
    edit("plugin.writeSource", "Write one whole file of a plugin crate made by plugin.new (src/lib.rs, a new module, plugin.toml). Paths outside the crate are refused.", &[
        req("name", Kind::String, "The plugin's name or crate name from plugin.new."),
        req("path", Kind::String, "Path inside the crate, such as src/lib.rs."),
        req("contents", Kind::String, "The whole file."),
    ]),
    edit("plugin.build", "Build a plugin crate (cargo build --release). Returns ok and the compiler's errors as {file, line, column, message, rendered}, never the whole log. The first build takes a minute; later ones seconds. Runs as a job in the app.", &[
        req("name", Kind::String, "The plugin's name or crate name from plugin.new."),
    ]),
    edit("plugin.publishLocal", "Build a plugin crate, install it as an lsuite plugin bundle and load it: it is in plugin.list at once, and songs already using it reload it, without restarting ryolune. A failed build installs nothing and returns its errors.", &[
        req("name", Kind::String, "The plugin's name or crate name from plugin.new."),
    ]),
];

pub fn serves(name: &str) -> bool {
    SPECS.iter().any(|s| s.name == name)
}

/// Building, installing and switching plugins is the agent's only with the `plugins`
/// permission (Settings › Agent; off until the person allows it).
pub fn denied_for_agent(name: &str, permissions: &crate::settings::Permissions) -> Option<String> {
    let gated = matches!(
        name,
        "plugin.new"
            | "plugin.writeSource"
            | "plugin.build"
            | "plugin.publishLocal"
            | "plugin.install"
            | "plugin.remove"
            | "plugin.enable"
            | "plugin.disable"
            | "plugin.scaffold"
    );
    (gated && !permissions.plugins).then(|| {
        format!("{name} is not allowed for agents: building and installing plugins is off in Settings › Agent (plugins). Ask the person to allow it, or to press Build in the Plugins window.")
    })
}

/// The descriptor an id (or a unique name) means, enabled or not.
pub fn find(id: &str) -> Result<crate::plugin::Descriptor> {
    let all = scan::installed();
    if let Some(d) = all.iter().find(|d| d.id == id) {
        return Ok(d.clone());
    }
    let lower = id.to_lowercase();
    let named: Vec<_> = all
        .iter()
        .filter(|d| d.name.to_lowercase() == lower)
        .collect();
    match named.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!("No plugin `{id}`. plugin.list shows them.")),
        many => Err(format!(
            "`{id}` is several plugins: {}",
            many.iter()
                .map(|d| d.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Where a plugin came from, for the Installed list and `plugin.info`.
pub fn origin(descriptor: &crate::plugin::Descriptor) -> Value {
    if descriptor.format.prefix() == "stock" {
        return json!({ "kind": "stock" });
    }
    let path = Path::new(&descriptor.path);
    if descriptor.format == crate::plugin::Format::Native {
        if let Some((bundle, manifest)) = bundle_of(path) {
            return json!({
                "kind": "lsuite", "bundle": bundle, "version": manifest.version,
                "description": manifest.description, "authors": manifest.authors,
                "removable": removable(path),
                "source": sources_dir().join(slug(&manifest.name)).join("Cargo.toml").is_file()
                    .then(|| sources_dir().join(slug(&manifest.name))),
            });
        }
        return json!({ "kind": "native", "removable": removable(path) });
    }
    json!({ "kind": descriptor.format.prefix() })
}

pub(crate) fn call(host: &mut dyn Host, name: &str, a: &Args) -> Result<Value> {
    match name {
        "plugin.guide" => Ok(
            json!({ "markdown": guide(), "abi": ryolune_plugin::ABI_VERSION, "sdk": { "git": SDK_GIT, "tag": sdk_tag() } }),
        ),
        "plugin.toolchain" => Ok(toolchain()),
        "plugin.new" => {
            let vendor = a
                .opt_str("vendor")
                .map(str::to_string)
                .or_else(|| {
                    crate::account::load()
                        .map(|acc| acc.name)
                        .filter(|n| !n.trim().is_empty())
                })
                .unwrap_or_else(|| "My Studio".into());
            new_plugin(a.str("name")?, a.str("kind")?, &vendor)
        }
        "plugin.writeSource" => write_source(a.str("name")?, a.str("path")?, a.str("contents")?),
        "plugin.build" => build(a.str("name")?),
        "plugin.publishLocal" => publish_local(a.str("name")?),
        "plugin.info" => {
            let descriptor = find(a.str("id")?)?;
            let mut value = crate::control::call(
                host,
                "plugin.describe",
                &json!({ "pluginId": descriptor.id }),
                false,
            )
            .unwrap_or_else(|_| serde_json::to_value(&descriptor).unwrap_or_default());
            let settings = host.settings();
            value["id"] = json!(descriptor.id);
            value["enabled"] = json!(!settings.plugins.disabled.contains(&descriptor.id));
            value["origin"] = origin(&descriptor);
            if value["description"].is_null() {
                value["description"] = value["origin"]["description"].clone();
            }
            Ok(value)
        }
        "plugin.enable" | "plugin.disable" => {
            let descriptor = find(a.str("id")?)?;
            let mut settings = host.settings();
            settings.plugins.disabled.retain(|id| *id != descriptor.id);
            if name == "plugin.disable" {
                settings.plugins.disabled.push(descriptor.id.clone());
            }
            settings.validate()?;
            host.update_settings(settings)?;
            Ok(
                json!({ "id": descriptor.id, "name": descriptor.name, "enabled": name == "plugin.enable" }),
            )
        }
        "plugin.remove" => {
            let descriptor = find(a.str("id")?)?;
            let reply = remove(&descriptor)?;
            let mut settings = host.settings();
            if settings.plugins.disabled.contains(&descriptor.id) {
                settings.plugins.disabled.retain(|id| *id != descriptor.id);
                host.update_settings(settings)?;
            }
            Ok(reply)
        }
        _ => Err(format!("Unknown plugin command `{name}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_follows_the_sdk() {
        let guide = guide();
        let items = prelude_items();
        assert!(items.len() > 10, "{items:?}");
        for item in &items {
            assert!(guide.contains(item.as_str()), "the guide names {item}");
        }
        for tool in [
            "plugin.toolchain",
            "plugin.new",
            "plugin.writeSource",
            "plugin.build",
            "plugin.publishLocal",
        ] {
            assert!(guide.contains(tool), "{tool}");
        }
        assert!(guide.contains(&format!("plugin ABI {}", ryolune_plugin::ABI_VERSION)));
        assert!(guide.contains("export_plugins!(WarmDrive);"));
        // The SDK built in is the SDK in this repository.
        assert_eq!(
            SDK_FILES.len(),
            std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../sdk/src"))
                .unwrap()
                .count()
        );
    }

    #[test]
    fn sources_stay_inside_their_crate() {
        assert_eq!(slug("Warm Drive 2"), "warm-drive-2");
        let (name, files, id) = crate_files("Night Crush", false, "Ada L").unwrap();
        assert_eq!(name, "night-crush");
        assert_eq!(id, "com.adal.nightcrush");
        let cargo = &files.iter().find(|(f, _)| f == "Cargo.toml").unwrap().1;
        assert!(cargo.contains(&format!("git = \"{SDK_GIT}\", tag = \"{}\"", sdk_tag())));
        let manifest: Manifest =
            toml::from_str(&files.iter().find(|(f, _)| f == "plugin.toml").unwrap().1).unwrap();
        assert_eq!(manifest.id, id);
        assert_eq!(manifest.library.linux, "libnight_crush.so");
        assert!(safe_id("../evil").is_err());
    }
}
