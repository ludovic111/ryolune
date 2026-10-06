//! Track headers beside the lanes: colour strip, the track's number in an ink chip, its whole
//! name (two lines when it needs them, double-click to rename), kind and routing, then the
//! M/S/R/monitor keys (always shown, boxed, lit when on: inverted, red for arm), the input
//! level of an armed audio track, volume and pan.
//! A click selects the track, a drag moves it in the list, a right click opens its menu.

use super::{gestures::TrackDrag, Arrangement, Drag, Editing};
use crate::ui::{
    daw::Daw,
    format,
    theme::{arrange, layout, radius, size, with_alpha, Theme, FONT_MONO},
    widgets::{self, icon, Key, Knob, Meter, Phase, Slider},
};
use gpui::{
    canvas, div, prelude::*, px, AnyElement, App, Context, Entity, MouseButton, MouseDownEvent,
    MouseUpEvent, SharedString, Window,
};
use ryolune_engine::model::{Monitor, Track};
use serde_json::{json, Value};

/// A fader, knob or slider of a track: one undo step per drag, nothing sent while the value
/// holds still (a press without a move).
fn continuous(
    daw: &Entity<Daw>,
    method: &'static str,
    track: &str,
    current: f64,
    value: impl Fn(f32) -> (f64, Value) + 'static,
) -> impl Fn(f32, Phase, &mut Window, &mut App) + 'static {
    let daw = daw.clone();
    let track = track.to_string();
    move |v, phase, _, cx| {
        let (number, json_value) = value(v);
        daw.update(cx, |daw, cx| {
            if phase == Phase::Start {
                daw.gesture(true);
            }
            if (number - current).abs() > 1e-6 {
                let mut params = json!({ "trackId": track });
                if let Value::Object(fields) = json_value {
                    for (k, v) in fields {
                        params[k] = v;
                    }
                }
                daw.run(method, params, cx);
            }
            if phase == Phase::End {
                daw.gesture(false);
            }
        })
    }
}

fn kind_label(track: &Track, tracks: &[Track]) -> String {
    let kind = match track.kind.as_str() {
        "audio" => "AUD",
        "bus" => "BUS",
        _ => "MIDI",
    };
    match track
        .output
        .as_ref()
        .and_then(|id| tracks.iter().find(|t| &t.id == id))
    {
        Some(bus) => format!("{kind} → {}", bus.name),
        None => kind.to_string(),
    }
}

impl Arrangement {
    pub(super) fn headers(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let s = self.session(cx);
        let row = layout::TRACK_HEIGHT;
        let rows: Vec<AnyElement> = s
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| self.header(i, t, &s.tracks, window, cx))
            .collect();
        let slot = match &self.drag {
            Some(Drag::Track(d)) if d.moved => Some(d.slot),
            _ => None,
        };
        let bounds = self.areas.headers.clone();
        div()
            .id("track-headers")
            .relative()
            .w(px(layout::TRACK_HEADER))
            .flex_none()
            .h_full()
            .overflow_hidden()
            .bg(theme.editor)
            .border_r_1()
            .border_color(theme.line)
            .child(
                canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(px(-self.scroll_y as f32))
                    .flex()
                    .flex_col()
                    .children(rows),
            )
            .when_some(slot, |d, slot| {
                d.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(slot as f32 * row - self.scroll_y as f32 - 1.0))
                        .h(px(2.0))
                        .bg(theme.accent),
                )
            })
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, e: &MouseUpEvent, _, cx| {
                    this.drag_end(e.position, e.modifiers, cx)
                }),
            )
            .into_any_element()
    }

    fn header(
        &mut self,
        index: usize,
        t: &Track,
        tracks: &[Track],
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let ink = theme.timeline();
        let app = &self.daw.read(cx).app;
        let s = app.store.session();
        let selected = s.view.selected_track_id.as_deref() == Some(t.id.as_str());
        let agent =
            app.agents.runtime.running() && s.clips.iter().any(|c| c.agent && c.track_id == t.id);
        let color = theme.track(&t.color, index);
        let id = t.id.clone();
        let bus = t.is_bus();
        let renaming = self.editing == Some(Editing::Track(t.id.clone()));
        let daw = self.daw.clone();

        let name: AnyElement = if renaming {
            div()
                .flex_1()
                .h(px(arrange::INLINE_INPUT_H))
                .child(
                    widgets::field(&self.name_input, true, cx)
                        .h_full()
                        .px(px(6.0))
                        .py(px(2.0))
                        .text_size(px(size::SM)),
                )
                .into_any_element()
        } else {
            let rename_id = t.id.clone();
            let rename_name = t.name.clone();
            div()
                .id(SharedString::from(format!("track-name-{}", t.id)))
                .flex_1()
                .min_w_0()
                // Names are never cut to make room: two lines instead.
                .line_clamp(2)
                .text_size(px(size::BASE))
                .line_height(px(15.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child(t.name.clone())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                        if e.click_count == 2 {
                            cx.stop_propagation();
                            this.cancel_drag(cx);
                            this.edit(
                                Editing::Track(rename_id.clone()),
                                rename_name.clone(),
                                window,
                                cx,
                            );
                        }
                    }),
                )
                .tooltip(|_, cx| widgets::tip("Double-click to rename".into(), cx))
                .into_any_element()
        };
        let kind_tip: SharedString = if bus {
            "Bus: sums the tracks routed or sent to it".into()
        } else {
            kind_label(t, tracks).into()
        };
        // The track's number, inverted in an ink chip.
        let chip = div()
            .flex_none()
            .min_w(px(22.0))
            .h(px(18.0))
            .px(px(4.0))
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.text)
            .text_color(theme.bg)
            .font_family(FONT_MONO)
            .text_size(px(10.0))
            .font_weight(gpui::FontWeight::BOLD)
            .child(format!("{}", index + 1));
        let name_row = div()
            .flex()
            .items_center()
            .gap(px(7.0))
            .child(chip)
            .child(name)
            .child(
                div()
                    .id(SharedString::from(format!("track-kind-{}", t.id)))
                    .font_family(FONT_MONO)
                    .text_size(px(10.0))
                    .text_color(theme.text_3)
                    .whitespace_nowrap()
                    .child(kind_label(t, tracks))
                    .tooltip(move |_, cx| widgets::tip(kind_tip.clone(), cx)),
            )
            .when(agent, |d| d.child(widgets::dot(theme.accent, 6.0)));

        let key = |label: &'static str,
                   on: bool,
                   color,
                   tip: &'static str,
                   method: &'static str,
                   field: &'static str| {
            let daw = daw.clone();
            let id = id.clone();
            Key::new(
                SharedString::from(format!("{method}-{id}")),
                label,
                on,
                color,
            )
            .tooltip(tip)
            .on_click(move |_, _, cx| {
                let id = id.clone();
                daw.update(cx, |daw, cx| {
                    daw.run(method, json!({ "trackId": id, field: !on }), cx);
                })
            })
        };
        let mut keys = div()
            .flex()
            .gap(px(3.0))
            .child(key(
                "M",
                t.mute,
                theme.mute,
                "Mute",
                "track.setMute",
                "muted",
            ))
            .child(key(
                "S",
                t.solo,
                theme.solo,
                "Solo",
                "track.setSolo",
                "solo",
            ));
        if !bus {
            keys = keys.child(key(
                "●",
                t.armed,
                theme.record,
                "Record arm",
                "track.setArmed",
                "armed",
            ));
        }
        if t.kind == "audio" {
            keys = keys.child(self.monitor_key(t, cx));
        }

        let volume = Slider::new(SharedString::from(format!("volume-{}", t.id)), t.volume)
            .default_value(format::FADER_UNITY)
            .on_change(continuous(
                &self.daw,
                "track.setVolume",
                &t.id,
                t.volume as f64,
                |v| {
                    let v = (v as f64 * 1000.0).round() / 1000.0;
                    (v, json!({ "volume": v }))
                },
            ));
        let pan_value = ((t.pan + 100.0) / 200.0).clamp(0.0, 1.0);
        let pan = Knob::new(SharedString::from(format!("pan-{}", t.id)), pan_value)
            .bipolar()
            .size(26.0)
            .tooltip(format!(
                "Pan {} · drag, double-click to centre",
                format::pan(t.pan / 100.0)
            ))
            .on_change(continuous(
                &self.daw,
                "track.setPan",
                &t.id,
                t.pan as f64,
                |v| {
                    let pan = (v as f64 * 200.0 - 100.0).round();
                    (pan, json!({ "pan": pan }))
                },
            ));
        let level = (t.armed && t.kind == "audio").then(|| {
            // Over a 48 dB window, so speech sits in the middle rather than at the bottom.
            let held = self.input_level;
            let position = if held > 0.004 {
                (1.0 + 20.0 * held.log10() / 48.0).clamp(0.0, 1.0)
            } else {
                0.0
            };
            div()
                .id(SharedString::from(format!("input-{}", t.id)))
                .w(px(24.0))
                .h(px(8.0))
                .flex_none()
                .child(Meter::new([position]).linear().segments(6))
                .tooltip(move |_, cx| {
                    widgets::tip(
                        if held >= 0.98 {
                            "Input is clipping: turn the microphone down".into()
                        } else {
                            "Microphone level".into()
                        },
                        cx,
                    )
                })
        });
        let controls = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(keys)
            .children(level)
            .child(volume)
            .child(pan);

        let bg = if selected {
            theme.lane_selected
        } else if agent {
            theme.lane_agent
        } else {
            ink.header
        };
        let drag_id = t.id.clone();
        let menu_id = t.id.clone();
        div()
            .id(SharedString::from(format!("track-{}", t.id)))
            .relative()
            .h(px(layout::TRACK_HEIGHT))
            .flex_none()
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(7.0))
            .pl(px(arrange::COLOR_STRIP + 10.0))
            .pr(px(10.0))
            .bg(bg)
            .border_b_1()
            .border_color(ink.lane_bottom)
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(px(arrange::COLOR_STRIP))
                    .bg(color)
                    .border_r_1()
                    .border_color(with_alpha(theme.bg_sunken, 0.25)),
            )
            .child(name_row)
            .child(controls)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    if this.menu.is_open() || this.drag.is_some() {
                        return;
                    }
                    if this.editing.is_some() {
                        this.commit_edit(cx);
                    }
                    let (_, y) = super::local(this.areas.headers.get(), e.position);
                    let drag = TrackDrag::new(drag_id.clone(), index, y + this.scroll_y);
                    this.start_drag(Drag::Track(drag), cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.track_menu(&menu_id, e.position, window, cx)
                }),
            )
            .into_any_element()
    }

    /// Input monitoring of an audio track: off, auto (shows A), on. Lit while it monitors.
    fn monitor_key(&self, t: &Track, cx: &App) -> AnyElement {
        let theme = Theme::get(cx);
        let on = t.monitor != Monitor::Off;
        let (next, tip) = match t.monitor {
            Monitor::Off => ("auto", "Input monitoring off · click for Auto"),
            Monitor::Auto => (
                "on",
                "Input monitoring Auto: heard while armed, until the track plays its own clip · click for On",
            ),
            Monitor::On => ("off", "Input monitoring On: always heard · click for Off"),
        };
        let fg = if on {
            theme.text_on_accent
        } else {
            theme.text_2
        };
        let daw = self.daw.clone();
        let id = t.id.clone();
        div()
            .id(SharedString::from(format!("monitor-{}", t.id)))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(20.0))
            .rounded(px(radius::XS))
            .bg(if on { theme.accent_fill } else { theme.control })
            .border_1()
            .border_color(if on {
                theme.accent_fill
            } else {
                theme.control_edge
            })
            .font_family(FONT_MONO)
            .text_size(px(10.0))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(fg)
            .cursor_pointer()
            .hover(|s| {
                s.bg(if on {
                    theme.accent_hover
                } else {
                    theme.control_hover
                })
            })
            .map(|d| {
                if t.monitor == Monitor::Auto {
                    d.child("A")
                } else {
                    d.child(icon("monitor", 11.0, fg))
                }
            })
            .tooltip(move |_, cx| widgets::tip(tip.into(), cx))
            .on_click(move |_, _, cx| {
                let id = id.clone();
                daw.update(cx, |daw, cx| {
                    daw.run(
                        "track.setMonitor",
                        json!({"trackId": id, "monitor": next}),
                        cx,
                    );
                })
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::super::geometry::tests::song;
    use super::kind_label;

    #[test]
    fn the_kind_label_names_the_bus_a_track_feeds() {
        let mut s = song();
        assert_eq!(kind_label(&s.tracks[0], &s.tracks), "MIDI");
        let mut bus = s.tracks[1].clone();
        bus.id = "bus".into();
        bus.name = "Drum bus".into();
        bus.kind = "bus".into();
        s.tracks.push(bus);
        s.tracks[1].output = Some("bus".into());
        assert_eq!(kind_label(&s.tracks[1], &s.tracks), "AUD → Drum bus");
        assert_eq!(kind_label(&s.tracks[2], &s.tracks), "BUS");
    }
}
