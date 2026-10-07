//! Recovery snapshot files: naming, validation and listing, shared by the window's recovery
//! worker and the `session.snapshots` command.

use crate::{host::scan::data_dir, Result};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn directory() -> PathBuf {
    data_dir().join("recovery")
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub path: PathBuf,
    pub title: String,
    #[serde(skip)]
    pub modified: SystemTime,
    pub modified_unix: u64,
    pub bytes: u64,
}

pub fn generated_path(directory: &Path, run: u128, generation: u64, name: &str) -> PathBuf {
    let title: String = name
        .trim_end_matches(".ryolune")
        .chars()
        .take(80)
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    directory.join(format!(
        "recovery-{run}-{}-{generation}-{}.{}",
        std::process::id(),
        if title.is_empty() { "Untitled" } else { &title },
        crate::document::EXTENSION
    ))
}

pub fn generated_title(path: &Path) -> Option<String> {
    if !crate::document::is_session_path(path) {
        return None;
    }
    let name = path.file_stem()?.to_str()?.strip_prefix("recovery-")?;
    let mut parts = name.splitn(4, '-');
    parts.next()?.parse::<u128>().ok()?;
    parts.next()?.parse::<u32>().ok()?;
    parts.next()?.parse::<u64>().ok()?;
    let title = parts.next()?;
    if title.is_empty()
        || !title
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    Some(title.replace('_', " "))
}

/// Only regular, ryolune-generated files inside the recovery directory may be restored.
pub fn ensure_generated(directory: &Path, path: &Path) -> Result<()> {
    if generated_title(path).is_none() {
        return Err("Choose a ryolune-generated recovery snapshot.".into());
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() {
        return Err("Recovery requires a regular generated snapshot file.".into());
    }
    let expected = directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let actual = path
        .parent()
        .ok_or("Recovery path has no parent")?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if actual != expected {
        return Err("Recovery files must be in ryolune's recovery directory.".into());
    }
    Ok(())
}

/// Snapshots newest first.
pub fn list(directory: &Path) -> Result<Vec<Snapshot>> {
    if !directory.exists() {
        return Ok(vec![]);
    }
    let mut candidates = vec![];
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            continue;
        }
        let path = entry.path();
        let Some(title) = generated_title(&path) else {
            continue;
        };
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        candidates.push(Snapshot {
            path,
            title,
            modified,
            modified_unix: modified
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            bytes: metadata.len(),
        });
    }
    candidates.sort_by_key(|c| std::cmp::Reverse(c.modified));
    Ok(candidates)
}
