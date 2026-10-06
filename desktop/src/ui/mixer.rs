//! The mixer, in place of the region editor (`ui.showPanel panel=mixer`, X): one strip per
//! track in arrangement order (bus tracks among them), then the A and B returns and the
//! Stereo Out pinned at the right. A strip shows its kind and effect count (or where it is
//! routed), pan, the fader with its post-fader meter, the level, the channel keys and its
//! name; the selected one is outlined in the accent and a click selects it. Strips are solid
//! work surfaces; the same commands as the inspector move them.

use super::{
    actions,
    daw::Daw,
    format, strip,
    theme::{radius, size, Theme, FONT_MONO},
    widgets::{group, panel_info, panel_title, tip, Button, Knob},
};
use gpui::{
    div, prelude::*, px, AnyElement, App, Context, Entity, MouseButton, SharedString, Window,
};
use ryolune_engine::{
    device::Monitoring,
    model::{self, Track, BUS_A, BUS_B, MASTER},
    render::METER_TRACKS,
};
use serde_json::json;

/// Fader box of a strip: the editor pane's height less the strip's labels, keys and pan.
const FADER_H: f32 = 206.0;
const STRIP_W: f32 = 84.0;
const MASTER_W: f32 = 96.0;

pub struct Mixer {
    daw: Entity<Daw>,
}

/// "MIDI · 2 FX", or where a routed track goes: "AUD · → Drums".
pub fn kind_label(kind: &str, fx: usize, output: Option<&str>) -> String {
    match output {
        Some(name) => format!("{} · → {name}", strip::kind_caps(kind)),
        None => format!("{} · {fx} FX", strip::kind_caps(kind)),
    }
}

/// "6 channels · X toggles".
pub fn channel_count(n: usize) -> String {
    format!(
        "{n} {} · X toggles",
        if n == 1 { "channel" } else { "channels" }
    )
}

impl Mixer {
    pub fn new(daw: Entity<Daw>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self { daw }
    }

    /// The strip's frame: solid, outlined in the accent when selected, selecting on press.
    fn frame(
        &self,
        id: &str,
        width: f32,
        selected: bool,
        select: Option<(&'static str, serde_json::Value)>,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = Theme::get(cx);
        let daw = self.daw.clone();
        div()
            .id(SharedString::from(format!("mixer-strip-{id}")))
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .gap(px(6.0))
            .w(px(width))
            .h_full()
            .px(px(5.0))
            .py(px(7.0))
            .rounded(px(radius::SM))
            .bg(theme.bg_raised)
            .border_1()
            .border_color(if selected { theme.accent } else { theme.line })
            .overflow_hidden()
            .when(selected, |d| {
                d.shadow(vec![gpui::BoxShadow {
                    color: theme.accent_soft,
                    offset: gpui::point(px(0.0), px(0.0)),
                    blur_radius: px(0.0),
                    spread_radius: px(1.0),
                }])
            })
            .when_some(select.filter(|_| !selected), |d, (method, params)| {
                d.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    strip::fire(&daw, method, params.clone(), cx)
                })
            })
    }

    fn caps_line(text: String, cx: &App) -> gpui::Div {
        let theme = Theme::get(cx);
        div()
            .max_w_full()
            .truncate()
            .font_family(FONT_MONO)
            .text_size(px(9.5))
            .text_color(theme.text_3)
            .child(text.to_uppercase())
    }

    fn value_line(text: String, cx: &App) -> gpui::Div {
        let theme = Theme::get(cx);
        div()
            .font_family(FONT_MONO)
            .text_size(px(10.5))
            .text_color(theme.text_2)
            .whitespace_nowrap()
            .child(text)
    }

    fn name_line(name: String, color: Option<gpui::Hsla>, cx: &App) -> gpui::Div {
        let theme = Theme::get(cx);
        div()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(5.0))
            .max_w_full()
            .h(px(18.0))
            .text_size(px(size::BASE))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(theme.text)
            .when_some(color, |d, c| {
                d.child(
                    div()
                        .flex_none()
                        .size(px(8.0))
                        .rounded(px(radius::XS))
                        .bg(c),
                )
            })
            .child(div().min_w_0().truncate().child(name))
    }

    fn track_strip(
        &self,
        track: &Track,
        index: usize,
        peak: f32,
        selected: bool,
        blocked: bool,
        cx: &App,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let s = self.daw.read(cx).app.store.session();
        let routed = track.output.as_ref().map(|_| strip::output_name(s, track));
        let label = kind_label(
            &track.kind,
            strip::fx_count(s, &track.id),
            routed.as_deref(),
        );
        let (pan_daw, pan_id) = (self.daw.clone(), track.id.clone());
        let (vol_daw, vol_id) = (self.daw.clone(), track.id.clone());
        let keys = strip::track_keys(
            &self.daw,
            track,
            blocked,
            if track.kind == "audio" { 17.0 } else { 19.0 },
            cx,
        );
        let pan = track.pan;
        self.frame(
            &track.id,
            STRIP_W,
            selected,
            Some(("track.select", json!({ "trackId": track.id }))),
            cx,
        )
        .child(
            Self::caps_line(label.clone(), cx)
                .id(SharedString::from(format!("kind-{}", track.id)))
                .tooltip(move |_, cx| tip(label.clone().into(), cx)),
        )
        .child(
            Knob::new(
                SharedString::from(format!("mixer-pan-{}", track.id)),
                strip::pan_to_knob(pan),
            )
            .size(30.0)
            .bipolar()
            .tooltip(format!("Pan {}", strip::pan_label(pan)))
            .on_change(move |v, phase, _, cx| strip::set_pan(&pan_daw, &pan_id, v, phase, cx)),
        )
        .child(div().flex_1().min_h_0())
        .child(strip::fader_block(
            SharedString::from(format!("mixer-fader-{}", track.id)),
            track.volume,
            &[if track.mute { 0.0 } else { peak }],
            FADER_H,
            false,
            move |v, phase, _, cx| strip::set_volume(&vol_daw, &vol_id, v, phase, cx),
            cx,
        ))
        .child(Self::value_line(
            format!("{} dB", format::db(format::fader_to_db(track.volume), 1)),
            cx,
        ))
        .child(
            div()
                .flex()
                .gap(px(2.0))
                .h(px(20.0))
                .items_center()
                .children(keys),
        )
        .child(Self::name_line(
            track.name.clone(),
            Some(theme.track(&track.color, index)),
            cx,
        ))
        .into_any_element()
    }

    /// An aux return (A · Reverb, B · Delay): its effects in place of a fader, which it does
    /// not have, and the key that opens it in the inspector.
    fn return_strip(&self, id: &'static str, selected: bool, cx: &App) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let s = self.daw.read(cx).app.store.session();
        let effects: Vec<(String, bool)> = strip::inserts(s, id)
            .into_iter()
            .filter(|i| !i.is_empty())
            .map(|i| (i.name.clone(), i.state == "bypassed"))
            .collect();
        let daw = self.daw.clone();
        let (short, name) = match id {
            BUS_A => ("A", "Reverb"),
            _ => ("B", "Delay"),
        };
        self.frame(
            id,
            STRIP_W,
            selected,
            Some(("ui.showPanel", json!({ "panel": id, "visible": true }))),
            cx,
        )
        .child(Self::caps_line(
            format!("AUX {short} · {} FX", effects.len()),
            cx,
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .w_full()
                .pt(px(4.0))
                .children(effects.into_iter().map(|(name, bypassed)| {
                    div()
                        .h(px(18.0))
                        .px(px(5.0))
                        .flex()
                        .items_center()
                        .rounded(px(radius::XS))
                        .bg(theme.control)
                        .border_1()
                        .border_color(theme.control_edge)
                        .text_size(px(size::XS))
                        .text_color(if bypassed { theme.text_3 } else { theme.text })
                        .child(div().min_w_0().truncate().child(name))
                })),
        )
        .child(div().flex_1())
        .child(
            Button::new(SharedString::from(format!("{id}-inserts")), "Inserts…")
                .compact()
                .tooltip("Open this return in the inspector")
                .on_click(move |_, _, cx| {
                    strip::fire(
                        &daw,
                        "ui.showPanel",
                        json!({ "panel": id, "visible": true }),
                        cx,
                    )
                }),
        )
        .child(div().h(px(20.0)))
        .child(Self::name_line(name.into(), Some(theme.accent), cx))
        .into_any_element()
    }

    fn master_strip(&self, volume: f32, peaks: [f32; 2], selected: bool, cx: &App) -> AnyElement {
        let daw = self.daw.clone();
        let vol_daw = self.daw.clone();
        self.frame(
            MASTER,
            MASTER_W,
            selected,
            Some(("ui.showPanel", json!({ "panel": MASTER, "visible": true }))),
            cx,
        )
        .child(Self::caps_line(model::bus_name(MASTER).into(), cx))
        .child(
            div().h(px(30.0)).flex().items_center().child(
                Button::new("master-inserts", "Inserts…")
                    .compact()
                    .tooltip("Open the master strip in the inspector")
                    .on_click(move |_, _, cx| {
                        strip::fire(
                            &daw,
                            "ui.showPanel",
                            json!({ "panel": MASTER, "visible": true }),
                            cx,
                        )
                    }),
            ),
        )
        .child(div().flex_1().min_h_0())
        .child(strip::fader_block(
            "mixer-fader-master",
            volume,
            &peaks,
            FADER_H,
            false,
            move |v, phase, _, cx| strip::set_volume(&vol_daw, MASTER, v, phase, cx),
            cx,
        ))
        .child(Self::value_line(
            format!("{} dB", format::db(format::fader_to_db(volume), 1)),
            cx,
        ))
        .child(div().h(px(20.0)))
        .child(Self::name_line("Master".into(), None, cx))
        .into_any_element()
    }
}

impl Render for Mixer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let app = &self.daw.read(cx).app;
        let s = app.store.session();
        let tracks = s.tracks.clone();
        let selected = s.view.selected_track_id.clone();
        let volume = s.master_volume;
        let (peaks, track_peaks) = app
            .device
            .as_ref()
            .map_or(([0.0; 4], [0.0; METER_TRACKS]), |d| {
                (d.telemetry.peaks(), d.telemetry.track_peaks())
            });
        let blocked = matches!(app.monitoring, Monitoring::FeedbackRisk { .. });
        let is = |id: &str| selected.as_deref() == Some(id);
        let daw = self.daw.clone();

        let tools = {
            let d = self.daw.read(cx);
            group(
                [
                    actions::tool("addBusTrack", "plus", "Bus", true, d).into_any_element(),
                    actions::tool("showMaster", "sliders", "Master", true, d).into_any_element(),
                ],
                cx,
            )
        };
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.0))
            .h(px(super::theme::layout::TOOLBAR))
            .px(px(12.0))
            .bg(theme.glass(1))
            .border_b_1()
            .border_color(theme.line)
            .text_size(px(size::BASE))
            .child(panel_title("Mixer", cx))
            .child(panel_info(channel_count(tracks.len()), cx))
            .child(div().flex_1())
            .child(tools)
            .child(group(
                [Button::new("show-editor", "Editor")
                    .with_icon("pencil")
                    .flush()
                    .tooltip("Back to the region editor (X)")
                    .on_click(move |_, _, cx| {
                        strip::fire(
                            &daw,
                            "ui.showPanel",
                            json!({ "panel": "mixer", "visible": false }),
                            cx,
                        )
                    })
                    .into_any_element()],
                cx,
            ));

        let strips = div()
            .id("mixer-strips")
            .flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .gap(px(6.0))
            .overflow_x_scroll()
            .children(tracks.iter().enumerate().map(|(index, track)| {
                let peak = track_peaks.get(index).copied().unwrap_or(0.0);
                self.track_strip(track, index, peak, is(&track.id), blocked, cx)
            }))
            .when(tracks.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .mx(px(12.0))
                        .text_color(theme.text_3)
                        .child("Add a track to see its channel here."),
                )
            });

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.editor)
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .pt(px(8.0))
                    .pb(px(6.0))
                    .child(strips)
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .gap(px(6.0))
                            .pl(px(6.0))
                            .border_l_1()
                            .border_color(theme.line)
                            .child(self.return_strip(BUS_A, is(BUS_A), cx))
                            .child(self.return_strip(BUS_B, is(BUS_B), cx))
                            .child(self.master_strip(volume, [peaks[0], peaks[1]], is(MASTER), cx)),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_labels_read_as_in_the_design() {
        assert_eq!(kind_label("midi", 2, None), "MIDI · 2 FX");
        assert_eq!(kind_label("audio", 0, Some("Drum Bus")), "AUD · → Drum Bus");
        assert_eq!(kind_label("bus", 1, None), "BUS · 1 FX");
        assert_eq!(channel_count(1), "1 channel · X toggles");
        assert_eq!(channel_count(6), "6 channels · X toggles");
    }
}
