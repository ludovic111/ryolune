use crate::{
    audio::{self, Library},
    model::Session,
    Result,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, io::Write, path::Path, sync::Arc};

/// Extension of a session file.
pub const EXTENSION: &str = "ryolune";
/// Extension of session files saved before the project was renamed from Ondera; they open
/// unchanged and keep their name when saved again.
pub const LEGACY_EXTENSION: &str = "ondera";
/// Every extension a session file may carry, newest first.
pub const EXTENSIONS: [&str; 2] = [EXTENSION, LEGACY_EXTENSION];
const FORMAT: &str = "ryolune-session";
/// The `format` field of files written before the rename.
const LEGACY_FORMAT: &str = "ondera-session";

/// Whether `path` names a session file by its extension.
pub fn is_session_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

#[derive(Deserialize, Serialize)]
struct SessionFile {
    format: String,
    version: u32,
    session: Session,
    audio: HashMap<String, String>,
}

pub fn decode_session(json: &str) -> Result<(Session, Library)> {
    if json.len() > 768 * 1024 * 1024 {
        return Err("Session file exceeds 768 MiB".into());
    }
    let mut file: SessionFile =
        serde_json::from_str(json).map_err(|e| format!("Invalid session: {e}"))?;
    if ![FORMAT, LEGACY_FORMAT].contains(&file.format.as_str()) || file.version != 1 {
        return Err("Unsupported ryolune session format or version".into());
    }
    file.session.normalize();
    file.session.validate()?;
    let mut library = Library::new();
    for (id, src) in &file.session.sources {
        if src.origin == "generated" {
            continue;
        }
        let data = file
            .audio
            .remove(id)
            .ok_or_else(|| format!("Session is missing embedded audio: {}", src.name))?;
        let bytes = STANDARD
            .decode(data)
            .map_err(|e| format!("Invalid embedded audio: {e}"))?;
        let buffer = audio::decode(bytes, Some("wav"))?;
        if audio::library_bytes(&library).saturating_add(buffer.frames.len() * 8)
            > audio::MAX_LIBRARY_BYTES
        {
            return Err("Decoded session audio exceeds 1 GiB".into());
        }
        library.insert(id.clone(), Arc::new(buffer));
    }
    audio::prepare_sources(&file.session, &mut library)?;
    file.session.transport.playing = false;
    file.session.transport.recording = false;
    Ok((file.session, library))
}
pub fn load(path: &Path) -> Result<(Session, Library)> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 768 * 1024 * 1024 {
        return Err("Session file exceeds 768 MiB".into());
    }
    let (mut session, library) =
        decode_session(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)?;
    if session.id.is_empty() {
        session.id = Session::id_for_path(path);
    }
    Ok((session, library))
}
pub fn save(session: &Session, library: &Library, path: &Path) -> Result<()> {
    session.validate()?;
    let encoded_bytes: usize = session
        .sources
        .values()
        .filter(|s| s.origin != "generated")
        .filter_map(|s| library.get(&s.id))
        .map(|b| (b.frames.len() * 8 + 128).div_ceil(3) * 4)
        .sum();
    if encoded_bytes > 700 * 1024 * 1024 {
        return Err(
            "Embedded audio exceeds the single-file session limit (700 MiB encoded)".into(),
        );
    }
    let mut audio = HashMap::new();
    for src in session.sources.values().filter(|s| s.origin != "generated") {
        let buf = library
            .get(&src.id)
            .ok_or_else(|| format!("Cannot save: audio missing for {}", src.name))?;
        audio.insert(src.id.clone(), STANDARD.encode(audio::encode_wav(buf)?));
    }
    let mut stopped = session.clone();
    stopped.transport.playing = false;
    stopped.transport.recording = false;
    let file = SessionFile {
        format: FORMAT.into(),
        version: 1,
        session: stopped,
        audio,
    };
    atomic_write(path, |f| {
        let mut writer = std::io::BufWriter::new(f);
        serde_json::to_writer(&mut writer, &file).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())
    })
}
/// Write, flush and sync a sibling temporary file, then atomically replace the destination.
/// The result has the permissions a plain write would give it: the replaced file's, or
/// readable by others for a new one (the temporary file starts owner-only, so a re-saved song
/// or an exported mix would otherwise turn 0600). A symlinked destination is written through.
/// `write` may tighten the permissions itself (settings do).
pub fn atomic_write(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> Result<()>,
) -> Result<()> {
    let resolved;
    let path = if path.is_symlink() {
        resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        resolved.as_path()
    } else {
        path
    };
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o7777)
            .unwrap_or(0o644);
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(mode))
            .map_err(|e| e.to_string())?;
    }
    write(tmp.as_file_mut())?;
    tmp.as_file().sync_all().map_err(|e| e.to_string())?;
    tmp.persist(path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    std::fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}
