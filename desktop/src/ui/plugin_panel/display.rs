//! A stock plugin's display: what the plugin does to a signal, drawn from its parameters
//! (an EQ curve, a compressor's transfer, an LFO, echo taps, an envelope). The picture is
//! built as marks in a fixed 560 × 148 box, then painted onto a canvas stretched to the
//! display, like the React face's SVG (`PluginDisplay.tsx`).

use super::response::*;
use crate::ui::theme::{with_alpha, Theme, FONT_MONO};
use gpui::{
    linear_color_stop, linear_gradient, point, px, App, Bounds, Hsla, PathBuilder, Pixels, Point,
    Window,
};
use std::f64::consts::PI;

pub const W: f64 = 560.0;
pub const H: f64 = 148.0;

/// How a line is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rule {
    /// The graticule.
    Grid,
    /// 0 dB, the centre line.
    Zero,
    /// Input = output, dashed.
    Unity,
    /// What the plugin could reach, dashed and faint.
    Ghost,
    /// A threshold or ceiling, dashed in the family colour.
    Marker,
}

/// One thing drawn in the display, in box units (x right, y down).
#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Line(Rule, (f64, f64), (f64, f64)),
    /// A dashed polyline (the dry signal, the reach of an LFO).
    Ghost(Vec<(f64, f64)>),
    /// The trace, with its glow; filled down (or up) to `fill_to` when given.
    Curve {
        points: Vec<(f64, f64)>,
        fill_to: Option<f64>,
    },
    /// A band or corner frequency.
    Handle(f64, f64),
    Text {
        at: (f64, f64),
        text: String,
        end: bool,
        marker: bool,
    },
    /// An echo tap; `dry` is the direct sound.
    Bar {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        dry: bool,
    },
    /// The stereo field.
    Field {
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
    },
}

/// Parameter values by name, with the face's fallbacks.
pub trait Values {
    fn v(&self, name: &str, fallback: f64) -> f64;
    fn has(&self, name: &str) -> bool;
}

const FREQ_MARKS: [f64; 8] = [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0];

fn hz_label(hz: f64) -> String {
    if hz >= 1000.0 {
        format!("{}k", hz / 1000.0)
    } else {
        format!("{hz}")
    }
}

fn text(at: (f64, f64), text: impl Into<String>) -> Mark {
    Mark::Text {
        at,
        text: text.into(),
        end: false,
        marker: false,
    }
}
fn text_end(at: (f64, f64), text: impl Into<String>) -> Mark {
    Mark::Text {
        at,
        text: text.into(),
        end: true,
        marker: false,
    }
}
fn hline(rule: Rule, y: f64) -> Mark {
    Mark::Line(rule, (0.0, y), (W, y))
}
fn vline(rule: Rule, x: f64) -> Mark {
    Mark::Line(rule, (x, 0.0), (x, H))
}

/// Log-frequency graticule with dB rules from `bottom` to `top`.
fn frequency_grid(marks: &mut Vec<Mark>, top: f64, bottom: f64) {
    for hz in FREQ_MARKS {
        let x = hz_to_x(hz) * W;
        marks.push(vline(Rule::Grid, x));
        marks.push(text((x + 3.0, H - 4.0), hz_label(hz)));
    }
    let step = if top - bottom > 40.0 { 12.0 } else { 6.0 };
    let mut db = (bottom / step).ceil() * step;
    while db <= top {
        let y = (top - db) / (top - bottom) * H;
        marks.push(hline(if db == 0.0 { Rule::Zero } else { Rule::Grid }, y));
        if db != bottom && db != top {
            marks.push(text(
                (4.0, y - 3.0),
                if db > 0.0 {
                    format!("+{db}")
                } else {
                    format!("{db}")
                },
            ));
        }
        db += step;
    }
}

/// Square dB-in / dB-out graticule for the dynamics plugins.
fn level_grid(marks: &mut Vec<Mark>, floor: f64) {
    let mut db = floor + 12.0;
    while db < 0.0 {
        let t = (db - floor) / -floor;
        marks.push(vline(Rule::Grid, t * W));
        marks.push(hline(Rule::Grid, (1.0 - t) * H));
        marks.push(text((t * W + 3.0, H - 4.0), format!("{db}")));
        db += 12.0;
    }
    marks.push(Mark::Line(Rule::Unity, (0.0, H), (W, 0.0)));
}

/// Sample `f` (0..1 up) over the box.
fn plot_box(f: impl Fn(f64) -> f64, samples: usize) -> Vec<(f64, f64)> {
    plot(f, samples)
        .into_iter()
        .map(|(x, y)| (x * W, (1.0 - y) * H))
        .collect()
}

fn frequency_response(db: impl Fn(f64) -> f64, top: f64, bottom: f64) -> Vec<(f64, f64)> {
    plot_box(|x| (db(x_to_hz(x)) - bottom) / (top - bottom), 140)
}

fn level_curve(f: impl Fn(f64) -> f64, floor: f64) -> Vec<(f64, f64)> {
    plot_box(|x| (f(floor + x * -floor) - floor) / -floor, 120)
}

fn curve(points: Vec<(f64, f64)>, fill_to: Option<f64>) -> Mark {
    Mark::Curve { points, fill_to }
}

fn marker(marks: &mut Vec<Mark>, x: f64, label: String) {
    marks.push(vline(Rule::Marker, x));
    let near_end = x > W - 60.0;
    marks.push(Mark::Text {
        at: (
            if near_end {
                x - 8.0
            } else {
                (W - 4.0).min(x + 4.0)
            },
            12.0,
        ),
        text: label,
        end: near_end,
        marker: true,
    });
}

/// The picture for one stock plugin, or `None` when it has nothing to show. Instruments
/// with an amplitude envelope show it.
pub fn picture(name: &str, v: &dyn Values) -> Option<Vec<Mark>> {
    display_for(name, v).or_else(|| envelope(v))
}

fn display_for(name: &str, v: &dyn Values) -> Option<Vec<Mark>> {
    let mut m = vec![];
    match name {
        "Channel EQ" => {
            let bands = EqBands {
                low_gain: v.v("Low Gain", 0.0),
                low_freq: v.v("Low Freq", 120.0),
                mid_gain: v.v("Mid Gain", 0.0),
                mid_freq: v.v("Mid Freq", 1000.0),
                mid_q: v.v("Mid Q", 0.8),
                high_gain: v.v("High Gain", 0.0),
                high_freq: v.v("High Freq", 6000.0),
            };
            let y = |db: f64| (18.0 - db) / 36.0 * H;
            frequency_grid(&mut m, 18.0, -18.0);
            m.push(curve(
                frequency_response(|hz| eq_db(&bands, hz), 18.0, -18.0),
                Some(y(0.0)),
            ));
            for hz in [bands.low_freq, bands.mid_freq, bands.high_freq] {
                m.push(Mark::Handle(hz_to_x(hz) * W, y(eq_db(&bands, hz))));
            }
        }
        "Filter" => {
            let cutoff = v.v("Cutoff", 1000.0);
            let (kind, resonance) = (v.v("Type", 0.0), v.v("Resonance", 0.0));
            let db = |hz: f64| filter_db(kind, cutoff, resonance, hz);
            frequency_grid(&mut m, 24.0, -48.0);
            m.push(curve(frequency_response(db, 24.0, -48.0), Some(H)));
            m.push(Mark::Handle(
                hz_to_x(cutoff) * W,
                (24.0 - db(cutoff)) / 72.0 * H,
            ));
        }
        "Tape Sat" | "Overdrive" => {
            let hard = name == "Overdrive";
            let drive = v.v("Drive", 0.0);
            let tone = v.v("Tone", 8000.0);
            m.push(hline(Rule::Zero, H / 2.0));
            m.push(vline(Rule::Zero, W / 2.0));
            m.push(Mark::Line(Rule::Unity, (0.0, H), (W, 0.0)));
            m.push(curve(
                plot_box(|x| 0.5 + 0.46 * saturate(x * 2.0 - 1.0, drive, hard), 120),
                None,
            ));
            m.push(text_end(
                (W - 6.0, H - 6.0),
                format!(
                    "tone {} Hz · {:.1} dB @ 10k",
                    hz_label((tone / 100.0).round() * 100.0),
                    tone_db(tone, 10_000.0)
                ),
            ));
        }
        "Bitcrusher" => {
            let (bits, down) = (v.v("Bits", 8.0), v.v("Downsample", 4.0));
            m.push(hline(Rule::Zero, H / 2.0));
            m.push(Mark::Ghost(plot_box(
                |x| 0.5 + 0.44 * (x * 4.0 * PI).sin(),
                120,
            )));
            m.push(curve(
                plot_box(|x| 0.5 + 0.44 * crush(x, bits, down), 384),
                None,
            ));
        }
        "ryolune Comp" => {
            let t = v.v("Threshold", -18.0);
            let (ratio, makeup) = (v.v("Ratio", 4.0), v.v("Makeup", 0.0));
            level_grid(&mut m, -60.0);
            m.push(curve(
                level_curve(|i| compress_db(i, t, ratio, makeup), -60.0),
                Some(H),
            ));
            marker(&mut m, (t + 60.0) / 60.0 * W, format!("{t:.1} dB"));
        }
        "Gate" => {
            let t = v.v("Threshold", -40.0);
            let range = v.v("Range", -80.0);
            level_grid(&mut m, -80.0);
            m.push(curve(level_curve(|i| gate_db(i, t, range), -80.0), Some(H)));
            marker(&mut m, (t + 80.0) / 80.0 * W, format!("{t:.1} dB"));
        }
        "Limiter" => {
            let c = v.v("Ceiling", -0.3);
            let input = v.v("Input", 0.0);
            level_grid(&mut m, -36.0);
            m.push(curve(
                level_curve(|i| limit_db(i, input, c), -36.0),
                Some(H),
            ));
            m.push(hline(Rule::Marker, -c / 36.0 * H));
        }
        "Auto Filter" => {
            let cutoff = v.v("Cutoff", 600.0);
            let resonance = v.v("Resonance", 45.0);
            // The sweep the LFO and the envelope can reach, as ghosts either side.
            let reach =
                v.v("LFO Depth", 40.0) / 100.0 * 3.0 + v.v("Envelope", 40.0).abs() / 100.0 * 2.0;
            let at = |octaves: f64| {
                move |hz: f64| {
                    filter_db(
                        0.0,
                        (cutoff * 2f64.powf(octaves)).clamp(30.0, 18_000.0),
                        resonance,
                        hz,
                    )
                }
            };
            frequency_grid(&mut m, 24.0, -48.0);
            if reach > 0.05 {
                m.push(Mark::Ghost(frequency_response(at(-reach), 24.0, -48.0)));
                m.push(Mark::Ghost(frequency_response(at(reach), 24.0, -48.0)));
            }
            m.push(curve(frequency_response(at(0.0), 24.0, -48.0), Some(H)));
            m.push(Mark::Handle(
                hz_to_x(cutoff) * W,
                (24.0 - at(0.0)(cutoff)) / 72.0 * H,
            ));
        }
        "De-Esser" => {
            let frequency = v.v("Frequency", 6500.0);
            let range = v.v("Range", 9.0);
            // The most it will take away: a high shelf of `range` dB above the split.
            let db = |hz: f64| {
                let f = (hz / (frequency * 0.8)).powi(2);
                -range * (f / (1.0 + f))
            };
            frequency_grid(&mut m, 6.0, -24.0);
            m.push(curve(
                frequency_response(db, 6.0, -24.0),
                Some(6.0 / 30.0 * H),
            ));
            m.push(Mark::Handle(
                hz_to_x(frequency) * W,
                (6.0 - db(frequency)) / 30.0 * H,
            ));
        }
        "Lo-Fi" => {
            let tone = v.v("Tone", 5200.0);
            let db = |hz: f64| 2.0 * tone_db(tone, hz);
            frequency_grid(&mut m, 12.0, -36.0);
            m.push(curve(frequency_response(db, 12.0, -36.0), Some(H)));
            m.push(Mark::Handle(
                hz_to_x(tone) * W,
                (12.0 - db(tone)) / 48.0 * H,
            ));
        }
        "Pitch Shift" => {
            let shift = v.v("Semitones", 7.0) + v.v("Fine", 0.0) / 100.0;
            // A 220 Hz note and where it lands, on the same log axis as the filters.
            let source = hz_to_x(220.0) * W;
            let target = hz_to_x(220.0 * 2f64.powf(shift / 12.0)) * W;
            frequency_grid(&mut m, 6.0, -6.0);
            m.push(Mark::Line(Rule::Ghost, (source, H * 0.2), (source, H)));
            m.push(Mark::Line(Rule::Marker, (target, H * 0.2), (target, H)));
            let digits = if shift.fract() != 0.0 { 2 } else { 0 };
            m.push(Mark::Text {
                at: (target + 6.0, H * 0.2 + 10.0),
                text: format!(
                    "{}{:.*} st",
                    if shift > 0.0 { "+" } else { "" },
                    digits,
                    shift
                ),
                end: false,
                marker: true,
            });
        }
        "Pump" => {
            let depth = v.v("Depth", 70.0) / 100.0;
            let recovery = (v.v("Recovery", 45.0) / 100.0).max(0.05);
            let offset = v.v("Offset", 0.0) / 100.0;
            let gain = |x: f64| {
                let p = (x * 4.0 - offset).rem_euclid(1.0);
                let t = (p / recovery).min(1.0);
                1.0 - depth * (1.0 - t * t * (3.0 - 2.0 * t))
            };
            for beat in 1..=3 {
                m.push(vline(Rule::Grid, beat as f64 / 4.0 * W));
            }
            m.push(curve(plot_box(|x| 0.06 + 0.88 * gain(x), 400), Some(H)));
            m.push(text_end((W - 6.0, H - 6.0), "4 pulses"));
        }
        "Chorus" | "Phaser" | "Flanger" | "Auto Pan" | "Tremolo" => {
            let depth = v.v("Depth", 50.0) / 100.0;
            // Two seconds of the modulator; the right channel is offset in stereo.
            let cycles = (v.v("Rate", 1.0) * 2.0).clamp(0.25, 12.0);
            let shape = if name == "Tremolo" || name == "Auto Pan" {
                v.v("Shape", 0.0)
            } else {
                0.0
            };
            let offset = match name {
                "Tremolo" => v.v("Stereo", 0.0) / 360.0,
                "Chorus" => v.v("Spread", 50.0) / 100.0 * 0.25,
                "Auto Pan" => 0.5,
                _ => 0.0,
            };
            let wave =
                |phase: f64| move |x: f64| 0.5 + 0.44 * depth * lfo(shape, x * cycles + phase);
            m.push(hline(Rule::Zero, H / 2.0));
            if offset > 0.0 {
                m.push(Mark::Ghost(plot_box(wave(offset), 240)));
            }
            m.push(curve(plot_box(wave(0.0), 240), None));
            m.push(text_end((W - 6.0, H - 6.0), "2 s"));
        }
        "Echo" => {
            let feedback = v.v("Feedback", 35.0) / 100.0;
            let time = v.v("Time", 375.0);
            let ping = v.v("Ping-pong", 0.0) >= 0.5;
            m.push(hline(Rule::Zero, H / 2.0));
            for i in 0..14 {
                let level = if i == 0 { 1.0 } else { feedback.powi(i) };
                let x = 14.0 + (i as f64 * time * (W - 28.0)) / (time * 6.0).max(2400.0);
                if level < 0.015 || x > W - 8.0 {
                    break;
                }
                let up = !ping || i % 2 == 0;
                let h = level * (H / 2.0 - 12.0);
                m.push(Mark::Bar {
                    x,
                    y: if up { H / 2.0 - h } else { H / 2.0 },
                    w: 5.0,
                    h,
                    dry: i == 0,
                });
            }
            if ping {
                m.push(text((6.0, 14.0), "L"));
                m.push(text((6.0, H - 6.0), "R"));
            }
        }
        "Space" => {
            let pre = v.v("Pre-delay", 10.0) / 100.0;
            let decay = 0.6 + v.v("Size", 55.0) / 100.0 * 5.0;
            let damp = 1.0 + v.v("Damp", 40.0) / 100.0 * 1.6;
            let start = 0.04 + pre * 0.2;
            m.push(hline(Rule::Zero, H - 1.0));
            m.push(Mark::Bar {
                x: 10.0,
                y: 10.0,
                w: 4.0,
                h: H - 11.0,
                dry: true,
            });
            m.push(curve(
                plot_box(
                    |x| {
                        if x < start {
                            0.0
                        } else {
                            0.86 * (-(x - start) * 6.0 * damp / decay).exp()
                        }
                    },
                    160,
                ),
                Some(H),
            ));
            m.push(text_end((W - 6.0, 14.0), format!("≈ {decay:.1} s")));
        }
        "Stereo Width" | "Utility" => {
            let width = if name == "Stereo Width" {
                v.v("Width", 100.0) / 100.0
            } else if v.v("Mono", 0.0) >= 0.5 {
                0.0
            } else {
                1.0
            };
            let pan = if name == "Utility" {
                v.v("Pan", 0.0) / 100.0
            } else {
                0.0
            };
            let gain = if name == "Utility" {
                10f64.powf(v.v("Gain", 0.0).min(12.0) / 40.0)
            } else {
                1.0
            };
            m.push(vline(Rule::Zero, W / 2.0));
            m.push(hline(Rule::Grid, H / 2.0));
            m.push(Mark::Field {
                cx: W / 2.0 + pan * (W / 2.0 - 80.0),
                cy: H / 2.0,
                rx: (width * 110.0).max(1.5),
                ry: (H / 2.0 - 8.0).min((H / 2.0 - 22.0) * gain),
            });
            m.push(text((8.0, H / 2.0 - 5.0), "L"));
            m.push(text_end((W - 8.0, H / 2.0 - 5.0), "R"));
        }
        "Transient" => {
            let attack = v.v("Attack", 0.0) / 100.0;
            let sustain = v.v("Sustain", 0.0) / 100.0;
            let shape = |a: f64, s: f64| {
                move |x: f64| {
                    let hit = (-x * 22.0).exp() * (0.62 + 0.36 * a);
                    let body = (-x * (3.2 - 2.0 * s)).exp() * 0.42 * (1.0 - (-x * 40.0).exp());
                    ((hit + body) * (x * 90.0).min(1.0)).min(0.98)
                }
            };
            m.push(hline(Rule::Zero, H - 1.0));
            m.push(Mark::Ghost(plot_box(shape(0.0, 0.0), 200)));
            m.push(curve(plot_box(shape(attack, sustain), 200), Some(H)));
        }
        _ => return None,
    }
    Some(m)
}

/// Instruments show their amplitude envelope.
fn envelope(v: &dyn Values) -> Option<Vec<Mark>> {
    if !v.has("Attack") && !v.has("Release") {
        return None;
    }
    let points = envelope_points(
        Adsr {
            attack: v.v("Attack", 5.0),
            decay: if v.has("Decay") {
                v.v("Decay", 200.0)
            } else {
                1.0
            },
            sustain: if v.has("Sustain") {
                v.v("Sustain", 100.0) / 100.0
            } else {
                1.0
            },
            release: v.v("Release", 300.0),
        },
        0.22,
    );
    let at = |(x, y): (f64, f64)| (8.0 + x * (W - 16.0), H - 8.0 - y * (H - 24.0));
    let labels = [
        "A",
        if v.has("Decay") { "D" } else { "" },
        if v.has("Sustain") { "S" } else { "" },
        "R",
    ];
    let mut m = vec![
        hline(Rule::Zero, H - 8.0),
        curve(points.iter().map(|p| at(*p)).collect(), Some(H - 8.0)),
    ];
    for (i, p) in points.iter().enumerate().skip(1) {
        let (x, y) = at(*p);
        if i < 4 {
            m.push(Mark::Handle(x, y));
        }
        if !labels[i - 1].is_empty() {
            let mid = 8.0 + (points[i - 1].0 + p.0) / 2.0 * (W - 16.0);
            m.push(Mark::Text {
                at: (mid - 3.0, 14.0),
                text: labels[i - 1].into(),
                end: false,
                marker: false,
            });
        }
    }
    Some(m)
}

/// Colours of one display: the family's (or the accent) over the theme's display inks.
#[derive(Clone, Copy)]
pub struct Inks {
    pub trace: Hsla,
    pub well: Hsla,
    pub grid: Hsla,
    pub zero: Hsla,
    pub faint: Hsla,
    pub dry: Hsla,
}

impl Inks {
    pub fn new(theme: &Theme, _family: Option<Hsla>) -> Self {
        Self {
            // The trace is ink in v2, whatever the plugin's family.
            trace: theme.accent,
            well: theme.display,
            grid: theme.display_grid,
            zero: theme.display_zero,
            faint: theme.display_ink,
            dry: theme.text_display,
        }
    }
}

/// Paint the marks over `bounds`, the box stretched to fit.
pub fn paint(
    marks: &[Mark],
    bounds: Bounds<Pixels>,
    inks: Inks,
    window: &mut Window,
    cx: &mut App,
) {
    let sx = f64::from(f32::from(bounds.size.width)) / W;
    let sy = f64::from(f32::from(bounds.size.height)) / H;
    let at = |(x, y): (f64, f64)| -> Point<Pixels> {
        point(
            bounds.origin.x + px((x * sx) as f32),
            bounds.origin.y + px((y * sy) as f32),
        )
    };
    for mark in marks {
        match mark {
            Mark::Line(rule, a, b) => {
                let (color, dash) = match rule {
                    Rule::Grid => (inks.grid, None),
                    Rule::Zero => (inks.zero, None),
                    Rule::Unity => (inks.zero, Some([3.0, 4.0])),
                    Rule::Ghost => (with_alpha(inks.faint, 0.7), Some([2.0, 3.0])),
                    Rule::Marker => (with_alpha(inks.trace, 0.8), Some([2.0, 3.0])),
                };
                stroke(window, &[at(*a), at(*b)], 1.0, color, dash);
            }
            Mark::Ghost(points) => {
                let points: Vec<_> = points.iter().map(|p| at(*p)).collect();
                stroke(
                    window,
                    &points,
                    1.0,
                    with_alpha(inks.faint, 0.7),
                    Some([2.0, 3.0]),
                );
            }
            Mark::Curve { points, fill_to } => {
                let pts: Vec<_> = points.iter().map(|p| at(*p)).collect();
                if let (Some(to), Some(first), Some(last)) =
                    (fill_to, points.first(), points.last())
                {
                    let mut path = PathBuilder::fill();
                    path.move_to(at((first.0, *to)));
                    for p in &pts {
                        path.line_to(*p);
                    }
                    path.line_to(at((last.0, *to)));
                    path.close();
                    if let Ok(path) = path.build() {
                        window.paint_path(
                            path,
                            linear_gradient(
                                180.0,
                                linear_color_stop(with_alpha(inks.trace, 0.32), 0.0),
                                linear_color_stop(with_alpha(inks.trace, 0.02), 1.0),
                            ),
                        );
                    }
                }
                // A soft halo under the trace, as on a phosphor screen.
                stroke(window, &pts, 6.0, with_alpha(inks.trace, 0.18), None);
                stroke(window, &pts, 1.75, inks.trace, None);
            }
            Mark::Handle(x, y) => {
                let c = at((*x, *y));
                let r = px(4.5);
                window.paint_quad(
                    gpui::fill(
                        Bounds::centered_at(c, gpui::size(r * 2.0, r * 2.0)),
                        inks.well,
                    )
                    .border_widths(px(1.5))
                    .border_color(inks.trace),
                );
            }
            Mark::Bar { x, y, w, h, dry } => {
                let origin = at((*x, *y));
                let size = gpui::size(px((*w * sx) as f32), px((*h * sy) as f32));
                let color = if *dry {
                    inks.dry
                } else {
                    with_alpha(inks.trace, 0.85)
                };
                window.paint_quad(gpui::fill(Bounds::new(origin, size), color));
            }
            Mark::Field {
                cx: x,
                cy: y,
                rx,
                ry,
            } => {
                let points: Vec<_> = (0..=64)
                    .map(|i| {
                        let a = i as f64 / 64.0 * 2.0 * PI;
                        at((x + rx * a.cos(), y + ry * a.sin()))
                    })
                    .collect();
                let mut path = PathBuilder::fill();
                path.move_to(points[0]);
                for p in &points[1..] {
                    path.line_to(*p);
                }
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, with_alpha(inks.trace, 0.22));
                }
                stroke(window, &points, 1.5, inks.trace, None);
            }
            Mark::Text {
                at: p,
                text,
                end,
                marker,
            } => {
                let color = if *marker { inks.trace } else { inks.faint };
                label(window, cx, text, at(*p), *end, color);
            }
        }
    }
}

/// A polyline, solid or dashed.
pub fn stroke(
    window: &mut Window,
    points: &[Point<Pixels>],
    width: f32,
    color: Hsla,
    dash: Option<[f32; 2]>,
) {
    if points.len() < 2 {
        return;
    }
    let mut path = PathBuilder::stroke(px(width));
    if let Some([on, off]) = dash {
        path = path.dash_array(&[px(on), px(off)]);
    }
    path.move_to(points[0]);
    for p in &points[1..] {
        path.line_to(*p);
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// A small mono label with its baseline at `at` (its end there when `end`).
pub fn label(
    window: &mut Window,
    cx: &mut App,
    text: &str,
    at: Point<Pixels>,
    end: bool,
    color: Hsla,
) {
    let size = px(9.0);
    let mut font = window.text_style().font();
    font.family = FONT_MONO.into();
    let run = gpui::TextRun {
        len: text.len(),
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(text.to_string().into(), size, &[run], None);
    let x = if end { at.x - line.width } else { at.x };
    let line_height = px(11.0);
    let _ = line.paint(point(x, at.y - px(9.0)), line_height, window, cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Map(HashMap<&'static str, f64>);
    impl Values for Map {
        fn v(&self, name: &str, fallback: f64) -> f64 {
            self.0.get(name).copied().unwrap_or(fallback)
        }
        fn has(&self, name: &str) -> bool {
            self.0.contains_key(name)
        }
    }

    #[test]
    fn every_stock_effect_with_a_display_draws_a_trace_inside_the_box() {
        let none = Map(HashMap::new());
        for name in [
            "Channel EQ",
            "Filter",
            "Tape Sat",
            "Overdrive",
            "Bitcrusher",
            "ryolune Comp",
            "Gate",
            "Limiter",
            "Auto Filter",
            "De-Esser",
            "Lo-Fi",
            "Pump",
            "Chorus",
            "Phaser",
            "Flanger",
            "Auto Pan",
            "Tremolo",
            "Space",
            "Transient",
        ] {
            let marks = picture(name, &none).unwrap_or_else(|| panic!("{name} has a display"));
            let curves: Vec<_> = marks
                .iter()
                .filter_map(|m| match m {
                    Mark::Curve { points, .. } => Some(points),
                    _ => None,
                })
                .collect();
            assert!(!curves.is_empty(), "{name} draws a trace");
            for (x, y) in curves.iter().flat_map(|c| c.iter()) {
                assert!(
                    (0.0..=W).contains(x) && (0.0..=H).contains(y),
                    "{name}: {x},{y}"
                );
            }
        }
        for name in ["Echo", "Stereo Width", "Utility", "Pitch Shift"] {
            assert!(picture(name, &none).is_some(), "{name}");
        }
        assert!(picture("Some Other Plugin", &none).is_none());
    }

    #[test]
    fn the_eq_handles_sit_on_the_curve_and_instruments_show_their_envelope() {
        let eq = Map(HashMap::from([("Mid Gain", 12.0), ("Mid Freq", 1000.0)]));
        let marks = picture("Channel EQ", &eq).unwrap();
        let handles: Vec<_> = marks
            .iter()
            .filter_map(|m| match m {
                Mark::Handle(x, y) => Some((*x, *y)),
                _ => None,
            })
            .collect();
        assert_eq!(handles.len(), 3);
        // +12 dB of 18 above the centre line.
        assert!((handles[1].1 - (6.0 / 36.0 * H)).abs() < 1.0);
        let synth = Map(HashMap::from([
            ("Attack", 5.0),
            ("Release", 300.0),
            ("Decay", 200.0),
            ("Sustain", 60.0),
        ]));
        let env = picture("ryolune Synth", &synth).unwrap();
        assert_eq!(
            env.iter().filter(|m| matches!(m, Mark::Handle(..))).count(),
            3
        );
        let texts: Vec<_> = env
            .iter()
            .filter_map(|m| match m {
                Mark::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["A", "D", "S", "R"]);
    }
}
