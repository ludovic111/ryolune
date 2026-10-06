//! Plugin discovery. ryolune native, CLAP, VST3, LV2 and LADSPA bundles are probed in a child
//! process so a crashing plugin cannot take the session down; Audio Units come from the
//! system registry. Results are cached next to the user's application data.
//!
//! LV2 and LADSPA results are kept in a file of their own, [`open_cache_path`]: the cache is
//! shared with kimchi and with older versions of both apps, and a version that does not know
//! a format would fail to read the whole file (and rescan, dropping the new formats). Old
//! versions never open the second file and never write it.

use crate::plugin::{Descriptor, Format};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const CACHE_VERSION: u32 = 1;
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CacheEntry {
    pub path: String,
    pub modified: u64,
    pub format: Option<Format>,
    #[serde(default)]
    pub descriptors: Vec<Descriptor>,
    #[serde(default)]
    pub error: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Cache {
    pub version: u32,
    #[serde(default)]
    pub entries: Vec<CacheEntry>,
    #[serde(default)]
    pub scanned_at: u64,
}
impl Cache {
    pub fn descriptors(&self) -> Vec<Descriptor> {
        let mut all: Vec<Descriptor> = self
            .entries
            .iter()
            .flat_map(|e| e.descriptors.iter().cloned())
            .collect();
        all.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        all.dedup_by(|a, b| a.id == b.id);
        all
    }
    pub fn find(&self, id: &str) -> Option<Descriptor> {
        self.entries
            .iter()
            .flat_map(|e| e.descriptors.iter())
            .find(|d| d.id == id)
            .cloned()
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}
/// Folder name the project used before it was renamed ryolune, per platform.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const LEGACY_NAME: &str = "Ondera";
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const LEGACY_NAME: &str = "ondera";

pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RYOLUNE_DATA_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(sandbox) = test_sandbox() {
        return sandbox.join("data");
    }
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| adopt_legacy(platform_dir(LEGACY_NAME), platform_dir("ryolune")))
        .clone()
}
/// A cargo test binary (`target/<profile>/deps/<name>-<hash>`) never touches the person's
/// profile, even when a test forgets `RYOLUNE_SETTINGS` / `RYOLUNE_DATA_DIR`: settings, data,
/// the control file and the lsuite folder all go to a scratch folder for the process.
pub fn test_sandbox() -> Option<PathBuf> {
    static SANDBOX: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    SANDBOX
        .get_or_init(|| {
            let exe = std::env::current_exe().ok()?;
            let in_deps = exe
                .parent()
                .and_then(|p| p.file_name())
                .is_some_and(|n| n == "deps");
            in_deps
                .then(|| std::env::temp_dir().join(format!("ryolune-test-{}", std::process::id())))
        })
        .clone()
}
fn platform_dir(name: &str) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home()
            .map(|h| h.join("Library/Application Support").join(name))
            .unwrap_or_else(|| PathBuf::from("."))
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(|a| PathBuf::from(a).join(name))
            .or_else(|| home().map(|h| h.join(name)))
            .unwrap_or_else(|| PathBuf::from("."))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(|a| PathBuf::from(a).join(name))
            .or_else(|| home().map(|h| h.join(".config").join(name)))
            .unwrap_or_else(|| PathBuf::from("."))
    }
}
/// The data folder of an install from before the rename (settings, presets, recovery
/// snapshots, plugin cache) becomes the new one the first time it is needed. If the move
/// fails the old folder is used as it is, so nothing is lost or left behind half-copied.
fn adopt_legacy(legacy: PathBuf, current: PathBuf) -> PathBuf {
    if current.exists() || !legacy.is_dir() {
        return current;
    }
    match std::fs::rename(&legacy, &current) {
        Ok(()) => current,
        Err(_) => legacy,
    }
}
pub fn cache_path() -> PathBuf {
    data_dir().join("plugins.json")
}
/// Where LV2 and LADSPA scan results live (see the module notes).
pub fn open_cache_path() -> PathBuf {
    data_dir().join("plugins-lv2-ladspa.json")
}
/// Both cache files, for callers that watch them for changes.
pub fn cache_paths() -> [PathBuf; 2] {
    [cache_path(), open_cache_path()]
}
/// Formats stored in [`open_cache_path`].
fn in_open_cache(format: Option<Format>) -> bool {
    matches!(format, Some(Format::Lv2) | Some(Format::Ladspa))
}
/// Standard bundle directories per format, plus `CLAP_PATH` / `VST3_PATH` / `LV2_PATH` /
/// `LADSPA_PATH` / `RYOLUNE_PLUGIN_PATH` and the extra paths from Settings > Plugins.
pub fn directories(format: Format) -> Vec<PathBuf> {
    let mut dirs = vec![];
    let env = match format {
        Format::Clap => Some("CLAP_PATH"),
        Format::Vst3 => Some("VST3_PATH"),
        Format::Native => Some("RYOLUNE_PLUGIN_PATH"),
        Format::Lv2 => Some("LV2_PATH"),
        Format::Ladspa => Some("LADSPA_PATH"),
        _ => None,
    };
    if let Some(var) = env.and_then(std::env::var_os) {
        dirs.extend(std::env::split_paths(&var));
    }
    dirs.extend(crate::settings::Settings::load().extra_plugin_paths(format));
    let home = home();
    match format {
        Format::Native => {
            dirs.push(data_dir().join("plugins"));
            // Folders under the former name stay scanned, after the new ones.
            for name in ["ryolune", LEGACY_NAME] {
                #[cfg(target_os = "macos")]
                {
                    if let Some(h) = &home {
                        dirs.push(h.join("Library/Audio/Plug-Ins").join(name));
                    }
                    dirs.push(PathBuf::from("/Library/Audio/Plug-Ins").join(name));
                }
                #[cfg(target_os = "windows")]
                {
                    if let Some(p) = std::env::var_os("COMMONPROGRAMFILES") {
                        dirs.push(PathBuf::from(p).join(name).join("Plugins"));
                    }
                }
                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                {
                    if let Some(h) = &home {
                        dirs.push(h.join(".local/lib").join(name).join("plugins"));
                    }
                    dirs.push(PathBuf::from("/usr/lib").join(name).join("plugins"));
                    dirs.push(PathBuf::from("/usr/local/lib").join(name).join("plugins"));
                }
            }
        }
        Format::Clap => {
            #[cfg(target_os = "macos")]
            {
                if let Some(h) = &home {
                    dirs.push(h.join("Library/Audio/Plug-Ins/CLAP"));
                }
                dirs.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
            }
            #[cfg(target_os = "windows")]
            {
                if let Some(p) = std::env::var_os("COMMONPROGRAMFILES") {
                    dirs.push(PathBuf::from(p).join("CLAP"));
                }
                if let Some(p) = std::env::var_os("LOCALAPPDATA") {
                    dirs.push(PathBuf::from(p).join("Programs/Common/CLAP"));
                }
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            {
                if let Some(h) = &home {
                    dirs.push(h.join(".clap"));
                }
                dirs.push(PathBuf::from("/usr/lib/clap"));
                dirs.push(PathBuf::from("/usr/local/lib/clap"));
            }
        }
        Format::Vst3 => {
            #[cfg(target_os = "macos")]
            {
                if let Some(h) = &home {
                    dirs.push(h.join("Library/Audio/Plug-Ins/VST3"));
                }
                dirs.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
            }
            #[cfg(target_os = "windows")]
            {
                if let Some(p) = std::env::var_os("COMMONPROGRAMFILES") {
                    dirs.push(PathBuf::from(p).join("VST3"));
                }
                if let Some(p) = std::env::var_os("LOCALAPPDATA") {
                    dirs.push(PathBuf::from(p).join("Programs/Common/VST3"));
                }
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            {
                if let Some(h) = &home {
                    dirs.push(h.join(".vst3"));
                }
                dirs.push(PathBuf::from("/usr/lib/vst3"));
                dirs.push(PathBuf::from("/usr/local/lib/vst3"));
            }
        }
        Format::Lv2 => {
            if let Some(h) = &home {
                dirs.push(h.join(".lv2"));
            }
            #[cfg(target_os = "macos")]
            {
                if let Some(h) = &home {
                    dirs.push(h.join("Library/Audio/Plug-Ins/LV2"));
                }
                dirs.push(PathBuf::from("/Library/Audio/Plug-Ins/LV2"));
            }
            #[cfg(target_os = "windows")]
            {
                if let Some(p) = std::env::var_os("APPDATA") {
                    dirs.push(PathBuf::from(p).join("LV2"));
                }
                if let Some(p) = std::env::var_os("COMMONPROGRAMFILES") {
                    dirs.push(PathBuf::from(p).join("LV2"));
                }
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            dirs.extend(unix_lib_dirs("lv2"));
        }
        Format::Ladspa => {
            if let Some(h) = &home {
                dirs.push(h.join(".ladspa"));
            }
            #[cfg(target_os = "macos")]
            {
                if let Some(h) = &home {
                    dirs.push(h.join("Library/Audio/Plug-Ins/LADSPA"));
                }
                dirs.push(PathBuf::from("/Library/Audio/Plug-Ins/LADSPA"));
            }
            #[cfg(target_os = "windows")]
            {
                if let Some(p) = std::env::var_os("COMMONPROGRAMFILES") {
                    dirs.push(PathBuf::from(p).join("LADSPA"));
                }
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            dirs.extend(unix_lib_dirs("ladspa"));
        }
        _ => {}
    }
    let _ = home;
    dirs.retain(|d| !d.as_os_str().is_empty());
    dirs
}
/// The system library folders for LV2 or LADSPA on Linux and the BSDs, most local first.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn unix_lib_dirs(name: &str) -> Vec<PathBuf> {
    let multiarch = if cfg!(target_arch = "aarch64") {
        "aarch64-linux-gnu"
    } else {
        "x86_64-linux-gnu"
    };
    [
        format!("/usr/local/lib/{name}"),
        format!("/usr/lib/{name}"),
        format!("/usr/lib/{multiarch}/{name}"),
        format!("/usr/local/lib64/{name}"),
        format!("/usr/lib64/{name}"),
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}
fn extensions(format: Format) -> Vec<&'static str> {
    match format {
        Format::Clap => vec!["clap"],
        Format::Vst3 => vec!["vst3"],
        Format::Native => vec!["onplug", super::native::library_extension()],
        Format::Lv2 => vec!["lv2"],
        // LADSPA libraries on macOS are as often `.so` as `.dylib`.
        Format::Ladspa if cfg!(target_os = "macos") => vec!["so", "dylib"],
        Format::Ladspa => vec![super::native::library_extension()],
        _ => vec![],
    }
}
/// Bundle paths found on disk for a format, searched two levels deep.
pub fn candidates(format: Format) -> Vec<PathBuf> {
    let mut found = vec![];
    let extensions = extensions(format);
    for dir in directories(format) {
        walk(&dir, &extensions, 0, &mut found);
    }
    if format == Format::Lv2 {
        // A bundle is a folder with a manifest; `.lv2` files are something else.
        found.retain(|p| p.join("manifest.ttl").is_file());
    } else if format == Format::Ladspa {
        found.retain(|p| p.is_file());
    }
    found.sort();
    found.dedup();
    // The same bundle reached through two folders (a symlinked system folder) once.
    let mut seen = std::collections::HashSet::new();
    found.retain(|p| seen.insert(std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())));
    found
}
fn walk(dir: &Path, extensions: &[&str], depth: usize, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|e| extensions.iter().any(|ext| e.eq_ignore_ascii_case(ext)))
        {
            found.push(path);
        } else if depth < 2 && path.is_dir() {
            walk(&path, extensions, depth + 1, found);
        }
    }
}
fn modified(path: &Path) -> u64 {
    let of = |p: &Path| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs())
    };
    // An LV2 bundle changes when its manifest does, even if the folder itself does not.
    of(path).max(of(&path.join("manifest.ttl")))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn cell() -> &'static Mutex<Option<Cache>> {
    static CACHE: OnceLock<Mutex<Option<Cache>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}
/// The cached scan results, read from disk once.
pub fn cache() -> Cache {
    let mut guard = cell().lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(load_cache());
    }
    guard.clone().unwrap_or_default()
}
fn read_cache(path: &Path) -> Option<Cache> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Cache>(&s).ok())
        .filter(|c| c.version == CACHE_VERSION)
}
/// Both cache files as one cache. Each file only contributes the formats it holds.
pub fn load_cache() -> Cache {
    let mut cache = read_cache(&cache_path()).unwrap_or_default();
    cache.entries.retain(|e| !in_open_cache(e.format));
    if let Some(open) = read_cache(&open_cache_path()) {
        cache.version = CACHE_VERSION;
        cache.entries.extend(open.entries.into_iter().filter(|e| in_open_cache(e.format)));
        cache.scanned_at = cache.scanned_at.max(open.scanned_at);
    }
    cache
}
fn write_cache(path: &Path, cache: &Cache) -> crate::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(cache).map_err(|e| e.to_string())?;
    crate::document::atomic_write(path, |f| {
        use std::io::Write;
        f.write_all(json.as_bytes()).map_err(|e| e.to_string())
    })
}
pub fn store_cache(cache: &Cache) -> crate::Result<()> {
    let (open, main): (Vec<CacheEntry>, Vec<CacheEntry>) =
        cache.entries.iter().cloned().partition(|e| in_open_cache(e.format));
    let part = |entries| Cache {
        version: cache.version,
        entries,
        scanned_at: cache.scanned_at,
    };
    write_cache(&cache_path(), &part(main))?;
    write_cache(&open_cache_path(), &part(open))?;
    *cell().lock().unwrap_or_else(|e| e.into_inner()) = Some(cache.clone());
    Ok(())
}
/// Look a descriptor up by id in the cache.
pub fn lookup(id: &str) -> Option<Descriptor> {
    cache().find(id)
}
/// Stock plugins followed by every cached external plugin.
pub fn installed() -> Vec<Descriptor> {
    let mut all = crate::stock::descriptors();
    all.extend(cache().descriptors());
    all
}

/// Probe one bundle in this process. Used by the `--scan-plugin` child.
pub fn probe(format: Format, path: &Path) -> crate::Result<Vec<Descriptor>> {
    match format {
        Format::Native => super::native::scan(path),
        Format::Clap => super::clap::scan(path),
        Format::Vst3 => super::vst3::scan(path),
        Format::Lv2 => super::lv2::scan(path),
        Format::Ladspa => super::ladspa::scan(path),
        _ => Err("Only native, CLAP, VST3, LV2 and LADSPA bundles are probed by path".into()),
    }
}
/// Probe a bundle in a child process with a timeout.
fn probe_isolated(format: Format, path: &Path) -> Result<Vec<Descriptor>, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut child = std::process::Command::new(exe)
        .arg("--scan-plugin")
        .arg(format.prefix())
        .arg(path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    let mut stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        if let Some(s) = stdout.as_mut() {
            use std::io::Read;
            let _ = s.read_to_string(&mut out);
        }
        out
    });
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().unwrap_or_default();
                if !status.success() {
                    return Err(format!("Plugin crashed while scanning ({status})"));
                }
                // Plugins may print to stdout while loading; the report is the last JSON line.
                let report = out
                    .lines()
                    .rev()
                    .find_map(|line| {
                        serde_json::from_str::<Result<Vec<Descriptor>, String>>(line.trim()).ok()
                    })
                    .ok_or_else(|| "Unreadable scan result".to_string())?;
                return report;
            }
            Ok(None) if started.elapsed() > PROBE_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Plugin did not respond while scanning".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.to_string()),
        }
    }
}
/// Scan every format. Unchanged bundles reuse their cached results; `progress`
/// receives the bundle currently being probed.
pub fn scan_all(mut progress: impl FnMut(&str)) -> Cache {
    let previous = load_cache();
    let known: HashMap<String, CacheEntry> = previous
        .entries
        .into_iter()
        .map(|e| (e.path.clone(), e))
        .collect();
    let mut entries = vec![];
    for format in [
        Format::Native,
        Format::Clap,
        Format::Vst3,
        Format::Lv2,
        Format::Ladspa,
    ] {
        for path in candidates(format) {
            let key = path.to_string_lossy().to_string();
            let stamp = modified(&path);
            if let Some(entry) = known.get(&key) {
                if entry.modified == stamp && entry.error.is_none() {
                    entries.push(entry.clone());
                    continue;
                }
            }
            progress(&key);
            let mut entry = CacheEntry {
                path: key,
                modified: stamp,
                format: Some(format),
                descriptors: vec![],
                error: None,
            };
            match probe_isolated(format, &path) {
                Ok(descriptors) => entry.descriptors = descriptors,
                Err(e) => entry.error = Some(e),
            }
            entries.push(entry);
        }
    }
    #[cfg(target_os = "macos")]
    {
        progress("Audio Units");
        entries.push(CacheEntry {
            path: "AudioUnits".into(),
            modified: 0,
            format: Some(Format::AudioUnit),
            descriptors: super::au::scan(),
            error: None,
        });
    }
    let cache = Cache {
        version: CACHE_VERSION,
        entries,
        scanned_at: now(),
    };
    let _ = store_cache(&cache);
    cache
}

#[cfg(test)]
mod tests {
    use super::adopt_legacy;

    #[test]
    fn an_install_from_before_the_rename_keeps_its_data() {
        let root = tempfile::tempdir().unwrap();
        let (legacy, current) = (root.path().join("Ondera"), root.path().join("ryolune"));
        std::fs::create_dir(&legacy).unwrap();
        std::fs::write(legacy.join("settings.json"), "{}").unwrap();
        assert_eq!(adopt_legacy(legacy.clone(), current.clone()), current);
        assert!(current.join("settings.json").is_file() && !legacy.exists());
        // Once the new folder exists, an old one that reappears is left alone.
        std::fs::create_dir(&legacy).unwrap();
        assert_eq!(adopt_legacy(legacy.clone(), current.clone()), current);
        assert!(legacy.exists());
    }
}
