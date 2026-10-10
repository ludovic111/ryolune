//! The built-in agent's saved conversations and each song's project memory, in
//! `<data dir>/agent-conversations.json`:
//!
//! ```json
//! { "format": 1, "songs": { "<song id>": {
//!     "name": "Nightfall", "current": "<thread id>", "memory": "Keep it in D minor…",
//!     "threads": [ { "id": "…", "title": "Add a bass line", "updatedAt": "2026-10-06T10:00:00Z",
//!                    "transcript": [ …chat entries… ], "conversation": [ …model messages… ],
//!                    "firstId": 0 } ] } } }
//! ```
//!
//! A song is found by its stable id (`Session::id`; "welcome" when it has none). Opening
//! another song stops the run, saves this song's conversation and shows the other song's.
//! The file is written atomically (0600). One that cannot be read is copied aside
//! (`agent-conversations.unreadable-<time>.json`) and saving pauses with the error shown,
//! so it is never overwritten. Conversations reopened from the file come back finished: an
//! entry still streaming is marked stopped, and no tool entry keeps a link into the undo
//! history, so nothing from an earlier run of the app can revert an unrelated edit.

use crate::{
    agent::{Entry, Message, Role},
    app::Ryolune,
};
use ryolune_engine::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Project memory is short context, not a document.
pub(crate) const MEMORY_LIMIT: usize = 32 * 1024;
pub(crate) const NEW_TITLE: &str = "New conversation";
const TITLE_CHARS: usize = 60;
const TITLE_LIMIT: usize = 120;
const THREADS_PER_SONG: usize = 100;
const SONGS: usize = 500;
/// A tool's answer kept in a saved conversation, at most.
const SAVED_RESULT: usize = 4000;
pub(crate) const FILE: &str = "agent-conversations.json";

/// A song's key in the file: its id when it is a plain one, a digest of it otherwise,
/// "welcome" without one.
pub(crate) fn song_key(id: &str) -> String {
    if id.is_empty() {
        "welcome".into()
    } else if id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        id.into()
    } else {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in id.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("song-{hash:016x}")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ThreadInfo {
    pub id: String,
    pub title: String,
    pub updated_at: String,
}
impl ThreadInfo {
    fn fresh() -> Self {
        Self {
            id: ryolune_engine::lsuite::uuid_v4(),
            title: NEW_TITLE.into(),
            updated_at: ryolune_engine::lsuite::now_rfc3339(),
        }
    }
}
impl Default for ThreadInfo {
    fn default() -> Self {
        Self::fresh()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Thread {
    #[serde(flatten)]
    pub info: ThreadInfo,
    #[serde(default)]
    pub transcript: Vec<Entry>,
    #[serde(default)]
    pub conversation: Vec<Message>,
    #[serde(default)]
    pub first_id: u64,
}
impl Thread {
    fn requests(&self) -> usize {
        self.transcript
            .iter()
            .filter(|e| e.role == Role::User)
            .count()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Song {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub current: Option<String>,
    #[serde(default)]
    pub threads: Vec<Thread>,
    #[serde(default)]
    pub memory: String,
}
impl Song {
    fn updated(&self) -> &str {
        self.threads
            .iter()
            .map(|t| t.info.updated_at.as_str())
            .max()
            .unwrap_or("")
    }
}

fn format_version() -> u32 {
    1
}
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Archive {
    #[serde(default = "format_version")]
    pub format: u32,
    #[serde(default)]
    pub songs: BTreeMap<String, Song>,
}
impl Default for Archive {
    fn default() -> Self {
        Self {
            format: 1,
            songs: BTreeMap::new(),
        }
    }
}

/// The conversations of every song, and which one the panel shows.
#[derive(Default)]
pub(crate) struct Conversations {
    /// Where they are saved; `None` keeps them in memory (unit tests, `--screenshot`).
    path: Option<PathBuf>,
    archive: Archive,
    /// The file could not be read: saving is paused so it is never overwritten.
    read_error: Option<String>,
    write_error: Option<String>,
    /// The song the panel shows, by key.
    song: Option<String>,
    /// The conversation open in the panel; its entries live in the agent runtime.
    pub(crate) thread: ThreadInfo,
    /// The song's project memory.
    pub(crate) memory: String,
}

impl Conversations {
    /// Why conversations are not being saved, if they are not.
    pub(crate) fn storage_error(&self) -> Option<&str> {
        self.read_error.as_deref().or(self.write_error.as_deref())
    }
    fn song_mut(&mut self) -> Option<&mut Song> {
        let key = self.song.clone()?;
        Some(self.archive.songs.entry(key).or_default())
    }
    fn song(&self) -> Option<&Song> {
        self.archive.songs.get(self.song.as_deref()?)
    }
    /// The song's other conversations and this one, newest first.
    pub(crate) fn list(&self, current_requests: usize) -> Vec<(ThreadInfo, usize, bool)> {
        let mut list: Vec<(ThreadInfo, usize, bool)> = self
            .song()
            .into_iter()
            .flat_map(|s| &s.threads)
            .filter(|t| t.info.id != self.thread.id)
            .map(|t| (t.info.clone(), t.requests(), false))
            .collect();
        list.push((self.thread.clone(), current_requests, true));
        list.sort_by(|a, b| b.0.updated_at.cmp(&a.0.updated_at));
        list
    }
    fn write(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if self.read_error.is_some() {
            return;
        }
        self.write_error = write(&path, &self.archive).err();
    }
}

/// Read the file: empty when there is none; an unreadable one is copied aside.
pub(crate) fn read(path: &Path) -> std::result::Result<Archive, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Archive::default()),
        Err(e) => {
            return Err(format!(
                "Could not read the agent's conversations at {}: {e}. Saving is paused to protect the file; fix it and restart ryolune.",
                path.display()
            ))
        }
    };
    match serde_json::from_slice::<Archive>(&bytes) {
        Ok(mut archive) => {
            for song in archive.songs.values_mut() {
                for thread in &mut song.threads {
                    settle(&mut thread.transcript);
                }
            }
            Ok(archive)
        }
        Err(e) => {
            let millis = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis());
            let backup = path.with_file_name(format!(
                "{}.unreadable-{millis}.json",
                FILE.trim_end_matches(".json")
            ));
            let kept = match std::fs::copy(path, &backup) {
                Ok(_) => format!("a copy is at {}", backup.display()),
                Err(error) => format!("it could not be copied aside ({error})"),
            };
            Err(format!(
                "The agent's conversations at {} could not be read ({e}); {kept}. Saving is paused so the file is not overwritten: restore it and restart ryolune.",
                path.display()
            ))
        }
    }
}

/// A run that was going when the file was written ended with the app: stopped.
/// Saving clears `streaming`, so a tool without an answer is the sign: it never finished.
fn settle(transcript: &mut [Entry]) {
    for entry in transcript {
        entry.streaming = false;
        if let Some(tool) = &mut entry.tool {
            if tool.result.is_none() {
                tool.result = Some(Err("Stopped".into()));
            }
            tool.sequence = None;
        }
    }
}

/// Write the file atomically, readable by this user only.
pub(crate) fn write(path: &Path, archive: &Archive) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_vec(archive).map_err(|e| e.to_string())?;
    ryolune_engine::document::atomic_write(path, |file| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        std::io::Write::write_all(file, &text).map_err(|e| e.to_string())
    })
    .map_err(|e| format!("Could not save the agent's conversations: {e}"))
}

/// A tool's answer as a saved conversation keeps it: bounded.
fn compact(transcript: &[Entry]) -> Vec<Entry> {
    transcript
        .iter()
        .cloned()
        .map(|mut entry| {
            entry.streaming = false;
            if let Some(Some(Ok(value))) = entry.tool.as_mut().map(|t| t.result.as_mut()) {
                let text = value.to_string();
                if text.len() > SAVED_RESULT {
                    *value = Value::String(crate::agent::bounded(&text, SAVED_RESULT));
                }
            }
            entry
        })
        .collect()
}

/// A title from the first request: its first line, 60 characters at most.
pub(crate) fn title_from(prompt: &str) -> String {
    let line = prompt.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let line = line.trim();
    let mut title: String = line.chars().take(TITLE_CHARS).collect();
    if line.chars().count() > TITLE_CHARS {
        title = title.trim_end().to_string();
        title.push('…');
    }
    if title.is_empty() {
        NEW_TITLE.into()
    } else {
        title
    }
}

impl Ryolune {
    /// Load the saved conversations (once, at start) and show the open song's.
    pub(crate) fn attach_conversations(&mut self, path: PathBuf) {
        let conversations = &mut self.agents.conversations;
        match read(&path) {
            Ok(archive) => conversations.archive = archive,
            Err(error) => conversations.read_error = Some(error),
        }
        conversations.path = Some(path);
        conversations.song = None;
        self.follow_song();
    }

    fn song_now(&self) -> String {
        song_key(&self.store.session().id)
    }

    /// Another song is open and the panel still shows this one's conversation: its run is
    /// being stopped, and its tool calls must not reach the other song.
    pub(crate) fn song_switching(&self) -> bool {
        self.agents
            .conversations
            .song
            .as_deref()
            .is_some_and(|song| song != self.song_now())
    }

    /// Show the open song's conversation. Another song was opened: stop the run first (the
    /// next frame finishes the switch), save this song's conversation, load the other's.
    pub(crate) fn follow_song(&mut self) {
        let key = self.song_now();
        if self.agents.conversations.song.as_deref() == Some(key.as_str()) {
            return;
        }
        // The first song the panel follows: what it shows already belongs to it.
        let first = self.agents.conversations.song.is_none();
        if !first {
            if self.agents.runtime.running() {
                self.agents.runtime.stop();
                return;
            }
            self.save_conversation(true);
        }
        let name = self.store.session().name.clone();
        let conversations = &mut self.agents.conversations;
        conversations.song = Some(key.clone());
        let song = conversations
            .archive
            .songs
            .get(&key)
            .cloned()
            .unwrap_or_default();
        conversations.memory = song.memory.clone();
        let current = song
            .current
            .as_ref()
            .and_then(|id| song.threads.iter().find(|t| &t.info.id == id))
            .cloned();
        match current {
            Some(thread) if !self.agents.runtime.running() => self.load_thread(thread),
            Some(_) => {}
            None if first => {}
            None => self.fresh_thread(),
        }
        if let Some(song) = self.agents.conversations.archive.songs.get_mut(&key) {
            song.name = name;
        }
    }

    fn load_thread(&mut self, thread: Thread) {
        let runtime = &mut self.agents.runtime;
        runtime.clear();
        runtime.first_id = thread.first_id;
        runtime.transcript = thread.transcript;
        settle(&mut runtime.transcript);
        runtime.history = thread.conversation;
        runtime.scroll_to_end = true;
        self.agents.conversations.thread = thread.info;
    }

    fn fresh_thread(&mut self) {
        self.agents.runtime.clear();
        self.agents.conversations.thread = ThreadInfo::fresh();
    }

    /// Put the open conversation into the archive and write the file. `leaving` drops the
    /// tool entries' links into the undo history (another document replaces it).
    pub(crate) fn save_conversation(&mut self, leaving: bool) {
        let runtime = &self.agents.runtime;
        let thread = Thread {
            info: self.agents.conversations.thread.clone(),
            transcript: compact(&runtime.transcript),
            conversation: runtime.history.clone(),
            first_id: runtime.first_id,
        };
        let keep = !thread.transcript.is_empty() || thread.info.title != NEW_TITLE;
        let conversations = &mut self.agents.conversations;
        let memory = conversations.memory.clone();
        let Some(song) = conversations.song_mut() else {
            return;
        };
        song.memory = memory;
        song.current = Some(thread.info.id.clone());
        let at = song
            .threads
            .iter()
            .position(|t| t.info.id == thread.info.id);
        match (at, keep) {
            (Some(i), true) => song.threads[i] = thread,
            (None, true) => song.threads.push(thread),
            (Some(i), false) => {
                song.threads.remove(i);
            }
            (None, false) => {}
        }
        if leaving {
            for thread in &mut song.threads {
                settle(&mut thread.transcript);
            }
        }
        if song.threads.len() > THREADS_PER_SONG {
            song.threads
                .sort_by(|a, b| b.info.updated_at.cmp(&a.info.updated_at));
            song.threads.truncate(THREADS_PER_SONG);
        }
        let archive = &mut conversations.archive;
        archive
            .songs
            .retain(|_, s| !s.threads.is_empty() || !s.memory.trim().is_empty());
        if archive.songs.len() > SONGS {
            let mut ages: Vec<(String, String)> = archive
                .songs
                .iter()
                .map(|(k, s)| (s.updated().to_string(), k.clone()))
                .collect();
            ages.sort();
            for (_, key) in ages.into_iter().take(archive.songs.len() - SONGS) {
                if Some(&key) != conversations.song.as_ref() {
                    archive.songs.remove(&key);
                }
            }
        }
        conversations.write();
    }

    /// A request is about to start: title a new conversation from it and date it.
    pub(crate) fn conversation_started(&mut self, prompt: &str) {
        self.follow_song();
        let asked = self
            .agents
            .runtime
            .transcript
            .iter()
            .any(|e| e.role == Role::User);
        let thread = &mut self.agents.conversations.thread;
        if thread.title == NEW_TITLE && !asked {
            thread.title = title_from(prompt);
        }
        thread.updated_at = ryolune_engine::lsuite::now_rfc3339();
    }

    fn idle_for(&self, what: &str) -> Result<()> {
        if self.agents.runtime.running() {
            return Err(format!(
                "The agent is working: stop it (agent.stop) before {what}."
            ));
        }
        Ok(())
    }

    pub(crate) fn conversations_json(&self) -> Value {
        let conversations = &self.agents.conversations;
        let asked = self
            .agents
            .runtime
            .transcript
            .iter()
            .filter(|e| e.role == Role::User)
            .count();
        json!({
            "current": conversations.thread.id,
            "conversations": conversations
                .list(asked)
                .into_iter()
                .map(|(info, requests, current)| json!({
                    "id": info.id, "title": info.title, "updatedAt": info.updated_at,
                    "requests": requests, "current": current,
                }))
                .collect::<Vec<_>>(),
            "memoryBytes": conversations.memory.len(),
            "storageError": conversations.storage_error(),
        })
    }

    pub(crate) fn conversation_json(&self) -> Value {
        let thread = &self.agents.conversations.thread;
        json!({ "id": thread.id, "title": thread.title, "updatedAt": thread.updated_at })
    }

    /// `agent.newConversation`: keep this one, open an empty one.
    pub(crate) fn new_conversation(&mut self) -> Result<Value> {
        self.follow_song();
        self.idle_for("starting a new conversation")?;
        self.save_conversation(false);
        self.fresh_thread();
        let id = self.agents.conversations.thread.id.clone();
        if let Some(song) = self.agents.conversations.song_mut() {
            song.current = Some(id);
        }
        self.agents.conversations.write();
        Ok(self.conversation_json())
    }

    /// `agent.selectConversation`.
    pub(crate) fn select_conversation(&mut self, id: &str) -> Result<Value> {
        self.follow_song();
        if id == self.agents.conversations.thread.id {
            return Ok(self.conversation_json());
        }
        self.idle_for("switching conversations")?;
        self.save_conversation(false);
        let thread = self
            .agents
            .conversations
            .song()
            .and_then(|s| s.threads.iter().find(|t| t.info.id == id))
            .cloned()
            .ok_or_else(|| {
                format!("No conversation {id} for this song: agent.conversations lists them.")
            })?;
        self.load_thread(thread);
        if let Some(song) = self.agents.conversations.song_mut() {
            song.current = Some(id.to_string());
        }
        self.agents.conversations.write();
        Ok(self.conversation_json())
    }

    /// `agent.renameConversation`: the open one unless `id` names another.
    pub(crate) fn rename_conversation(&mut self, id: Option<&str>, title: &str) -> Result<Value> {
        self.follow_song();
        let title = title.trim();
        if title.is_empty()
            || title.chars().count() > TITLE_LIMIT
            || title.chars().any(char::is_control)
        {
            return Err("A conversation title is 1 to 120 characters on one line.".into());
        }
        let conversations = &mut self.agents.conversations;
        match id.filter(|id| *id != conversations.thread.id) {
            None => conversations.thread.title = title.into(),
            Some(id) => {
                let thread = conversations
                    .song_mut()
                    .and_then(|s| s.threads.iter_mut().find(|t| t.info.id == id))
                    .ok_or_else(|| {
                        format!(
                            "No conversation {id} for this song: agent.conversations lists them."
                        )
                    })?;
                thread.info.title = title.into();
                let info = thread.info.clone();
                conversations.write();
                return Ok(
                    json!({ "id": info.id, "title": info.title, "updatedAt": info.updated_at }),
                );
            }
        }
        self.save_conversation(false);
        Ok(self.conversation_json())
    }

    /// `agent.deleteConversation`: gone for good; deleting the open one opens the newest
    /// other (or an empty one).
    pub(crate) fn delete_conversation(&mut self, id: &str) -> Result<Value> {
        self.follow_song();
        let current = id == self.agents.conversations.thread.id;
        if current {
            self.idle_for("deleting this conversation")?;
        }
        let conversations = &mut self.agents.conversations;
        let found = conversations.song_mut().and_then(|s| {
            let at = s.threads.iter().position(|t| t.info.id == id)?;
            Some(s.threads.remove(at))
        });
        if found.is_none() && !current {
            return Err(format!(
                "No conversation {id} for this song: agent.conversations lists them."
            ));
        }
        if current {
            let next = conversations.song().and_then(|s| {
                s.threads
                    .iter()
                    .max_by(|a, b| a.info.updated_at.cmp(&b.info.updated_at))
                    .cloned()
            });
            match next {
                Some(thread) => self.load_thread(thread),
                None => self.fresh_thread(),
            }
            let open = self.agents.conversations.thread.id.clone();
            if let Some(song) = self.agents.conversations.song_mut() {
                song.current = Some(open);
            }
        }
        self.agents.conversations.write();
        Ok(json!({ "deleted": id, "current": self.conversation_json() }))
    }

    /// `agent.setMemory`.
    pub(crate) fn set_memory(&mut self, text: &str) -> Result<Value> {
        self.follow_song();
        if text.len() > MEMORY_LIMIT {
            return Err(format!(
                "Keep project memory under {} KB ({} bytes now).",
                MEMORY_LIMIT / 1024,
                text.len()
            ));
        }
        self.agents.conversations.memory = text.to_string();
        self.save_conversation(false);
        Ok(self.memory_json())
    }

    pub(crate) fn memory_json(&self) -> Value {
        let memory = &self.agents.conversations.memory;
        json!({ "memory": memory, "bytes": memory.len(), "limit": MEMORY_LIMIT })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::ToolRecord;
    use ryolune_engine::store;

    fn app_with(dir: &Path) -> Ryolune {
        let mut app = Ryolune::from_session(store::demo(), None);
        app.attach_conversations(dir.join(FILE));
        app
    }

    fn say(app: &mut Ryolune, text: &str) {
        app.conversation_started(text);
        app.agents.runtime.transcript.push(Entry {
            role: Role::User,
            text: text.into(),
            tool: None,
            streaming: false,
        });
        app.agents.runtime.history.push(Message {
            role: "user",
            parts: vec![crate::agent::Part::Text(text.into())],
        });
        app.agents.runtime.history.push(Message {
            role: "assistant",
            parts: vec![crate::agent::Part::ToolUse {
                id: "call-1".into(),
                name: "session_info".into(),
                input: json!({}),
            }],
        });
        app.save_conversation(false);
    }

    #[test]
    fn conversations_are_saved_per_song_and_come_back_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with(dir.path());
        let song = app.store.session().id.clone();
        assert!(!song.is_empty(), "every song has an id");
        say(
            &mut app,
            "Add a bass line that follows the kick, in D minor please",
        );
        let first = app.agents.conversations.thread.clone();
        assert_eq!(
            first.title,
            "Add a bass line that follows the kick, in D minor please"
        );
        app.new_conversation().unwrap();
        say(&mut app, "Make the drums swing");
        app.set_memory("Keep it at 92 BPM.").unwrap();
        let listed = app.conversations_json();
        assert_eq!(listed["conversations"].as_array().unwrap().len(), 2);
        assert_eq!(
            listed["current"],
            app.agents.conversations.thread.id.as_str()
        );

        // Another song: its own (empty) conversation and memory.
        let mut other = store::empty();
        other.ensure_id();
        app.store.load(other).unwrap();
        app.reset_agent_history();
        assert!(app.agents.runtime.transcript.is_empty());
        assert!(app.agents.conversations.memory.is_empty());
        say(&mut app, "A new song");

        // A restart finds the first song as it was.
        let mut back = store::demo();
        back.id = song;
        let mut app = Ryolune::from_session(back, None);
        app.attach_conversations(dir.path().join(FILE));
        assert_eq!(
            app.agents.conversations.thread.title,
            "Make the drums swing"
        );
        assert_eq!(app.agents.conversations.memory, "Keep it at 92 BPM.");
        app.select_conversation(&first.id).unwrap();
        assert_eq!(
            app.agents.runtime.transcript[0].text,
            "Add a bass line that follows the kick, in D minor please"
        );
        // The model's side of the conversation comes back too, for the next request.
        let history = &app.agents.runtime.history;
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].role, "assistant");
        assert!(
            matches!(&history[1].parts[0], crate::agent::Part::ToolUse { name, .. } if name == "session_info")
        );
        // Delete it: the other one opens.
        app.delete_conversation(&first.id).unwrap();
        assert_eq!(
            app.agents.conversations.thread.title,
            "Make the drums swing"
        );
        assert_eq!(
            app.conversations_json()["conversations"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn a_reopened_conversation_cannot_revert_and_its_runs_are_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app_with(dir.path());
        say(&mut app, "Rename the song");
        app.agents.runtime.transcript.push(Entry {
            role: Role::Tool,
            text: String::new(),
            tool: Some(ToolRecord {
                name: "session.rename".into(),
                args: json!({"name": "x"}),
                result: None,
                sequence: Some(7),
            }),
            streaming: true,
        });
        app.save_conversation(false);
        let id = app.store.session().id.clone();
        let mut back = store::demo();
        back.id = id;
        let mut app = Ryolune::from_session(back, None);
        app.attach_conversations(dir.path().join(FILE));
        let tool = app.agents.runtime.transcript[1].tool.clone().unwrap();
        assert_eq!(
            tool.sequence, None,
            "no link into this window's undo history"
        );
        assert!(
            matches!(tool.result, Some(Err(_))),
            "the run ended with the app"
        );
        assert!(!app.agents.runtime.transcript[1].streaming);
    }

    #[test]
    fn an_unreadable_file_is_kept_aside_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        std::fs::write(&path, b"{ not json").unwrap();
        let mut app = app_with(dir.path());
        assert!(app
            .agents
            .conversations
            .storage_error()
            .unwrap()
            .contains("could not be read"));
        say(&mut app, "Hello");
        app.set_memory("notes").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not json");
        let copies: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("agent-conversations.unreadable-")
            })
            .collect();
        assert_eq!(copies.len(), 1);
        assert_eq!(std::fs::read(copies[0].path()).unwrap(), b"{ not json");
        assert!(app.conversations_json()["storageError"].is_string());
    }

    #[test]
    fn memory_is_capped_and_titles_are_short() {
        let mut app = Ryolune::from_session(store::demo(), None);
        assert!(app.set_memory(&"x".repeat(MEMORY_LIMIT + 1)).is_err());
        assert!(app.set_memory(&"x".repeat(MEMORY_LIMIT)).is_ok());
        assert_eq!(title_from("\n  Short one  \nsecond line"), "Short one");
        let long = title_from(&"word ".repeat(40));
        assert!(long.chars().count() <= TITLE_CHARS + 1 && long.ends_with('…'));
        assert!(app.rename_conversation(None, "").is_err());
        app.rename_conversation(None, "Bass ideas").unwrap();
        assert_eq!(app.agents.conversations.thread.title, "Bass ideas");
        assert_eq!(song_key(""), "welcome");
        assert_eq!(song_key("file-0123abcd"), "file-0123abcd");
        assert!(song_key("../../etc").starts_with("song-"));
    }
}
