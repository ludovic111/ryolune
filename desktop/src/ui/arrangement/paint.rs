//! Painting the arrangement's canvases: the lanes with their clips, the ruler with the cycle,
//! markers and the playhead, and the tempo track. Everything is placed through
//! [`super::geometry`], the same numbers the view hit-tests with.

use super::{
    geometry::{self as geo_, Envelope, Geo, TempoDrag},
    gestures::{LaneOverlay, RulerOverlay},
};
use crate::ui::{
    format,
    theme::{arrange, Theme, Timeline, FONT_MONO, FONT_UI},
};
use gpui::{
    linear_color_stop, linear_gradient, point, px, size, App, BorderStyle, Bounds, BoxShadow,
    ContentMask, Corners, FontWeight, Hsla, PathBuilder, Pixels, Point, Rgba, TextRun, Window,
};
use ryolune_engine::{
    audio::AudioBuffer,
    model::{Clip, ClipData, Session, Track},
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

/// Local coordinates inside a canvas: `x`, `y` in pixels from its top-left corner.
#[derive(Clone, Copy)]
pub struct Pen {
    origin: Point<Pixels>,
}

impl Pen {
    pub fn new(bounds: Bounds<Pixels>) -> Self {
        Self {
            origin: bounds.origin,
        }
    }
    pub fn at(&self, x: f64, y: f64) -> Point<Pixels> {
        point(self.origin.x + px(x as f32), self.origin.y + px(y as f32))
    }
    pub fn rect(&self, x: f64, y: f64, w: f64, h: f64) -> Bounds<Pixels> {
        Bounds::new(
            self.at(x, y),
            size(px(w.max(0.0) as f32), px(h.max(0.0) as f32)),
        )
    }
}

pub fn fill(window: &mut Window, bounds: Bounds<Pixels>, color: Hsla) {
    window.paint_quad(gpui::fill(bounds, color));
}
/// A filled box. v2 corners are square, so the radius the callers pass is not drawn; it stays
/// in the signature as the step they asked for.
pub fn fill_round(window: &mut Window, bounds: Bounds<Pixels>, _radius: f32, color: Hsla) {
    window.paint_quad(gpui::fill(bounds, color));
}
/// An outlined box (square, like [`fill_round`]).
pub fn stroke_round(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    _radius: f32,
    width: f32,
    color: Hsla,
) {
    window.paint_quad(gpui::outline(bounds, color, BorderStyle::Solid).border_widths(px(width)));
}
fn glow(window: &mut Window, bounds: Bounds<Pixels>, radius: f32, color: Hsla, blur: f32) {
    window.paint_shadows(
        bounds,
        Corners::all(px(radius)),
        &[BoxShadow {
            color,
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(blur),
            spread_radius: px(0.0),
        }],
    );
}
fn polyline(window: &mut Window, points: &[Point<Pixels>], width: f32, color: Hsla) {
    if points.len() < 2 {
        return;
    }
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(points[0]);
    for p in &points[1..] {
        path.line_to(*p);
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}
fn polygon(window: &mut Window, points: &[Point<Pixels>], color: Hsla) {
    if points.len() < 3 {
        return;
    }
    let mut path = PathBuilder::fill();
    path.move_to(points[0]);
    for p in &points[1..] {
        path.line_to(*p);
    }
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// Diagonal hatching over a box: 1 px stripes every 9 px, leaning right.
fn hatch(window: &mut Window, pen: &Pen, x: f64, y: f64, w: f64, h: f64, color: Hsla) {
    const STEP: f64 = 9.0;
    let mut k = 0.0;
    while k < w + h {
        // A stripe from the box's bottom edge up to its top, clipped to the box.
        let (x0, x1) = (x + k - h, x + k);
        let clamp = |v: f64| v.clamp(x, x + w);
        let pts = [
            pen.at(clamp(x0), y + h - (clamp(x0) - x0)),
            pen.at(clamp(x1), y + (x1 - clamp(x1))),
            pen.at(clamp(x1 + 1.0), y + (x1 + 1.0 - clamp(x1 + 1.0))),
            pen.at(clamp(x0 + 1.0), y + h - (clamp(x0 + 1.0) - x0 - 1.0)),
        ];
        polygon(window, &pts, color);
        k += STEP;
    }
}

/// A run of text in one face.
#[derive(Clone, Copy)]
pub struct Face {
    pub size: f32,
    pub mono: bool,
    pub weight: FontWeight,
}
impl Face {
    pub const fn ui(size: f32, weight: FontWeight) -> Self {
        Self {
            size,
            mono: false,
            weight,
        }
    }
    pub const fn mono(size: f32) -> Self {
        Self {
            size,
            mono: true,
            weight: FontWeight::NORMAL,
        }
    }
    fn shape(&self, text: &str, color: Hsla, window: &Window) -> gpui::ShapedLine {
        let mut font = gpui::font(if self.mono { FONT_MONO } else { FONT_UI });
        font.weight = self.weight;
        let run = TextRun {
            len: text.len(),
            font,
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window
            .text_system()
            .shape_line(text.to_string().into(), px(self.size), &[run], None)
    }
    pub fn measure(&self, text: &str, window: &Window) -> f64 {
        f32::from(self.shape(text, gpui::black(), window).width) as f64
    }
    /// Paint `text` from `at` (left edge, vertical middle); returns its width.
    pub fn paint(
        &self,
        text: &str,
        at: Point<Pixels>,
        color: Hsla,
        window: &mut Window,
        cx: &mut App,
    ) -> f64 {
        let line = self.shape(text, color, window);
        let height = px(self.size * 1.3);
        let _ = line.paint(point(at.x, at.y - height / 2.0), height, window, cx);
        f32::from(line.width) as f64
    }
}

const CLIP_NAME: Face = Face::ui(11.5, FontWeight::SEMIBOLD);
const AGENT_TAG: Face = Face::mono(9.5);
const BUS_LABEL: Face = Face::ui(12.0, FontWeight::MEDIUM);
const BAR_NUMBER: Face = Face::mono(11.0);
const BUBBLE: Face = Face::mono(10.5);
const FLAG_NAME: Face = Face::ui(11.0, FontWeight::SEMIBOLD);
const TEMPO_LABEL: Face = Face::mono(10.5);

fn mix(a: Hsla, b: Hsla, keep_a: f32) -> Hsla {
    let (a, b): (Rgba, Rgba) = (a.into(), b.into());
    let t = keep_a.clamp(0.0, 1.0);
    Rgba {
        r: a.r * t + b.r * (1.0 - t),
        g: a.g * t + b.g * (1.0 - t),
        b: a.b * t + b.b * (1.0 - t),
        a: 1.0,
    }
    .into()
}

/// Waveform peaks of one source and how many there are per second.
#[derive(Clone)]
pub struct Peaks {
    pub buffer: Arc<AudioBuffer>,
    pub rate: f64,
}
impl Peaks {
    pub fn of(buffer: &Arc<AudioBuffer>) -> Self {
        // `AudioBuffer::new` keeps one peak per sample_rate / 400 frames.
        let chunk = (buffer.sample_rate / 400).max(1) as f64;
        Self {
            buffer: buffer.clone(),
            rate: buffer.sample_rate as f64 / chunk,
        }
    }
}

/// Everything the lanes draw, taken from the host when the view renders.
pub struct LaneScene {
    pub session: Arc<Session>,
    pub geo: Geo,
    pub scroll_y: f64,
    pub overlay: LaneOverlay,
    pub marker_drag: Option<(String, f64)>,
    pub playhead_bar: f64,
    pub peaks: HashMap<String, Peaks>,
    /// Tracks the running agent is working on.
    pub agent_tracks: HashSet<String>,
    pub theme: Theme,
}

/// The track lanes: backgrounds, cycle, grid, markers, clips, the gesture in progress and the
/// playhead.
pub fn paint_lanes(bounds: Bounds<Pixels>, scene: LaneScene, window: &mut Window, cx: &mut App) {
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        lanes(bounds, &scene, window, cx)
    });
}

fn lanes(bounds: Bounds<Pixels>, scene: &LaneScene, window: &mut Window, cx: &mut App) {
    let LaneScene {
        session: s,
        geo,
        scroll_y,
        overlay,
        theme,
        ..
    } = scene;
    let ink = theme.timeline();
    let pen = Pen::new(bounds);
    let w = f32::from(bounds.size.width) as f64;
    let h = f32::from(bounds.size.height) as f64;
    let row = geo.row;
    let y = |content: f64| content - scroll_y;
    let tracks_bottom = y(s.tracks.len() as f64 * row).min(h);

    fill(window, pen.rect(0.0, 0.0, w, h), theme.lane_empty);
    let visible = |i: usize| {
        let top = y(i as f64 * row);
        top + row >= 0.0 && top <= h
    };
    for (i, track) in s.tracks.iter().enumerate() {
        if !visible(i) {
            continue;
        }
        let top = y(i as f64 * row);
        let selected = s.view.selected_track_id.as_deref() == Some(track.id.as_str());
        let color = if selected {
            theme.lane_selected
        } else if scene.agent_tracks.contains(&track.id) {
            theme.lane_agent
        } else if track.is_bus() {
            theme.lane_empty
        } else {
            theme.lane
        };
        fill(window, pen.rect(0.0, top, w, row), color);
        if overlay.drop_track == Some(i) {
            fill(window, pen.rect(0.0, top, w, row), ink.drop_target);
        }
        fill(window, pen.rect(0.0, top, w, 1.0), ink.lane_top);
        fill(
            window,
            pen.rect(0.0, top + row - 1.0, w, 1.0),
            ink.lane_bottom,
        );
    }

    let t = &s.transport;
    let top = y(0.0).max(0.0);
    if t.cycle {
        let x0 = geo.x(t.cycle_start_bar);
        let x1 = geo.x(t.cycle_end_bar);
        fill(
            window,
            pen.rect(x0, top, x1 - x0, tracks_bottom - top),
            theme.cycle_lane,
        );
    }
    grid(
        window,
        &pen,
        geo,
        w,
        top,
        tracks_bottom - top,
        t.time_signature.numerator,
        theme,
    );
    // Past the song's end the lanes are hatched, so the end reads at a glance.
    let end = geo.x(s.end_bar()).max(0.0);
    if end < w && tracks_bottom > top {
        hatch(
            window,
            &pen,
            end,
            top,
            w - end,
            tracks_bottom - top,
            ink.hatch,
        );
    }

    // A bus lane names what it sums.
    for (i, track) in s.tracks.iter().enumerate() {
        if !track.is_bus() || !visible(i) {
            continue;
        }
        let inputs: Vec<&str> = s
            .tracks
            .iter()
            .filter(|t| t.output.as_deref() == Some(track.id.as_str()))
            .map(|t| t.name.as_str())
            .collect();
        let label = if inputs.is_empty() {
            "Bus · route tracks here from their Output, or send to it".to_string()
        } else {
            format!("Bus · {}", inputs.join(", "))
        };
        BUS_LABEL.paint(
            &label,
            pen.at(12.0, y(i as f64 * row + row / 2.0)),
            theme.text_3,
            window,
            cx,
        );
    }

    // Markers: a line down every lane where a section starts.
    let drag = scene
        .marker_drag
        .as_ref()
        .map(|(id, bar)| (id.as_str(), *bar));
    for m in geo_::markers_with(s, drag) {
        let x = geo.x(m.bar).round();
        if (0.0..=w).contains(&x) {
            let color = m
                .color
                .as_deref()
                .and_then(crate::ui::theme::css_color)
                .map_or(ink.marker_lane, |c| c.opacity(0.3));
            fill(window, pen.rect(x, top, 1.0, tracks_bottom - top), color);
        }
    }

    for (i, track) in s.tracks.iter().enumerate() {
        if !visible(i) {
            continue;
        }
        for clip in s.clips.iter().filter(|c| c.track_id == track.id) {
            let x = geo.x(clip.start_bar) + 1.0;
            let cw = clip.length_bars * geo.ppb - 2.0;
            if x + cw < 0.0 || x > w {
                continue;
            }
            let face = ClipFace {
                x,
                y: y(i as f64 * row) + arrange::CLIP_INSET as f64,
                w: cw,
                h: row - arrange::CLIP_INSET as f64 * 2.0,
            };
            clip_face(window, cx, &pen, scene, &ink, clip, track, i, face, w);
        }
    }

    if let Some(p) = overlay.pencil {
        let b = pen.rect(
            geo.x(p.start) + 1.0,
            y(p.track as f64 * row) + arrange::CLIP_INSET as f64,
            (p.length * geo.ppb - 2.0).max(2.0),
            row - arrange::CLIP_INSET as f64 * 2.0,
        );
        fill_round(window, b, arrange::CLIP_RADIUS, ink.pencil_preview);
        stroke_round(window, b, arrange::CLIP_RADIUS, 1.0, theme.accent);
    }
    if let Some(g) = overlay.ghost {
        let b = pen.rect(
            geo.x(g.start) + 1.5,
            y(g.track as f64 * row) + arrange::CLIP_INSET as f64 + 0.5,
            (g.length * geo.ppb - 3.0).max(2.0),
            row - arrange::CLIP_INSET as f64 * 2.0 - 1.0,
        );
        fill_round(window, b, arrange::CLIP_RADIUS, ink.drag_ghost);
        stroke_round(window, b, arrange::CLIP_RADIUS, 1.0, ink.drag_ghost_edge);
    }
    if let Some((track, bar)) = overlay.split {
        let x = geo.x(bar).round();
        fill(
            window,
            pen.rect(x, y(track as f64 * row), 1.0, row),
            ink.split_guide,
        );
    }

    playhead(window, &pen, geo.x(scene.playhead_bar).round(), w, h, theme);
}

fn playhead(window: &mut Window, pen: &Pen, x: f64, w: f64, h: f64, theme: &Theme) {
    if (0.0..=w).contains(&x) {
        let line = pen.rect(x, 0.0, 1.0, h);
        glow(window, line, 0.0, theme.accent_glow, 4.0);
        fill(window, line, theme.accent);
    }
}

#[allow(clippy::too_many_arguments)]
fn grid(
    window: &mut Window,
    pen: &Pen,
    geo: &Geo,
    w: f64,
    top: f64,
    h: f64,
    numerator: u32,
    theme: &Theme,
) {
    if h <= 0.0 {
        return;
    }
    let first = geo.scroll.floor() as i64;
    let last = (geo.scroll + w / geo.ppb).ceil() as i64;
    let beats = format::beat_line_offsets(geo.ppb as f32, numerator);
    for bar in first..=last {
        let x = geo.x(bar as f64).round();
        fill(window, pen.rect(x, top, 1.0, h), theme.bar_line);
        for offset in &beats {
            fill(
                window,
                pen.rect((x + *offset as f64).round(), top, 1.0, h),
                theme.beat_line,
            );
        }
    }
}

#[derive(Clone, Copy)]
struct ClipFace {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[allow(clippy::too_many_arguments)]
fn clip_face(
    window: &mut Window,
    cx: &mut App,
    pen: &Pen,
    scene: &LaneScene,
    ink: &Timeline,
    clip: &Clip,
    track: &Track,
    index: usize,
    f: ClipFace,
    lane_w: f64,
) {
    let theme = &scene.theme;
    let s = &scene.session;
    let selected = s.view.selected_clip_id.as_deref() == Some(clip.id.as_str());
    let r = arrange::CLIP_RADIUS;
    let color = theme.track(&track.color, index);
    let top = mix(color, ink.face_base, ink.face_top);
    let bottom = mix(color, ink.face_base, ink.face_bottom);
    let b = pen.rect(f.x, f.y, f.w, f.h);
    if clip.agent {
        // What the agent wrote glows, under the face so the face keeps its colour.
        glow(window, b, r, theme.accent_glow, 8.0);
    }
    window.paint_shadows(
        b,
        Corners::all(px(r)),
        &[BoxShadow {
            color: ink.clip_shadow,
            offset: point(px(0.0), px(1.0)),
            blur_radius: px(3.0),
            spread_radius: px(0.0),
        }],
    );
    window.paint_quad(
        gpui::fill(
            b,
            linear_gradient(
                180.0,
                linear_color_stop(top, 0.0),
                linear_color_stop(bottom, 1.0),
            ),
        )
        .corner_radii(px(r)),
    );

    let title_h = arrange::CLIP_TITLE as f64;
    let env = Envelope::of(&clip.data).map(|mut env| {
        if let Some((id, fin, fout)) = &scene.overlay.fade {
            if id == &clip.id {
                env.fade_in = *fin;
                env.fade_out = *fout;
            }
        }
        env
    });
    let rates = geo_::fade_rates(s, scene.geo.ppb, clip.start_bar, clip.length_bars);
    window.with_content_mask(Some(ContentMask { bounds: b }), |window| {
        let title = pen.rect(f.x, f.y, f.w, title_h);
        window.paint_quad(gpui::fill(title, theme.clip_title).corner_radii(Corners {
            top_left: px(r),
            top_right: px(r),
            bottom_left: px(0.0),
            bottom_right: px(0.0),
        }));
        fill(
            window,
            pen.rect(f.x, f.y + title_h - 1.0, f.w, 1.0),
            ink.clip_title_bottom,
        );
        fill(
            window,
            pen.rect(f.x + r as f64, f.y, f.w - r as f64 * 2.0, 1.0),
            ink.clip_highlight,
        );
        fill(
            window,
            pen.rect(f.x + r as f64, f.y + f.h - 1.0, f.w - r as f64 * 2.0, 1.0),
            ink.clip_contact,
        );

        let fades_px = env.map(|e| (e.fade_in * rates.0, e.fade_out * rates.1));
        let name_w = CLIP_NAME.measure(&clip.name, window);
        let name_x = geo_::name_start(f.x, f.w, name_w, fades_px, selected);
        let tag = clip.agent && f.w > 70.0;
        let name_end = if tag {
            f.x + f.w - 52.0
        } else {
            f.x + f.w - 6.0
        };
        if name_end > name_x {
            let mask = pen.rect(name_x, f.y, name_end - name_x, title_h);
            window.with_content_mask(Some(ContentMask { bounds: mask }), |window| {
                CLIP_NAME.paint(
                    &clip.name,
                    pen.at(name_x, f.y + title_h / 2.0),
                    theme.clip_text,
                    window,
                    cx,
                );
            });
        }
        if tag {
            let tw = AGENT_TAG.measure("AGENT", window);
            AGENT_TAG.paint(
                "AGENT",
                pen.at(f.x + f.w - 6.0 - tw, f.y + title_h / 2.0),
                theme.accent,
                window,
                cx,
            );
        }

        let content = ClipFace {
            y: f.y + title_h,
            h: f.h - title_h,
            ..f
        };
        match (&clip.data, env) {
            (
                ClipData::Audio {
                    source_id,
                    offset_seconds,
                    ..
                },
                Some(env),
            ) => {
                if let Some(peaks) = scene.peaks.get(source_id) {
                    waveform(
                        window,
                        pen,
                        s,
                        clip,
                        *offset_seconds,
                        &env,
                        peaks,
                        content,
                        lane_w,
                        theme,
                        ink,
                    );
                }
                fades(window, pen, &env, f, selected, rates, ink);
            }
            (ClipData::Midi { notes, .. }, _) => {
                let px_per_beat = scene.geo.ppb / s.beats_per_bar();
                midi_preview(window, pen, notes, content, px_per_beat, theme, ink);
            }
            _ => {}
        }
    });

    if clip.agent {
        stroke_round(window, b, r, 1.0, theme.accent);
    } else if selected {
        let ring = pen.rect(f.x - 0.5, f.y - 0.5, f.w + 1.0, f.h + 1.0);
        stroke_round(window, ring, r + 0.5, 1.5, ink.clip_selected);
    }
}

/// Peaks from the host's audio library, drawn as they sound: scaled by the clip's gain and
/// fades. Under a tempo change the audio no longer spreads evenly across the clip, so each
/// column asks the tempo map how far into the audio it is.
#[allow(clippy::too_many_arguments)]
fn waveform(
    window: &mut Window,
    pen: &Pen,
    s: &Session,
    clip: &Clip,
    offset: f64,
    env: &Envelope,
    peaks: &Peaks,
    f: ClipFace,
    lane_w: f64,
    theme: &Theme,
    ink: &Timeline,
) {
    let data = &peaks.buffer.peaks;
    if data.is_empty() || f.w <= 0.0 || f.h <= 0.0 {
        return;
    }
    let bpb = s.beats_per_bar();
    let map = s.tempo_map();
    let start_beat = clip.start_bar * bpb;
    let length_beats = clip.length_bars * bpb;
    let seconds = map.duration(start_beat, start_beat + length_beats);
    let steady = !s
        .tempo_changes
        .iter()
        .any(|p| p.bar > clip.start_bar && p.bar < clip.start_bar + clip.length_bars);
    let age = |col: f64| {
        let frac = ((col - f.x) / f.w).clamp(0.0, 1.0);
        if steady {
            frac * seconds
        } else {
            map.duration(start_beat, start_beat + frac * length_beats)
        }
    };
    let c0 = f.x.max(0.0).floor() as i64;
    let c1 = (f.x + f.w).min(lane_w).ceil() as i64;
    if c1 <= c0 {
        return;
    }
    let mid = f.y + f.h / 2.0;
    let half = f.h / 2.0;
    let mut top = Vec::with_capacity((c1 - c0 + 1) as usize);
    let mut a0 = age(c0 as f64);
    for col in c0..c1 {
        let a1 = age(col as f64 + 1.0);
        let i0 = ((offset + a0) * peaks.rate).floor().max(0.0) as usize;
        let i1 = (((offset + a1) * peaks.rate).ceil() as usize)
            .max(i0 + 1)
            .min(data.len());
        let peak = data
            .get(i0..i1)
            .map_or(0.0, |p| p.iter().fold(0f32, |m, v| m.max(*v)));
        let centre = (a0 + a1) / 2.0;
        let gain = env.gain_at(centre, seconds - centre);
        let a = (peak as f64 * gain * half * 0.95).clamp(0.8, half);
        top.push((col as f64, a));
        top.push((col as f64 + 1.0, a));
        a0 = a1;
    }
    let mut points: Vec<Point<Pixels>> = top.iter().map(|(x, a)| pen.at(*x, mid - a)).collect();
    points.extend(top.iter().rev().map(|(x, a)| pen.at(*x, mid + a)));
    polygon(window, &points, theme.waveform);
    fill(
        window,
        pen.rect(c0 as f64, mid.round(), (c1 - c0) as f64, 1.0),
        ink.waveform_mid,
    );
}

/// The fades over an audio clip: the part above each curve shaded, the curve drawn, and a
/// handle in the title strip where each fade ends.
fn fades(
    window: &mut Window,
    pen: &Pen,
    env: &Envelope,
    f: ClipFace,
    selected: bool,
    rates: (f64, f64),
    ink: &Timeline,
) {
    let top = f.y + arrange::CLIP_TITLE as f64;
    let ch = f.h - arrange::CLIP_TITLE as f64;
    let fin = (env.fade_in * rates.0).min(f.w);
    let fout = (env.fade_out * rates.1).min(f.w);
    const STEPS: usize = 24;
    let shade = |window: &mut Window, from: f64, span: f64, rising: bool| {
        if span < 1.0 {
            return;
        }
        let curve: Vec<Point<Pixels>> = (0..=STEPS)
            .map(|i| {
                let t = i as f64 / STEPS as f64;
                let g = env.curve.gain(if rising { t } else { 1.0 - t });
                pen.at(from + t * span, top + ch * (1.0 - g))
            })
            .collect();
        let mut area = vec![pen.at(from, top), pen.at(from + span, top)];
        area.extend(curve.iter().rev().copied());
        polygon(window, &area, ink.fade_shade);
        polyline(window, &curve, 1.0, ink.fade_curve);
    };
    shade(window, f.x, fin, true);
    shade(window, f.x + f.w - fout, fout, false);
    if !geo_::shows_fade_handles(f.w, fin, fout, selected) {
        return;
    }
    let hs = arrange::FADE_HANDLE as f64;
    let hy = f.y + (arrange::CLIP_TITLE as f64 - hs) / 2.0;
    for centre in [f.x + fin, f.x + f.w - fout] {
        let left = (centre - hs / 2.0).min(f.x + f.w - hs - 1.0).max(f.x + 1.0);
        fill_round(window, pen.rect(left, hy, hs, hs), 2.0, ink.fade_handle);
    }
}

fn midi_preview(
    window: &mut Window,
    pen: &Pen,
    notes: &[ryolune_engine::model::Note],
    f: ClipFace,
    px_per_beat: f64,
    theme: &Theme,
    ink: &Timeline,
) {
    let Some(lo) = notes.iter().map(|n| n.pitch).min() else {
        return;
    };
    let hi = notes.iter().map(|n| n.pitch).max().unwrap_or(lo);
    let range = ((hi - lo) as f64).max(12.0);
    let pad = 4.0;
    let note_h = arrange::CLIP_NOTE_H as f64;
    for n in notes {
        let x = f.x + n.start * px_per_beat;
        if x > f.x + f.w {
            continue;
        }
        let w = (n.length * px_per_beat).max(3.0);
        let y = f.y + pad + (1.0 - (n.pitch - lo) as f64 / range) * (f.h - pad * 2.0 - note_h);
        let b = pen.rect(x, y, w, note_h);
        if n.agent {
            glow(window, b, 1.0, theme.accent_glow, 4.0);
            fill(window, b, theme.accent);
        } else {
            fill(window, b, ink.midi_note);
        }
    }
}

/// Everything the ruler draws.
pub struct RulerScene {
    pub session: Arc<Session>,
    pub geo: Geo,
    pub overlay: RulerOverlay,
    pub playhead_beats: f64,
    pub theme: Theme,
    /// Marker name widths, measured here for the view's hit testing.
    pub widths: Rc<RefCell<HashMap<String, f32>>>,
}

/// The ruler: cycle range, bar numbers and beat ticks, marker flags and the playhead with its
/// position bubble.
pub fn paint_ruler(bounds: Bounds<Pixels>, scene: RulerScene, window: &mut Window, cx: &mut App) {
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        ruler(bounds, &scene, window, cx)
    });
}

fn ruler(bounds: Bounds<Pixels>, scene: &RulerScene, window: &mut Window, cx: &mut App) {
    let RulerScene {
        session: s,
        geo,
        overlay,
        theme,
        ..
    } = scene;
    let ink = theme.timeline();
    let pen = Pen::new(bounds);
    let w = f32::from(bounds.size.width) as f64;
    let h = f32::from(bounds.size.height) as f64;
    let t = &s.transport;
    fill(window, pen.rect(0.0, 0.0, w, h), theme.ruler);

    let cycle = overlay
        .cycle
        .or(t.cycle.then_some((t.cycle_start_bar, t.cycle_end_bar)));
    if let Some((start, end)) = cycle {
        let x0 = geo.x(start).round();
        let x1 = geo.x(end).round();
        let grip = arrange::CYCLE_GRIP as f64;
        fill(window, pen.rect(x0, 0.0, x1 - x0, h), theme.cycle);
        fill(window, pen.rect(x0, 0.0, 1.0, h), ink.cycle_edge);
        fill(window, pen.rect(x1 - 1.0, 0.0, 1.0, h), ink.cycle_edge);
        fill(window, pen.rect(x0, 0.0, grip, 3.0), ink.cycle_handle);
        fill(
            window,
            pen.rect(x1 - grip, 0.0, grip, 3.0),
            ink.cycle_handle,
        );
    }

    let every = geo_::label_every(geo.ppb);
    let beats = format::beat_line_offsets(geo.ppb as f32, t.time_signature.numerator);
    let tick = arrange::RULER_TICK as f64;
    let first = geo.scroll.floor().max(0.0) as i64;
    let last = (geo.scroll + w / geo.ppb).ceil() as i64;
    for bar in first..=last {
        let x = geo.x(bar as f64).round();
        fill(window, pen.rect(x, 0.0, 1.0, h), ink.ruler_bar);
        if bar as u64 % every == 0 {
            BAR_NUMBER.paint(
                &(bar + 1).to_string(),
                pen.at(x + 5.0, 9.0),
                theme.text_2,
                window,
                cx,
            );
        }
        for offset in &beats {
            fill(
                window,
                pen.rect((x + *offset as f64).round(), h - tick, 1.0, tick),
                ink.ruler_tick,
            );
        }
    }
    fill(window, pen.rect(0.0, h - 1.0, w, 1.0), ink.ruler_bottom);

    // Marker flags in the lower half.
    let drag = overlay.marker.as_ref().map(|(id, bar)| (id.as_str(), *bar));
    let list = geo_::markers_with(s, drag);
    {
        let mut widths = scene.widths.borrow_mut();
        widths.retain(|id, _| list.iter().any(|m| &m.id == id));
        for m in &list {
            widths.insert(m.id.clone(), FLAG_NAME.measure(&m.name, window) as f32);
        }
    }
    let widths = scene.widths.borrow().clone();
    let top = arrange::MARKER_TOP as f64;
    let fh = arrange::MARKER_H as f64;
    for i in 0..list.len() {
        let m = &list[i];
        let (x, fw) = geo_::flag_span(&list, i, geo, &widths);
        if x + fw < 0.0 || x > w {
            continue;
        }
        let color = m
            .color
            .as_deref()
            .and_then(crate::ui::theme::css_color)
            .unwrap_or(theme.marker);
        fill_round(window, pen.rect(x, top, fw, fh), 2.0, ink.marker_flag);
        fill(window, pen.rect(x, top, geo_::FLAG_STRIPE, fh), color);
        fill(window, pen.rect(x, top + fh, 1.0, h - top - fh), color);
        if fw > geo_::FLAG_STRIPE + geo_::FLAG_PAD {
            let mask = pen.rect(x + geo_::FLAG_STRIPE, top, fw - geo_::FLAG_STRIPE - 2.0, fh);
            window.with_content_mask(Some(ContentMask { bounds: mask }), |window| {
                FLAG_NAME.paint(
                    &m.name,
                    pen.at(x + geo_::FLAG_STRIPE + geo_::FLAG_PAD, top + fh / 2.0),
                    theme.text_display,
                    window,
                    cx,
                );
            });
        }
    }

    // The playhead: line, flag and the position bubble, kept above the marker flags so it
    // never hides a section name.
    let bar = scene.playhead_beats / s.beats_per_bar();
    let x = geo.x(bar).round();
    let flag_w = arrange::PLAYHEAD_FLAG_W as f64;
    if x < -flag_w || x > w + flag_w {
        return;
    }
    let line = pen.rect(x, 0.0, 1.0, h);
    glow(window, line, 0.0, theme.accent_glow, 4.0);
    fill(window, line, theme.accent);
    let tri = [
        pen.at(x - flag_w / 2.0, 0.0),
        pen.at(x + flag_w / 2.0 + 1.0, 0.0),
        pen.at(x + 0.5, arrange::PLAYHEAD_FLAG_H as f64),
    ];
    polygon(window, &tri, theme.accent);
    let label = format::bar_beat_short(scene.playhead_beats, s.beats_per_bar());
    let tw = BUBBLE.measure(&label, window);
    let (bx, by) = (x + 9.0, 1.0);
    let (bw, bh) = (tw.ceil() + 12.0, top - 2.0 * by - 1.0);
    let bubble = pen.rect(bx, by, bw, bh);
    window.paint_shadows(
        bubble,
        Corners::all(px(0.0)),
        &[BoxShadow {
            color: theme.drop,
            offset: point(px(2.0), px(2.0)),
            blur_radius: px(0.0),
            spread_radius: px(0.0),
        }],
    );
    window.paint_quad(
        gpui::fill(bubble, theme.glass(2))
            .border_widths(px(1.0))
            .border_color(theme.glass_edge),
    );
    BUBBLE.paint(
        &label,
        pen.at(bx + 6.0, by + bh / 2.0),
        theme.text_display,
        window,
        cx,
    );
}

/// Everything the tempo track draws.
pub struct TempoScene {
    pub session: Arc<Session>,
    pub geo: Geo,
    pub drag: Option<TempoDrag>,
    pub playhead_bar: f64,
    pub theme: Theme,
}

/// The tempo track: the starting tempo from bar 1, then each change reached by a step or, for
/// a ramp, a straight line (tempo is linear in beats, so in pixels), the area under it lightly
/// filled, each point with its tempo.
pub fn paint_tempo(bounds: Bounds<Pixels>, scene: TempoScene, window: &mut Window, cx: &mut App) {
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        tempo(bounds, &scene, window, cx)
    });
}

fn tempo(bounds: Bounds<Pixels>, scene: &TempoScene, window: &mut Window, cx: &mut App) {
    let TempoScene {
        session: s,
        geo,
        theme,
        ..
    } = scene;
    let ink = theme.timeline();
    let pen = Pen::new(bounds);
    let w = f32::from(bounds.size.width) as f64;
    let h = f32::from(bounds.size.height) as f64;
    fill(window, pen.rect(0.0, 0.0, w, h), theme.lane_empty);
    grid(
        window,
        &pen,
        geo,
        w,
        0.0,
        h,
        s.transport.time_signature.numerator,
        theme,
    );
    fill(window, pen.rect(0.0, h - 1.0, w, 1.0), ink.lane_bottom);

    let points = geo_::tempo_points(s, scene.drag.as_ref());
    let range = geo_::tempo_range(&points);
    let x = |bar: f64| geo.x(bar);
    let y = |bpm: f64| geo_::y_of_bpm(bpm, range, h);
    let mut curve = vec![pen.at(x(0.0).min(0.0), y(points[0].bpm))];
    for pair in points.windows(2) {
        if !pair[1].ramp {
            curve.push(pen.at(x(pair[1].bar), y(pair[0].bpm)));
        }
        curve.push(pen.at(x(pair[1].bar), y(pair[1].bpm)));
    }
    curve.push(pen.at(w, y(points[points.len() - 1].bpm)));
    let mut area = curve.clone();
    area.push(pen.at(w, h));
    area.push(pen.at(x(0.0).min(0.0), h));
    polygon(window, &area, ink.tempo_fill);
    polyline(window, &curve, 1.5, theme.accent);

    let grip = arrange::TEMPO_GRIP as f64;
    for (i, p) in points.iter().enumerate() {
        let (px_, py) = (x(p.bar), y(p.bpm));
        let dragged = scene
            .drag
            .is_some_and(|d| (p.bar - d.bar).abs() < geo_::SAME_BAR);
        if i > 0 && px_ >= -grip && px_ <= w + grip {
            let r = arrange::TEMPO_POINT as f64 + if dragged { 1.0 } else { 0.0 };
            let dot = pen.rect(px_ - r, py - r, r * 2.0, r * 2.0);
            if dragged {
                glow(window, dot, r as f32, theme.accent_glow, 6.0);
            }
            fill_round(window, dot, r as f32, theme.accent);
        }
        let label_x = (px_ + 5.0).max(4.0);
        let room = points
            .get(i + 1)
            .map_or(f64::INFINITY, |n| x(n.bar) - label_x);
        if room < 28.0 || label_x > w {
            continue;
        }
        let above = py - 4.0 > 14.0;
        let ly = if above { py - 9.0 } else { py + 10.0 };
        let color = if dragged {
            theme.text_display
        } else {
            theme.text_2
        };
        TEMPO_LABEL.paint(
            &geo_::bpm_label(p.bpm),
            pen.at(label_x, ly),
            color,
            window,
            cx,
        );
    }

    let px_ = x(scene.playhead_bar).round();
    if (0.0..=w).contains(&px_) {
        fill(window, pen.rect(px_, 0.0, 1.0, h), theme.accent);
    }
}
