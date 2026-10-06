//! The automation window: read automation lanes and their breakpoints. A lane picker, the
//! lane's read switch and interpolation, and a graph where a double click adds a point, a
//! drag moves one (¼-beat snap, Shift for free), and a right click or Delete removes it. A
//! row below makes new lanes for a track's volume or pan, the Stereo Out, or any plugin
//! parameter. Every edit is an `automation.*` registry command, so undo, the CLI and the
//! agent see the same lanes; a point's drag is previewed here and committed on release as
//! one step. Glass tier 2, dragged by its header, closed with `ui.showPanel`.

use super::{
    actions::Do,
    daw::Daw,
    format,
    plugin_panel::display::{label, stroke},
    theme::{radius, size, with_alpha, Theme, FONT_MONO},
    widgets::{
        self, caps, field, Button, InputEvent, MenuHost, MenuItem, Segmented, Switch, TextInput,
    },
};
use gpui::{
    canvas, div, point, prelude::*, px, AnyElement, App, Bounds, Context, ElementId, Empty, Entity,
    FocusHandle, Global, Hsla, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point,
    ScrollWheelEvent, SharedString, Subscription, Window,
};
use ryolune_engine::{
    automation::{AutomationLane, AutomationPoint, AutomationTarget, Interpolation},
    model::{bus_name, Session, Strip, BUS_A, BUS_B, MASTER},
};
use serde_json::{json, Value};
use std::{cell::Cell, rc::Rc};

/// A lane the window should show next: set by a plugin window's Automate key.
#[derive(Default)]
pub struct FocusLane(pub Option<String>);
impl Global for FocusLane {}

const PANEL_W: f32 = 780.0;
const GRAPH_H: f32 = 260.0;
/// The graph's margins inside its well: value labels at the left, beats below.
const LEFT: f32 = 56.0;
const RIGHT: f32 = 14.0;
const TOP: f32 = 14.0;
const BOTTOM: f32 = 22.0;
/// How close the pointer must come to a point to take it.
const HIT: f32 = 7.0;
/// Points snap to quarter beats unless Shift is held.
const SNAP: f64 = 0.25;

/// The graph's mapping between beats and values and the pixels inside its well.
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    /// The plotting area, inside the margins.
    pub area: Bounds<Pixels>,
    pub start: f64,
    pub length: f64,
    pub min: f64,
    pub max: f64,
}

impl Geometry {
    pub fn new(well: Bounds<Pixels>, start: f64, length: f64, min: f64, max: f64) -> Self {
        let area = Bounds::new(
            point(well.origin.x + px(LEFT), well.origin.y + px(TOP)),
            gpui::size(
                (well.size.width - px(LEFT + RIGHT)).max(px(1.0)),
                (well.size.height - px(TOP + BOTTOM)).max(px(1.0)),
            ),
        );
        Self {
            area,
            start,
            length: length.max(1e-6),
            min,
            max,
        }
    }
    pub fn x(&self, beat: f64) -> Pixels {
        self.area.origin.x
            + px(((beat - self.start) / self.length) as f32 * f32::from(self.area.size.width))
    }
    pub fn y(&self, value: f64) -> Pixels {
        let span = (self.max - self.min).max(1e-12);
        self.area.bottom()
            - px(((value - self.min) / span) as f32 * f32::from(self.area.size.height))
    }
    pub fn at(&self, beat: f64, value: f64) -> Point<Pixels> {
        point(self.x(beat), self.y(value))
    }
    /// The beat and value under a pointer, held inside the visible range; beats snapped
    /// to quarter beats when `snap`.
    pub fn locate(&self, p: Point<Pixels>, snap: bool) -> (f64, f64) {
        let fx =
            (f32::from(p.x - self.area.origin.x) / f32::from(self.area.size.width)).clamp(0.0, 1.0);
        let fy = (f32::from(self.area.bottom() - p.y) / f32::from(self.area.size.height))
            .clamp(0.0, 1.0);
        let beat = (self.start + fx as f64 * self.length).max(0.0);
        let beat = if snap {
            (beat / SNAP).round() * SNAP
        } else {
            beat
        };
        (beat, self.min + fy as f64 * (self.max - self.min))
    }
    /// The point under the pointer, nearest first.
    pub fn hit<'a>(
        &self,
        points: &'a [AutomationPoint],
        p: Point<Pixels>,
    ) -> Option<&'a AutomationPoint> {
        points
            .iter()
            .map(|q| {
                let at = self.at(q.beat, q.value);
                let d = f32::from(at.x - p.x).hypot(f32::from(at.y - p.y));
                (d, q)
            })
            .filter(|(d, _)| *d <= HIT)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, q)| q)
    }
}

/// Where a dragged point may go: after its left neighbour, before its right one.
pub fn neighbours(points: &[AutomationPoint], id: &str) -> (f64, f64) {
    let mut sorted: Vec<&AutomationPoint> = points.iter().collect();
    sorted.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    let Some(i) = sorted.iter().position(|p| p.id == id) else {
        return (0.0, f64::MAX);
    };
    let low = i.checked_sub(1).map_or(0.0, |j| sorted[j].beat + 1e-6);
    let high = sorted.get(i + 1).map_or(f64::MAX, |p| p.beat - 1e-6);
    (low, high.max(low))
}

/// A lane's value as people read it: decibels for a volume, L/R for a pan.
pub fn value_text(lane: &AutomationLane, value: f64) -> String {
    match &lane.target {
        AutomationTarget::TrackVolume { .. } | AutomationTarget::MasterVolume => {
            format!("{} dB", format::db(format::fader_to_db(value as f32), 1))
        }
        AutomationTarget::TrackPan { .. } => format::pan((value / 100.0) as f32),
        _ => {
            let digits = if (lane.max - lane.min).abs() >= 100.0 {
                0
            } else {
                2
            };
            format!("{value:.digits$}")
        }
    }
}

/// What a new lane automates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Volume,
    Pan,
    Plugin,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Volume => "Volume",
            Kind::Pan => "Pan",
            Kind::Plugin => "Plugin parameter",
        }
    }
    /// What a strip can automate: the Stereo Out has no pan, the A and B returns only
    /// their plugins.
    fn for_strip(strip: &str) -> &'static [Kind] {
        match strip {
            MASTER => &[Kind::Volume, Kind::Plugin],
            BUS_A | BUS_B => &[Kind::Plugin],
            _ => &[Kind::Volume, Kind::Pan, Kind::Plugin],
        }
    }
}

/// A plugin on a strip that a lane can automate: its rack key, its label and slot.
fn plugin_choices(session: &Session, strip_id: &str) -> Vec<(String, String, Option<usize>)> {
    let strip = session
        .strips
        .get(strip_id)
        .cloned()
        .unwrap_or_else(Strip::default);
    let mut choices: Vec<_> = strip
        .inserts
        .iter()
        .enumerate()
        .filter(|(_, i)| !i.is_empty())
        .map(|(slot, i)| {
            (
                i.id.clone(),
                format!("{} · {}", slot + 1, i.name),
                Some(slot),
            )
        })
        .collect();
    if session
        .tracks
        .iter()
        .any(|t| t.id == strip_id && t.kind == "midi")
    {
        choices.insert(
            0,
            (
                strip.synth_key(strip_id),
                format!("Instrument · {}", strip.instrument_name()),
                None,
            ),
        );
    }
    choices
}

fn strip_name(session: &Session, id: &str) -> String {
    session
        .tracks
        .iter()
        .find(|t| t.id == id)
        .map_or_else(|| bus_name(id).to_string(), |t| t.name.clone())
}

/// A point in a drag: where it is now, kept here until release.
#[derive(Clone, Debug)]
struct PointDrag {
    lane: String,
    point: String,
    beat: f64,
    value: f64,
    origin: Point<Pixels>,
    moved: bool,
}

#[derive(Clone)]
struct GraphDrag;
#[derive(Clone)]
struct HeaderDrag;

/// The parameters of the plugin a new lane would automate, read once per document change.
struct ParamCache {
    key: String,
    revision: u64,
    params: Vec<(u32, String)>,
}

pub struct Automation {
    daw: Entity<Daw>,
    focus: FocusHandle,
    origin: Point<Pixels>,
    moving: Option<(Point<Pixels>, Point<Pixels>)>,
    lane: Option<String>,
    point: Option<String>,
    /// The visible beats; `None` follows the song.
    view: Option<(f64, f64)>,
    snap: bool,
    drag: Option<PointDrag>,
    /// The graph's well, as last laid out, for hit testing.
    well: Rc<Cell<Option<Bounds<Pixels>>>>,
    // The new lane.
    strip: Option<String>,
    kind: Kind,
    plugin: Option<String>,
    parameter: Option<u32>,
    params: Option<ParamCache>,
    param_filter: Entity<TextInput>,
    // The selected point's fields.
    beat_input: Entity<TextInput>,
    value_input: Entity<TextInput>,
    shown_point: Option<(String, f64, f64)>,
    menu: MenuHost,
    _subscriptions: Vec<Subscription>,
}

impl Automation {
    pub fn new(daw: Entity<Daw>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let param_filter = cx.new(|cx| TextInput::new(cx).placeholder("Find parameter"));
        let beat_input = cx.new(|cx| TextInput::new(cx).mono());
        let value_input = cx.new(|cx| TextInput::new(cx).mono());
        let subscriptions = vec![
            cx.subscribe_in(&param_filter, window, |_, _, _: &InputEvent, _, cx| {
                cx.notify()
            }),
            cx.subscribe_in(&beat_input, window, |this, input, e: &InputEvent, _, cx| {
                if matches!(e, InputEvent::Submit | InputEvent::Blur) {
                    let text = input.read(cx).text().to_string();
                    this.commit_field(true, &text, cx);
                }
            }),
            cx.subscribe_in(
                &value_input,
                window,
                |this, input, e: &InputEvent, _, cx| {
                    if matches!(e, InputEvent::Submit | InputEvent::Blur) {
                        let text = input.read(cx).text().to_string();
                        this.commit_field(false, &text, cx);
                    }
                },
            ),
        ];
        Self {
            daw,
            focus: cx.focus_handle(),
            origin: point(px(260.0), px(150.0)),
            moving: None,
            lane: None,
            point: None,
            view: None,
            snap: true,
            drag: None,
            well: Rc::new(Cell::new(None)),
            strip: None,
            kind: Kind::Volume,
            plugin: None,
            parameter: None,
            params: None,
            param_filter,
            beat_input,
            value_input,
            shown_point: None,
            menu: MenuHost::default(),
            _subscriptions: subscriptions,
        }
    }

    fn run(&mut self, method: &str, params: Value, cx: &mut Context<Self>) -> Option<Value> {
        self.daw.update(cx, |daw, cx| daw.run(method, params, cx))
    }

    /// The selected lane, falling back to the first; drops a selection that is gone.
    fn current(&mut self, cx: &mut Context<Self>) -> Option<AutomationLane> {
        if let Some(lane) = cx.try_global::<FocusLane>().and_then(|f| f.0.clone()) {
            cx.set_global(FocusLane(None));
            self.lane = Some(lane);
            self.point = None;
        }
        let session = self.daw.read(cx).app.store.session();
        let lane = session
            .automation
            .iter()
            .find(|l| Some(&l.id) == self.lane.as_ref())
            .or_else(|| session.automation.first())
            .cloned();
        if lane.as_ref().map(|l| &l.id) != self.lane.as_ref() {
            self.lane = lane.as_ref().map(|l| l.id.clone());
            self.point = None;
        }
        if let (Some(lane), Some(point)) = (&lane, &self.point) {
            if !lane.points.iter().any(|p| &p.id == point) {
                self.point = None;
            }
        }
        lane
    }

    /// The visible beats: the user's, or the song's length (at least 32 beats).
    fn visible(&self, cx: &App) -> (f64, f64) {
        self.view.unwrap_or_else(|| {
            let s = self.daw.read(cx).app.store.session();
            (0.0, (s.end_bar() * s.beats_per_bar()).max(32.0))
        })
    }

    fn geometry(&self, lane: &AutomationLane, cx: &App) -> Option<Geometry> {
        let (start, length) = self.visible(cx);
        self.well
            .get()
            .map(|well| Geometry::new(well, start, length, lane.min, lane.max))
    }

    /// A press in the graph: take a point, add one on a double click, or let go of the
    /// selection.
    fn graph_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        let Some(lane) = self.current(cx) else {
            return;
        };
        let Some(g) = self.geometry(&lane, cx) else {
            return;
        };
        if let Some(p) = g.hit(&lane.points, e.position) {
            self.point = Some(p.id.clone());
            self.drag = Some(PointDrag {
                lane: lane.id.clone(),
                point: p.id.clone(),
                beat: p.beat,
                value: p.value,
                origin: e.position,
                moved: false,
            });
        } else if e.click_count >= 2 && g.area.contains(&e.position) {
            let (beat, value) = g.locate(e.position, self.snap && !e.modifiers.shift);
            self.add_point(&lane, beat, value, cx);
        } else {
            self.point = None;
        }
        cx.notify();
    }

    fn add_point(&mut self, lane: &AutomationLane, beat: f64, value: f64, cx: &mut Context<Self>) {
        let before: Vec<String> = lane.points.iter().map(|p| p.id.clone()).collect();
        // A point already on that beat takes the new value.
        let existing = lane.points.iter().find(|p| (p.beat - beat).abs() < 1e-8);
        let mut params = json!({ "laneId": lane.id, "beat": beat, "value": value });
        if let Some(p) = existing {
            params["pointId"] = json!(p.id);
        }
        if let Some(result) = self.run("automation.setPoint", params, cx) {
            self.point = existing.map(|p| p.id.clone()).or_else(|| {
                result["lane"]["points"]
                    .as_array()?
                    .iter()
                    .filter_map(|p| p["id"].as_str())
                    .find(|id| !before.iter().any(|b| b == id))
                    .map(str::to_string)
            });
        }
    }

    fn graph_move(&mut self, position: Point<Pixels>, shift: bool, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.clone() else {
            return;
        };
        let Some(lane) = self.current(cx).filter(|l| l.id == drag.lane) else {
            self.drag = None;
            return;
        };
        let Some(g) = self.geometry(&lane, cx) else {
            return;
        };
        if !drag.moved
            && f32::from(position.x - drag.origin.x).hypot(f32::from(position.y - drag.origin.y))
                < 2.0
        {
            return;
        }
        let (beat, value) = g.locate(position, self.snap && !shift);
        let (low, high) = neighbours(&lane.points, &drag.point);
        self.drag = Some(PointDrag {
            beat: beat.clamp(low, high),
            value: value.clamp(lane.min.min(lane.max), lane.max.max(lane.min)),
            moved: true,
            ..drag
        });
        cx.notify();
    }

    /// Release: a moved point is one `automation.setPoint`, one undo step.
    fn graph_up(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if drag.moved {
            self.run(
                "automation.setPoint",
                json!({ "laneId": drag.lane, "pointId": drag.point, "beat": drag.beat, "value": drag.value }),
                cx,
            );
        }
        cx.notify();
    }

    fn remove_point(&mut self, lane: &str, point: &str, cx: &mut Context<Self>) {
        self.run(
            "automation.removePoint",
            json!({ "laneId": lane, "pointId": point }),
            cx,
        );
        if self.point.as_deref() == Some(point) {
            self.point = None;
        }
        cx.notify();
    }

    fn graph_right(&mut self, e: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some(lane) = self.current(cx) else {
            return;
        };
        let Some(g) = self.geometry(&lane, cx) else {
            return;
        };
        if let Some(p) = g.hit(&lane.points, e.position) {
            let id = p.id.clone();
            self.remove_point(&lane.id, &id, cx);
        }
    }

    /// The wheel scrolls the visible beats; with ⌘ or Ctrl it zooms around the pointer.
    fn graph_wheel(&mut self, e: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let (start, length) = self.visible(cx);
        let delta = e.delta.pixel_delta(px(16.0));
        let width = self
            .well
            .get()
            .map_or(600.0, |w| f32::from(w.size.width) - LEFT - RIGHT)
            .max(1.0) as f64;
        if e.modifiers.platform || e.modifiers.control {
            let factor = (1.0 - f32::from(delta.y) as f64 / 200.0).clamp(0.5, 2.0);
            let anchor = self.well.get().map_or(0.5, |w| {
                ((f32::from(e.position.x - w.origin.x) - LEFT) as f64 / width).clamp(0.0, 1.0)
            });
            let next = (length * factor).clamp(1.0, 100_000.0);
            let at = start + anchor * length;
            self.view = Some(((at - anchor * next).max(0.0), next));
        } else {
            let shift = f32::from(if delta.x != px(0.0) {
                -delta.x
            } else {
                -delta.y
            }) as f64;
            self.view = Some(((start + shift / width * length).max(0.0), length));
        }
        cx.notify();
    }

    /// Delete removes the selected point while the window has focus; otherwise the
    /// window's own Delete runs.
    fn on_do(&mut self, action: &Do, _: &mut Window, cx: &mut Context<Self>) {
        if action.id == "deleteSelection" {
            if let (Some(lane), Some(point)) = (self.lane.clone(), self.point.clone()) {
                self.remove_point(&lane, &point, cx);
                return;
            }
        }
        cx.propagate();
    }

    fn point_at_playhead(&mut self, lane: &AutomationLane, cx: &mut Context<Self>) {
        let beat = self.daw.read(cx).app.position.max(0.0);
        let value = lane.value_at(beat).unwrap_or((lane.min + lane.max) * 0.5);
        self.add_point(lane, beat, value, cx);
    }

    fn commit_field(&mut self, beat_field: bool, text: &str, cx: &mut Context<Self>) {
        let Some(lane) = self.current(cx) else {
            return;
        };
        let Some(point) = self.point.clone() else {
            return;
        };
        let Some(p) = lane.points.iter().find(|p| p.id == point) else {
            return;
        };
        let Ok(number) = text.trim().parse::<f64>() else {
            self.shown_point = None;
            cx.notify();
            return;
        };
        if !number.is_finite() {
            return;
        }
        let (beat, value) = if beat_field {
            let (low, high) = neighbours(&lane.points, &point);
            (number.clamp(low, high), p.value)
        } else {
            (
                p.beat,
                number.clamp(lane.min.min(lane.max), lane.max.max(lane.min)),
            )
        };
        if beat != p.beat || value != p.value {
            self.run(
                "automation.setPoint",
                json!({ "laneId": lane.id, "pointId": point, "beat": beat, "value": value }),
                cx,
            );
        }
        self.shown_point = None;
        cx.notify();
    }

    /// The new lane's strip: the one chosen, else the selected track, else the Stereo Out.
    fn new_strip(&self, cx: &App) -> String {
        self.strip.clone().unwrap_or_else(|| {
            self.daw
                .read(cx)
                .app
                .store
                .session()
                .view
                .selected_track_id
                .clone()
                .unwrap_or_else(|| MASTER.into())
        })
    }

    fn new_kind(&self, strip: &str) -> Kind {
        let kinds = Kind::for_strip(strip);
        if kinds.contains(&self.kind) {
            self.kind
        } else {
            kinds[0]
        }
    }

    /// The parameters of the chosen plugin, read through `strip.parameters`.
    fn plugin_params(
        &mut self,
        strip: &str,
        choice: &(String, String, Option<usize>),
        cx: &mut Context<Self>,
    ) -> Vec<(u32, String)> {
        let revision = self.daw.read(cx).app.store.revision;
        if let Some(cache) = &self.params {
            if cache.key == choice.0 && cache.revision == revision {
                return cache.params.clone();
            }
        }
        let mut params = json!({ "trackId": strip, "limit": 10000 });
        if let Some(slot) = choice.2 {
            params["slot"] = json!(slot);
        }
        let list: Vec<(u32, String)> = self
            .daw
            .update(cx, |daw, _| {
                daw.app
                    .run_control_command("strip.parameters", &params, false, "Interface")
            })
            .ok()
            .and_then(|v| v["parameters"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter(|p| p["max"].as_f64() > p["min"].as_f64())
            .filter(|p| p["automatable"].as_bool().unwrap_or(true))
            .filter_map(|p| {
                Some((
                    u32::try_from(p["id"].as_u64()?).ok()?,
                    p["name"].as_str()?.to_string(),
                ))
            })
            .collect();
        self.params = Some(ParamCache {
            key: choice.0.clone(),
            revision,
            params: list.clone(),
        });
        list
    }

    fn create_lane(&mut self, cx: &mut Context<Self>) {
        let strip = self.new_strip(cx);
        let kind = self.new_kind(&strip);
        let mut params = match kind {
            Kind::Volume if strip == MASTER => json!({ "target": "masterVolume" }),
            Kind::Volume => json!({ "target": "trackVolume", "trackId": strip }),
            Kind::Pan => json!({ "target": "trackPan", "trackId": strip }),
            Kind::Plugin => {
                let session = self.daw.read(cx).app.store.session();
                let choices = plugin_choices(session, &strip);
                let Some(choice) = self
                    .plugin
                    .as_ref()
                    .and_then(|k| choices.iter().find(|c| &c.0 == k))
                    .or(choices.first())
                    .cloned()
                else {
                    return;
                };
                let Some(parameter) = self.parameter else {
                    return;
                };
                let mut p = json!({ "target": "pluginParameter", "trackId": strip, "parameterId": parameter });
                if let Some(slot) = choice.2 {
                    p["slot"] = json!(slot);
                }
                p
            }
        };
        if params.get("trackId").is_none() && strip != MASTER {
            params["trackId"] = json!(strip);
        }
        if let Some(result) = self.run("automation.create", params, cx) {
            self.lane = result["lane"]["id"].as_str().map(str::to_string);
            self.point = None;
        }
        cx.notify();
    }

    fn open_menu(
        &mut self,
        items: Vec<MenuItem>,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = point(e.position.x - px(8.0), e.position.y + px(14.0));
        self.menu.open(items, at, window, cx);
    }
}

fn id(what: &str) -> ElementId {
    ElementId::Name(SharedString::from(format!("automation/{what}")))
}

impl Automation {
    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let this = cx.entity().downgrade();
        div()
            .id(id("header"))
            .flex()
            .items_center()
            .justify_between()
            .px(px(14.0))
            .pt(px(12.0))
            .pb(px(10.0))
            .border_b_1()
            .border_color(theme.line)
            .cursor_grab()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, _| {
                    this.moving = Some((e.position, this.origin));
                }),
            )
            .on_drag(HeaderDrag, |_, _, _, cx| cx.new(|_| Empty))
            .on_drag_move::<HeaderDrag>(cx.listener(
                |this, e: &gpui::DragMoveEvent<HeaderDrag>, window, cx| {
                    let Some((from, origin)) = this.moving else {
                        return;
                    };
                    let view = window.viewport_size();
                    let to = origin + (e.event.position - from);
                    this.origin = point(
                        to.x.clamp(px(-(PANEL_W - 120.0)), view.width - px(120.0)),
                        to.y.clamp(px(0.0), view.height - px(48.0)),
                    );
                    cx.notify();
                },
            ))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(10.0))
                    .child(
                        div()
                            .text_size(px(size::LG))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme.text)
                            .child("Automation"),
                    )
                    .child(caps("Read · absolute beats", cx)),
            )
            .child(
                Button::icon(id("close"), "close")
                    .ghost()
                    .icon_size(10.0)
                    .tooltip("Close")
                    .on_click(move |_, _, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.run(
                                "ui.showPanel",
                                json!({ "panel": "automation", "visible": false }),
                                cx,
                            )
                        });
                    }),
            )
            .into_any_element()
    }

    /// The lane picker and the lane's own controls.
    fn lane_bar(&self, lane: Option<&AutomationLane>, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let this = cx.entity().downgrade();
        let session = self.daw.read(cx).app.store.session();
        let lanes: Vec<(String, String, Hsla)> = session
            .automation
            .iter()
            .map(|l| (l.id.clone(), l.name.clone(), lane_color(session, l, &theme)))
            .collect();
        let picked = lane.map(|l| l.id.clone());
        let picker = widgets::select_button(
            id("lane"),
            lane.map_or("No lanes yet".to_string(), |l| l.name.clone()),
            cx,
        )
        .w(px(280.0))
        .on_mouse_down(MouseButton::Left, {
            let this = this.clone();
            move |e: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                let items: Vec<MenuItem> = lanes
                    .iter()
                    .map(|(lane_id, name, color)| {
                        let (this, lane_id2) = (this.clone(), lane_id.clone());
                        MenuItem::new(name.clone(), move |_, cx| {
                            let _ = this.update(cx, |this, cx| {
                                this.lane = Some(lane_id2.clone());
                                this.point = None;
                                cx.notify();
                            });
                        })
                        .swatch(*color)
                        .checked(picked.as_ref() == Some(lane_id))
                    })
                    .collect();
                if items.is_empty() {
                    return;
                }
                let e = e.clone();
                let _ = this.update(cx, |this, cx| this.open_menu(items, &e, window, cx));
            }
        });
        let mut bar = div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(14.0))
            .py(px(10.0))
            .child(picker);
        if let Some(lane) = lane {
            let lane_id = lane.id.clone();
            let enabled = lane.enabled;
            bar = bar
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(Switch::new(id("read"), enabled).on_toggle({
                            let (this, lane_id) = (this.clone(), lane_id.clone());
                            move |on, _, cx| {
                                let params = json!({ "laneId": lane_id, "enabled": on });
                                let _ = this.update(cx, |this, cx| {
                                    this.run("automation.setEnabled", params, cx)
                                });
                            }
                        }))
                        .child(
                            div()
                                .text_size(px(size::BASE))
                                .text_color(theme.text_2)
                                .child("Read"),
                        ),
                )
                .child(
                    Segmented::new(
                        id("interpolation"),
                        ["Linear", "Step"],
                        usize::from(lane.interpolation == Interpolation::Step),
                    )
                    .on_select({
                        let (this, lane_id) = (this.clone(), lane_id.clone());
                        move |i, _, cx| {
                            let params = json!({ "laneId": lane_id,
                                "interpolation": if i == 1 { "step" } else { "linear" } });
                            let _ = this.update(cx, |this, cx| {
                                this.run("automation.setInterpolation", params, cx)
                            });
                        }
                    }),
                )
                .child(div().flex_1())
                .child(
                    Button::new(id("playhead"), "Point at playhead")
                        .compact()
                        .on_click({
                            let (this, lane) = (this.clone(), lane.clone());
                            move |_, _, cx| {
                                let _ =
                                    this.update(cx, |this, cx| this.point_at_playhead(&lane, cx));
                            }
                        }),
                )
                .child(
                    Button::icon(id("remove"), "trash")
                        .compact()
                        .danger()
                        .icon_size(11.0)
                        .tooltip("Delete lane")
                        .on_click({
                            let this = this.clone();
                            move |_, _, cx| {
                                let params = json!({ "laneId": lane_id });
                                let _ = this.update(cx, |this, cx| {
                                    this.run("automation.remove", params, cx);
                                    this.lane = None;
                                    cx.notify();
                                });
                            }
                        }),
                );
        }
        bar.into_any_element()
    }

    /// The breakpoint graph.
    fn graph(&self, lane: &AutomationLane, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let app = &self.daw.read(cx).app;
        let session = app.store.session();
        let color = lane_color(session, lane, &theme);
        let bpb = session.beats_per_bar();
        let playhead = app.position;
        let (start, length) = self.visible(cx);
        // The lane as it reads with the dragged point where the pointer is.
        let mut shown = lane.clone();
        if let Some(drag) = self.drag.as_ref().filter(|d| d.lane == lane.id && d.moved) {
            if let Some(p) = shown.points.iter_mut().find(|p| p.id == drag.point) {
                p.beat = drag.beat;
                p.value = drag.value;
            }
        }
        shown.points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        let selected = self.point.clone();
        let well = self.well.clone();
        div()
            .id(id("graph"))
            .mx(px(14.0))
            .h(px(GRAPH_H))
            .rounded(px(radius::MD))
            .overflow_hidden()
            .bg(theme.display)
            .border_1()
            .border_color(theme.hairline)
            .cursor_crosshair()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::graph_down))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.graph_right(e, cx)
                }),
            )
            .on_drag(GraphDrag, |_, _, _, cx| cx.new(|_| Empty))
            .on_drag_move::<GraphDrag>(cx.listener(
                |this, e: &gpui::DragMoveEvent<GraphDrag>, _, cx| {
                    this.graph_move(e.event.position, e.event.modifiers.shift, cx)
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.graph_up(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.graph_up(cx)),
            )
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                cx.stop_propagation();
                this.graph_wheel(e, cx)
            }))
            .child(
                canvas(
                    move |bounds, _, _| well.set(Some(bounds)),
                    move |bounds, _, window, cx| {
                        let g = Geometry::new(bounds, start, length, shown.min, shown.max);
                        paint_graph(
                            &shown,
                            &g,
                            bpb,
                            playhead,
                            color,
                            selected.as_deref(),
                            &theme,
                            window,
                            cx,
                        );
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }

    /// The selected point's beat and value, or the hint.
    fn point_bar(
        &mut self,
        lane: &AutomationLane,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let this = cx.entity().downgrade();
        let selected = self
            .point
            .as_ref()
            .and_then(|id| lane.points.iter().find(|p| &p.id == id))
            .cloned();
        let mut bar = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(14.0))
            .py(px(10.0))
            .text_size(px(size::SM))
            .text_color(theme.text_2);
        if let Some(p) = selected {
            // Refill the fields when the point changed underneath them (drag, undo).
            let beat_focused = self.beat_input.read(cx).is_focused(window);
            let value_focused = self.value_input.read(cx).is_focused(window);
            let fresh = Some((p.id.clone(), p.beat, p.value));
            if self.shown_point != fresh && !beat_focused && !value_focused {
                self.shown_point = fresh;
                self.beat_input
                    .update(cx, |i, cx| i.set_text(format!("{:.3}", p.beat), cx));
                self.value_input
                    .update(cx, |i, cx| i.set_text(format!("{:.3}", p.value), cx));
            }
            let bpb = self.daw.read(cx).app.store.session().beats_per_bar();
            bar = bar
                .child(caps("Point", cx))
                .child(div().child("Beat"))
                .child(field(&self.beat_input, beat_focused, cx).w(px(84.0)))
                .child(
                    div()
                        .font_family(FONT_MONO)
                        .text_color(theme.text_3)
                        .child(format::bar_beat_short(p.beat, bpb)),
                )
                .child(div().pl(px(8.0)).child("Value"))
                .child(field(&self.value_input, value_focused, cx).w(px(84.0)))
                .child(
                    div()
                        .font_family(FONT_MONO)
                        .text_color(theme.text_3)
                        .child(value_text(lane, p.value)),
                )
                .child(div().flex_1())
                .child(
                    Button::new(id("delete-point"), "Delete point")
                        .compact()
                        .on_click({
                            let (this, lane_id, point_id) =
                                (this.clone(), lane.id.clone(), p.id.clone());
                            move |_, _, cx| {
                                let _ = this.update(cx, |this, cx| {
                                    this.remove_point(&lane_id, &point_id, cx)
                                });
                            }
                        }),
                );
        } else {
            bar = bar
                .child(
                    div()
                        .text_color(theme.text_3)
                        .child("Double-click to add · drag to move · right-click or Delete removes · Shift drags off the grid"),
                )
                .child(div().flex_1());
        }
        let snap = self.snap;
        bar.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(Switch::new(id("snap"), snap).on_toggle({
                    let this = this.clone();
                    move |on, _, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.snap = on;
                            cx.notify();
                        });
                    }
                }))
                .child("Snap ¼"),
        )
        .child(
            Button::new(id("fit"), "Fit song")
                .compact()
                .ghost()
                .on_click(move |_, _, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.view = None;
                        cx.notify();
                    });
                }),
        )
        .into_any_element()
    }

    /// The row that makes a new lane.
    fn new_lane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let this = cx.entity().downgrade();
        let strip = self.new_strip(cx);
        let kind = self.new_kind(&strip);
        let (strips, choices, strip_label) = {
            let session = self.daw.read(cx).app.store.session();
            let mut strips: Vec<(String, String)> = [MASTER, BUS_A, BUS_B]
                .iter()
                .map(|id| (id.to_string(), bus_name(id).to_string()))
                .collect();
            strips.extend(
                session
                    .tracks
                    .iter()
                    .map(|t| (t.id.clone(), t.name.clone())),
            );
            (
                strips,
                plugin_choices(session, &strip),
                strip_name(session, &strip),
            )
        };
        let choice = self
            .plugin
            .as_ref()
            .and_then(|k| choices.iter().find(|c| &c.0 == k))
            .or(choices.first())
            .cloned();
        let params = match (&choice, kind) {
            (Some(choice), Kind::Plugin) => self.plugin_params(&strip, choice, cx),
            _ => vec![],
        };
        if kind == Kind::Plugin && !params.iter().any(|p| Some(p.0) == self.parameter) {
            self.parameter = params.first().map(|p| p.0);
        }
        let parameter_label = params
            .iter()
            .find(|p| Some(p.0) == self.parameter)
            .map_or("No parameter".to_string(), |p| p.1.clone());
        let can_create = kind != Kind::Plugin || (choice.is_some() && self.parameter.is_some());

        let strip_select = widgets::select_button(id("strip"), strip_label, cx)
            .w(px(150.0))
            .on_mouse_down(MouseButton::Left, {
                let (this, current) = (this.clone(), strip.clone());
                move |e: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let items: Vec<MenuItem> = strips
                        .iter()
                        .map(|(sid, name)| {
                            let (this, sid2) = (this.clone(), sid.clone());
                            MenuItem::new(name.clone(), move |_, cx| {
                                let _ = this.update(cx, |this, cx| {
                                    this.strip = Some(sid2.clone());
                                    this.plugin = None;
                                    this.parameter = None;
                                    cx.notify();
                                });
                            })
                            .checked(*sid == current)
                        })
                        .collect();
                    let e = e.clone();
                    let _ = this.update(cx, |this, cx| this.open_menu(items, &e, window, cx));
                }
            });
        let kind_select = widgets::select_button(id("kind"), kind.label(), cx)
            .w(px(150.0))
            .on_mouse_down(MouseButton::Left, {
                let (this, kinds) = (this.clone(), Kind::for_strip(&strip));
                move |e: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let items: Vec<MenuItem> = kinds
                        .iter()
                        .map(|k| {
                            let (this, k2) = (this.clone(), *k);
                            MenuItem::new(k.label(), move |_, cx| {
                                let _ = this.update(cx, |this, cx| {
                                    this.kind = k2;
                                    cx.notify();
                                });
                            })
                            .checked(*k == kind)
                        })
                        .collect();
                    let e = e.clone();
                    let _ = this.update(cx, |this, cx| this.open_menu(items, &e, window, cx));
                }
            });
        let mut row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.0))
            .px(px(14.0))
            .pt(px(10.0))
            .pb(px(14.0))
            .border_t_1()
            .border_color(theme.line)
            .child(div().w(px(70.0)).child(caps("New lane", cx)))
            .child(strip_select)
            .child(kind_select);
        if kind == Kind::Plugin {
            if choices.is_empty() {
                row = row.child(
                    div()
                        .text_size(px(size::SM))
                        .text_color(theme.text_3)
                        .child("No plugin on this strip"),
                );
            } else {
                let filter = self.param_filter.read(cx).text().to_lowercase();
                let focused = self.param_filter.read(cx).is_focused(window);
                let current_plugin = choice.as_ref().map(|c| c.0.clone());
                let plugin_select = widgets::select_button(
                    id("plugin"),
                    choice.as_ref().map_or(String::new(), |c| c.1.clone()),
                    cx,
                )
                .w(px(170.0))
                .on_mouse_down(MouseButton::Left, {
                    let this = this.clone();
                    move |e: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        let items: Vec<MenuItem> = choices
                            .iter()
                            .map(|(key, label, _)| {
                                let (this, key2) = (this.clone(), key.clone());
                                MenuItem::new(label.clone(), move |_, cx| {
                                    let _ = this.update(cx, |this, cx| {
                                        this.plugin = Some(key2.clone());
                                        this.parameter = None;
                                        cx.notify();
                                    });
                                })
                                .checked(current_plugin.as_ref() == Some(key))
                            })
                            .collect();
                        let e = e.clone();
                        let _ = this.update(cx, |this, cx| this.open_menu(items, &e, window, cx));
                    }
                });
                let picked = self.parameter;
                let parameter_select = widgets::select_button(id("parameter"), parameter_label, cx)
                    .w(px(170.0))
                    .on_mouse_down(MouseButton::Left, {
                        let this = this.clone();
                        move |e: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            let words: Vec<&str> = filter.split_whitespace().collect();
                            let matching: Vec<&(u32, String)> = params
                                .iter()
                                .filter(|(_, name)| {
                                    let name = name.to_lowercase();
                                    words.iter().all(|w| name.contains(w))
                                })
                                .collect();
                            let mut items: Vec<MenuItem> = matching
                                .iter()
                                .take(300)
                                .map(|(pid, name)| {
                                    let (this, pid) = (this.clone(), *pid);
                                    MenuItem::new(name.clone(), move |_, cx| {
                                        let _ = this.update(cx, |this, cx| {
                                            this.parameter = Some(pid);
                                            cx.notify();
                                        });
                                    })
                                    .checked(picked == Some(pid))
                                })
                                .collect();
                            if matching.len() > 300 {
                                items.push(MenuItem::Header(
                                    format!("{} more: narrow the search", matching.len() - 300)
                                        .into(),
                                ));
                            }
                            if items.is_empty() {
                                items.push(
                                    MenuItem::new("No parameter matches", |_, _| {}).disabled(true),
                                );
                            }
                            let e = e.clone();
                            let _ =
                                this.update(cx, |this, cx| this.open_menu(items, &e, window, cx));
                        }
                    });
                row = row
                    .child(plugin_select)
                    .child(field(&self.param_filter, focused, cx).w(px(110.0)))
                    .child(parameter_select);
            }
        }
        row.child(div().flex_1())
            .child(
                Button::new(id("create"), "Add lane")
                    .primary()
                    .compact()
                    .disabled(!can_create)
                    .on_click(move |_, _, cx| {
                        let _ = this.update(cx, |this, cx| this.create_lane(cx));
                    }),
            )
            .into_any_element()
    }
}

/// The colour of a lane: its track's, or the accent for the Stereo Out and the returns.
fn lane_color(session: &Session, lane: &AutomationLane, theme: &Theme) -> Hsla {
    lane.target
        .track_id()
        .and_then(|id| {
            session
                .tracks
                .iter()
                .enumerate()
                .find(|(_, t)| t.id == id)
                .map(|(i, t)| theme.track(&t.color, i))
        })
        .unwrap_or(theme.accent)
}

#[allow(clippy::too_many_arguments)]
fn paint_graph(
    lane: &AutomationLane,
    g: &Geometry,
    beats_per_bar: f64,
    playhead: f64,
    color: Hsla,
    selected: Option<&str>,
    theme: &Theme,
    window: &mut Window,
    cx: &mut App,
) {
    let area = g.area;
    // Value rules and labels.
    for row in 0..=4 {
        let value = lane.min + (lane.max - lane.min) * row as f64 / 4.0;
        let y = g.y(value);
        stroke(
            window,
            &[point(area.left(), y), point(area.right(), y)],
            1.0,
            if row == 0 || row == 4 {
                theme.display_zero
            } else {
                theme.display_grid
            },
            None,
        );
        label(
            window,
            cx,
            &value_text(lane, value),
            point(area.left() - px(8.0), y + px(4.0)),
            true,
            theme.display_ink,
        );
    }
    // Bar lines, labelled as often as they fit.
    let bpb = beats_per_bar.max(1.0);
    let px_per_bar = f32::from(area.size.width) as f64 / (g.length / bpb);
    let every = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0]
        .into_iter()
        .find(|n| n * px_per_bar >= 48.0)
        .unwrap_or(256.0);
    let first = (g.start / bpb / every).floor() * every;
    let mut bar = first;
    while bar * bpb <= g.start + g.length {
        let beat = bar * bpb;
        if beat >= g.start {
            let x = g.x(beat);
            stroke(
                window,
                &[point(x, area.top()), point(x, area.bottom())],
                1.0,
                theme.display_grid,
                None,
            );
            label(
                window,
                cx,
                &format!("{}", bar as i64 + 1),
                point(x + px(3.0), area.bottom() + px(15.0)),
                false,
                theme.display_ink,
            );
        }
        bar += every;
    }
    window.with_content_mask(Some(gpui::ContentMask { bounds: area }), |window| {
        // The curve, from the left edge to the right one, with the area under it.
        if !lane.points.is_empty() {
            let end = g.start + g.length;
            let mut pts = vec![g.at(
                g.start,
                lane.value_at(g.start).unwrap_or(lane.points[0].value),
            )];
            for p in lane
                .points
                .iter()
                .filter(|p| p.beat > g.start && p.beat < end)
            {
                let next = g.at(p.beat, p.value);
                if lane.interpolation == Interpolation::Step {
                    let previous = *pts.last().expect("a start");
                    pts.push(point(next.x, previous.y));
                }
                pts.push(next);
            }
            let last = lane.points.last().map_or(lane.min, |p| p.value);
            let tail = g.at(end, lane.value_at(end).unwrap_or(last));
            if lane.interpolation == Interpolation::Step {
                let previous = *pts.last().expect("a start");
                pts.push(point(tail.x, previous.y));
            } else {
                pts.push(tail);
            }
            let mut fill = gpui::PathBuilder::fill();
            fill.move_to(point(pts[0].x, area.bottom()));
            for p in &pts {
                fill.line_to(*p);
            }
            fill.line_to(point(pts.last().expect("points").x, area.bottom()));
            fill.close();
            if let Ok(path) = fill.build() {
                window.paint_path(
                    path,
                    with_alpha(color, if lane.enabled { 0.12 } else { 0.05 }),
                );
            }
            let trace = if lane.enabled {
                color
            } else {
                with_alpha(color, 0.45)
            };
            stroke(
                window,
                &pts,
                1.8,
                trace,
                if lane.enabled { None } else { Some([4.0, 3.0]) },
            );
        }
        // The playhead.
        if (g.start..=g.start + g.length).contains(&playhead) {
            let x = g.x(playhead);
            stroke(
                window,
                &[point(x, area.top()), point(x, area.bottom())],
                1.0,
                theme.accent,
                None,
            );
        }
    });
    // Points, the selected one larger and ringed.
    for p in &lane.points {
        if !(g.start..=g.start + g.length).contains(&p.beat) {
            continue;
        }
        let c = g.at(p.beat, p.value);
        let on = selected == Some(p.id.as_str());
        let r = px(if on { 5.5 } else { 4.0 });
        window.paint_quad(
            gpui::fill(
                Bounds::centered_at(c, gpui::size(r * 2.0, r * 2.0)),
                if on { color } else { theme.display },
            )
            .border_widths(px(1.5))
            .border_color(if on { theme.text_display } else { color }),
        );
    }
}

impl Render for Automation {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let lane = self.current(cx);
        let lane_bar = self.lane_bar(lane.as_ref(), cx);
        let body = match &lane {
            Some(lane) => div()
                .flex()
                .flex_col()
                .child(self.graph(lane, cx))
                .child(self.point_bar(lane, window, cx))
                .into_any_element(),
            None => div()
                .mx(px(14.0))
                .mb(px(12.0))
                .h(px(120.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(radius::MD))
                .bg(theme.display)
                .text_color(theme.text_3)
                .text_size(px(size::BASE))
                .child("Add a lane below, then double-click its graph to draw a fade or a parameter change.")
                .into_any_element(),
        };
        let new_lane = self.new_lane(window, cx);
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(
                widgets::surface(2, cx)
                    .id(id("panel"))
                    .key_context("Automation")
                    .track_focus(&self.focus)
                    .on_action(cx.listener(Self::on_do))
                    .occlude()
                    .absolute()
                    .left(self.origin.x)
                    .top(self.origin.y)
                    .w(px(PANEL_W))
                    .flex()
                    .flex_col()
                    .child(self.header(cx))
                    .child(lane_bar)
                    .child(body)
                    .child(new_lane),
            )
            .children(self.menu.render(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Ryolune;
    use ryolune_engine::store::{self, Command};

    fn well() -> Bounds<Pixels> {
        Bounds::new(
            point(px(100.0), px(50.0)),
            gpui::size(px(LEFT + RIGHT + 640.0), px(TOP + BOTTOM + 200.0)),
        )
    }

    #[test]
    fn the_graph_maps_beats_and_values_both_ways_and_snaps() {
        let g = Geometry::new(well(), 0.0, 32.0, 0.0, 1.0);
        let at = g.at(8.0, 0.5);
        assert_eq!(at, point(px(100.0 + LEFT + 160.0), px(50.0 + TOP + 100.0)));
        assert_eq!(g.locate(at, true), (8.0, 0.5));
        let off = point(at.x + px(2.0), at.y);
        let (beat, _) = g.locate(off, true);
        assert_eq!(beat, 8.0, "a quarter beat is 5 px here: 2 px snaps back");
        assert!(g.locate(off, false).0 > 8.0);
        // Outside the area: held at the edges, never before beat 0.
        assert_eq!(g.locate(point(px(0.0), px(0.0)), false), (0.0, 1.0));
        let points = vec![
            AutomationPoint {
                id: "a".into(),
                beat: 8.0,
                value: 0.5,
            },
            AutomationPoint {
                id: "b".into(),
                beat: 9.0,
                value: 0.5,
            },
        ];
        assert_eq!(
            g.hit(&points, point(at.x + px(2.0), at.y))
                .map(|p| p.id.as_str()),
            Some("a")
        );
        assert!(g
            .hit(&points, point(at.x + px(10.0), at.y + px(10.0)))
            .is_none());
    }

    #[test]
    fn a_dragged_point_stays_between_its_neighbours() {
        let points = vec![
            AutomationPoint {
                id: "c".into(),
                beat: 12.0,
                value: 0.0,
            },
            AutomationPoint {
                id: "a".into(),
                beat: 4.0,
                value: 0.0,
            },
            AutomationPoint {
                id: "b".into(),
                beat: 8.0,
                value: 0.0,
            },
        ];
        let (low, high) = neighbours(&points, "b");
        assert!(low > 4.0 && low < 4.001 && high < 12.0 && high > 11.999);
        assert_eq!(neighbours(&points, "a").0, 0.0);
        assert_eq!(neighbours(&points, "c").1, f64::MAX);
    }

    #[test]
    fn lane_values_read_in_their_units() {
        let mut lane = AutomationLane {
            id: "l".into(),
            name: "Master".into(),
            target: AutomationTarget::MasterVolume,
            min: 0.0,
            max: 1.0,
            manual_value: 0.75,
            interpolation: Interpolation::Linear,
            enabled: true,
            points: vec![],
        };
        assert_eq!(value_text(&lane, 0.75), "0.0 dB");
        lane.target = AutomationTarget::TrackPan {
            track_id: "t".into(),
        };
        assert_eq!(value_text(&lane, -40.0), "L 40");
        assert_eq!(Kind::for_strip(BUS_A), &[Kind::Plugin]);
        assert!(!Kind::for_strip(MASTER).contains(&Kind::Pan));
    }

    /// The window's edits through the registry: a double click adds, a drag moves on
    /// release as one undo step, Delete removes.
    #[gpui::test]
    fn graph_edits_are_registry_commands_with_undo(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            cx.set_global(Theme::new(crate::ui::theme::Mode::Dark, true));
            crate::ui::actions::bind(cx);
        });
        let mut app = Ryolune::from_session(store::empty(), None);
        app.run_control_command(
            "automation.create",
            &json!({ "target": "masterVolume" }),
            false,
            "test",
        )
        .unwrap();
        let daw = cx.new(|_| Daw::new(app));
        let window = cx.add_window(|window, cx| Automation::new(daw.clone(), window, cx));
        window
            .update(cx, |view, window, cx| {
                view.well.set(Some(well()));
                let lane = view.current(cx).expect("the lane");
                let g = view.geometry(&lane, cx).unwrap();
                // A double click in the graph adds a point there, snapped.
                let at = g.at(8.1, 0.5);
                let mut down = MouseDownEvent {
                    button: MouseButton::Left,
                    position: at,
                    modifiers: Default::default(),
                    click_count: 2,
                    first_mouse: false,
                };
                view.graph_down(&down, window, cx);
                let lane = view.current(cx).unwrap();
                assert_eq!(lane.points.len(), 1);
                assert_eq!(lane.points[0].beat, 8.0);
                assert_eq!(view.point.as_deref(), Some(lane.points[0].id.as_str()));
                let original = lane.points[0].clone();
                // Press on it and drag: nothing is written until release.
                down.click_count = 1;
                down.position = g.at(8.0, 0.5);
                view.graph_down(&down, window, cx);
                view.graph_move(g.at(12.0, 0.8), false, cx);
                assert_eq!(view.current(cx).unwrap().points[0], original);
                view.graph_up(cx);
                let moved = view.current(cx).unwrap().points[0].clone();
                assert_eq!((moved.id.clone(), moved.beat), (original.id.clone(), 12.0));
                assert!((moved.value - 0.8).abs() < 0.01);
                // One undo step puts it back.
                view.daw
                    .update(cx, |daw, _| daw.app.store.dispatch(Command::Undo).unwrap());
                assert_eq!(view.current(cx).unwrap().points[0], original);
                // Delete removes the selected point.
                view.point = Some(original.id.clone());
                view.on_do(
                    &Do {
                        id: "deleteSelection",
                    },
                    window,
                    cx,
                );
                assert!(view.current(cx).unwrap().points.is_empty());
            })
            .unwrap();
    }

    #[gpui::test]
    fn new_lanes_follow_the_target_picker(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| cx.set_global(Theme::new(crate::ui::theme::Mode::Dark, true)));
        let mut app = Ryolune::from_session(store::empty(), None);
        let bass = app
            .store
            .session()
            .tracks
            .iter()
            .find(|t| t.name == "Bass")
            .unwrap()
            .id
            .clone();
        crate::ui::plugin_panel::tests::put_inserts(&mut app, &bass, &["Channel EQ"]);
        let daw = cx.new(|_| Daw::new(app));
        let window = cx.add_window(|window, cx| Automation::new(daw.clone(), window, cx));
        window
            .update(cx, |view, _, cx| {
                view.strip = Some(bass.clone());
                view.kind = Kind::Pan;
                view.create_lane(cx);
                let lane = view.current(cx).unwrap();
                assert_eq!(lane.target, AutomationTarget::TrackPan { track_id: bass.clone() });
                assert_eq!((lane.min, lane.max), (-100.0, 100.0));
                // A plugin parameter: the EQ in slot 1 (after the instrument), by name.
                view.kind = Kind::Plugin;
                let choices = plugin_choices(view.daw.read(cx).app.store.session(), &bass);
                assert_eq!(choices.len(), 2, "instrument and the EQ");
                view.plugin = Some(choices[1].0.clone());
                let params = view.plugin_params(&bass, &choices[1], cx);
                let gain = params.iter().find(|p| p.1 == "Mid Gain").unwrap().0;
                view.parameter = Some(gain);
                view.create_lane(cx);
                let lane = view.current(cx).unwrap();
                assert!(matches!(lane.target, AutomationTarget::PluginParameter { parameter_id, .. } if parameter_id == gain));
                assert_eq!(view.daw.read(cx).app.store.session().automation.len(), 2);
            })
            .unwrap();
    }
}
