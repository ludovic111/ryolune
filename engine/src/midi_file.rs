//! Standard MIDI File interchange. Quarter-note timing is preserved. Notes, control changes,
//! pitch bend and channel pressure travel both ways; instrument patches, polyphonic pressure
//! and SysEx are reported rather than silently approximated.
use crate::{control::new_id, document, model::*, store::Command, Result};
use midly::{
    num::{u15, u24, u28, u4, u7},
    Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    path::Path,
};

const MAX_FILE: u64 = 32 * 1024 * 1024;
const PPQ: u16 = 960;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportOptions {
    pub start_bar: f64,
    pub import_tempo: bool,
    /// One track per file track with every event on its own channel, instead of a track per
    /// channel played on channel 1.
    pub keep_channels: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub track_ids: Vec<String>,
    pub clip_ids: Vec<String>,
    pub notes: usize,
    pub controllers: usize,
    pub file_tempo: f64,
    pub tempo_imported: bool,
    /// Tempo changes after the file's start that became the song's (importTempo only).
    pub tempo_changes: usize,
    pub warnings: Vec<String>,
}

/// Decode completely and construct a single undoable batch before mutating a store.
pub fn import(
    path: &Path,
    session: &Session,
    options: &ImportOptions,
    agent: bool,
) -> Result<(Command, ImportReport)> {
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if metadata.len() > MAX_FILE {
        return Err("MIDI file exceeds 32 MiB".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    import_bytes(&bytes, session, options, agent)
}

pub fn import_bytes(
    bytes: &[u8],
    session: &Session,
    options: &ImportOptions,
    agent: bool,
) -> Result<(Command, ImportReport)> {
    if bytes.len() as u64 > MAX_FILE {
        return Err("MIDI file exceeds 32 MiB".into());
    }
    if !valid_time(options.start_bar) {
        return Err("MIDI startBar must be finite and nonnegative".into());
    }
    let smf = Smf::parse(bytes).map_err(|e| format!("Invalid MIDI file: {e}"))?;
    if smf.header.format == Format::Sequential {
        return Err(
            "SMF type 2 contains independent songs; export it as type 0 or type 1 before importing"
                .into(),
        );
    }
    let ppq = match smf.header.timing {
        Timing::Metrical(ppq) if ppq.as_int() > 0 => ppq.as_int() as f64,
        Timing::Metrical(_) => {
            return Err("MIDI ticks per quarter note must be greater than zero".into())
        }
        Timing::Timecode(_, _) => {
            return Err(
                "SMPTE MIDI timing is not supported; export with ticks per quarter note".into(),
            )
        }
    };
    if smf.tracks.len() > 1024 {
        return Err("MIDI file exceeds 1024 tracks".into());
    }
    let mut tempo_events = vec![];
    let mut meters = vec![];
    // Name, channel, notes, controllers and end beat of each clip to create.
    type Lane = (String, u8, Vec<Note>, Vec<Controller>, f64);
    let mut lanes: Vec<Lane> = vec![];
    let mut controller_total = 0usize;
    let mut ignored = 0usize;
    let mut unmatched = 0usize;
    let mut total = 0usize;
    for (track_index, events) in smf.tracks.iter().enumerate() {
        let mut tick = 0u64;
        let mut name = format!("MIDI {}", track_index + 1);
        let mut open: BTreeMap<(u8, u8), VecDeque<(u64, u8)>> = BTreeMap::new();
        let mut notes: BTreeMap<u8, Vec<Note>> = BTreeMap::new();
        let mut controls: BTreeMap<u8, Vec<Controller>> = BTreeMap::new();
        for event in events {
            tick = tick
                .checked_add(event.delta.as_int() as u64)
                .ok_or("MIDI timing overflow")?;
            if tick as f64 / ppq > 1_000_000.0 {
                return Err("MIDI timing exceeds session limits".into());
            }
            match event.kind {
                TrackEventKind::Meta(MetaMessage::TrackName(value)) => {
                    name = String::from_utf8_lossy(value).chars().take(256).collect()
                }
                TrackEventKind::Meta(MetaMessage::Tempo(value)) => {
                    tempo_events.push((tick, value.as_int()))
                }
                TrackEventKind::Meta(MetaMessage::TimeSignature(n, d, _, _)) => {
                    meters.push((tick, n, d))
                }
                TrackEventKind::Midi { channel, message } => {
                    let channel = channel.as_int();
                    match message {
                        MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                            total += 1;
                            if total > 200_000 {
                                return Err("MIDI file exceeds 200,000 notes".into());
                            }
                            open.entry((channel, key.as_int()))
                                .or_default()
                                .push_back((tick, vel.as_int()));
                        }
                        MidiMessage::NoteOff { key, .. } | MidiMessage::NoteOn { key, .. } => {
                            if let Some((start, velocity)) = open
                                .get_mut(&(channel, key.as_int()))
                                .and_then(VecDeque::pop_front)
                            {
                                notes.entry(channel).or_default().push(Note {
                                    id: new_id("note"),
                                    start: start as f64 / ppq,
                                    length: (tick.saturating_sub(start).max(1)) as f64 / ppq,
                                    pitch: key.as_int(),
                                    velocity,
                                    agent,
                                    channel: 0,
                                });
                            } else {
                                unmatched += 1;
                            }
                        }
                        MidiMessage::Controller { controller, value }
                            if controller.as_int() < 120 =>
                        {
                            controls.entry(channel).or_default().push(Controller {
                                id: new_id("ctl"),
                                kind: ControllerKind::Cc,
                                number: Some(controller.as_int()),
                                time: tick as f64 / ppq,
                                value: value.as_int() as i16,
                                agent,
                                channel: 0,
                            });
                        }
                        MidiMessage::PitchBend { bend } => {
                            controls.entry(channel).or_default().push(Controller {
                                id: new_id("ctl"),
                                kind: ControllerKind::Bend,
                                number: None,
                                time: tick as f64 / ppq,
                                value: bend.as_int(),
                                agent,
                                channel: 0,
                            });
                        }
                        MidiMessage::Aftertouch { key, vel } => {
                            controls.entry(channel).or_default().push(Controller {
                                id: new_id("ctl"),
                                kind: ControllerKind::PolyPressure,
                                number: Some(key.as_int()),
                                time: tick as f64 / ppq,
                                value: vel.as_int() as i16,
                                agent,
                                channel: 0,
                            });
                        }
                        MidiMessage::ChannelAftertouch { vel } => {
                            controls.entry(channel).or_default().push(Controller {
                                id: new_id("ctl"),
                                kind: ControllerKind::Pressure,
                                number: None,
                                time: tick as f64 / ppq,
                                value: vel.as_int() as i16,
                                agent,
                                channel: 0,
                            });
                        }
                        _ => ignored += 1,
                    }
                    if total + controls.values().map(Vec::len).sum::<usize>() > 200_000 {
                        return Err("MIDI file exceeds 200,000 notes and controller changes".into());
                    }
                }
                TrackEventKind::SysEx(_) | TrackEventKind::Escape(_) => ignored += 1,
                _ => {}
            }
        }
        for ((channel, pitch), starts) in open {
            for (start, velocity) in starts {
                unmatched += 1;
                notes.entry(channel).or_default().push(Note {
                    id: new_id("note"),
                    start: start as f64 / ppq,
                    length: (tick.saturating_sub(start).max(1)) as f64 / ppq,
                    pitch,
                    velocity,
                    agent,
                    channel: 0,
                });
            }
        }
        if options.keep_channels && !notes.is_empty() {
            // One lane for the whole file track: each note and controller keeps its channel.
            let mut all_notes = vec![];
            let mut all_controls = vec![];
            for (channel, list) in std::mem::take(&mut notes) {
                all_notes.extend(list.into_iter().map(|mut n| {
                    n.channel = channel;
                    n
                }));
            }
            for (channel, list) in std::mem::take(&mut controls) {
                all_controls.extend(list.into_iter().map(|mut c| {
                    c.channel = channel;
                    c
                }));
            }
            notes.insert(0, all_notes);
            controls.insert(0, all_controls);
        }
        let split = notes.len() > 1;
        // Controllers on a channel without notes belong to the track's only note channel;
        // with several note channels there is no telling which one they meant.
        let mut orphans = vec![];
        for (channel, list) in std::mem::take(&mut controls) {
            if notes.contains_key(&channel) {
                controls.insert(channel, list);
            } else {
                orphans.extend(list);
            }
        }
        if !orphans.is_empty() {
            match notes.keys().next().copied().filter(|_| !split) {
                Some(only) => controls.entry(only).or_default().extend(orphans),
                None => ignored += orphans.len(),
            }
        }
        for (channel, mut notes) in notes {
            let mut controllers = controls.remove(&channel).unwrap_or_default();
            crate::controllers::sort(&mut controllers);
            controller_total += controllers.len();
            notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
            let end = notes
                .iter()
                .map(|n| n.start + n.length)
                .fold(tick as f64 / ppq, f64::max);
            let label = if split {
                format!("{name} · Ch {}", channel + 1)
            } else {
                name.clone()
            };
            lanes.push((label, channel, notes, controllers, end));
        }
    }
    if lanes.is_empty() {
        return Err("The MIDI file contains no notes".into());
    }
    tempo_events.sort_by_key(|e| e.0);
    meters.sort_by_key(|e| e.0);
    // A tempo event after tick zero does not replace the MIDI default before it.
    let micros = tempo_events
        .iter()
        .filter(|(tick, _)| *tick == 0)
        .map(|(_, v)| *v)
        .next_back()
        .unwrap_or(500_000);
    if micros == 0 {
        return Err("MIDI tempo cannot be zero".into());
    }
    // A file stores whole microseconds per quarter, so 90 BPM comes back as 89.99995:
    // thousandths of a BPM are all a tempo shows, and a round trip lands where it started.
    let bpm = |micros: u32| (60_000_000.0 / micros as f64 * 1000.0).round() / 1000.0;
    let tempo = bpm(micros);
    let mut warnings = vec![];
    // Later changes, in beats from the file's start, each differing from the one before.
    let mut later: Vec<(f64, f64)> = vec![];
    for &(tick, value) in tempo_events.iter().filter(|(tick, _)| *tick > 0) {
        if value == 0 {
            return Err("MIDI tempo cannot be zero".into());
        }
        let beat = tick as f64 / ppq;
        if later.last().is_some_and(|(b, _)| *b == beat) {
            later.pop();
        }
        let previous = later.last().map_or(tempo, |(_, v)| *v);
        let value = bpm(value);
        if value != previous {
            later.push((beat, value));
        }
    }
    if !later.is_empty() && !options.import_tempo {
        warnings.push(format!(
            "The file changes tempo {} times; they were not imported (importTempo=true follows them). Note positions remain in quarter-note beats.",
            later.len()
        ));
    }
    if meters.iter().any(|(tick, _, _)| *tick > 0) {
        warnings.push("Later time-signature changes are not imported.".into());
    }
    if ignored > 0 {
        warnings.push(format!("{ignored} program change, channel mode, polyphonic pressure or SysEx events were not imported. Choose instruments in ryolune."));
    }
    if unmatched > 0 {
        warnings.push(format!("{unmatched} unmatched note events were repaired or ignored; open notes end at their source track's end."));
    }
    let mut transport = session.transport.clone();
    if options.import_tempo {
        if !(20.0..=400.0).contains(&tempo) {
            return Err("The file's initial tempo is outside ryolune's 20–400 BPM range; import with importTempo=false to retain session tempo".into());
        }
        transport.tempo = tempo;
        transport.time_signature = TimeSignature {
            numerator: 4,
            denominator: 4,
        };
        if let Some((_, numerator, exponent)) = meters.iter().rfind(|(tick, _, _)| *tick == 0) {
            let denominator = 1u32
                .checked_shl(*exponent as u32)
                .ok_or("Invalid MIDI time signature")?;
            if !(1..=32).contains(numerator) || ![1, 2, 4, 8, 16, 32].contains(&denominator) {
                return Err("The file's initial time signature is unsupported".into());
            }
            transport.time_signature = TimeSignature {
                numerator: *numerator as u32,
                denominator,
            };
        }
        if !session.clips.is_empty() {
            warnings.push(
                "Initial tempo and time signature were applied to the entire existing session."
                    .into(),
            );
        }
    }
    let beats_per_bar = transport.time_signature.numerator as f64 * 4.0
        / transport.time_signature.denominator as f64;
    let mut commands = vec![];
    let mut imported_changes = 0;
    if options.import_tempo {
        // The file's tempo map becomes the song's: its first tempo from the start, as it
        // always did, and its later changes from startBar on.
        let mut points = vec![];
        let mut clamped = false;
        for (beat, value) in &later {
            let bar = options.start_bar + beat / beats_per_bar;
            if !valid_time(bar) || points.len() == crate::tempo::MAX_POINTS {
                warnings.push(format!(
                    "Only the first {} tempo changes were imported.",
                    points.len()
                ));
                break;
            }
            let limited = value.clamp(crate::tempo::MIN_BPM, crate::tempo::MAX_BPM);
            clamped |= limited != *value;
            points.push(crate::tempo::TempoPoint {
                bar,
                bpm: limited,
                ramp: false,
            });
        }
        if clamped {
            warnings.push("Tempo changes outside 20-400 BPM were limited to that range.".into());
        }
        if points.is_empty() && !session.tempo_changes.is_empty() {
            warnings.push(
                "The song's own tempo changes were replaced by the file's single tempo.".into(),
            );
        }
        imported_changes = points.len();
        // A new meter moves the existing clips, so the automation follows them, as
        // `transport.setTimeSignature` does, in this same undo step.
        let lanes = crate::control::automation_on_bars(session, &transport.time_signature);
        commands.push(Command::SetTransport(transport));
        commands.push(Command::SetTempoChanges(points));
        commands.extend(lanes);
    }
    let mut report = ImportReport {
        track_ids: vec![],
        clip_ids: vec![],
        notes: total,
        controllers: controller_total,
        file_tempo: tempo,
        tempo_imported: options.import_tempo,
        tempo_changes: imported_changes,
        warnings,
    };
    for (index, (name, channel, notes, controllers, end)) in lanes.into_iter().enumerate() {
        let track_id = new_id("track");
        let clip_id = new_id("clip");
        let mut extra = std::collections::HashMap::new();
        extra.insert("midiChannel".into(), serde_json::json!(channel));
        commands.push(Command::AddTrack(Track {
            id: track_id.clone(),
            name: name.clone(),
            kind: "midi".into(),
            color: crate::control::TRACK_PALETTE[(session.tracks.len() + index) % 8].into(),
            armed: false,
            monitor: Default::default(),
            volume: 0.75,
            pan: 0.0,
            mute: false,
            solo: false,
            output: None,
            extra,
        }));
        commands.push(Command::SetStrip {
            track: track_id.clone(),
            strip: Strip::default(),
        });
        commands.push(Command::PutClip(Clip {
            id: clip_id.clone(),
            name,
            agent,
            track_id: track_id.clone(),
            start_bar: options.start_bar,
            length_bars: (end / beats_per_bar).max(1.0 / PPQ as f64),
            data: ClipData::Midi { notes, controllers },
        }));
        report.track_ids.push(track_id);
        report.clip_ids.push(clip_id);
    }
    // Validate against all existing content and capacity limits before returning a batch.
    let command = Command::Batch(commands);
    let mut probe = crate::store::Store::new(session.clone())?;
    probe.dispatch(command.clone())?;
    Ok((command, report))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiExportReport {
    pub path: std::path::PathBuf,
    pub track_count: usize,
    pub note_count: usize,
    pub controller_count: usize,
    pub ticks_per_quarter: u16,
    pub warnings: Vec<String>,
}

/// Export arrangement MIDI as SMF type 1 at 960 PPQ, clipping notes to region
/// bounds. Every selected MIDI track is included regardless of mute/solo state.
pub fn export(
    session: &Session,
    path: &Path,
    track_ids: Option<&[String]>,
) -> Result<MidiExportReport> {
    session.validate()?;
    let selected = crate::export::select_tracks(session, track_ids)?;
    if track_ids.is_some() && selected.iter().any(|t| t.kind != "midi") {
        return Err("MIDI export accepts instrument tracks only".into());
    }
    let tracks: Vec<_> = selected.into_iter().filter(|t| t.kind == "midi").collect();
    if tracks.is_empty() {
        return Err("There are no MIDI tracks to export".into());
    }
    let meter = &session.transport.time_signature;
    let mut smf = Smf::new(Header::new(
        Format::Parallel,
        Timing::Metrical(u15::new(PPQ)),
    ));
    let mut conductor = vec![];
    let mut previous = 0;
    for (tick, micros) in tempo_events(session) {
        conductor.push(TrackEvent {
            delta: u28::new((tick - previous).min(0x0fff_ffff) as u32),
            kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(micros))),
        });
        previous = tick;
    }
    // The meter goes with the first tempo, at tick zero.
    conductor.insert(
        1,
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::TimeSignature(
                meter.numerator as u8,
                meter.denominator.trailing_zeros() as u8,
                24,
                8,
            )),
        },
    );
    conductor.push(TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });
    smf.tracks.push(conductor);
    let mut count = 0;
    let mut controller_count = 0;
    for (index, track) in tracks.iter().enumerate() {
        let channel = track
            .extra
            .get("midiChannel")
            .and_then(serde_json::Value::as_u64)
            .filter(|v| *v < 16)
            .unwrap_or((index % 16) as u64) as u8;
        // (tick, rank, channel, message): at one tick note-offs go first, then controllers,
        // then note-ons, so a bend or pedal is in place before the note it shapes. What was
        // played on channel 0 goes out on the track's channel; any other channel is kept.
        let mut events: Vec<(u64, u8, u8, MidiMessage)> = vec![];
        let on = |own: u8| if own == 0 { channel } else { own & 15 };
        for clip in session.clips.iter().filter(|c| c.track_id == track.id) {
            if let ClipData::Midi { notes, controllers } = &clip.data {
                let offset = clip.start_bar * session.beats_per_bar();
                let length = clip.length_bars * session.beats_per_bar();
                let end = offset + length;
                for note in notes {
                    let start = offset + note.start;
                    if start >= end {
                        continue;
                    }
                    let note_end = (start + note.length).min(end);
                    let start_tick = (start * PPQ as f64).round() as u64;
                    let off = ((note_end * PPQ as f64).round() as u64).max(start_tick + 1);
                    events.push((
                        start_tick,
                        2,
                        on(note.channel),
                        MidiMessage::NoteOn {
                            key: u7::new(note.pitch),
                            vel: u7::new(note.velocity),
                        },
                    ));
                    events.push((
                        off,
                        0,
                        on(note.channel),
                        MidiMessage::NoteOff {
                            key: u7::new(note.pitch),
                            vel: u7::new(0),
                        },
                    ));
                    count += 1;
                }
                for played in crate::controllers::playback(controllers, length) {
                    let tick = ((offset + played.time) * PPQ as f64).round() as u64;
                    let message = match played.kind {
                        ControllerKind::Cc => MidiMessage::Controller {
                            controller: u7::new(played.number.unwrap_or(0).min(127)),
                            value: u7::new(played.value.clamp(0, 127) as u8),
                        },
                        ControllerKind::Bend => MidiMessage::PitchBend {
                            bend: midly::PitchBend::from_int(played.value),
                        },
                        ControllerKind::Pressure => MidiMessage::ChannelAftertouch {
                            vel: u7::new(played.value.clamp(0, 127) as u8),
                        },
                        ControllerKind::PolyPressure => MidiMessage::Aftertouch {
                            key: u7::new(played.number.unwrap_or(0).min(127)),
                            vel: u7::new(played.value.clamp(0, 127) as u8),
                        },
                    };
                    // A reset shares its tick with the next clip's first value; it goes first.
                    events.push((
                        tick,
                        if played.reset { 0 } else { 1 },
                        on(played.channel),
                        message,
                    ));
                    controller_count += 1;
                }
            }
        }
        events.sort_by_key(|(tick, rank, _, _)| (*tick, *rank));
        let mut sequence = vec![TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::TrackName(track.name.as_bytes())),
        }];
        let mut previous = 0;
        for (tick, _, channel, message) in events {
            let delta = tick - previous;
            if delta > 0x0fff_ffff {
                return Err("MIDI gap exceeds the Standard MIDI File delta-time limit".into());
            }
            sequence.push(TrackEvent {
                delta: u28::new(delta as u32),
                kind: TrackEventKind::Midi {
                    channel: u4::new(channel),
                    message,
                },
            });
            previous = tick;
        }
        sequence.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });
        smf.tracks.push(sequence);
    }
    document::atomic_write(path, |file| smf.write_std(file).map_err(|e| e.to_string()))?;
    Ok(MidiExportReport{path:path.into(),track_count:tracks.len(),note_count:count,controller_count,ticks_per_quarter:PPQ,
        warnings:vec!["MIDI contains notes, controllers, pitch bend, pressure, track names, the tempo changes (a ramp as a step every sixteenth note) and the meter. Audio, plugins, mixer settings and automation are not embedded.".into()]})
}

/// The song's tempo as (tick, microseconds per quarter) at 960 PPQ, starting at tick zero.
/// A file cannot glide, so a ramp becomes a step every sixteenth note, each lasting exactly
/// as long as that stretch of the ramp: the notes after it land on the same seconds.
fn tempo_events(session: &Session) -> Vec<(u64, u32)> {
    let map = session.tempo_map();
    let bpb = session.beats_per_bar();
    let micros = |bpm: f64| (60_000_000.0 / bpm).round().clamp(1.0, 16_777_215.0) as u32;
    let tick = |beat: f64| (beat * PPQ as f64).round() as u64;
    let mut events = BTreeMap::from([(0, micros(session.transport.tempo))]);
    let mut previous = 0.0;
    for point in &session.tempo_changes {
        let beat = point.bar * bpb;
        if point.ramp {
            let steps = ((beat - previous) * 4.0).ceil().max(1.0) as usize;
            for i in 0..steps {
                let from = previous + (beat - previous) * i as f64 / steps as f64;
                let to = previous + (beat - previous) * (i + 1) as f64 / steps as f64;
                events.insert(
                    tick(from),
                    micros(60.0 * (to - from) / map.duration(from, to)),
                );
            }
        }
        events.insert(tick(beat), micros(point.bpm));
        previous = beat;
    }
    events.into_iter().collect()
}
