//! Continuous controls: knob, vertical fader, horizontal slider, and the level meters.
//!
//! Values are normalised 0-1; the caller maps them to decibels, pan or a parameter. Every
//! change reports a [`Phase`]: `Start` on press, `Move` while dragging, `End` on release,
//! so the caller wraps the drag in one undo step (`Daw::gesture`). Drags are never eased.
//! Shift drags finely; a double click returns to the default; the wheel nudges.

use crate::ui::theme::{radius, with_alpha, Theme};
use gpui::{
    canvas, div, point, prelude::*, px, App, Bounds, ElementId, Empty, Hsla, MouseButton,
    PathBuilder, Pixels, Point, ScrollWheelEvent, SharedString, Window,
};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Start,
    Move,
    End,
}

pub type ChangeHandler = Rc<dyn Fn(f32, Phase, &mut Window, &mut App)>;

/// Where a drag started, kept across frames for the control under the pointer.
#[derive(Default)]
struct DragState {
    active: bool,
    origin: Point<Pixels>,
    start: f32,
    last: f32,
}

/// The payload GPUI carries while a dial is dragged; the id tells controls apart.
#[derive(Clone)]
struct DialDrag(ElementId);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    /// Knob: up is more, measured from the press.
    Knob,
    /// Fader: the cap follows the pointer.
    Vertical,
    /// Slider: the thumb follows the pointer.
    Horizontal,
}

#[derive(Clone)]
struct Drive {
    id: ElementId,
    axis: Axis,
    value: f32,
    default: f32,
    on_change: Option<ChangeHandler>,
}

impl Drive {
    /// Press, drag, release, double click and wheel on a control's element.
    fn attach(
        self,
        el: gpui::Stateful<gpui::Div>,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Stateful<gpui::Div> {
        let Some(f) = self.on_change.clone() else {
            return el;
        };
        let state = window.use_keyed_state(self.id.clone(), cx, |_, _| DragState::default());
        let id = self.id.clone();
        let value = self.value;
        let default = self.default;
        let axis = self.axis;
        let down_state = state.clone();
        let down_f = f.clone();
        let move_state = state.clone();
        let move_f = f.clone();
        let up_state = state.clone();
        let up_f = f.clone();
        let up_out_state = state;
        let up_out_f = f.clone();
        let wheel_f = f.clone();
        let click_f = f;
        let end = move |state: &gpui::Entity<DragState>,
                        f: &ChangeHandler,
                        w: &mut Window,
                        cx: &mut App| {
            let (active, last) = {
                let s = state.read(cx);
                (s.active, s.last)
            };
            if active {
                state.update(cx, |s, _| s.active = false);
                f(last, Phase::End, w, cx);
            }
        };
        let end_up = end;
        el.on_mouse_down(MouseButton::Left, move |e, w, cx| {
            cx.stop_propagation();
            if e.click_count >= 2 {
                return;
            }
            down_state.update(cx, |s, _| {
                s.active = true;
                s.origin = e.position;
                s.start = value;
                s.last = value;
            });
            down_f(value, Phase::Start, w, cx);
        })
        .on_drag(DialDrag(id.clone()), |_, _, _, cx| cx.new(|_| Empty))
        .on_drag_move::<DialDrag>(move |e, w, cx| {
            if e.drag(cx).0 != id {
                return;
            }
            let (active, origin, start) = {
                let s = move_state.read(cx);
                (s.active, s.origin, s.start)
            };
            if !active {
                return;
            }
            let fine = if e.event.modifiers.shift { 0.15 } else { 1.0 };
            let b = e.bounds;
            let next = match axis {
                Axis::Knob => start + f32::from(origin.y - e.event.position.y) / 160.0 * fine,
                Axis::Vertical => {
                    let travel = f32::from(b.size.height).max(1.0);
                    start + f32::from(origin.y - e.event.position.y) / travel * fine
                }
                Axis::Horizontal => {
                    let travel = f32::from(b.size.width).max(1.0);
                    start + f32::from(e.event.position.x - origin.x) / travel * fine
                }
            }
            .clamp(0.0, 1.0);
            move_state.update(cx, |s, _| s.last = next);
            move_f(next, Phase::Move, w, cx);
        })
        .on_mouse_up(MouseButton::Left, move |_, w, cx| {
            end_up(&up_state, &up_f, w, cx)
        })
        .on_mouse_up_out(MouseButton::Left, move |_, w, cx| {
            end(&up_out_state, &up_out_f, w, cx)
        })
        .on_click(move |e, w, cx| {
            if e.click_count() == 2 {
                click_f(default, Phase::Start, w, cx);
                click_f(default, Phase::End, w, cx);
            }
        })
        .on_scroll_wheel(move |e: &ScrollWheelEvent, w, cx| {
            let delta = e.delta.pixel_delta(px(16.0));
            let step = f32::from(delta.y - delta.x) / 600.0;
            if step != 0.0 {
                let next = (value + step).clamp(0.0, 1.0);
                wheel_f(next, Phase::Start, w, cx);
                wheel_f(next, Phase::End, w, cx);
                cx.stop_propagation();
            }
        })
    }
}

/// Drag any element up and down to change a number: the tempo display, a value field.
/// `per_px` is how much one pixel of travel changes the value; Shift is ten times finer.
pub struct NumberDrag {
    pub id: ElementId,
    pub value: f32,
    pub per_px: f32,
    pub min: f32,
    pub max: f32,
}
impl NumberDrag {
    pub fn attach(
        self,
        el: gpui::Stateful<gpui::Div>,
        on_change: impl Fn(f32, Phase, &mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Stateful<gpui::Div> {
        let span = (self.max - self.min).max(1e-6);
        // Drive works on 0-1; map the number onto it with the requested sensitivity.
        let normal = (self.value - self.min) / span;
        let (min, per_px) = (self.min, self.per_px);
        let f: ChangeHandler =
            Rc::new(move |v, phase, w, cx| on_change(min + v * span, phase, w, cx));
        let drive = Drive {
            id: self.id,
            axis: Axis::Knob,
            value: normal,
            default: normal,
            on_change: Some(f),
        };
        // Knob drags move 1/160 of the range per pixel; scale to `per_px`.
        let scale = per_px * 160.0 / span;
        let scaled: ChangeHandler = {
            let inner = drive.on_change.clone().unwrap();
            Rc::new(move |v, phase, w, cx| {
                let v = (normal + (v - normal) * scale).clamp(0.0, 1.0);
                inner(v, phase, w, cx)
            })
        };
        Drive {
            on_change: Some(scaled),
            ..drive
        }
        .attach(el, window, cx)
    }
}

fn arc_points(center: Point<Pixels>, r: f32, from: f32, to: f32) -> Vec<Point<Pixels>> {
    let steps = (((to - from).abs() / 0.08).ceil() as usize).max(2);
    (0..=steps)
        .map(|i| {
            let a = from + (to - from) * i as f32 / steps as f32;
            point(center.x + px(r * a.cos()), center.y + px(r * a.sin()))
        })
        .collect()
}
fn stroke(window: &mut Window, points: &[Point<Pixels>], width: f32, color: Hsla) {
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

/// A rotary control: 270° of travel, the value as a lit arc.
#[derive(IntoElement)]
pub struct Knob {
    drive: Drive,
    size: f32,
    bipolar: bool,
    color: Option<Hsla>,
    tooltip: Option<SharedString>,
}
impl Knob {
    pub fn new(id: impl Into<ElementId>, value: f32) -> Self {
        Self {
            drive: Drive {
                id: id.into(),
                axis: Axis::Knob,
                value: value.clamp(0.0, 1.0),
                default: 0.5,
                on_change: None,
            },
            size: 28.0,
            bipolar: false,
            color: None,
            tooltip: None,
        }
    }
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
    /// The arc grows from the middle: pan, a send's balance, an EQ gain.
    pub fn bipolar(mut self) -> Self {
        self.bipolar = true;
        self.drive.default = 0.5;
        self
    }
    pub fn default_value(mut self, value: f32) -> Self {
        self.drive.default = value;
        self
    }
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }
    pub fn on_change(mut self, f: impl Fn(f32, Phase, &mut Window, &mut App) + 'static) -> Self {
        self.drive.on_change = Some(Rc::new(f));
        self
    }
}
impl RenderOnce for Knob {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let value = self.drive.value;
        let color = self.color.unwrap_or(theme.accent);
        let bipolar = self.bipolar;
        let size_px = self.size;
        let tooltip = self.tooltip.clone();
        let el = div()
            .id(self.drive.id.clone())
            .flex_none()
            .size(px(size_px))
            .cursor(gpui::CursorStyle::ResizeUpDown)
            .child(
                canvas(
                    |_, _, _| (),
                    move |b: Bounds<Pixels>, _, window, _| {
                        let c = b.center();
                        let r = f32::from(b.size.width) / 2.0;
                        let start = std::f32::consts::PI * 0.75;
                        let sweep = std::f32::consts::PI * 1.5;
                        // Track and value arcs around the cap.
                        stroke(
                            window,
                            &arc_points(c, r - 1.5, start, start + sweep),
                            2.0,
                            theme.groove,
                        );
                        let (a, z) = if bipolar {
                            let mid = start + sweep * 0.5;
                            let v = start + sweep * value;
                            (mid.min(v), mid.max(v))
                        } else {
                            (start, start + sweep * value)
                        };
                        if z - a > 0.01 {
                            stroke(window, &arc_points(c, r - 1.5, a, z), 2.0, color);
                        }
                        // The cap: raised face, crisp edge, a fine top highlight.
                        let cap = r - 4.5;
                        let cap_bounds =
                            Bounds::centered_at(c, gpui::size(px(cap * 2.0), px(cap * 2.0)));
                        window.paint_quad(
                            gpui::fill(cap_bounds, theme.knob)
                                .corner_radii(px(cap))
                                .border_widths(px(1.0))
                                .border_color(theme.knob_edge),
                        );
                        let angle = start + sweep * value;
                        let inner = point(
                            c.x + px((cap * 0.25) * angle.cos()),
                            c.y + px((cap * 0.25) * angle.sin()),
                        );
                        let outer = point(
                            c.x + px((cap - 1.5) * angle.cos()),
                            c.y + px((cap - 1.5) * angle.sin()),
                        );
                        stroke(window, &[inner, outer], 1.8, theme.text);
                    },
                )
                .size_full(),
            )
            .when_some(tooltip, |d, text| {
                d.tooltip(move |_, cx| super::controls::tip(text.clone(), cx))
            });
        self.drive.attach(el, window, cx)
    }
}

/// A vertical channel fader: a groove and a cap.
#[derive(IntoElement)]
pub struct Fader {
    drive: Drive,
    width: f32,
}
impl Fader {
    pub fn new(id: impl Into<ElementId>, value: f32) -> Self {
        Self {
            drive: Drive {
                id: id.into(),
                axis: Axis::Vertical,
                value: value.clamp(0.0, 1.0),
                default: 0.75,
                on_change: None,
            },
            width: 28.0,
        }
    }
    pub fn on_change(mut self, f: impl Fn(f32, Phase, &mut Window, &mut App) + 'static) -> Self {
        self.drive.on_change = Some(Rc::new(f));
        self
    }
}
impl RenderOnce for Fader {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let value = self.drive.value;
        let el = div()
            .id(self.drive.id.clone())
            .flex_none()
            .w(px(self.width))
            .h_full()
            .cursor(gpui::CursorStyle::ResizeUpDown)
            .child(
                canvas(
                    |_, _, _| (),
                    move |b: Bounds<Pixels>, _, window, _| {
                        let cap_h = 14.0;
                        let travel = f32::from(b.size.height) - cap_h;
                        let groove = Bounds::new(
                            point(b.center().x - px(2.0), b.origin.y + px(cap_h / 2.0)),
                            gpui::size(px(4.0), px(travel)),
                        );
                        window.paint_quad(gpui::fill(groove, theme.groove));
                        let y = b.origin.y + px(travel * (1.0 - value));
                        let cap = Bounds::new(
                            point(b.origin.x + px(2.0), y),
                            gpui::size(b.size.width - px(4.0), px(cap_h)),
                        );
                        window.paint_shadows(
                            cap,
                            gpui::Corners::all(px(0.0)),
                            &[gpui::BoxShadow {
                                color: theme.glass_shadow,
                                offset: point(px(0.0), px(2.0)),
                                blur_radius: px(4.0),
                                spread_radius: px(0.0),
                            }],
                        );
                        window.paint_quad(
                            gpui::fill(cap, theme.thumb)
                                .border_widths(px(1.0))
                                .border_color(theme.control_edge),
                        );
                        let line = Bounds::new(
                            point(cap.origin.x + px(3.0), cap.center().y - px(0.5)),
                            gpui::size(cap.size.width - px(6.0), px(1.0)),
                        );
                        window.paint_quad(gpui::fill(line, with_alpha(theme.bg_sunken, 0.6)));
                    },
                )
                .size_full(),
            );
        self.drive.attach(el, window, cx)
    }
}

/// A horizontal slider: zoom, a send level, a setting.
#[derive(IntoElement)]
pub struct Slider {
    drive: Drive,
    color: Option<Hsla>,
}
impl Slider {
    pub fn new(id: impl Into<ElementId>, value: f32) -> Self {
        Self {
            drive: Drive {
                id: id.into(),
                axis: Axis::Horizontal,
                value: value.clamp(0.0, 1.0),
                default: 0.5,
                on_change: None,
            },
            color: None,
        }
    }
    pub fn default_value(mut self, value: f32) -> Self {
        self.drive.default = value;
        self
    }
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
    pub fn on_change(mut self, f: impl Fn(f32, Phase, &mut Window, &mut App) + 'static) -> Self {
        self.drive.on_change = Some(Rc::new(f));
        self
    }
}
impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let value = self.drive.value;
        let color = self.color.unwrap_or(theme.accent);
        let el = div()
            .id(self.drive.id.clone())
            .flex_1()
            .min_w(px(40.0))
            .h(px(18.0))
            .cursor(gpui::CursorStyle::ResizeLeftRight)
            .child(
                canvas(
                    |_, _, _| (),
                    move |b: Bounds<Pixels>, _, window, _| {
                        let thumb = 12.0;
                        let travel = f32::from(b.size.width) - thumb;
                        let groove = Bounds::new(
                            point(b.origin.x + px(thumb / 2.0), b.center().y - px(2.0)),
                            gpui::size(px(travel), px(4.0)),
                        );
                        window.paint_quad(gpui::fill(groove, theme.groove));
                        let filled =
                            Bounds::new(groove.origin, gpui::size(px(travel * value), px(4.0)));
                        window.paint_quad(gpui::fill(filled, color));
                        let x = b.origin.x + px(travel * value);
                        let t = Bounds::new(
                            point(x, b.center().y - px(thumb / 2.0)),
                            gpui::size(px(thumb), px(thumb)),
                        );
                        window.paint_quad(
                            gpui::fill(t, theme.thumb)
                                .corner_radii(px(thumb / 2.0))
                                .border_widths(px(1.0))
                                .border_color(theme.control_edge),
                        );
                    },
                )
                .size_full(),
            );
        self.drive.attach(el, window, cx)
    }
}

/// Peak (0-1, linear) to a meter position: -60 dB to +6 dB.
pub fn meter_position(peak: f32) -> f32 {
    if peak <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * peak.log10();
    ((db + 60.0) / 66.0).clamp(0.0, 1.0)
}

/// An LED level meter: segments lit up to the level, mint, amber near 0 dB, red over.
#[derive(IntoElement)]
pub struct Meter {
    levels: Vec<f32>,
    vertical: bool,
    segments: usize,
    linear: bool,
}
impl Meter {
    /// One bar per channel, peaks linear 0-1.
    pub fn new(levels: impl IntoIterator<Item = f32>) -> Self {
        Self {
            levels: levels.into_iter().collect(),
            vertical: false,
            segments: 24,
            linear: false,
        }
    }
    /// Levels are already 0-1 positions (CPU load, a percentage), not audio peaks.
    pub fn linear(mut self) -> Self {
        self.linear = true;
        self
    }
    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }
    pub fn segments(mut self, n: usize) -> Self {
        self.segments = n.max(4);
        self
    }
}
impl RenderOnce for Meter {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let levels = self.levels;
        let vertical = self.vertical;
        let n = self.segments;
        let linear = self.linear;
        canvas(
            |_, _, _| (),
            move |b: Bounds<Pixels>, _, window, _| {
                let channels = levels.len().max(1);
                let gap = 1.0;
                for (ch, level) in levels.iter().enumerate() {
                    let position = if linear {
                        level.clamp(0.0, 1.0)
                    } else {
                        meter_position(*level)
                    };
                    let lit = (position * n as f32).round() as usize;
                    for i in 0..n {
                        let t = i as f32 / n as f32;
                        let color = if i >= lit {
                            theme.meter_off
                        } else if t > 0.94 {
                            theme.meter_clip
                        } else if t > 0.8 {
                            theme.meter_hot
                        } else {
                            theme.meter
                        };
                        let seg = if vertical {
                            let w = (f32::from(b.size.width) - gap * (channels - 1) as f32)
                                / channels as f32;
                            let h = (f32::from(b.size.height) - gap * (n - 1) as f32) / n as f32;
                            Bounds::new(
                                point(
                                    b.origin.x + px(ch as f32 * (w + gap)),
                                    b.origin.y + b.size.height
                                        - px((i + 1) as f32 * h + i as f32 * gap),
                                ),
                                gpui::size(px(w), px(h)),
                            )
                        } else {
                            let h = (f32::from(b.size.height) - gap * (channels - 1) as f32)
                                / channels as f32;
                            let w = (f32::from(b.size.width) - gap * (n - 1) as f32) / n as f32;
                            Bounds::new(
                                point(
                                    b.origin.x + px(i as f32 * (w + gap)),
                                    b.origin.y + px(ch as f32 * (h + gap)),
                                ),
                                gpui::size(px(w), px(h)),
                            )
                        };
                        window
                            .paint_quad(gpui::fill(seg, color).corner_radii(px(radius::XS / 4.0)));
                    }
                }
            },
        )
        .size_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn meter_scale_spans_minus_sixty_to_plus_six() {
        assert_eq!(meter_position(0.0), 0.0);
        assert!((meter_position(1.0) - 60.0 / 66.0).abs() < 1e-4);
        assert_eq!(meter_position(10.0), 1.0);
    }
}
