//! What changed between two versions of a song, in a person's words: the live context
//! ("since your last step the person moved Bass 1 to bar 4"), a turn's change list and
//! `harness.changes`.

use crate::model::*;
use std::collections::{HashMap, HashSet};

fn db(volume: f32) -> String {
    match crate::control_overview::fader_db(volume) {
        Some(v) => format!("{v:+.1} dB"),
        None => "-inf dB".into(),
    }
}
fn bars(clip: &Clip) -> String {
    format!(
        "bars {}–{}",
        trim(clip.start_bar),
        trim(clip.start_bar + clip.length_bars)
    )
}
fn trim(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r.fract() == 0.0 {
        format!("{r:.0}")
    } else {
        format!("{r}")
    }
}
fn plugins(strip: Option<&Strip>) -> Vec<String> {
    strip
        .map(|s| {
            s.inserts
                .iter()
                .filter(|i| !i.name.is_empty())
                .map(|i| {
                    if i.state == "bypassed" {
                        format!("{} (bypassed)", i.name)
                    } else {
                        i.name.clone()
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}
fn params(strip: Option<&Strip>) -> Vec<(String, std::collections::BTreeMap<u32, f64>)> {
    strip
        .map(|s| {
            s.synth
                .iter()
                .chain(s.inserts.iter())
                .map(|i| (i.name.clone(), i.params.clone()))
                .collect()
        })
        .unwrap_or_default()
}
fn notes(clip: &Clip) -> Option<&[Note]> {
    match &clip.data {
        ClipData::Midi { notes, .. } => Some(notes),
        ClipData::Audio { .. } => None,
    }
}

/// Every change from `old` to `new`, at most `limit` lines (the rest counted in a last line).
pub fn describe(old: &Session, new: &Session, limit: usize) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let name_of = |s: &Session, id: &str| {
        s.tracks
            .iter()
            .find(|t| t.id == id)
            .map_or_else(|| id.to_string(), |t| t.name.clone())
    };
    if old.name != new.name {
        out.push(format!("Renamed the song to {}", new.name));
    }
    let (a, b) = (&old.transport, &new.transport);
    if a.tempo != b.tempo {
        out.push(format!("Tempo {} → {} BPM", trim(a.tempo), trim(b.tempo)));
    }
    if a.key != b.key {
        out.push(format!("Key {} → {}", a.key, b.key));
    }
    if a.time_signature.numerator != b.time_signature.numerator
        || a.time_signature.denominator != b.time_signature.denominator
    {
        out.push(format!(
            "Meter {}/{} → {}/{}",
            a.time_signature.numerator,
            a.time_signature.denominator,
            b.time_signature.numerator,
            b.time_signature.denominator
        ));
    }
    if a.cycle != b.cycle
        || a.cycle_start_bar != b.cycle_start_bar
        || a.cycle_end_bar != b.cycle_end_bar
    {
        out.push(if b.cycle {
            format!(
                "Cycle on, bars {}–{}",
                trim(b.cycle_start_bar),
                trim(b.cycle_end_bar)
            )
        } else {
            "Cycle off".into()
        });
    }
    if old.tempo_changes.len() != new.tempo_changes.len()
        || old
            .tempo_changes
            .iter()
            .zip(&new.tempo_changes)
            .any(|(x, y)| x.bar != y.bar || x.bpm != y.bpm || x.ramp != y.ramp)
    {
        out.push(format!(
            "Tempo changes: {} → {}",
            old.tempo_changes.len(),
            new.tempo_changes.len()
        ));
    }
    if old.master_volume != new.master_volume {
        out.push(format!(
            "Master fader {} → {}",
            db(old.master_volume),
            db(new.master_volume)
        ));
    }
    // Tracks.
    let old_tracks: HashMap<&str, &Track> = old.tracks.iter().map(|t| (t.id.as_str(), t)).collect();
    let new_ids: HashSet<&str> = new.tracks.iter().map(|t| t.id.as_str()).collect();
    for t in &new.tracks {
        match old_tracks.get(t.id.as_str()) {
            None => {
                let strip = new.strips.get(&t.id);
                out.push(format!(
                    "Added {} track {}{}",
                    t.kind,
                    t.name,
                    match (t.kind.as_str(), strip) {
                        ("midi", Some(s)) => format!(" ({})", s.instrument_name()),
                        ("midi", None) => " (ryolune Synth)".into(),
                        _ => String::new(),
                    }
                ));
            }
            Some(o) => {
                let mut what = vec![];
                if o.name != t.name {
                    what.push(format!("renamed from {}", o.name));
                }
                if o.mute != t.mute {
                    what.push(if t.mute { "muted" } else { "unmuted" }.to_string());
                }
                if o.solo != t.solo {
                    what.push(if t.solo { "soloed" } else { "unsoloed" }.to_string());
                }
                if o.volume != t.volume {
                    what.push(format!("fader {} → {}", db(o.volume), db(t.volume)));
                }
                if o.pan != t.pan {
                    what.push(format!("pan {:.0} → {:.0}", o.pan, t.pan));
                }
                if o.output != t.output {
                    what.push(format!(
                        "output → {}",
                        t.output
                            .as_deref()
                            .map_or_else(|| "Stereo Out".to_string(), |b| name_of(new, b))
                    ));
                }
                if o.armed != t.armed {
                    what.push(if t.armed { "armed" } else { "disarmed" }.to_string());
                }
                if !what.is_empty() {
                    out.push(format!("{}: {}", t.name, what.join(", ")));
                }
            }
        }
    }
    for t in &old.tracks {
        if !new_ids.contains(t.id.as_str()) {
            out.push(format!("Removed track {}", t.name));
        }
    }
    let old_order: Vec<&str> = old
        .tracks
        .iter()
        .filter(|t| new_ids.contains(t.id.as_str()))
        .map(|t| t.id.as_str())
        .collect();
    let new_order: Vec<&str> = new
        .tracks
        .iter()
        .filter(|t| old_tracks.contains_key(t.id.as_str()))
        .map(|t| t.id.as_str())
        .collect();
    if old_order != new_order {
        out.push("Reordered tracks".into());
    }
    // Strips (instrument, inserts, sends, plugin parameters), tracks and buses alike.
    let mut strip_ids: Vec<&String> = new.strips.keys().chain(old.strips.keys()).collect();
    strip_ids.sort();
    strip_ids.dedup();
    for id in strip_ids {
        let (o, n) = (old.strips.get(id), new.strips.get(id));
        if !old_tracks.contains_key(id.as_str()) && new.tracks.iter().any(|t| &t.id == id) {
            // A new track: its strip is part of "Added".
            continue;
        }
        if !new_ids.contains(id.as_str()) && old_tracks.contains_key(id.as_str()) {
            continue;
        }
        let label = match id.as_str() {
            MASTER => "Master".to_string(),
            BUS_A => "A · Reverb".to_string(),
            BUS_B => "B · Delay".to_string(),
            other => name_of(new, other),
        };
        let mut what = vec![];
        let instrument = |s: Option<&Strip>| s.map(Strip::instrument_name);
        if instrument(o) != instrument(n)
            && new.tracks.iter().any(|t| &t.id == id && t.kind == "midi")
        {
            what.push(format!(
                "instrument → {}",
                instrument(n).unwrap_or_else(|| "ryolune Synth".into())
            ));
        }
        let (po, pn) = (plugins(o), plugins(n));
        if po != pn {
            what.push(format!("inserts [{}] → [{}]", po.join(", "), pn.join(", ")));
        } else if params(o) != params(n) {
            let changed: Vec<String> = params(o)
                .into_iter()
                .zip(params(n))
                .filter(|(a, b)| a.1 != b.1)
                .map(|(_, b)| b.0)
                .collect();
            what.push(format!("{} settings changed", changed.join(", ")));
        }
        let sends = |s: Option<&Strip>| {
            s.map(|s| {
                s.sends
                    .iter()
                    .map(|x| (x.level_db.map(|v| (v * 10.0).round() as i32), x.bus.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
        };
        if sends(o) != sends(n) {
            what.push("sends changed".into());
        }
        if !what.is_empty() {
            out.push(format!("{label}: {}", what.join(", ")));
        }
    }
    // Clips.
    let old_clips: HashMap<&str, &Clip> = old.clips.iter().map(|c| (c.id.as_str(), c)).collect();
    let new_clip_ids: HashSet<&str> = new.clips.iter().map(|c| c.id.as_str()).collect();
    let mut added: HashMap<String, (usize, usize, f64, f64)> = HashMap::new();
    for c in &new.clips {
        match old_clips.get(c.id.as_str()) {
            None => {
                let entry =
                    added
                        .entry(name_of(new, &c.track_id))
                        .or_insert((0, 0, f64::INFINITY, 0.0));
                entry.0 += 1;
                entry.1 += notes(c).map_or(0, <[Note]>::len);
                entry.2 = entry.2.min(c.start_bar);
                entry.3 = entry.3.max(c.start_bar + c.length_bars);
            }
            Some(o) => {
                let mut what = vec![];
                if o.track_id != c.track_id {
                    what.push(format!("moved to {}", name_of(new, &c.track_id)));
                }
                if o.start_bar != c.start_bar || o.length_bars != c.length_bars {
                    what.push(format!("{} → {}", bars(o), bars(c)));
                }
                match (notes(o), notes(c)) {
                    (Some(a), Some(b)) if a.len() != b.len() => {
                        what.push(format!("{} → {} notes", a.len(), b.len()))
                    }
                    (Some(a), Some(b))
                        if a.iter().zip(b).any(|(x, y)| {
                            x.pitch != y.pitch
                                || x.start != y.start
                                || x.length != y.length
                                || x.velocity != y.velocity
                        }) =>
                    {
                        what.push("notes edited".into())
                    }
                    (None, None) => {
                        if let (
                            ClipData::Audio {
                                fade_in: a,
                                fade_out: b,
                                gain_db: g,
                                ..
                            },
                            ClipData::Audio {
                                fade_in: c2,
                                fade_out: d,
                                gain_db: h,
                                ..
                            },
                        ) = (&o.data, &c.data)
                        {
                            if a != c2 || b != d {
                                what.push("fades changed".into());
                            }
                            if g != h {
                                what.push(format!("gain {g:+.1} → {h:+.1} dB"));
                            }
                        }
                    }
                    _ => {}
                }
                if o.name != c.name && what.is_empty() {
                    what.push(format!("renamed from {}", o.name));
                }
                if !what.is_empty() {
                    out.push(format!(
                        "Clip {} on {}: {}",
                        c.name,
                        name_of(new, &c.track_id),
                        what.join(", ")
                    ));
                }
            }
        }
    }
    let mut added: Vec<(String, (usize, usize, f64, f64))> = added.into_iter().collect();
    added.sort_by(|a, b| a.0.cmp(&b.0));
    for (track, (count, note_count, from, to)) in added {
        out.push(format!(
            "Added {count} clip{} on {track} (bars {}–{}{})",
            if count == 1 { "" } else { "s" },
            trim(from),
            trim(to),
            if note_count > 0 {
                format!(", {note_count} notes")
            } else {
                String::new()
            }
        ));
    }
    let mut removed: HashMap<String, usize> = HashMap::new();
    for c in &old.clips {
        if !new_clip_ids.contains(c.id.as_str()) {
            *removed.entry(name_of(old, &c.track_id)).or_default() += 1;
        }
    }
    let mut removed: Vec<(String, usize)> = removed.into_iter().collect();
    removed.sort();
    for (track, count) in removed {
        out.push(format!(
            "Removed {count} clip{} from {track}",
            if count == 1 { "" } else { "s" }
        ));
    }
    // Sections and automation.
    let marks = |s: &Session| {
        s.markers
            .iter()
            .map(|m| format!("{} @{}", m.name, trim(m.bar)))
            .collect::<Vec<_>>()
    };
    if marks(old) != marks(new) {
        out.push(format!("Sections: {}", marks(new).join(", ")));
    }
    let lanes = |s: &Session| {
        s.automation
            .iter()
            .map(|l| serde_json::to_string(l).unwrap_or_default())
            .collect::<Vec<_>>()
    };
    if lanes(old) != lanes(new) {
        out.push(format!(
            "Automation changed ({} → {} lanes)",
            old.automation.len(),
            new.automation.len()
        ));
    }
    if out.len() > limit {
        let more = out.len() - limit;
        out.truncate(limit);
        out.push(format!("… and {more} more changes"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_read_like_a_person_would_say_them() {
        let old = crate::store::empty();
        let mut new = old.clone();
        new.transport.tempo = 96.0;
        new.transport.key = "A minor".into();
        new.tracks[1].volume = 0.5;
        new.tracks[0].mute = true;
        new.clips.push(Clip {
            id: "c1".into(),
            name: "Groove".into(),
            agent: true,
            track_id: new.tracks[0].id.clone(),
            start_bar: 0.0,
            length_bars: 4.0,
            data: ClipData::Midi {
                notes: vec![Note {
                    id: "n1".into(),
                    start: 0.0,
                    length: 1.0,
                    pitch: 36,
                    velocity: 100,
                    agent: true,
                    channel: 0,
                }],
                controllers: vec![],
            },
        });
        let lines = describe(&old, &new, 20);
        let text = lines.join("\n");
        assert!(text.contains("Tempo"), "{text}");
        assert!(text.contains("Key"), "{text}");
        assert!(text.contains("muted"), "{text}");
        assert!(text.contains("fader"), "{text}");
        assert!(text.contains("Added 1 clip on"), "{text}");
        assert!(describe(&old, &old, 20).is_empty());
        assert_eq!(describe(&old, &new, 2).len(), 3);
    }
}
