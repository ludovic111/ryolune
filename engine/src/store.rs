use crate::{model::*, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", content = "params", rename_all = "camelCase")]
pub enum Command {
    Rename(String),
    RestoreTake(Box<Session>),
    AddTrack(Track),
    UpdateTrack(Track),
    RemoveTrack(String),
    MoveTrack {
        id: String,
        index: usize,
    },
    PutClip(Clip),
    RemoveClip(String),
    PutSource(Source),
    /// Add or replace a marker by id; markers stay in bar order.
    PutMarker(Marker),
    RemoveMarker(String),
    /// Replace the tempo changes after the start; `SetTransport` sets the starting tempo.
    SetTempoChanges(Vec<crate::tempo::TempoPoint>),
    SetStrip {
        track: String,
        strip: Strip,
    },
    SetTransport(Transport),
    SetMasterVolume(f32),
    PutAutomation(crate::automation::AutomationLane),
    RemoveAutomation(String),
    Select {
        track: Option<String>,
        clip: Option<String>,
        note: Option<String>,
    },
    SetView(View),
    Undo,
    Redo,
    Batch(Vec<Command>),
}

/// Every document edit enters here, including GUI edits and headless commands.
/// Audio buffers live outside history; snapshots share immutable sessions.
pub struct Store {
    session: Arc<Session>,
    past: Vec<(Arc<Session>, u64)>,
    future: Vec<(Arc<Session>, u64)>,
    pub revision: u64,
    document_id: u64,
    saved_id: Option<u64>,
    gesture: bool,
    gesture_recorded: bool,
    /// The redo history a gesture's first edit set aside, restored if the gesture is
    /// cancelled (a failed atomic batch changes nothing, Redo included).
    gesture_future: Option<Vec<(Arc<Session>, u64)>>,
    /// The document as an agent last saw it, by agent (`harness.context` reports what
    /// changed since). Not history: an undo does not move them.
    marks: std::collections::HashMap<String, Arc<Session>>,
    /// Checkpoints taken before an agent's edits (`harness.checkpoint`, one per built-in agent
    /// turn), newest last; `harness.revert` returns to one in a single undo step.
    checkpoints: Vec<Checkpoint>,
    next_checkpoint: u64,
}

/// The document at a moment an agent may want to return to.
#[derive(Clone, Debug)]
pub struct Checkpoint {
    pub id: String,
    pub label: String,
    pub session: Arc<Session>,
    /// The store's revision when it was taken.
    pub revision: u64,
    pub created_at: String,
}

/// Checkpoints kept per document.
pub const CHECKPOINTS: usize = 24;

impl Store {
    pub fn new(mut session: Session) -> Result<Self> {
        session.normalize();
        session.validate()?;
        let session = Arc::new(session);
        Ok(Self {
            document_id: 0,
            saved_id: Some(0),
            gesture: false,
            gesture_recorded: false,
            gesture_future: None,
            session,
            past: vec![],
            future: vec![],
            revision: 0,
            marks: Default::default(),
            checkpoints: vec![],
            next_checkpoint: 0,
        })
    }
    /// Remember the document as `key` (an agent) sees it now.
    pub fn set_mark(&mut self, key: &str) {
        self.marks.insert(key.to_string(), self.session.clone());
    }
    /// The document as `key` last saw it.
    pub fn mark(&self, key: &str) -> Option<Arc<Session>> {
        self.marks.get(key).cloned()
    }
    /// Take a checkpoint of the document as it is now.
    pub fn checkpoint(&mut self, label: &str) -> Checkpoint {
        self.next_checkpoint += 1;
        let checkpoint = Checkpoint {
            id: format!("cp-{}", self.next_checkpoint),
            label: label.chars().take(120).collect(),
            session: self.session.clone(),
            revision: self.revision,
            created_at: crate::lsuite::now_rfc3339(),
        };
        self.checkpoints.push(checkpoint.clone());
        if self.checkpoints.len() > CHECKPOINTS {
            self.checkpoints.remove(0);
        }
        checkpoint
    }
    /// Checkpoints of this document, oldest first.
    pub fn checkpoints(&self) -> &[Checkpoint] {
        &self.checkpoints
    }
    /// One checkpoint by id, or the newest.
    pub fn find_checkpoint(&self, id: Option<&str>) -> Option<&Checkpoint> {
        match id {
            Some(id) => self.checkpoints.iter().find(|c| c.id == id),
            None => self.checkpoints.last(),
        }
    }
    pub fn session(&self) -> &Session {
        &self.session
    }
    pub fn snapshot(&self) -> Arc<Session> {
        self.session.clone()
    }
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }
    /// Number of undo steps behind the current document; lets a caller return to
    /// an earlier depth with repeated `Undo` or `Redo`.
    pub fn undo_depth(&self) -> usize {
        self.past.len()
    }
    pub fn dirty(&self) -> bool {
        Some(self.document_id) != self.saved_id
    }
    pub fn mark_saved(&mut self, revision: u64) {
        if self.revision == revision {
            self.saved_id = Some(self.document_id);
        }
    }
    /// A recovered/imported copy has no saved project counterpart. This leaves
    /// history untouched and remains dirty through undo until a save succeeds.
    pub fn mark_unsaved(&mut self) {
        self.saved_id = None;
    }
    pub fn load(&mut self, mut session: Session) -> Result<()> {
        session.normalize();
        session.validate()?;
        self.session = Arc::new(session);
        self.past.clear();
        self.future.clear();
        self.revision += 1;
        self.document_id = self.revision;
        self.saved_id = Some(self.document_id);
        self.gesture_recorded = false;
        self.gesture_future = None;
        self.marks.clear();
        self.checkpoints.clear();
        Ok(())
    }
    /// Update derived data (captured plugin state) without touching history
    /// or the dirty flag. The revision still advances so audio resyncs.
    pub fn amend(&mut self, edit: impl FnOnce(&mut Session)) -> Result<()> {
        let mut next = (*self.session).clone();
        edit(&mut next);
        next.validate()?;
        self.session = Arc::new(next);
        self.revision += 1;
        Ok(())
    }
    /// Whether edits are being coalesced into one undo step (a drag, a batch).
    pub fn gesture_active(&self) -> bool {
        self.gesture
    }
    /// Coalesce a slider drag or one focused text edit into one undo step.
    pub fn set_gesture(&mut self, active: bool) {
        if !active || !self.gesture {
            self.gesture_recorded = false;
            self.gesture_future = None;
        }
        self.gesture = active;
    }
    /// Drop every edit made since the gesture began, as if it never happened.
    /// Returns whether there was anything to drop.
    pub fn cancel_gesture(&mut self) -> bool {
        let recorded = self.gesture && self.gesture_recorded;
        if recorded {
            if let Some((session, id)) = self.past.pop() {
                self.session = session;
                self.document_id = id;
                self.revision += 1;
            }
            if let Some(future) = self.gesture_future.take() {
                self.future = future;
            }
        }
        self.gesture = false;
        self.gesture_recorded = false;
        self.gesture_future = None;
        recorded
    }
    pub fn dispatch(&mut self, command: Command) -> Result<bool> {
        let transient = matches!(command, Command::Select { .. } | Command::SetView(_));
        if let Command::Undo = command {
            if let Some((s, id)) = self.past.pop() {
                self.future.push((self.session.clone(), self.document_id));
                self.session = s;
                self.document_id = id;
                self.gesture_recorded = false;
                self.gesture_future = None;
                self.revision += 1;
                return Ok(true);
            }
            return Ok(false);
        }
        if let Command::Redo = command {
            if let Some((s, id)) = self.future.pop() {
                self.past.push((self.session.clone(), self.document_id));
                self.session = s;
                self.document_id = id;
                self.gesture_recorded = false;
                self.gesture_future = None;
                self.revision += 1;
                return Ok(true);
            }
            return Ok(false);
        }
        let mut next = (*self.session).clone();
        apply(&mut next, command, 0)?;
        next.validate()?;
        if transient {
            self.session = Arc::new(next);
        } else {
            if !self.gesture || !self.gesture_recorded {
                self.past.push((self.session.clone(), self.document_id));
                self.gesture_recorded = self.gesture;
                if self.gesture {
                    self.gesture_future = Some(std::mem::take(&mut self.future));
                }
            }
            if self.past.len() > 200 {
                self.past.remove(0);
            }
            self.future.clear();
            self.session = Arc::new(next);
            self.revision += 1;
            self.document_id = self.revision;
        }
        Ok(!transient)
    }
}
fn apply(s: &mut Session, command: Command, depth: usize) -> Result<()> {
    if depth > 8 {
        return Err("Command batch nesting exceeds capacity".into());
    }
    match command {
        Command::Rename(name) => s.name = name,
        Command::RestoreTake(session) => {
            // A take is the same song: it keeps the song's id (takes saved before songs had
            // one carry none).
            let id = std::mem::take(&mut s.id);
            *s = *session;
            s.id = id;
        }
        Command::AddTrack(track) => {
            if s.tracks.iter().any(|t| t.id == track.id) {
                return Err("Track ID already exists".into());
            }
            s.view.selected_track_id = Some(track.id.clone());
            s.tracks.push(track);
        }
        Command::UpdateTrack(track) => {
            let t = s
                .tracks
                .iter_mut()
                .find(|t| t.id == track.id)
                .ok_or("Track not found")?;
            *t = track;
        }
        Command::RemoveTrack(id) => {
            s.tracks.retain(|t| t.id != id);
            s.clips.retain(|c| c.track_id != id);
            s.strips.remove(&id);
            // Tracks that fed a removed bus go back to the Stereo Out.
            s.prune_routing();
            crate::automation::retain_targets(s);
            if s.view.selected_track_id.as_ref() == Some(&id) {
                s.view.selected_track_id = s.tracks.first().map(|t| t.id.clone());
            }
            sanitize_selection(s);
        }
        Command::MoveTrack { id, index } => {
            let from = s
                .tracks
                .iter()
                .position(|t| t.id == id)
                .ok_or("Track not found")?;
            let t = s.tracks.remove(from);
            s.tracks.insert(index.min(s.tracks.len()), t);
        }
        Command::PutClip(mut clip) => {
            // Trims, resizes and tempo-free edits all land here: keep fades inside the clip.
            let seconds = s.bars_seconds(clip.start_bar, clip.start_bar + clip.length_bars);
            if let ClipData::Audio {
                fade_in, fade_out, ..
            } = &mut clip.data
            {
                (*fade_in, *fade_out) = clamp_fades(*fade_in, *fade_out, seconds);
            }
            if let Some(c) = s.clips.iter_mut().find(|c| c.id == clip.id) {
                *c = clip;
            } else {
                s.clips.push(clip);
            }
            // clip.setNotes and note.remove can take the selected note with them.
            sanitize_selection(s);
        }
        Command::RemoveClip(id) => {
            s.clips.retain(|c| c.id != id);
            sanitize_selection(s);
        }
        Command::PutSource(source) => {
            s.sources.insert(source.id.clone(), source);
        }
        Command::SetStrip { track, strip } => {
            if !is_bus(&track) && !s.tracks.iter().any(|t| t.id == track) {
                return Err("Track not found".into());
            }
            s.strips.insert(track, strip);
            crate::automation::retain_targets(s);
        }
        Command::PutAutomation(mut lane) => {
            lane.points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
            if let Some(old) = s.automation.iter_mut().find(|old| old.id == lane.id) {
                *old = lane;
            } else {
                s.automation.push(lane);
            }
        }
        Command::RemoveAutomation(id) => s.automation.retain(|lane| lane.id != id),
        Command::PutMarker(marker) => {
            if let Some(m) = s.markers.iter_mut().find(|m| m.id == marker.id) {
                *m = marker;
            } else {
                s.markers.push(marker);
            }
            s.markers.sort_by(|a, b| a.bar.total_cmp(&b.bar));
        }
        Command::RemoveMarker(id) => {
            let before = s.markers.len();
            s.markers.retain(|m| m.id != id);
            if s.markers.len() == before {
                return Err("Marker not found".into());
            }
        }
        Command::SetTempoChanges(mut points) => {
            points.sort_by(|a, b| a.bar.total_cmp(&b.bar));
            s.tempo_changes = points;
        }
        Command::SetTransport(t) => s.transport = t,
        Command::SetMasterVolume(v) => s.master_volume = v,
        Command::Select { track, clip, note } => {
            s.view.selected_track_id = track;
            s.view.selected_clip_id = clip.clone();
            s.view.editor_clip_id = clip;
            s.view.selected_note_id = note;
        }
        Command::SetView(v) => s.view = v,
        Command::Batch(commands) => {
            if commands.len() > 10_000 {
                return Err("Command batch exceeds capacity".into());
            }
            for command in commands {
                if matches!(
                    command,
                    Command::Undo | Command::Redo | Command::SetView(_) | Command::Select { .. }
                ) {
                    return Err("History and view commands cannot be batched".into());
                }
                apply(s, command, depth + 1)?;
            }
        }
        Command::Undo | Command::Redo => return Err("History command cannot be nested".into()),
    }
    Ok(())
}
fn sanitize_selection(s: &mut Session) {
    if !s
        .clips
        .iter()
        .any(|c| Some(&c.id) == s.view.selected_clip_id.as_ref())
    {
        s.view.selected_clip_id = None;
        s.view.selected_note_id = None;
    }
    if !s
        .clips
        .iter()
        .any(|c| Some(&c.id) == s.view.editor_clip_id.as_ref())
    {
        s.view.editor_clip_id = None;
    }
    if let Some(note) = &s.view.selected_note_id {
        let present = s
            .clips
            .iter()
            .find(|c| Some(&c.id) == s.view.selected_clip_id.as_ref())
            .is_some_and(|c| match &c.data {
                crate::model::ClipData::Midi { notes, .. } => notes.iter().any(|n| &n.id == note),
                _ => false,
            });
        if !present {
            s.view.selected_note_id = None;
        }
    }
}

pub fn demo() -> Session {
    let mut s: Session = serde_json::from_str(include_str!("../tests/fixtures/nightfall.json"))
        .expect("Bundled demo is validated by tests");
    s.transport.position_beats = 0.0;
    s.transport.playing = false;
    s.transport.recording = false;
    s.extra.insert("agent".into(),serde_json::json!({"status":"idle","transport":"no agent connected","current":null,"log":[],"draft":""}));
    s.normalize();
    s
}
pub fn empty() -> Session {
    let mut s = demo();
    s.name = "Untitled.ryolune".into();
    s.clips.clear();
    s.sources.clear();
    s.strips.clear();
    // A new song plays at once: Drums on the drum machine and Bass on a bass, plus one audio
    // track to record into. (It used to start with an audio Drums track that stayed silent.)
    s.tracks
        .retain(|t| matches!(t.id.as_str(), "drums" | "bass" | "vox"));
    for t in &mut s.tracks {
        t.mute = false;
        t.solo = false;
        t.armed = false;
        t.volume = 0.75;
        t.pan = 0.0;
        match t.id.as_str() {
            "drums" => t.kind = "midi".into(),
            "vox" => t.name = "Vocals".into(),
            _ => {}
        }
    }
    s.normalize();
    for (id, instrument) in [("drums", "Drum Machine"), ("bass", "Analog Bass")] {
        s.strips.entry(id.into()).or_default().instrument = instrument.into();
    }
    s.transport.cycle = false;
    s.transport.cycle_start_bar = 0.0;
    s.transport.cycle_end_bar = 4.0;
    s.view.selected_track_id = s.tracks.first().map(|t| t.id.clone());
    s.view.selected_clip_id = None;
    s.view.editor_clip_id = None;
    s.view.selected_note_id = None;
    s
}

/// Clip splitting keeps offsets and notes aligned, including notes crossing the cut. The
/// right half starts with each controller's value at the cut.
pub fn split(session: &Session, clip: &Clip, bar: f64, id: String) -> Result<(Clip, Clip)> {
    let bpb = session.beats_per_bar();
    let relative = bar - clip.start_bar;
    if relative <= 0.0 || relative >= clip.length_bars {
        return Err("Split position must be inside the clip".into());
    }
    let mut left = clip.clone();
    let mut right = clip.clone();
    right.id = id;
    right.start_bar = bar;
    left.length_bars = relative;
    right.length_bars = clip.length_bars - relative;
    match &clip.data {
        ClipData::Audio {
            offset_seconds,
            fade_in,
            fade_out,
            ..
        } => {
            // The cut is a hard edge: the left part keeps the fade-in, the right the fade-out.
            // Seconds of audio between the clip's start and `bars` into it.
            let seconds = |bars: f64| session.bars_seconds(clip.start_bar, clip.start_bar + bars);
            if let ClipData::Audio {
                fade_in: left_in,
                fade_out: left_out,
                ..
            } = &mut left.data
            {
                (*left_in, *left_out) = clamp_fades(*fade_in, 0.0, seconds(left.length_bars));
            }
            if let ClipData::Audio {
                offset_seconds: right_offset,
                fade_in: right_in,
                fade_out: right_out,
                ..
            } = &mut right.data
            {
                *right_offset = offset_seconds + seconds(relative);
                (*right_in, *right_out) = clamp_fades(
                    0.0,
                    *fade_out,
                    seconds(clip.length_bars) - seconds(relative),
                );
            }
        }
        ClipData::Midi { notes, controllers } => {
            let cut = relative * bpb;
            left.data = ClipData::Midi {
                notes: notes
                    .iter()
                    .filter(|n| n.start < cut)
                    .map(|n| {
                        let mut n = n.clone();
                        n.length = n.length.min(cut - n.start);
                        n
                    })
                    .collect(),
                controllers: crate::controllers::window(controllers, 0.0, cut, || {
                    crate::control::new_id("ctl")
                }),
            };
            right.data = ClipData::Midi {
                notes: notes
                    .iter()
                    .filter(|n| n.start + n.length > cut)
                    .map(|n| {
                        let mut n = n.clone();
                        let end = n.start + n.length - cut;
                        n.start = (n.start - cut).max(0.0);
                        n.length = end - n.start;
                        n
                    })
                    .collect(),
                controllers: crate::controllers::window(
                    controllers,
                    cut,
                    (clip.length_bars - relative) * bpb,
                    || crate::control::new_id("ctl"),
                ),
            };
        }
    }
    Ok((left, right))
}
