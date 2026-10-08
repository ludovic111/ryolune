//! The agent harness (lsuite's HARNESS.md): what makes an agent good at music in ryolune,
//! whichever agent it is: the built-in one, Claude Code or Codex through `ryolune-mcp`, or the
//! lsuite app's suite agent.
//!
//! - **Brief** (`brief.md`): the expert system prompt, one source for the built-in agent and
//!   the MCP server's `instructions`, followed by the index of the skills.
//! - **Skills** (`skills/*.md`): playbooks for the trade's jobs, loaded with `harness.skill`;
//!   over MCP each is also a prompt and a resource (`ryolune://skills/<name>`).
//! - **Live context** (`harness.context`): the song in brief and what the person changed since
//!   the agent's last step, sent before every model step.
//! - **Eyes and ears** (`harness.look`, `harness.measure`, [`look`]): a picture of a bar range
//!   and the loudness numbers.
//! - **Checkpoints** (`harness.checkpoint`, `harness.changes`, `harness.revert`): one undo for
//!   a whole agent turn, with its change list.

pub mod analysis;
pub mod changes;
pub mod look;
pub mod loudness;

use crate::{
    control::{edit, opt, query, req, Args, Host, Kind, Spec},
    model::*,
    store::Command,
    Result,
};
use serde_json::{json, Value};

const BRIEF: &str = include_str!("brief.md");

/// The skills built in, as `(file name, markdown with its front matter)`.
const SKILL_FILES: &[(&str, &str)] = &[
    (
        "compose-from-brief",
        include_str!("skills/compose-from-brief.md"),
    ),
    (
        "drum-programming",
        include_str!("skills/drum-programming.md"),
    ),
    ("bass-and-chords", include_str!("skills/bass-and-chords.md")),
    (
        "melody-and-hooks",
        include_str!("skills/melody-and-hooks.md"),
    ),
    ("arrangement", include_str!("skills/arrangement.md")),
    ("sound-design", include_str!("skills/sound-design.md")),
    (
        "automation-and-movement",
        include_str!("skills/automation-and-movement.md"),
    ),
    ("mixing", include_str!("skills/mixing.md")),
    ("mastering", include_str!("skills/mastering.md")),
    (
        "stems-and-export",
        include_str!("skills/stems-and-export.md"),
    ),
    (
        "score-to-picture",
        include_str!("skills/score-to-picture.md"),
    ),
    ("write-a-plugin", include_str!("skills/write-a-plugin.md")),
    ("review-and-fix", include_str!("skills/review-and-fix.md")),
];

/// One skill: a playbook for a job of the trade.
#[derive(Clone, Debug)]
pub struct Skill {
    pub name: &'static str,
    pub title: &'static str,
    /// When to use it, one sentence.
    pub when: &'static str,
    /// The playbook, markdown without its front matter.
    pub body: &'static str,
}

fn parse(file: &'static str, text: &'static str) -> Skill {
    // A Windows checkout may turn the files' line endings into CRLF.
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        .unwrap_or_else(|| panic!("skill {file} has no front matter"));
    let (head, body) = rest
        .split_once("\n---\n")
        .or_else(|| rest.split_once("\n---\r\n"))
        .unwrap_or_else(|| panic!("skill {file} has an unfinished front matter"));
    let field = |key: &str| {
        head.lines()
            .find_map(|l| l.strip_prefix(key).and_then(|v| v.strip_prefix(':')))
            .map(str::trim)
            .unwrap_or_else(|| panic!("skill {file} has no `{key}`"))
    };
    Skill {
        name: field("name"),
        title: field("title"),
        when: field("when"),
        body: body.trim_start(),
    }
}

/// Every built-in skill, in the order of the index.
pub fn skills() -> &'static [Skill] {
    static SKILLS: std::sync::LazyLock<Vec<Skill>> = std::sync::LazyLock::new(|| {
        SKILL_FILES
            .iter()
            .map(|(file, text)| parse(file, text))
            .collect()
    });
    &SKILLS
}
pub fn skill(name: &str) -> Option<&'static Skill> {
    let name = name.trim().trim_end_matches(".md");
    skills().iter().find(|s| s.name == name)
}

/// The brief with the index of the skills: what the built-in agent gets as its system prompt
/// and what `ryolune-mcp` sends as its `instructions` (after a line about its mode).
pub fn brief() -> String {
    let mut text = BRIEF.trim_end().to_string();
    text.push_str(
        "\n\n## Skills\n\nLoad one with `harness.skill name=<name>` before the job it covers:\n\n",
    );
    for s in skills() {
        text.push_str(&format!("- `{}`: {}\n", s.name, s.title));
    }
    text
}

const KEY: crate::control::Param = opt(
    "key",
    Kind::String,
    "Whose last look to compare with: `agent` (the built-in agent, default) or `mcp` (an outside agent). The window moves it after each of that agent's commands.",
);

pub const SPECS: &[Spec] = &[
    query("harness.brief", "The agent's expert brief (markdown): the music producer's role in ryolune, the song's mental model, the commands for common jobs, the quality bar (levels, loudness targets), the finish routine and the index of skills. The built-in agent and ryolune-mcp's instructions use this same text.", &[]),
    query("harness.skills", "The playbooks for music jobs (compose, drums, bass and chords, melody, arrangement, sound design, automation, mixing, mastering, export, scoring to picture, writing a plugin, review): [{name, title, when}]. Load one with harness.skill.", &[]),
    query("harness.skill", "One skill's playbook (markdown): when to use it, the steps with the exact commands, and the checks that prove the job worked.", &[
        req("name", Kind::String, "Skill name from harness.skills, for example mixing."),
    ]),
    query("harness.context", "The live context an agent gets before each step: the song in brief (tempo, key, meter, length, sections), every track in one line, the selection, the playhead, the newest checkpoint and what changed since this agent's last command (the person's edits while it was thinking).", &[KEY]),
    query("harness.look", "Look at and listen to a bar range: renders it offline like an export and returns a picture (waveform with the bar grid and sections, short-term loudness, average spectrum against a pink slope, piano roll of the notes in track colours) plus the numbers: integrated/short-term/momentary loudness (LUFS), loudness range, true peak (dBTP), sample peak, clipped samples, energy per band, and findings that name the fix. The picture reaches the model as an image (built-in agent with a vision model; MCP image content) and is written as a PNG (`image.path`).", &[
        opt("fromBar", Kind::Number, "Zero-based first bar (default 0)."),
        opt("toBar", Kind::Number, "Exclusive end bar (default the end of the song). At most 600 seconds."),
        opt("trackId", Kind::String, "Only this track, soloed (its buses and the master chain still apply)."),
        opt("view", Kind::String, "all (default), mix (waveform, loudness, spectrum) or notes (piano roll only: no render, quick)."),
        opt("targetLufs", Kind::Number, "A loudness target to draw and compare with, for example -14."),
        opt("tailSeconds", Kind::Number, "Seconds after the range to include (reverb tails), 0-30, default 0."),
        opt("path", Kind::String, "Where to write the PNG. Defaults to a new file in the app data folder (looks/)."),
    ]),
    query("harness.measure", "Measure loudness without a picture: integrated, short-term max and momentary max loudness (LUFS, ITU-R BS.1770 / EBU R128 gating), loudness range (LU), true peak (dBTP, 4x oversampled), sample peak (dBFS), clipped samples, energy per band (sub, bass, low mids, high mids, air) and findings. tracks=true measures each track on its own as well (gain staging).", &[
        opt("fromBar", Kind::Number, "Zero-based first bar (default 0)."),
        opt("toBar", Kind::Number, "Exclusive end bar (default the end of the song). At most 600 seconds."),
        opt("trackId", Kind::String, "Only this track, soloed."),
        opt("tracks", Kind::Boolean, "Also measure every track on its own (ranges up to 120 seconds)."),
        opt("targetLufs", Kind::Number, "Integrated loudness to aim for; findings say how far off it is."),
        opt("tailSeconds", Kind::Number, "Seconds after the range to include, 0-30, default 0."),
    ]),
    query("harness.checkpoint", "Take a checkpoint of the song before a job, so the whole job can be reverted in one step (harness.revert) and its changes listed (harness.changes). The built-in agent takes one at the start of every turn. Checkpoints last until another song is opened.", &[
        opt("label", Kind::String, "What the job is, for example \"Mix pass\"."),
    ]),
    query("harness.checkpoints", "The checkpoints of this song, oldest first, each with how many changes were made since.", &[]),
    query("harness.changes", "What changed since a checkpoint (default the newest), in plain words: tracks, clips, notes, sounds, levels, sections, tempo and key.", &[
        opt("checkpoint", Kind::String, "Checkpoint id from harness.checkpoints (default the newest)."),
    ]),
    edit("harness.revert", "Return the song to a checkpoint (default the newest) in one undo step: everything changed since is undone together, and history.undo brings it back. Answers with the changes it undid.", &[
        opt("checkpoint", Kind::String, "Checkpoint id from harness.checkpoints (default the newest)."),
    ]),
];

pub fn serves(name: &str) -> bool {
    SPECS.iter().any(|s| s.name == name)
}

fn bars_text(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r.fract() == 0.0 {
        format!("{r:.0}")
    } else {
        format!("{r}")
    }
}

/// One line per track: number, name, kind, instrument, clips and where, state, fader.
fn track_line(s: &Session, index: usize, t: &Track) -> String {
    let mut parts = vec![format!("{} {}", index + 1, t.name), t.kind.clone()];
    if t.kind == "midi" {
        parts.push(
            s.strips
                .get(&t.id)
                .map_or_else(|| "ryolune Synth".to_string(), Strip::instrument_name),
        );
    }
    let clips: Vec<&Clip> = s.clips.iter().filter(|c| c.track_id == t.id).collect();
    if !clips.is_empty() {
        let from = clips
            .iter()
            .map(|c| c.start_bar)
            .fold(f64::INFINITY, f64::min);
        let to = clips
            .iter()
            .map(|c| c.start_bar + c.length_bars)
            .fold(0.0, f64::max);
        let notes: usize = clips
            .iter()
            .map(|c| match &c.data {
                ClipData::Midi { notes, .. } => notes.len(),
                ClipData::Audio { .. } => 0,
            })
            .sum();
        parts.push(format!(
            "{} clip{} bars {}–{}{}",
            clips.len(),
            if clips.len() == 1 { "" } else { "s" },
            bars_text(from),
            bars_text(to),
            if notes > 0 {
                format!(" ({notes} notes)")
            } else {
                String::new()
            }
        ));
    } else if t.kind != "bus" {
        parts.push("empty".into());
    }
    let inserts: Vec<String> = s
        .strips
        .get(&t.id)
        .map(|st| {
            st.inserts
                .iter()
                .filter(|i| !i.name.is_empty())
                .map(|i| i.name.clone())
                .collect()
        })
        .unwrap_or_default();
    if !inserts.is_empty() {
        parts.push(format!("fx {}", inserts.join(" > ")));
    }
    if let Some(bus) = &t.output {
        let name = s
            .tracks
            .iter()
            .find(|x| &x.id == bus)
            .map_or(bus.as_str(), |x| x.name.as_str());
        parts.push(format!("→ {name}"));
    }
    if t.mute {
        parts.push("MUTED".into());
    }
    if t.solo {
        parts.push("SOLO".into());
    }
    parts.push(match crate::control_overview::fader_db(t.volume) {
        Some(db) => format!("{db:+.1} dB"),
        None => "fader at -inf".into(),
    });
    parts.join(" · ")
}

/// `harness.context`.
pub fn context(host: &dyn Host, key: &str) -> Value {
    let s = host.store().session();
    let t = &s.transport;
    let bpb = s.beats_per_bar();
    let end = s.end_bar();
    let seconds = s.bars_seconds(0.0, end);
    let mut song = format!(
        "{} · {} BPM{} · {}/{} · {} bars ({}:{:02})",
        s.name,
        bars_text(t.tempo),
        if s.tempo_changes.is_empty() {
            String::new()
        } else {
            format!(" ({} tempo changes)", s.tempo_changes.len())
        },
        t.time_signature.numerator,
        t.time_signature.denominator,
        bars_text(end),
        seconds as u64 / 60,
        seconds as u64 % 60
    );
    if !t.key.trim().is_empty() {
        song.push_str(&format!(" · {}", t.key));
    }
    const MAX_TRACKS: usize = 32;
    let tracks: Vec<String> = s
        .tracks
        .iter()
        .enumerate()
        .take(MAX_TRACKS)
        .map(|(i, tr)| track_line(s, i, tr))
        .collect();
    let master: Vec<String> = s
        .strips
        .get(MASTER)
        .map(|st| {
            st.inserts
                .iter()
                .filter(|i| !i.name.is_empty())
                .map(|i| i.name.clone())
                .collect()
        })
        .unwrap_or_default();
    let selected_track = s
        .view
        .selected_track_id
        .as_ref()
        .and_then(|id| s.tracks.iter().find(|tr| &tr.id == id))
        .map(|tr| tr.name.clone());
    let selected_clip = s
        .view
        .selected_clip_id
        .as_ref()
        .and_then(|id| s.clips.iter().find(|c| &c.id == id))
        .map(|c| {
            format!(
                "{} (bars {}–{})",
                c.name,
                bars_text(c.start_bar),
                bars_text(c.start_bar + c.length_bars)
            )
        });
    let mut out = json!({
        "song": song,
        "playhead": format!("bar {}{}", bars_text(host.position() / bpb), if host.playing() { " (playing)" } else { "" }),
        "cycle": t.cycle.then(|| format!("bars {}–{}", bars_text(t.cycle_start_bar), bars_text(t.cycle_end_bar))),
        "sections": s.markers.iter().map(|m| format!("{} @{}", m.name, bars_text(m.bar))).collect::<Vec<_>>(),
        "tracks": tracks,
        "master": format!(
            "{}{}",
            crate::control_overview::fader_db(s.master_volume).map_or("-inf dB".into(), |db| format!("{db:+.1} dB")),
            if master.is_empty() { String::new() } else { format!(" · fx {}", master.join(" > ")) }
        ),
        "selection": {"track": selected_track, "clip": selected_clip},
        "undo": host.store().undo_depth(),
        "revision": host.store().revision,
    });
    if s.tracks.len() > MAX_TRACKS {
        out["moreTracks"] = json!(s.tracks.len() - MAX_TRACKS);
    }
    if let Some(cp) = host.store().find_checkpoint(None) {
        out["checkpoint"] = json!({
            "id": cp.id, "label": cp.label,
            "changes": changes::describe(&cp.session, s, 200).len(),
        });
    }
    if let Some(seen) = host.store().mark(key) {
        let changed = changes::describe(&seen, s, 12);
        if !changed.is_empty() {
            out["changedSinceYourLastStep"] = json!(changed);
        }
    }
    out
}

/// Compact text of a context for a model message.
pub fn context_text(value: &Value) -> String {
    let mut lines = vec![];
    if let Some(song) = value["song"].as_str() {
        lines.push(format!("Song: {song}"));
    }
    if let Some(p) = value["playhead"].as_str() {
        let cycle = value["cycle"]
            .as_str()
            .map(|c| format!(" · cycle {c}"))
            .unwrap_or_default();
        lines.push(format!("Playhead: {p}{cycle}"));
    }
    if let Some(sections) = value["sections"].as_array().filter(|a| !a.is_empty()) {
        let names: Vec<&str> = sections.iter().filter_map(Value::as_str).collect();
        lines.push(format!("Sections: {}", names.join(", ")));
    }
    if let Some(tracks) = value["tracks"].as_array() {
        lines.push("Tracks:".into());
        for t in tracks.iter().filter_map(Value::as_str) {
            lines.push(format!("  {t}"));
        }
        if let Some(more) = value["moreTracks"].as_u64() {
            lines.push(format!("  … {more} more (session.overview)"));
        }
    }
    if let Some(m) = value["master"].as_str() {
        lines.push(format!("Master: {m}"));
    }
    let sel = &value["selection"];
    if sel["track"].is_string() || sel["clip"].is_string() {
        lines.push(format!(
            "Selected: {}{}",
            sel["track"].as_str().unwrap_or("no track"),
            sel["clip"]
                .as_str()
                .map(|c| format!(", clip {c}"))
                .unwrap_or_default()
        ));
    }
    if let Some(changed) = value["changedSinceYourLastStep"].as_array() {
        lines.push(
            "Changed since your last step (by the person or a finished job; keep their changes):"
                .into(),
        );
        for c in changed.iter().filter_map(Value::as_str) {
            lines.push(format!("  - {c}"));
        }
    }
    lines.join("\n")
}

pub(crate) fn call(host: &mut dyn Host, name: &str, a: &Args) -> Result<Value> {
    match name {
        "harness.brief" => {
            let text = brief();
            Ok(json!({ "words": text.split_whitespace().count(), "markdown": text }))
        }
        "harness.skills" => Ok(json!(skills()
            .iter()
            .map(|s| json!({"name": s.name, "title": s.title, "when": s.when}))
            .collect::<Vec<_>>())),
        "harness.skill" => {
            let wanted = a.str("name")?;
            let s = skill(wanted).ok_or_else(|| {
                format!(
                    "No skill `{wanted}`. Skills: {}",
                    skills()
                        .iter()
                        .map(|s| s.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            Ok(json!({"name": s.name, "title": s.title, "when": s.when, "markdown": s.body}))
        }
        "harness.context" => Ok(context(host, a.opt_str("key").unwrap_or("agent"))),
        "harness.look" => look::look(host, a),
        "harness.measure" => look::measure(host, a),
        "harness.checkpoint" => {
            let label = a
                .opt_str("label")
                .unwrap_or("Checkpoint")
                .trim()
                .to_string();
            let cp = host.store_mut().checkpoint(if label.is_empty() {
                "Checkpoint"
            } else {
                &label
            });
            Ok(
                json!({"id": cp.id, "label": cp.label, "createdAt": cp.created_at, "revision": cp.revision}),
            )
        }
        "harness.checkpoints" => {
            let s = host.store().session();
            Ok(json!(host
                .store()
                .checkpoints()
                .iter()
                .map(|cp| json!({
                    "id": cp.id, "label": cp.label, "createdAt": cp.created_at,
                    "changes": changes::describe(&cp.session, s, 200).len(),
                }))
                .collect::<Vec<_>>()))
        }
        "harness.changes" => {
            let cp = checkpoint(host, a)?;
            let list = changes::describe(&cp.session, host.store().session(), 60);
            Ok(
                json!({"checkpoint": cp.id, "label": cp.label, "createdAt": cp.created_at, "changes": list}),
            )
        }
        "harness.revert" => {
            let cp = checkpoint(host, a)?;
            let undone = changes::describe(&cp.session, host.store().session(), 60);
            if undone.is_empty() {
                return Ok(
                    json!({"checkpoint": cp.id, "reverted": false, "undone": undone, "message": "Nothing changed since that checkpoint."}),
                );
            }
            host.dispatch(Command::RestoreTake(Box::new((*cp.session).clone())))?;
            Ok(json!({"checkpoint": cp.id, "label": cp.label, "reverted": true, "undone": undone}))
        }
        _ => Err(format!("Unknown command `{name}`")),
    }
}

fn checkpoint(host: &dyn Host, a: &Args) -> Result<crate::store::Checkpoint> {
    let id = a.opt_str("checkpoint");
    host.store()
        .find_checkpoint(id)
        .cloned()
        .ok_or_else(|| match id {
            Some(id) => format!("No checkpoint `{id}`: harness.checkpoints lists them."),
            None => "No checkpoint yet: harness.checkpoint takes one before a job.".into(),
        })
}

#[cfg(test)]
mod tests {
    #[test]
    fn skills_parse_with_windows_line_endings() {
        let text: &'static str = Box::leak(
            "---\nname: x\ntitle: X\nwhen: always\n---\n## Steps\n"
                .replace('\n', "\r\n")
                .into_boxed_str(),
        );
        let skill = super::parse("x", text);
        assert_eq!((skill.name, skill.title, skill.when), ("x", "X", "always"));
        assert!(skill.body.starts_with("## Steps"));
    }

    use super::*;

    #[test]
    fn the_brief_and_skills_are_complete() {
        let words = brief().split_whitespace().count();
        assert!((800..=1600).contains(&words), "the brief has {words} words");
        assert!((8..=15).contains(&skills().len()));
        for s in skills() {
            assert!(!s.title.is_empty() && !s.when.is_empty(), "{}", s.name);
            assert!(s.body.contains("## Checks"), "{} has no checks", s.name);
            assert!(s.body.contains("## Steps"), "{} has no steps", s.name);
            // Every command a skill names exists.
            for word in s.body.split('`') {
                let name = word.split_whitespace().next().unwrap_or("");
                let looks_like = name.contains('.')
                    && name.split('.').count() == 2
                    && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
                    && name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
                    && !name.ends_with(".md");
                let file = [".toml", ".json", ".rs", ".md", ".wav", ".mid", ".png"]
                    .iter()
                    .any(|x| name.ends_with(x));
                if looks_like && !file {
                    assert!(
                        crate::control::spec(name).is_some(),
                        "skill {} names unknown command {name}",
                        s.name
                    );
                }
            }
        }
        assert!(skill("mixing.md").is_some());
        // The brief names only real commands too.
        for word in BRIEF.split('`') {
            let name = word.split_whitespace().next().unwrap_or("");
            if name.contains('.')
                && name.split('.').count() == 2
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
                && name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
            {
                assert!(crate::control::spec(name).is_some(), "brief names {name}");
            }
        }
    }
}
