//! The arrangement: the tool bar, the ruler with markers and the cycle range, the tempo track,
//! the track headers and the lanes with their regions.
//!
//! Lanes, ruler and tempo track are canvases ([`paint`]); what a press, a drag and a release
//! do is worked out in [`gestures`] from the same [`geometry`] the canvases paint with. A drag
//! is local to this view (the ghost it draws) until release, when its registry commands run
//! inside one `Daw::gesture`, so it is one undo step and the CLI, MCP and the agent could have
//! made the same edit.

mod geometry;
mod gestures;
mod headers;
mod menus;
mod paint;
#[cfg(test)]
mod tests;
mod toolbar;

use super::{
    browser::BrowserDrag,
    daw::Daw,
    theme::{arrange, layout, radius, size, Theme},
    widgets::{self, Button, InputEvent, MenuHost, TextInput},
};
use geometry::{Geo, TempoDrag};
use gestures::{
    Call, Cursor, LaneDrag, LaneOverlay, RulerDrag, RulerOverlay, TempoGesture, TrackDrag,
};
use gpui::{
    canvas, div, prelude::*, px, AnyElement, Bounds, Context, CursorStyle, DispatchPhase, Entity,
    ExternalPaths, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, ScrollWheelEvent, Subscription, Task, Window,
};
use ryolune_engine::model::Session;
use serde_json::json;
use std::{cell::Cell, cell::RefCell, collections::HashMap, rc::Rc, sync::Arc, time::Duration};

/// What is being typed in place.
#[derive(Clone, Debug, PartialEq)]
enum Editing {
    Track(String),
    Clip(String),
    Marker(String),
    /// A tempo point, by its bar (0 is the starting tempo).
    Tempo(f64),
}

/// The drag in progress, whichever part of the arrangement it started in.
enum Drag {
    Lane(LaneDrag),
    Ruler(RulerDrag),
    Tempo(TempoGesture),
    Track(TrackDrag),
}

/// Where each canvas was laid out on the last frame, for turning window positions into
/// lane coordinates.
#[derive(Clone, Default)]
struct Areas {
    lanes: Rc<Cell<Bounds<Pixels>>>,
    ruler: Rc<Cell<Bounds<Pixels>>>,
    tempo: Rc<Cell<Bounds<Pixels>>>,
    headers: Rc<Cell<Bounds<Pixels>>>,
}

fn local(bounds: Bounds<Pixels>, at: Point<Pixels>) -> (f64, f64) {
    (
        f32::from(at.x - bounds.origin.x) as f64,
        f32::from(at.y - bounds.origin.y) as f64,
    )
}

pub struct Arrangement {
    daw: Entity<Daw>,
    menu: MenuHost,
    areas: Areas,
    /// Vertical scroll of the track list, in pixels.
    scroll_y: f64,
    drag: Option<Drag>,
    lane_overlay: LaneOverlay,
    /// Where a browser row being dragged would land: the track row and the bar.
    browser_target: Option<(usize, f64)>,
    ruler_overlay: RulerOverlay,
    tempo_overlay: Option<TempoDrag>,
    lane_cursor: Cursor,
    ruler_cursor: Cursor,
    tempo_cursor: Cursor,
    /// Marker name widths from the last ruler paint, for hitting a whole flag.
    flag_widths: Rc<RefCell<HashMap<String, f32>>>,
    /// The lane width last reported to the host (`view.set laneWidth`).
    reported_width: Rc<Cell<f64>>,
    editing: Option<Editing>,
    name_input: Entity<TextInput>,
    bpm_input: Entity<TextInput>,
    /// The microphone level of armed audio tracks, held so it falls slowly.
    input_level: f32,
    meter: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Arrangement {
    pub fn new(daw: Entity<Daw>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name_input = cx.new(TextInput::new);
        let bpm_input = cx.new(|cx| TextInput::new(cx).mono());
        let on_input = |this: &mut Self,
                        _: &Entity<TextInput>,
                        e: &InputEvent,
                        _: &mut Window,
                        cx: &mut Context<Self>| {
            match e {
                InputEvent::Submit | InputEvent::Blur => this.commit_edit(cx),
                InputEvent::Cancel => this.editing = None,
                InputEvent::Changed => {}
            }
            cx.notify();
        };
        let subscriptions = vec![
            cx.subscribe_in(&name_input, window, on_input),
            cx.subscribe_in(&bpm_input, window, on_input),
            cx.observe(&daw, |this, _, cx| {
                this.follow_playhead(cx);
                cx.notify();
            }),
        ];
        Self {
            daw,
            menu: MenuHost::default(),
            areas: Areas::default(),
            scroll_y: 0.0,
            drag: None,
            lane_overlay: LaneOverlay::default(),
            browser_target: None,
            ruler_overlay: RulerOverlay::default(),
            tempo_overlay: None,
            lane_cursor: Cursor::Default,
            ruler_cursor: Cursor::Default,
            tempo_cursor: Cursor::Default,
            flag_widths: Rc::default(),
            reported_width: Rc::new(Cell::new(0.0)),
            editing: None,
            name_input,
            bpm_input,
            input_level: 0.0,
            meter: None,
            _subscriptions: subscriptions,
        }
    }

    fn session(&self, cx: &gpui::App) -> Arc<Session> {
        self.daw.read(cx).app.store.snapshot()
    }
    fn geo(&self, cx: &gpui::App) -> Geo {
        let app = &self.daw.read(cx).app;
        Geo::new(app.zoom, app.scroll)
    }
    fn tool(&self, cx: &gpui::App) -> usize {
        self.daw.read(cx).app.tool
    }

    /// Run registry commands from the window, in order.
    fn run(&self, calls: Vec<Call>, cx: &mut Context<Self>) {
        if calls.is_empty() {
            return;
        }
        self.daw.update(cx, |daw, cx| {
            for (method, params) in calls {
                if daw.run(method, params, cx).is_none() {
                    break;
                }
            }
        });
    }

    /// While playing with follow on, page the view so the playhead stays in sight.
    fn follow_playhead(&mut self, cx: &mut Context<Self>) {
        if self.drag.is_some() {
            return;
        }
        let app = &self.daw.read(cx).app;
        let s = app.store.session();
        if !app.playing || !s.view.follow_playhead {
            return;
        }
        let geo = Geo::new(app.zoom, app.scroll);
        let bar = app.position / s.beats_per_bar();
        if let Some(scroll) = geometry::follow_scroll(&geo, bar, app.lane_width) {
            self.run(vec![("view.set", json!({ "scrollBar": scroll }))], cx);
        }
    }

    fn start_drag(&mut self, drag: Drag, cx: &mut Context<Self>) {
        self.daw.update(cx, |daw, _| daw.gesture(true));
        self.drag = Some(drag);
        cx.notify();
    }

    /// The pointer moved with a drag in progress, anywhere in the window.
    fn drag_move(
        &mut self,
        at: Point<Pixels>,
        m: Modifiers,
        pressed: bool,
        cx: &mut Context<Self>,
    ) {
        if self.drag.is_none() {
            return;
        }
        if !pressed {
            // The button came up where the window could not see it: drop the drag, unmade.
            self.cancel_drag(cx);
            return;
        }
        let s = self.session(cx);
        let geo = self.geo(cx);
        match self.drag.as_mut() {
            Some(Drag::Lane(d)) => {
                let (x, y) = local(self.areas.lanes.get(), at);
                self.lane_overlay = gestures::lane_move(d, &s, &geo, x, y + self.scroll_y, m.alt);
                self.lane_cursor = match d {
                    LaneDrag::Move { moved: true, .. } => Cursor::Grabbing,
                    LaneDrag::Resize { .. } | LaneDrag::Fade { .. } => Cursor::ResizeX,
                    _ => self.lane_cursor,
                };
            }
            Some(Drag::Ruler(d)) => {
                let (x, _) = local(self.areas.ruler.get(), at);
                self.ruler_overlay = gestures::ruler_move(d, &s, &geo, x, m.alt);
                if self.ruler_overlay.marker.is_some() {
                    self.ruler_cursor = Cursor::Grabbing;
                }
            }
            Some(Drag::Tempo(g)) => {
                let b = self.areas.tempo.get();
                let (x, y) = local(b, at);
                let h = f32::from(b.size.height) as f64;
                if let Some(d) = gestures::tempo_move(g, &s, &geo, x, y, h, m.alt, m.shift) {
                    self.tempo_overlay = Some(d);
                    self.tempo_cursor = Cursor::Grabbing;
                }
            }
            Some(Drag::Track(d)) => {
                let (_, y) = local(self.areas.headers.get(), at);
                d.follow(y + self.scroll_y, geo.row, s.tracks.len());
            }
            None => {}
        }
        cx.notify();
    }

    /// The button came up: the drag's commands run, as one undo step.
    fn drag_end(&mut self, at: Point<Pixels>, m: Modifiers, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        let s = self.session(cx);
        let geo = self.geo(cx);
        let calls = match drag {
            Drag::Lane(d) => gestures::lane_release(d, &s),
            Drag::Ruler(d) => {
                let (x, _) = local(self.areas.ruler.get(), at);
                gestures::ruler_release(d, &s, &geo, x, m.alt)
            }
            Drag::Tempo(g) => {
                let b = self.areas.tempo.get();
                let (x, y) = local(b, at);
                let h = f32::from(b.size.height) as f64;
                gestures::tempo_release(g, &s, &geo, x, y, h, m.alt, m.shift)
            }
            Drag::Track(d) => d.release(),
        };
        self.run(calls, cx);
        self.daw.update(cx, |daw, _| daw.gesture(false));
        self.clear_overlays();
        cx.notify();
    }

    fn cancel_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            self.daw.update(cx, |daw, _| daw.gesture(false));
        }
        self.clear_overlays();
        cx.notify();
    }

    fn clear_overlays(&mut self) {
        let drop_track = self.lane_overlay.drop_track;
        self.lane_overlay = LaneOverlay {
            drop_track,
            ..Default::default()
        };
        self.ruler_overlay = RulerOverlay::default();
        self.tempo_overlay = None;
        self.lane_cursor = Cursor::Default;
        self.ruler_cursor = Cursor::Default;
        self.tempo_cursor = Cursor::Default;
    }

    /// Horizontal wheel scrolls the song, Ctrl or Cmd with the wheel zooms around the pointer,
    /// the vertical wheel scrolls the track list.
    fn wheel(&mut self, e: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = e.delta.pixel_delta(px(16.0));
        let (dx, dy) = (f32::from(delta.x) as f64, f32::from(delta.y) as f64);
        let geo = self.geo(cx);
        let lanes = self.areas.lanes.get();
        if e.modifiers.control || e.modifiers.platform {
            let (x, _) = local(lanes, e.position);
            let (ppb, scroll) = geometry::zoom_around(&geo, (dy * 0.01).exp(), x.max(0.0));
            self.run(
                vec![(
                    "view.set",
                    json!({"pixelsPerBar": ppb, "scrollBar": scroll}),
                )],
                cx,
            );
            cx.stop_propagation();
            return;
        }
        let horizontal = e.modifiers.shift || dx.abs() > dy.abs();
        if horizontal {
            let d = if dx != 0.0 { dx } else { dy };
            let scroll = (geo.scroll - d / geo.ppb).max(0.0);
            if scroll != geo.scroll {
                self.run(vec![("view.set", json!({ "scrollBar": scroll }))], cx);
            }
        } else {
            self.scroll_y -= dy;
            self.clamp_scroll(cx);
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn clamp_scroll(&mut self, cx: &gpui::App) {
        let tracks = self.daw.read(cx).app.store.session().tracks.len() as f64;
        let visible = f32::from(self.areas.lanes.get().size.height) as f64;
        let max = (tracks * layout::TRACK_HEIGHT as f64 + 40.0 - visible).max(0.0);
        self.scroll_y = self.scroll_y.clamp(0.0, max);
    }

    /// Start typing a name or a tempo in place.
    fn edit(
        &mut self,
        editing: Editing,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = if matches!(editing, Editing::Tempo(_)) {
            &self.bpm_input
        } else {
            &self.name_input
        };
        input.update(cx, |input, cx| {
            input.set_text(text, cx);
            input.select_all_text(cx);
            input.focus(window);
        });
        self.editing = Some(editing);
        cx.notify();
    }

    fn commit_edit(&mut self, cx: &mut Context<Self>) {
        let Some(editing) = self.editing.take() else {
            return;
        };
        let name = self.name_input.read(cx).text().trim().to_string();
        let call = match editing {
            Editing::Tempo(bar) => {
                let Ok(bpm) = self.bpm_input.read(cx).text().trim().parse::<f64>() else {
                    return;
                };
                if !bpm.is_finite() {
                    return;
                }
                (
                    "tempo.set",
                    json!({"bar": bar, "bpm": bpm.clamp(20.0, 400.0)}),
                )
            }
            _ if name.is_empty() => return,
            Editing::Track(id) => ("track.rename", json!({"trackId": id, "name": name})),
            Editing::Clip(id) => ("clip.rename", json!({"clipId": id, "name": name})),
            Editing::Marker(id) => ("marker.rename", json!({"markerId": id, "name": name})),
        };
        self.run(vec![call], cx);
    }

    /// The field for what is being typed, placed at `(left, top)` of its area.
    fn edit_field(&self, left: f64, top: f64, width: f64, cx: &gpui::App) -> AnyElement {
        let input = if matches!(self.editing, Some(Editing::Tempo(_))) {
            &self.bpm_input
        } else {
            &self.name_input
        };
        div()
            .absolute()
            .left(px(left as f32))
            .top(px(top as f32))
            .w(px(width as f32))
            .h(px(arrange::INLINE_INPUT_H))
            .child(
                widgets::field(input, true, cx)
                    .h_full()
                    .px(px(6.0))
                    .py(px(2.0))
                    .text_size(px(size::SM)),
            )
            .into_any_element()
    }

    /// Keep the microphone level of armed audio tracks moving while one is armed. The level
    /// is read on a 30 Hz timer, and the window redraws only when the meter would look
    /// different: a silent input (or none) costs no frames. Redrawing the whole window 30
    /// times a second for a still meter kept a core busy while idle.
    fn input_meter(&mut self, armed: bool, cx: &mut Context<Self>) {
        if !armed {
            self.meter = None;
            self.input_level = 0.0;
            return;
        }
        if self.meter.is_none() {
            self.meter = Some(cx.spawn(async move |this, cx| loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let alive = this.update(cx, |this, cx| {
                    let peak = this
                        .daw
                        .read(cx)
                        .app
                        .device
                        .as_ref()
                        .map_or(0.0, |d| d.telemetry.take_input_peak().min(1.0));
                    let level = peak.max(this.input_level * 0.86);
                    let level = if level < 0.002 { 0.0 } else { level };
                    let shown = |l: f32| (l * 120.0).round() as i32;
                    if shown(level) != shown(this.input_level) {
                        this.input_level = level;
                        cx.notify();
                    } else {
                        this.input_level = level;
                    }
                });
                if alive.is_err() {
                    break;
                }
            }));
        }
    }

    /// The lanes canvas: paints the lanes and reports their width to the host when it changes.
    fn lanes_canvas(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let app = &self.daw.read(cx).app;
        let s = app.store.snapshot();
        let running = app.agents.runtime.running();
        let agent_tracks = s
            .tracks
            .iter()
            .filter(|t| running && s.clips.iter().any(|c| c.agent && c.track_id == t.id))
            .map(|t| t.id.clone())
            .collect();
        let peaks = s
            .clips
            .iter()
            .filter_map(|c| match &c.data {
                ryolune_engine::model::ClipData::Audio { source_id, .. } => app
                    .library
                    .get(source_id)
                    .map(|b| (source_id.clone(), paint::Peaks::of(b))),
                _ => None,
            })
            .collect();
        let scene = paint::LaneScene {
            geo: Geo::new(app.zoom, app.scroll),
            scroll_y: self.scroll_y,
            overlay: self.lane_overlay.clone(),
            marker_drag: self.ruler_overlay.marker.clone(),
            playhead_bar: app.position / s.beats_per_bar(),
            peaks,
            agent_tracks,
            theme: Theme::get(cx).clone(),
            session: s,
        };
        let bounds = self.areas.lanes.clone();
        let reported = self.reported_width.clone();
        let daw = self.daw.clone();
        canvas(
            move |b, _, cx| {
                bounds.set(b);
                let width = f32::from(b.size.width).round() as f64;
                if width >= 50.0 && width != reported.get() {
                    reported.set(width);
                    cx.defer(move |cx| {
                        daw.update(cx, |daw, cx| {
                            daw.run("view.set", json!({ "laneWidth": width }), cx);
                        })
                    });
                }
            },
            move |b, _, window, cx| paint::paint_lanes(b, scene, window, cx),
        )
        .absolute()
        .size_full()
    }

    fn lanes(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let canvas = self.lanes_canvas(cx);
        let s = self.session(cx);
        let geo = self.geo(cx);
        let mut el = div()
            .id("lanes")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .cursor(cursor_style(self.lane_cursor))
            .child(canvas)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::lane_down))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, e: &MouseDownEvent, window, cx| this.lane_menu(e, window, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, e: &MouseUpEvent, _, cx| {
                    this.drag_end(e.position, e.modifiers, cx)
                }),
            )
            .on_mouse_move(cx.listener(Self::lane_hover))
            .on_drag_move::<ExternalPaths>(cx.listener(Self::file_over))
            .on_drop::<ExternalPaths>(cx.listener(Self::file_drop))
            .on_drag_move::<BrowserDrag>(cx.listener(Self::browser_over))
            .on_drop::<BrowserDrag>(cx.listener(Self::browser_drop));

        if let Some(Editing::Clip(id)) = &self.editing {
            if let Some(clip) = s.clips.iter().find(|c| &c.id == id) {
                let row = s
                    .tracks
                    .iter()
                    .position(|t| t.id == clip.track_id)
                    .unwrap_or(0);
                el = el.child(self.edit_field(
                    geo.x(clip.start_bar) + 2.0,
                    row as f64 * geo.row + arrange::CLIP_INSET as f64 - 2.0 - self.scroll_y,
                    (clip.length_bars * geo.ppb - 4.0).max(100.0),
                    cx,
                ));
            }
        }
        el = el.children(self.agent_chips(&s, &geo, cx));
        if let Some(row) = self
            .lane_overlay
            .drop_track
            .filter(|_| cx.has_active_drag())
        {
            let target = s
                .tracks
                .get(row)
                .filter(|t| t.kind == "audio")
                .map_or("Import onto a new audio track".to_string(), |t| {
                    format!("Import onto {}", t.name)
                });
            el = el.child(
                div()
                    .absolute()
                    .top(px(8.0))
                    .left(px(8.0))
                    .px(px(10.0))
                    .py(px(4.0))
                    .rounded(px(radius::MD))
                    .bg(theme.glass(2))
                    .border_1()
                    .border_color(theme.glass_edge)
                    .text_size(px(size::SM))
                    .child(target),
            );
        }
        if s.tracks.is_empty() {
            el = el.child(self.empty_card(window, cx));
        }
        el.into_any_element()
    }

    /// A frosted chip over each lane the agent is editing, at the end of its clip.
    fn agent_chips(&self, s: &Session, geo: &Geo, cx: &gpui::App) -> Vec<AnyElement> {
        let app = &self.daw.read(cx).app;
        if !app.agents.runtime.running() {
            return vec![];
        }
        let theme = Theme::get(cx);
        let status = app.agents.runtime.status.clone();
        s.tracks
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                let clip = s
                    .clips
                    .iter()
                    .rev()
                    .find(|c| c.agent && c.track_id == t.id)?;
                let x = geo.x(clip.start_bar + clip.length_bars) + 6.0;
                let y = (i as f64 * geo.row - self.scroll_y + 4.0).max(2.0);
                Some(
                    div()
                        .absolute()
                        .left(px(x as f32))
                        .top(px(y as f32))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .px(px(8.0))
                        .py(px(3.0))
                        .rounded(px(radius::MD))
                        .bg(theme.glass(2))
                        .border_1()
                        .border_color(theme.accent_ring)
                        .text_size(px(size::XS))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .whitespace_nowrap()
                        .child(widgets::dot(theme.accent, 6.0))
                        .child(if status.is_empty() {
                            "Agent".to_string()
                        } else {
                            format!("Agent · {status}")
                        })
                        .into_any_element(),
                )
            })
            .collect()
    }

    /// An empty song: say how to start.
    fn empty_card(&self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let act = |id: &'static str| {
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut gpui::App| {
                window.dispatch_action(Box::new(super::actions::Do { id }), cx)
            }
        };
        let daw = self.daw.clone();
        div()
            .absolute()
            .top(px(28.0))
            .left(px(24.0))
            .right(px(24.0))
            .max_w(px(480.0))
            .p(px(24.0))
            .rounded(px(radius::LG))
            .bg(theme.bg_raised)
            .border_1()
            .border_color(theme.line)
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .text_size(px(size::LG))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Your next track starts here"),
            )
            .child(div().text_color(theme.text_2).line_height(px(20.0)).child(
                "Add an instrument and draw a region, import a recording, or explore the demo.",
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.0))
                    .child(
                        Button::new("empty-instrument", "Add an instrument")
                            .on_click(act("addMidiTrack")),
                    )
                    .child(
                        Button::new("empty-import", "Import audio…").on_click(act("importAudio")),
                    )
                    .child(Button::new("empty-demo", "Open demo").on_click(act("openDemo"))),
            )
            .child(
                div().flex().child(
                    Button::new("empty-agent", "Make something with an agent →")
                        .ghost()
                        .on_click(move |_, _, cx| {
                            daw.update(cx, |daw, cx| {
                                daw.run(
                                    "ui.showPanel",
                                    json!({"panel": "agent", "visible": true}),
                                    cx,
                                );
                            })
                        }),
                ),
            )
            .into_any_element()
    }

    fn lane_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_open() || self.drag.is_some() {
            return;
        }
        if self.editing.is_some() {
            self.commit_edit(cx);
        }
        let s = self.session(cx);
        let geo = self.geo(cx);
        let (x, y) = local(self.areas.lanes.get(), e.position);
        let y = y + self.scroll_y;
        if e.click_count >= 2 {
            // A double click opens a MIDI region in the editor.
            if let Some(clip) = geometry::clip_at(&s, &geo, x, y) {
                if matches!(clip.data, ryolune_engine::model::ClipData::Midi { .. }) {
                    let id = clip.id.clone();
                    self.cancel_drag(cx);
                    self.run(vec![("view.set", json!({ "editorClipId": id }))], cx);
                }
            }
            return;
        }
        let (drag, calls) = gestures::lane_press(&s, &geo, self.tool(cx), x, y, e.modifiers.alt);
        self.run(calls, cx);
        if let Some(drag) = drag {
            self.start_drag(Drag::Lane(drag), cx);
        }
        let _ = window;
    }

    fn lane_hover(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.drag.is_some() {
            return;
        }
        let s = self.session(cx);
        let geo = self.geo(cx);
        let (x, y) = local(self.areas.lanes.get(), e.position);
        let (cursor, split) = gestures::lane_hover(
            &s,
            &geo,
            self.tool(cx),
            x,
            y + self.scroll_y,
            e.modifiers.alt,
        );
        if cursor != self.lane_cursor || split != self.lane_overlay.split {
            self.lane_cursor = cursor;
            self.lane_overlay.split = split;
            cx.notify();
        }
    }

    /// Files dragged over the lanes: light the lane they would land on.
    fn file_over(
        &mut self,
        e: &gpui::DragMoveEvent<ExternalPaths>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bounds = self.areas.lanes.get();
        let row = if bounds.contains(&e.event.position) {
            let (_, y) = local(bounds, e.event.position);
            let s = self.session(cx);
            let geo = self.geo(cx);
            // Past the last track, a new one is made.
            Some(
                geo.row_at(y + self.scroll_y, s.tracks.len())
                    .unwrap_or(s.tracks.len()),
            )
        } else {
            None
        };
        if row != self.lane_overlay.drop_track {
            self.lane_overlay.drop_track = row;
            cx.notify();
        }
    }

    /// A browser row dragged over the lanes: light the lane and remember the bar it would
    /// land on.
    fn browser_over(
        &mut self,
        e: &gpui::DragMoveEvent<BrowserDrag>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bounds = self.areas.lanes.get();
        let target = if bounds.contains(&e.event.position) {
            let (x, y) = local(bounds, e.event.position);
            let s = self.session(cx);
            let geo = self.geo(cx);
            let row = geo
                .row_at(y + self.scroll_y, s.tracks.len())
                .unwrap_or(s.tracks.len());
            Some((row, geometry::snap(&s, geo.bar(x).max(0.0), false)))
        } else {
            None
        };
        self.browser_target = target;
        let row = target.map(|(row, _)| row);
        if row != self.lane_overlay.drop_track {
            self.lane_overlay.drop_track = row;
            cx.notify();
        }
    }

    /// A browser row dropped on a lane: an instrument or an effect goes to that track, a loop
    /// or audio lands there at the bar under the pointer (one undo step).
    fn browser_drop(&mut self, item: &BrowserDrag, _: &mut Window, cx: &mut Context<Self>) {
        self.lane_overlay.drop_track = None;
        let Some((row, bar)) = self.browser_target.take() else {
            return;
        };
        let track = self.session(cx).tracks.get(row).map(|t| t.id.clone());
        let item = item.clone();
        self.daw.update(cx, |daw, cx| {
            item.apply(daw, track.as_deref(), Some(bar), cx);
        });
        cx.notify();
    }

    /// Files dropped on an audio lane import onto that track; anywhere else they import as
    /// a window drop does (a new audio track, MIDI on new instrument tracks).
    fn file_drop(&mut self, paths: &ExternalPaths, _: &mut Window, cx: &mut Context<Self>) {
        let row = self.lane_overlay.drop_track.take();
        let s = self.session(cx);
        if let Some(track) = row
            .and_then(|i| s.tracks.get(i))
            .filter(|t| t.kind == "audio")
        {
            self.run(vec![("track.select", json!({"trackId": track.id}))], cx);
        }
        let paths = paths.paths().to_vec();
        self.daw.update(cx, |daw, cx| {
            if let Err(error) = daw.app.drop_files(paths) {
                daw.app.error = Some(error);
            }
            cx.notify();
        });
    }

    fn ruler(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let app = &self.daw.read(cx).app;
        let scene = paint::RulerScene {
            session: app.store.snapshot(),
            geo: Geo::new(app.zoom, app.scroll),
            overlay: self.ruler_overlay.clone(),
            playhead_beats: app.position,
            theme: Theme::get(cx).clone(),
            widths: self.flag_widths.clone(),
        };
        let bounds = self.areas.ruler.clone();
        let mut el = div()
            .id("ruler")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .cursor(cursor_style(self.ruler_cursor))
            .child(
                canvas(move |b, _, _| bounds.set(b), move |b, _, window, cx| {
                    paint::paint_ruler(b, scene, window, cx)
                })
                .absolute()
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(Self::ruler_down))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, e: &MouseDownEvent, window, cx| {
                this.ruler_menu(e, window, cx)
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, e: &MouseUpEvent, _, cx| {
                this.drag_end(e.position, e.modifiers, cx)
            }))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if this.drag.is_some() {
                    return;
                }
                let s = this.session(cx);
                let geo = this.geo(cx);
                let (x, y) = local(this.areas.ruler.get(), e.position);
                let cursor = gestures::ruler_hover(&s, &geo, x, y, &this.flag_widths.borrow());
                if cursor != this.ruler_cursor {
                    this.ruler_cursor = cursor;
                    cx.notify();
                }
            }))
            .tooltip(|_, cx| {
                widgets::tip(
                    "Click to locate · drag to set the cycle range · drag a marker to move it, double-click to rename".into(),
                    cx,
                )
            });
        if let Some(Editing::Marker(id)) = &self.editing {
            let s = self.session(cx);
            if let Some(m) = s.markers.iter().find(|m| &m.id == id) {
                let width = self.flag_widths.borrow().get(id).copied().unwrap_or(48.0) as f64;
                el = el.child(self.edit_field(
                    self.geo(cx).x(m.bar).round() + geometry::FLAG_STRIPE,
                    layout::RULER as f64 - arrange::INLINE_INPUT_H as f64 - 1.0,
                    (width + 24.0).max(110.0),
                    cx,
                ));
            }
        }
        el.into_any_element()
    }

    fn ruler_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_open() || self.drag.is_some() {
            return;
        }
        let s = self.session(cx);
        let geo = self.geo(cx);
        let (x, y) = local(self.areas.ruler.get(), e.position);
        if e.click_count >= 2 {
            let marker = geometry::marker_at(&s, &geo, x, y, &self.flag_widths.borrow());
            if let Some(m) = marker {
                self.edit(Editing::Marker(m.id), m.name, window, cx);
            }
            return;
        }
        let drag = gestures::ruler_press(&s, &geo, x, y, &self.flag_widths.borrow());
        self.start_drag(Drag::Ruler(drag), cx);
    }

    fn tempo_lane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let app = &self.daw.read(cx).app;
        let s = app.store.snapshot();
        let scene = paint::TempoScene {
            geo: Geo::new(app.zoom, app.scroll),
            drag: self.tempo_overlay,
            playhead_bar: app.position / s.beats_per_bar(),
            theme: Theme::get(cx).clone(),
            session: s.clone(),
        };
        let bounds = self.areas.tempo.clone();
        let mut el = div()
            .id("tempo-lane")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .cursor(cursor_style(self.tempo_cursor))
            .child(
                canvas(move |b, _, _| bounds.set(b), move |b, _, window, cx| {
                    paint::paint_tempo(b, scene, window, cx)
                })
                .absolute()
                .size_full(),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(Self::tempo_down))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, e: &MouseDownEvent, window, cx| {
                this.tempo_menu(e, window, cx)
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, e: &MouseUpEvent, _, cx| {
                this.drag_end(e.position, e.modifiers, cx)
            }))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if this.drag.is_some() {
                    return;
                }
                let s = this.session(cx);
                let geo = this.geo(cx);
                let b = this.areas.tempo.get();
                let (x, y) = local(b, e.position);
                let cursor = gestures::tempo_hover(&s, &geo, x, y, f32::from(b.size.height) as f64);
                if cursor != this.tempo_cursor {
                    this.tempo_cursor = cursor;
                    cx.notify();
                }
            }))
            .tooltip(|_, cx| {
                widgets::tip(
                    "Click to add a tempo change · drag a point to move it or change its tempo (Shift for tenths, Option off the grid) · double-click to type · right-click to ramp".into(),
                    cx,
                )
            });
        if let Some(Editing::Tempo(bar)) = self.editing {
            let points = geometry::tempo_points(&s, None);
            if let Some(p) = points
                .iter()
                .find(|p| (p.bar - bar).abs() < geometry::SAME_BAR)
            {
                let h = arrange::TEMPO_LANE as f64;
                let field_h = arrange::INLINE_INPUT_H as f64;
                let y =
                    geometry::y_of_bpm(p.bpm, geometry::tempo_range(&points), h) - field_h / 2.0;
                el = el.child(self.edit_field(
                    (self.geo(cx).x(p.bar) + 4.0).max(2.0),
                    y.clamp(0.0, h - field_h),
                    72.0,
                    cx,
                ));
            }
        }
        el.into_any_element()
    }

    fn tempo_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_open() || self.drag.is_some() {
            return;
        }
        let s = self.session(cx);
        let geo = self.geo(cx);
        let b = self.areas.tempo.get();
        let (x, y) = local(b, e.position);
        let h = f32::from(b.size.height) as f64;
        if e.click_count >= 2 {
            if let Some(bar) = geometry::tempo_at(&s, &geo, x, y, h) {
                self.edit_tempo(bar, window, cx);
            }
            return;
        }
        let gesture = gestures::tempo_press(&s, &geo, x, y, h);
        self.start_drag(Drag::Tempo(gesture), cx);
    }

    fn edit_tempo(&mut self, bar: f64, window: &mut Window, cx: &mut Context<Self>) {
        let s = self.session(cx);
        let bpm = geometry::tempo_points(&s, None)
            .iter()
            .find(|p| (p.bar - bar).abs() < geometry::SAME_BAR)
            .map_or(s.transport.tempo, |p| p.bpm);
        self.edit(Editing::Tempo(bar), geometry::bpm_label(bpm), window, cx);
    }

    /// The corner left of the ruler, or of the tempo track.
    fn corner(&self, cx: &gpui::App) -> gpui::Div {
        let theme = Theme::get(cx);
        div()
            .w(px(layout::TRACK_HEADER))
            .flex_none()
            .h_full()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(10.0))
            .bg(theme.glass(1))
            .border_r_1()
            .border_color(theme.line)
    }
}

pub(crate) fn cursor_style(cursor: Cursor) -> CursorStyle {
    match cursor {
        Cursor::Default => CursorStyle::Arrow,
        Cursor::ResizeX => CursorStyle::ResizeLeftRight,
        Cursor::Crosshair => CursorStyle::Crosshair,
        Cursor::Split => CursorStyle::ResizeColumn,
        Cursor::Grab => CursorStyle::OpenHand,
        Cursor::Grabbing => CursorStyle::ClosedHand,
        Cursor::Copy => CursorStyle::DragCopy,
    }
}

impl Render for Arrangement {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let (show_tempo, armed, tempo_now) = {
            let app = &self.daw.read(cx).app;
            let s = app.store.session();
            (
                app.show_tempo,
                s.tracks.iter().any(|t| t.armed && t.kind == "audio"),
                s.tempo_map().bpm(app.position),
            )
        };
        if !cx.has_active_drag() {
            self.lane_overlay.drop_track = None;
        }
        self.input_meter(armed, cx);
        self.clamp_scroll(cx);

        let toolbar = self.toolbar(window, cx);
        let ruler = self.ruler(cx);
        let tempo = show_tempo.then(|| self.tempo_lane(cx));
        let headers = self.headers(window, cx);
        let lanes = self.lanes(window, cx);
        let menu = self.menu.render(window, cx);

        // Global pointer listeners while a drag runs, so it follows the pointer anywhere in
        // the window and ends wherever the button comes up.
        let dragging = self.drag.is_some();
        let this = cx.entity().downgrade();
        let listeners = canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                if !dragging {
                    return;
                }
                let moved = this.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        let pressed = e.pressed_button == Some(MouseButton::Left);
                        let _ = moved.update(cx, |this, cx| {
                            this.drag_move(e.position, e.modifiers, pressed, cx)
                        });
                    }
                });
                let released = this.clone();
                window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture && e.button == MouseButton::Left {
                        let _ = released
                            .update(cx, |this, cx| this.drag_end(e.position, e.modifiers, cx));
                    }
                });
            },
        )
        .absolute()
        .size_full();

        let corner_add = Button::icon("add-track", "plus")
            .compact()
            .icon_size(10.0)
            .tooltip("Add track")
            .on_click(cx.listener(|this, e: &gpui::ClickEvent, window, cx| {
                this.add_track_menu(e.position(), window, cx)
            }));
        let ruler_row = div()
            .h(px(layout::RULER))
            .flex_none()
            .flex()
            .child(
                self.corner(cx)
                    .child(corner_add)
                    .child(widgets::caps("Tracks", cx)),
            )
            .child(ruler);
        let tempo_row = tempo.map(|lane| {
            div()
                .h(px(arrange::TEMPO_LANE))
                .flex_none()
                .flex()
                .border_t_1()
                .border_color(theme.line)
                .child(
                    self.corner(cx).child(widgets::caps("Tempo", cx)).child(
                        div()
                            .ml_auto()
                            .font_family(super::theme::FONT_MONO)
                            .text_size(px(size::SM))
                            .text_color(theme.text_2)
                            .child(format!("{} BPM", geometry::bpm_label(tempo_now))),
                    ),
                )
                .child(lane)
        });

        div()
            .id("arrangement")
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme.lane_empty)
            .child(toolbar)
            .child(
                div()
                    .id("arrangement-body")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .on_scroll_wheel(cx.listener(Self::wheel))
                    .child(ruler_row)
                    .children(tempo_row)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .border_t_1()
                            .border_color(theme.line)
                            .child(headers)
                            .child(lanes),
                    ),
            )
            .child(listeners)
            .children(menu)
    }
}
