//! Eyes and ears: `harness.look` and `harness.measure`.
//!
//! Both render a bar range offline with the same renderer as an export (so plugins, sends,
//! buses and the master chain are all heard), stream it through the loudness meter, the
//! waveform columns and the spectrum, and answer with numbers. `harness.look` also draws a
//! picture: the waveform with the bar grid and the sections, the short-term loudness, the
//! average spectrum and a piano roll of the MIDI notes in the range, as a PNG the model sees
//! (an image block for the built-in agent's vision providers, MCP image content outside).
//! The picture is an SVG rasterised by resvg with IBM Plex Mono built in, so it looks the same
//! in the window, the CLI and the MCP server, with no system fonts.

use super::analysis::{Spectrum, Waveform};
use super::loudness::{Meter, Reading};
use crate::{
    audio::{self, Library},
    control::{Args, Host},
    model::*,
    plugin::MAX_BLOCK,
    render, Result,
};
use base64::Engine;
use serde_json::{json, Value};
use std::{fmt::Write as _, path::PathBuf};

const RATE: u32 = 48000;
/// The longest range one look or measurement renders.
const MAX_SECONDS: f64 = 600.0;
const WIDTH: f64 = 1200.0;
const LEFT: f64 = 64.0;
const RIGHT: f64 = 20.0;
const FONT: &[u8] = include_bytes!("../../../desktop/assets/fonts/IBMPlexMono-Regular.ttf");

/// The bars a look or measurement covers, zero-based, `to` exclusive.
#[derive(Clone, Copy, Debug)]
pub struct Range {
    pub from_bar: f64,
    pub to_bar: f64,
    pub start_beat: f64,
    pub end_beat: f64,
}
impl Range {
    pub(crate) fn of(session: &Session, a: &Args) -> Result<Self> {
        let end = session.end_bar();
        let from_bar = a.opt_f64("fromBar").unwrap_or(0.0);
        let to_bar = a.opt_f64("toBar").unwrap_or(end.max(from_bar + 1.0));
        if !from_bar.is_finite() || from_bar < 0.0 || !to_bar.is_finite() || to_bar <= from_bar {
            return Err(
                "Give a range with 0 <= fromBar < toBar (zero-based bars, toBar exclusive)".into(),
            );
        }
        let bpb = session.beats_per_bar();
        let range = Self {
            from_bar,
            to_bar,
            start_beat: from_bar * bpb,
            end_beat: to_bar * bpb,
        };
        if range.seconds(session) > MAX_SECONDS {
            return Err(format!(
                "That range is {:.0} seconds; look at or measure at most {MAX_SECONDS:.0} seconds at a time (narrow fromBar/toBar).",
                range.seconds(session)
            ));
        }
        Ok(range)
    }
    pub fn seconds(&self, session: &Session) -> f64 {
        session.tempo_map().duration(self.start_beat, self.end_beat)
    }
}

/// What one render of a range yields.
pub struct Heard {
    pub reading: Reading,
    pub spectrum: Spectrum,
    pub waveform: Waveform,
}

/// Render `range` of the song (only `solo` when given, as the mixer's solo would) and listen
/// to it. `tail` seconds after the range are included (reverb and delay tails).
pub fn listen(
    session: &Session,
    library: &Library,
    range: &Range,
    solo: Option<&str>,
    tail: f64,
    columns: usize,
) -> Result<Heard> {
    let mut song = session.clone();
    song.transport.cycle = false;
    song.transport.metronome = false;
    if let Some(id) = solo {
        for track in &mut song.tracks {
            track.solo = track.id == id;
            if track.id == id {
                track.mute = false;
            }
        }
    }
    let bpb = song.beats_per_bar();
    let end = range.end_beat + song.tempo_map().beats_for(range.end_beat, tail);
    song.clips.retain(|clip| clip.start_bar * bpb < end);
    for clip in &mut song.clips {
        clip.length_bars = clip.length_bars.min(end / bpb - clip.start_bar);
    }
    crate::automation::hold_after(&mut song, end);
    let mut library = library.clone();
    audio::prepare_sources(&song, &mut library)?;
    let (mut renderer, mut rack) = render::offline(&song, &library, RATE)?;
    renderer.playing = true;
    renderer.locate(0.0);
    let tempo = song.tempo_map();
    let frames =
        ((tempo.duration(range.start_beat, range.end_beat) + tail) * RATE as f64).ceil() as u64;
    let mut block = [[0.0f32; 2]; MAX_BLOCK];
    let mut skip = (tempo.seconds(range.start_beat) * RATE as f64).round() as u64
        + renderer.latency_samples() as u64;
    while skip > 0 {
        let n = skip.min(MAX_BLOCK as u64) as usize;
        renderer.render(&mut rack, &mut block[..n]);
        rack.idle();
        skip -= n as u64;
    }
    let mut meter = Meter::new(RATE);
    let mut spectrum = Spectrum::new(RATE, frames);
    let mut waveform = Waveform::new(columns, frames);
    let mut done = 0;
    while done < frames {
        let n = (frames - done).min(MAX_BLOCK as u64) as usize;
        renderer.render(&mut rack, &mut block[..n]);
        rack.idle();
        if block[..n].iter().flatten().any(|s| !s.is_finite()) {
            return Err("A plugin produced non-finite audio in this range".into());
        }
        meter.push(&block[..n]);
        spectrum.push(&block[..n]);
        waveform.push(&block[..n]);
        done += n as u64;
    }
    Ok(Heard {
        reading: meter.reading(),
        spectrum,
        waveform,
    })
}

fn num(v: f64) -> Value {
    if v.is_finite() {
        json!((v * 10.0).round() / 10.0)
    } else {
        Value::Null
    }
}

/// The numbers of a reading, with what they mean for the job.
pub fn loudness_json(r: &Reading, target: Option<f64>) -> Value {
    json!({
        "integratedLufs": num(r.integrated),
        "shortTermMaxLufs": num(r.short_term_max),
        "momentaryMaxLufs": num(r.momentary_max),
        "loudnessRangeLu": num(r.range),
        "truePeakDbtp": num(r.true_peak),
        "samplePeakDbfs": num(r.sample_peak),
        "clippedSamples": r.clipped,
        "seconds": num(r.seconds),
        "targetLufs": target,
    })
}

/// Plain-language findings from the numbers: what is wrong and which command fixes it.
pub fn findings(r: &Reading, bands: &[(&str, f64)], target: Option<f64>) -> Vec<String> {
    let mut out = vec![];
    if !r.integrated.is_finite() {
        out.push("The range is silent: check `problems` in session.overview (a mute, a solo elsewhere, an empty clip, a zero fader).".into());
        return out;
    }
    if r.clipped > 0 {
        out.push(format!(
            "{} samples are above 0 dBFS (clipping): lower the loudest tracks or master.setVolume, or put a Limiter last on the master with its ceiling at -1 dB.",
            r.clipped
        ));
    }
    if r.true_peak > -1.0 {
        out.push(format!(
            "True peak {:.1} dBTP is above -1 dBTP, the usual ceiling for streaming and lossy files: lower the master Limiter's Ceiling or the gain into it.",
            r.true_peak
        ));
    }
    if let Some(target) = target {
        let off = r.integrated - target;
        if off.abs() > 1.0 {
            out.push(format!(
                "Integrated {:.1} LUFS is {:.1} LU {} the {target:.1} LUFS target: {} the gain into the master Limiter by about {:.1} dB and measure again.",
                r.integrated,
                off.abs(),
                if off < 0.0 { "under" } else { "over" },
                if off < 0.0 { "raise" } else { "lower" },
                off.abs()
            ));
        }
    }
    if r.integrated < -30.0 {
        out.push(format!(
            "Integrated {:.1} LUFS is very quiet: check faders and instrument levels before mastering.",
            r.integrated
        ));
    }
    let band = |name: &str| {
        bands
            .iter()
            .find(|b| b.0 == name)
            .map_or(f64::NEG_INFINITY, |b| b.1)
    };
    if band("sub") > -3.0 {
        out.push("Sub (20-60 Hz) holds most of the energy: check the kick and bass levels and high-pass what does not need lows.".into());
    }
    if band("air") < -35.0 && band("highMids") < -25.0 {
        out.push("Little energy above 2 kHz: the mix may sound dull (a Channel EQ high shelf, brighter sounds, or hats).".into());
    }
    if r.range > 15.0 {
        out.push(format!(
            "Loudness range {:.1} LU is wide for pop and electronic music (usually 4-10 LU).",
            r.range
        ));
    }
    out
}

fn bands_json(spectrum: &Spectrum) -> (Value, Vec<(&'static str, f64)>) {
    let (bands, centroid) = spectrum.bands();
    let mut map = serde_json::Map::new();
    for (name, db) in &bands {
        map.insert((*name).into(), num(*db));
    }
    (
        json!({ "bandsDb": map, "centroidHz": centroid.map(|c| c.round()) , "note": "Each band's share of the energy from 20 Hz to 20 kHz, in dB (0 dB would be all of it)."}),
        bands,
    )
}

/// Notes of the MIDI clips that sound in the range: (seconds from the range start, seconds
/// long, pitch, velocity, track index).
fn notes_in(session: &Session, range: &Range) -> Vec<(f64, f64, u8, u8, usize)> {
    let tempo = session.tempo_map();
    let bpb = session.beats_per_bar();
    let mut out = vec![];
    for clip in &session.clips {
        let ClipData::Midi { notes, .. } = &clip.data else {
            continue;
        };
        let Some(track) = session.tracks.iter().position(|t| t.id == clip.track_id) else {
            continue;
        };
        let clip_start = clip.start_bar * bpb;
        let clip_end = clip_start + clip.length_bars * bpb;
        for n in notes {
            let start = clip_start + n.start;
            let end = (start + n.length).min(clip_end);
            if start >= clip_end || end <= range.start_beat || start >= range.end_beat {
                continue;
            }
            let (s, e) = (start.max(range.start_beat), end.min(range.end_beat));
            out.push((
                tempo.duration(range.start_beat, s),
                tempo.duration(s, e),
                n.pitch,
                n.velocity,
                track,
            ));
        }
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn colour(track: &Track, index: usize) -> String {
    let c = track.color.trim();
    if c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit()) {
        c.to_string()
    } else {
        crate::control::TRACK_PALETTE[index % crate::control::TRACK_PALETTE.len()].to_string()
    }
}
fn mmss(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

const INK: &str = "#f2f2f2";
const DIM: &str = "#8c8c8c";
const GRID: &str = "#2c2c2c";
const PAGE: &str = "#0d0d0d";
const PANEL: &str = "#151515";
const CLIP: &str = "#ff4040";

struct Picture {
    svg: String,
    y: f64,
}
impl Picture {
    fn text(&mut self, x: f64, y: f64, size: f64, fill: &str, anchor: &str, text: &str) {
        let _ = write!(
            self.svg,
            r#"<text x="{x:.1}" y="{y:.1}" font-size="{size}" fill="{fill}" text-anchor="{anchor}">{}</text>"#,
            escape(text)
        );
    }
    fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, stroke: &str, extra: &str) {
        let _ = write!(
            self.svg,
            r#"<line x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}" stroke="{stroke}" stroke-width="1" {extra}/>"#
        );
    }
    fn panel(&mut self, height: f64, title: &str) -> (f64, f64) {
        let top = self.y;
        let _ = write!(
            self.svg,
            r#"<rect x="{LEFT}" y="{top:.1}" width="{:.1}" height="{height:.1}" fill="{PANEL}"/>"#,
            WIDTH - LEFT - RIGHT
        );
        self.text(8.0, top + 14.0, 11.0, DIM, "start", title);
        self.y += height + 18.0;
        (top, top + height)
    }
}

/// Bar lines with zero-based numbers, and the sections, over a time panel.
fn bar_grid(
    p: &mut Picture,
    session: &Session,
    range: &Range,
    seconds: f64,
    top: f64,
    bottom: f64,
) {
    let tempo = session.tempo_map();
    let bpb = session.beats_per_bar();
    let width = WIDTH - LEFT - RIGHT;
    let bars = (range.to_bar - range.from_bar).max(1.0);
    let every = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0]
        .into_iter()
        .find(|step| width / bars * step >= 34.0)
        .unwrap_or(128.0);
    let mut bar = range.from_bar.ceil();
    while bar < range.to_bar {
        let x = LEFT + tempo.duration(range.start_beat, bar * bpb) / seconds * width;
        let major = (bar / every).fract() == 0.0;
        p.line(x, top, x, bottom, if major { "#3a3a3a" } else { GRID }, "");
        if major {
            p.text(x + 3.0, bottom - 4.0, 10.0, DIM, "start", &format!("{bar}"));
        }
        bar += 1.0;
    }
    for marker in &session.markers {
        if marker.bar >= range.from_bar && marker.bar < range.to_bar {
            let x = LEFT + tempo.duration(range.start_beat, marker.bar * bpb) / seconds * width;
            p.line(x, top, x, bottom, INK, r#"stroke-dasharray="3 3""#);
            p.text(x + 3.0, top + 12.0, 11.0, INK, "start", &marker.name);
        }
    }
}

pub struct Drawn {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Draw the look. `heard` is `None` for a notes-only view (no render).
pub fn draw(
    session: &Session,
    range: &Range,
    heard: Option<&Heard>,
    target: Option<f64>,
    solo: Option<&Track>,
    roll: bool,
) -> Result<Drawn> {
    let seconds = range.seconds(session).max(1e-6);
    let notes = notes_in(session, range);
    let mut p = Picture {
        svg: String::new(),
        y: 52.0,
    };
    let width = WIDTH - LEFT - RIGHT;
    // Header.
    let tempo = session.tempo_map();
    let title = format!(
        "{}{} · bars {}–{} · {}–{} · {:.0} BPM{}",
        session.name,
        solo.map(|t| format!(" · {}", t.name)).unwrap_or_default(),
        range.from_bar,
        range.to_bar,
        mmss(tempo.seconds(range.start_beat)),
        mmss(tempo.seconds(range.end_beat)),
        tempo.bpm(range.start_beat),
        if session.transport.key.trim().is_empty() {
            String::new()
        } else {
            format!(" · {}", session.transport.key)
        }
    );
    p.text(LEFT, 22.0, 15.0, INK, "start", &title);
    match heard {
        Some(h) => {
            let r = &h.reading;
            let fmt = |v: f64, unit: &str| {
                if v.is_finite() {
                    format!("{v:.1} {unit}")
                } else {
                    format!("-inf {unit}")
                }
            };
            p.text(
                LEFT,
                42.0,
                12.0,
                INK,
                "start",
                &format!(
                    "{} integrated · {} short-term max · {} true peak · LRA {}{}",
                    fmt(r.integrated, "LUFS"),
                    fmt(r.short_term_max, "LUFS"),
                    fmt(r.true_peak, "dBTP"),
                    fmt(r.range, "LU"),
                    target
                        .map(|t| format!(" · target {t:.1} LUFS"))
                        .unwrap_or_default()
                ),
            );
            p.text(
                WIDTH - RIGHT,
                42.0,
                12.0,
                if r.clipped > 0 { CLIP } else { DIM },
                "end",
                &if r.clipped > 0 {
                    format!("{} samples clipped", r.clipped)
                } else {
                    "no clipping".into()
                },
            );
            // Waveform.
            let (top, bottom) = p.panel(180.0, "wave");
            let mid = (top + bottom) / 2.0;
            let half = (bottom - top) / 2.0 - 6.0;
            for (gain, label) in [(1.0, "0"), (0.5, "-6")] {
                for sign in [-1.0, 1.0] {
                    let y = mid - sign * gain * half;
                    p.line(LEFT, y, WIDTH - RIGHT, y, GRID, "");
                    if sign > 0.0 {
                        p.text(LEFT - 4.0, y + 4.0, 10.0, DIM, "end", label);
                    }
                }
            }
            bar_grid(&mut p, session, range, seconds, top, bottom);
            let columns = h.waveform.columns();
            let step = width / columns.len() as f64;
            // Columns past the range are the tail: draw only the range.
            let in_range = columns.len() as f64
                * (seconds / (seconds + (h.reading.seconds - seconds).max(0.0)));
            let mut path = String::new();
            let mut clipped = String::new();
            for (i, (lo, hi, clip)) in columns.iter().enumerate() {
                if i as f64 >= in_range {
                    break;
                }
                let x = LEFT + (i as f64 + 0.5) * step * (columns.len() as f64 / in_range.max(1.0));
                let (y1, y2) = (
                    mid - (*hi as f64).clamp(-1.05, 1.05) * half,
                    mid - (*lo as f64).clamp(-1.05, 1.05) * half,
                );
                let target = if *clip { &mut clipped } else { &mut path };
                let _ = write!(target, "M{x:.1} {y1:.1}V{:.1}", y2.max(y1 + 0.6));
            }
            let _ = write!(
                p.svg,
                r#"<path d="{path}" stroke="{INK}" stroke-width="{:.2}" opacity="0.85"/><path d="{clipped}" stroke="{CLIP}" stroke-width="{:.2}"/>"#,
                (step * 0.9).max(0.6),
                (step * 0.9).max(1.0)
            );
            // Short-term loudness.
            let (top, bottom) = p.panel(110.0, "LUFS");
            let (lo, hi) = (-42.0, 0.0);
            let y_of = |v: f64| bottom - (v.clamp(lo, hi) - lo) / (hi - lo) * (bottom - top);
            for v in [-36.0, -24.0, -12.0] {
                let y = y_of(v);
                p.line(LEFT, y, WIDTH - RIGHT, y, GRID, "");
                p.text(LEFT - 4.0, y + 4.0, 10.0, DIM, "end", &format!("{v:.0}"));
            }
            bar_grid(&mut p, session, range, seconds, top, bottom);
            let mut curve = String::new();
            for (i, v) in h.reading.short_term.iter().enumerate() {
                let t = (i + 1) as f64 / 10.0;
                if t > seconds || !v.is_finite() {
                    continue;
                }
                let x = LEFT + t / seconds * width;
                let _ = write!(
                    curve,
                    "{}{x:.1} {:.1}",
                    if curve.is_empty() { "M" } else { "L" },
                    y_of(*v)
                );
            }
            let _ = write!(
                p.svg,
                r#"<path d="{curve}" fill="none" stroke="{INK}" stroke-width="1.6"/>"#
            );
            if r.integrated.is_finite() {
                let y = y_of(r.integrated);
                p.line(LEFT, y, WIDTH - RIGHT, y, DIM, r#"stroke-dasharray="6 4""#);
                p.text(WIDTH - RIGHT - 4.0, y - 4.0, 10.0, DIM, "end", "integrated");
            }
            if let Some(t) = target {
                let y = y_of(t);
                p.line(LEFT, y, WIDTH - RIGHT, y, INK, r#"stroke-dasharray="2 3""#);
                p.text(
                    LEFT + 4.0,
                    y - 4.0,
                    10.0,
                    INK,
                    "start",
                    &format!("target {t:.0}"),
                );
            }
            // Spectrum.
            let (top, bottom) = p.panel(170.0, "spectrum");
            let bins = h.spectrum.bins();
            let x_of = |hz: f64| LEFT + (hz / 20.0).log10() / 3.0 * width;
            let points = 240;
            let mut smooth: Vec<(f64, f64)> = vec![];
            for i in 0..points {
                let (a, b) = (
                    20.0 * 1000f64.powf(i as f64 / points as f64),
                    20.0 * 1000f64.powf((i + 1) as f64 / points as f64),
                );
                let (sum, count) = bins
                    .iter()
                    .filter(|(hz, _)| *hz >= a && *hz < b)
                    .fold((0.0, 0), |(s, c), (_, p)| (s + p, c + 1));
                if count > 0 && sum > 0.0 {
                    smooth.push(((a * b).sqrt(), 10.0 * (sum / count as f64).log10()));
                }
            }
            let peak = smooth.iter().map(|s| s.1).fold(f64::NEG_INFINITY, f64::max);
            let ceiling = if peak.is_finite() {
                (peak / 6.0).ceil() * 6.0
            } else {
                0.0
            };
            let floor = ceiling - 72.0;
            let y_db = |db: f64| {
                top + (ceiling - db.clamp(floor, ceiling)) / (ceiling - floor) * (bottom - top)
            };
            for hz in [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0] {
                let x = x_of(hz);
                p.line(x, top, x, bottom, GRID, "");
                let label = if hz >= 1000.0 {
                    format!("{}k", hz / 1000.0)
                } else {
                    format!("{hz}")
                };
                p.text(x + 3.0, bottom - 4.0, 10.0, DIM, "start", &label);
            }
            for step in 1..4 {
                let db = ceiling - step as f64 * 18.0;
                let y = y_db(db);
                p.line(LEFT, y, WIDTH - RIGHT, y, GRID, "");
                p.text(LEFT - 4.0, y + 4.0, 10.0, DIM, "end", &format!("{db:.0}"));
            }
            let mut curve = String::new();
            for (hz, db) in &smooth {
                let _ = write!(
                    curve,
                    "{}{:.1} {:.1}",
                    if curve.is_empty() { "M" } else { "L" },
                    x_of(*hz),
                    y_db(*db)
                );
            }
            let _ = write!(
                p.svg,
                r#"<path d="{curve}" fill="none" stroke="{INK}" stroke-width="1.6"/>"#
            );
            // Pink noise's slope (-3 dB per octave per bin) through the curve at 1 kHz.
            if let Some((_, at_1k)) = smooth
                .iter()
                .min_by(|a, b| (a.0 - 1000.0).abs().total_cmp(&(b.0 - 1000.0).abs()))
            {
                let db_at = |hz: f64| at_1k - 3.0 * (hz / 1000.0).log2();
                p.line(
                    x_of(20.0),
                    y_db(db_at(20.0)),
                    x_of(20000.0),
                    y_db(db_at(20000.0)),
                    DIM,
                    r#"stroke-dasharray="4 4""#,
                );
                p.text(
                    WIDTH - RIGHT - 4.0,
                    top + 12.0,
                    10.0,
                    DIM,
                    "end",
                    "dashed: pink slope through 1 kHz",
                );
            }
        }
        None => {
            p.text(
                LEFT,
                42.0,
                12.0,
                DIM,
                "start",
                &format!(
                    "{} notes in this range (piano roll only: no render)",
                    notes.len()
                ),
            );
        }
    }
    // Piano roll.
    if !roll {
        return finish(p);
    }
    let roll_height = if heard.is_some() { 230.0 } else { 380.0 };
    let (top, bottom) = p.panel(roll_height, "notes");
    bar_grid(&mut p, session, range, seconds, top, bottom);
    if notes.is_empty() {
        p.text(
            LEFT + width / 2.0,
            (top + bottom) / 2.0,
            12.0,
            DIM,
            "middle",
            "No MIDI notes in this range",
        );
    } else {
        let low = notes
            .iter()
            .map(|n| n.2)
            .min()
            .unwrap_or(48)
            .saturating_sub(1);
        let high = notes
            .iter()
            .map(|n| n.2)
            .max()
            .unwrap_or(72)
            .saturating_add(1)
            .min(127);
        let rows = (high - low + 1) as f64;
        let row = (bottom - top - 16.0) / rows;
        let y_of = |pitch: u8| top + (high - pitch) as f64 * row;
        for pitch in low..=high {
            if pitch % 12 == 0 {
                let y = y_of(pitch) + row;
                p.line(LEFT, y, WIDTH - RIGHT, y, "#333333", "");
                p.text(
                    LEFT - 4.0,
                    y,
                    10.0,
                    DIM,
                    "end",
                    &crate::control_overview::pitch_name(pitch),
                );
            }
        }
        for (start, length, pitch, velocity, track) in &notes {
            let t = &session.tracks[*track];
            let x = LEFT + start / seconds * width;
            let w = (length / seconds * width).max(1.5);
            let opacity = if t.mute {
                0.3
            } else {
                0.45 + 0.55 * *velocity as f64 / 127.0
            };
            let _ = write!(
                p.svg,
                r#"<rect x="{x:.1}" y="{:.1}" width="{w:.1}" height="{:.1}" fill="{}" opacity="{opacity:.2}"/>"#,
                y_of(*pitch) + 0.5,
                (row - 1.0).max(1.0),
                colour(t, *track)
            );
        }
        // Legend: the tracks with notes here, in their colours.
        let mut seen: Vec<usize> = notes.iter().map(|n| n.4).collect();
        seen.sort_unstable();
        seen.dedup();
        let mut x = LEFT + 6.0;
        for index in seen {
            let t = &session.tracks[index];
            let label = format!("{}{}", t.name, if t.mute { " (muted)" } else { "" });
            // A plate behind the label so notes under it do not hide it.
            let _ = write!(
                p.svg,
                r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="16" fill="{PAGE}" opacity="0.8"/>"#,
                x - 3.0,
                top + 1.0,
                18.0 + label.chars().count() as f64 * 6.7
            );
            let _ = write!(
                p.svg,
                r#"<rect x="{x:.1}" y="{:.1}" width="8" height="8" fill="{}"/>"#,
                top + 5.0,
                colour(t, index)
            );
            p.text(
                x + 12.0,
                top + 13.0,
                11.0,
                &colour(t, index),
                "start",
                &label,
            );
            x += 26.0 + label.chars().count() as f64 * 6.7;
        }
    }
    finish(p)
}

fn finish(p: Picture) -> Result<Drawn> {
    let height = p.y.ceil();
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height}" viewBox="0 0 {WIDTH} {height}" font-family="IBM Plex Mono"><rect width="100%" height="100%" fill="{PAGE}"/>{}</svg>"#,
        p.svg
    );
    rasterise(&svg)
}

fn fonts() -> std::sync::Arc<resvg::usvg::fontdb::Database> {
    static DB: std::sync::LazyLock<std::sync::Arc<resvg::usvg::fontdb::Database>> =
        std::sync::LazyLock::new(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_font_data(FONT.to_vec());
            std::sync::Arc::new(db)
        });
    DB.clone()
}

/// SVG text to PNG bytes.
pub fn rasterise(svg: &str) -> Result<Drawn> {
    let options = resvg::usvg::Options {
        font_family: "IBM Plex Mono".into(),
        fontdb: fonts(),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(svg, &options)
        .map_err(|e| format!("Could not draw the picture: {e}"))?;
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or("Could not allocate the picture")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    let png = pixmap
        .encode_png()
        .map_err(|e| format!("Could not encode the picture: {e}"))?;
    Ok(Drawn {
        png,
        width: size.width(),
        height: size.height(),
    })
}

/// Where looks are written when no path is given: `<data dir>/looks`, the newest 30 kept.
fn default_path() -> PathBuf {
    let dir = crate::host::scan::data_dir().join("looks");
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        files.sort();
        let excess = files.len().saturating_sub(29);
        for (_, path) in files.into_iter().take(excess) {
            let _ = std::fs::remove_file(path);
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    dir.join(format!("look-{stamp}.png"))
}

fn solo_track<'a>(session: &'a Session, a: &Args) -> Result<Option<&'a Track>> {
    match a.opt_str("trackId") {
        None => Ok(None),
        Some(id) => session
            .tracks
            .iter()
            .find(|t| t.id == id)
            .map(Some)
            .ok_or_else(|| format!("No track `{id}`")),
    }
}
fn notes_json(session: &Session, range: &Range) -> Value {
    let notes = notes_in(session, range);
    let mut per: Vec<(String, usize)> = vec![];
    for n in &notes {
        let name = &session.tracks[n.4].name;
        match per.iter_mut().find(|(t, _)| t == name) {
            Some(entry) => entry.1 += 1,
            None => per.push((name.clone(), 1)),
        }
    }
    json!({
        "count": notes.len(),
        "lowest": notes.iter().map(|n| n.2).min().map(crate::control_overview::pitch_name),
        "highest": notes.iter().map(|n| n.2).max().map(crate::control_overview::pitch_name),
        "perTrack": per.into_iter().map(|(t, n)| json!({"track": t, "notes": n})).collect::<Vec<_>>(),
    })
}

/// `harness.look`.
pub(crate) fn look(host: &mut dyn Host, a: &Args) -> Result<Value> {
    let session = host.store().session().clone();
    let range = Range::of(&session, a)?;
    let view = a.opt_str("view").unwrap_or("all");
    if !matches!(view, "all" | "mix" | "notes") {
        return Err("view is all (default), mix or notes".into());
    }
    let target = a.opt_f64("targetLufs");
    let solo = solo_track(&session, a)?;
    let path = match a.opt_str("path") {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => default_path(),
    };
    if path
        .extension()
        .is_none_or(|e| !e.eq_ignore_ascii_case("png"))
    {
        return Err("A look is written as .png".into());
    }
    crate::control::protect_session_file(host.path(), &path)?;
    let heard = if view == "notes" {
        None
    } else {
        Some(listen(
            &session,
            host.library(),
            &range,
            solo.map(|t| t.id.as_str()),
            a.opt_f64("tailSeconds").unwrap_or(0.0).clamp(0.0, 30.0),
            ((WIDTH - LEFT - RIGHT) as usize) / 2,
        )?)
    };
    let drawn = draw(
        &session,
        &range,
        heard.as_ref(),
        target,
        solo,
        view != "mix",
    )?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    crate::document::atomic_write(&path, |f| {
        use std::io::Write;
        f.write_all(&drawn.png).map_err(|e| e.to_string())
    })?;
    let mut out = json!({
        "range": {"fromBar": range.from_bar, "toBar": range.to_bar, "seconds": num(range.seconds(&session))},
        "track": solo.map(|t| t.name.clone()),
        "view": view,
        "notes": notes_json(&session, &range),
        "image": {
            "path": path,
            "mimeType": "image/png",
            "width": drawn.width,
            "height": drawn.height,
            "data": base64::engine::general_purpose::STANDARD.encode(&drawn.png),
        },
    });
    if let Some(h) = &heard {
        let (spectrum, bands) = bands_json(&h.spectrum);
        out["loudness"] = loudness_json(&h.reading, target);
        out["spectrum"] = spectrum;
        out["findings"] = json!(findings(&h.reading, &bands, target));
    }
    Ok(out)
}

/// `harness.measure`.
pub(crate) fn measure(host: &mut dyn Host, a: &Args) -> Result<Value> {
    let session = host.store().session().clone();
    let range = Range::of(&session, a)?;
    let target = a.opt_f64("targetLufs");
    let solo = solo_track(&session, a)?;
    let tail = a.opt_f64("tailSeconds").unwrap_or(0.0).clamp(0.0, 30.0);
    let heard = listen(
        &session,
        host.library(),
        &range,
        solo.map(|t| t.id.as_str()),
        tail,
        1,
    )?;
    let (spectrum, bands) = bands_json(&heard.spectrum);
    let mut out = json!({
        "range": {"fromBar": range.from_bar, "toBar": range.to_bar, "seconds": num(range.seconds(&session))},
        "track": solo.map(|t| t.name.clone()),
        "loudness": loudness_json(&heard.reading, target),
        "spectrum": spectrum,
        "findings": findings(&heard.reading, &bands, target),
    });
    if a.opt_bool("tracks").unwrap_or(false) && solo.is_none() {
        if range.seconds(&session) > 120.0 {
            return Err(
                "tracks=true measures every track on its own: keep the range under 120 seconds"
                    .into(),
            );
        }
        let mut rows = vec![];
        for t in session.tracks.iter().filter(|t| t.kind != "bus") {
            let h = listen(&session, host.library(), &range, Some(&t.id), tail, 1)?;
            let r = &h.reading;
            rows.push(json!({
                "track": t.name, "trackId": t.id,
                "integratedLufs": num(r.integrated), "truePeakDbtp": num(r.true_peak),
                "samplePeakDbfs": num(r.sample_peak), "clippedSamples": r.clipped,
                "fader": crate::control_overview::fader_db(t.volume),
                "muted": t.mute,
            }));
        }
        out["tracks"] = json!(rows);
    }
    Ok(out)
}
