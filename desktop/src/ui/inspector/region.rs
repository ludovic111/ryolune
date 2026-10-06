//! The inspector's Region section: the selected clip's name, start bar and length, and for an
//! audio clip its gain, fade curve and fades. Each field commits on Enter or when focus leaves
//! (one registry command, and none when the value did not change, so tabbing through adds no
//! empty undo step); Escape puts the clip's value back.

use crate::ui::{
    daw::Daw,
    theme::{radius, size, Theme, FONT_MONO},
    widgets::{select_button, InputEvent, MenuHost, MenuItem, TextInput},
};
use gpui::{div, prelude::*, px, Context, Entity, MouseButton, Subscription, Window};
use ryolune_engine::model::{Clip, ClipData, FadeCurve, CLIP_GAIN_MAX_DB, CLIP_GAIN_MIN_DB};
use serde_json::json;

/// A field of the section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Name,
    Bar,
    Length,
    Gain,
    FadeIn,
    FadeOut,
}
const FIELDS: [Field; 6] = [
    Field::Name,
    Field::Bar,
    Field::Length,
    Field::Gain,
    Field::FadeIn,
    Field::FadeOut,
];

/// A number as a field shows it: no trailing zeros, at most four decimals.
pub fn num(v: f64) -> String {
    let text = format!("{v:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".into()
    } else {
        text.into()
    }
}

pub fn curve_label(curve: FadeCurve) -> &'static str {
    match curve {
        FadeCurve::EqualPower => "Equal power",
        FadeCurve::Linear => "Linear",
        FadeCurve::Exponential => "Exponential",
    }
}

/// What a field shows for a clip; `None` when the clip has no such value (MIDI fades).
fn value_of(clip: &Clip, field: Field) -> Option<String> {
    let audio = match &clip.data {
        ClipData::Audio {
            fade_in,
            fade_out,
            gain_db,
            ..
        } => Some((*fade_in, *fade_out, *gain_db)),
        ClipData::Midi { .. } => None,
    };
    Some(match field {
        Field::Name => clip.name.clone(),
        Field::Bar => num(clip.start_bar + 1.0),
        Field::Length => num(clip.length_bars),
        Field::Gain => num(((audio?.2 * 10.0).round() / 10.0) as f64),
        Field::FadeIn => num((audio?.0 * 1000.0).round()),
        Field::FadeOut => num((audio?.1 * 1000.0).round()),
    })
}

/// The command a field's text asks for, or `None` when it is unreadable or changes nothing.
fn command_for(clip: &Clip, field: Field, text: &str) -> Option<(&'static str, serde_json::Value)> {
    let text = text.trim();
    let id = clip.id.clone();
    let number = || {
        text.replace('−', "-")
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
    };
    match field {
        Field::Name => (!text.is_empty() && text != clip.name)
            .then(|| ("clip.rename", json!({ "clipId": id, "name": text }))),
        Field::Bar => {
            let start = number()? - 1.0;
            (start >= 0.0 && start != clip.start_bar)
                .then(|| ("clip.move", json!({ "clipId": id, "startBar": start })))
        }
        Field::Length => {
            let length = number()?;
            (length > 0.0 && length != clip.length_bars)
                .then(|| ("clip.resize", json!({ "clipId": id, "lengthBars": length })))
        }
        Field::Gain => {
            let ClipData::Audio { gain_db, .. } = &clip.data else {
                return None;
            };
            let db = number()?.clamp(CLIP_GAIN_MIN_DB as f64, CLIP_GAIN_MAX_DB as f64);
            (db as f32 != *gain_db).then(|| ("clip.setGain", json!({ "clipId": id, "gainDb": db })))
        }
        Field::FadeIn | Field::FadeOut => {
            let ClipData::Audio {
                fade_in, fade_out, ..
            } = &clip.data
            else {
                return None;
            };
            let seconds = number()?.max(0.0) / 1000.0;
            let (key, current) = if field == Field::FadeIn {
                ("fadeInSeconds", *fade_in)
            } else {
                ("fadeOutSeconds", *fade_out)
            };
            ((seconds - current).abs() > 1e-9)
                .then(|| ("clip.setFades", json!({ "clipId": id, key: seconds })))
        }
    }
}

pub struct Region {
    daw: Entity<Daw>,
    inputs: Vec<(Field, Entity<TextInput>)>,
    /// The clip and values the fields last showed.
    shown: String,
    menu: MenuHost,
    _subscriptions: Vec<Subscription>,
}

impl Region {
    pub fn new(daw: Entity<Daw>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = vec![cx.observe_in(&daw, window, |this, _, window, cx| {
            this.sync(false, window, cx)
        })];
        let inputs: Vec<_> = FIELDS
            .iter()
            .map(|&field| {
                let input = cx.new(|cx| {
                    let input = TextInput::new(cx);
                    if field == Field::Name {
                        input
                    } else {
                        input.mono()
                    }
                });
                subscriptions.push(cx.subscribe_in(
                    &input,
                    window,
                    move |this, input, event, window, cx| match event {
                        InputEvent::Submit => {
                            this.commit(field, input, cx);
                            window.blur();
                        }
                        InputEvent::Blur => this.commit(field, input, cx),
                        InputEvent::Cancel => {
                            this.sync(true, window, cx);
                            window.blur();
                        }
                        InputEvent::Changed => {}
                    },
                ));
                (field, input)
            })
            .collect();
        let mut region = Self {
            daw,
            inputs,
            shown: String::new(),
            menu: MenuHost::default(),
            _subscriptions: subscriptions,
        };
        region.sync(true, window, cx);
        region
    }

    fn clip<'a>(&self, cx: &'a gpui::App) -> Option<&'a Clip> {
        let s = self.daw.read(cx).app.store.session();
        let id = s.view.selected_clip_id.as_deref()?;
        s.clips.iter().find(|c| c.id == id)
    }

    /// Show the selected clip's values in the fields that are not being typed in, whenever
    /// the clip or one of its values changed (or always, to undo a draft).
    fn sync(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(clip) = self.clip(cx).cloned() else {
            self.shown.clear();
            return;
        };
        let values: Vec<_> = FIELDS.iter().map(|f| value_of(&clip, *f)).collect();
        let key = format!("{}|{values:?}", clip.id);
        if key == self.shown && !force {
            return;
        }
        self.shown = key;
        for ((_, input), value) in self.inputs.iter().zip(values) {
            if force || !input.read(cx).is_focused(window) {
                input.update(cx, |input, cx| {
                    input.set_text(value.unwrap_or_default(), cx)
                });
            }
        }
        cx.notify();
    }

    fn commit(&mut self, field: Field, input: &Entity<TextInput>, cx: &mut Context<Self>) {
        let Some(clip) = self.clip(cx).cloned() else {
            return;
        };
        let text = input.read(cx).text().to_string();
        match command_for(&clip, field, &text) {
            Some((method, params)) => {
                self.daw.update(cx, |daw, cx| {
                    daw.run(method, params, cx);
                });
            }
            // Unreadable or unchanged: show the clip's value again.
            None => input.update(cx, |input, cx| {
                input.set_text(value_of(&clip, field).unwrap_or_default(), cx)
            }),
        }
    }

    fn input(&self, field: Field) -> Entity<TextInput> {
        self.inputs
            .iter()
            .find(|(f, _)| *f == field)
            .map(|(_, i)| i.clone())
            .expect("every field has an input")
    }

    /// A labelled value in a well: the label at the left, the field at the right.
    fn cell(
        &self,
        label: &'static str,
        field: Field,
        unit: Option<&'static str>,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let theme = Theme::get(cx);
        let input = self.input(field);
        let focused = input.read(cx).is_focused(window);
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(px(6.0))
            .h(px(28.0))
            .px(px(8.0))
            .rounded(px(radius::SM))
            .bg(theme.well)
            .border_1()
            .border_color(if focused {
                theme.accent_ring
            } else {
                theme.line
            })
            .child(
                div()
                    .flex_none()
                    .text_size(px(size::SM))
                    .text_color(theme.text_3)
                    .child(label),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_family(FONT_MONO)
                    .text_size(px(size::SM))
                    .text_color(theme.text)
                    .child(input),
            )
            .when_some(unit, |d, unit| {
                d.child(
                    div()
                        .flex_none()
                        .font_family(FONT_MONO)
                        .text_size(px(10.5))
                        .text_color(theme.text_3)
                        .child(unit),
                )
            })
    }
}

impl Render for Region {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let Some(clip) = self.clip(cx).cloned() else {
            return div().into_any_element();
        };
        let audio = match &clip.data {
            ClipData::Audio { fade_curve, .. } => Some(*fade_curve),
            ClipData::Midi { .. } => None,
        };
        let meta = match &clip.data {
            ClipData::Midi { notes, .. } => format!(
                "{} {}",
                notes.len(),
                if notes.len() == 1 { "note" } else { "notes" }
            ),
            ClipData::Audio { .. } => "Audio".into(),
        };
        let name = self.input(Field::Name);
        let name_focused = name.read(cx).is_focused(window);
        let menu = self.menu.render(window, cx);
        let row = || div().flex().gap(px(8.0)).w_full();
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .px(px(14.0))
            .pt(px(10.0))
            .pb(px(14.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(crate::ui::widgets::heading("Region", cx).flex_1())
                    .child(
                        div()
                            .font_family(FONT_MONO)
                            .text_size(px(10.5))
                            .text_color(theme.text_3)
                            .child(meta),
                    ),
            )
            .child(
                div()
                    .h(px(30.0))
                    .flex()
                    .items_center()
                    .px(px(8.0))
                    .rounded(px(radius::SM))
                    .bg(theme.well)
                    .border_1()
                    .border_color(if name_focused { theme.accent_ring } else { theme.line })
                    .text_size(px(size::MD))
                    .text_color(theme.text)
                    .child(div().flex_1().child(name)),
            )
            .child(
                row()
                    .child(self.cell("Bar", Field::Bar, None, window, cx))
                    .child(self.cell("Length", Field::Length, None, window, cx)),
            )
            .when_some(audio, |d, curve| {
                let daw = self.daw.clone();
                let clip_id = clip.id.clone();
                d.child(
                    row()
                        .child(self.cell("Gain", Field::Gain, Some("dB"), window, cx))
                        .child(
                            div().flex_1().min_w_0().child(
                                select_button("fade-curve", curve_label(curve), cx)
                                    .h(px(28.0))
                                    .tooltip(|_, cx| crate::ui::widgets::tip("Fade shape".into(), cx))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, e: &gpui::MouseDownEvent, window, cx| {
                                            let items = FadeCurve::ALL
                                                .iter()
                                                .map(|&c| {
                                                    let (daw, clip_id) = (daw.clone(), clip_id.clone());
                                                    MenuItem::new(curve_label(c), move |_, cx| {
                                                        crate::ui::strip::fire(
                                                            &daw,
                                                            "clip.setFades",
                                                            json!({ "clipId": clip_id, "curve": c.as_str() }),
                                                            cx,
                                                        )
                                                    })
                                                    .checked(c == curve)
                                                })
                                                .collect();
                                            this.menu.open(items, e.position, window, cx);
                                        }),
                                    ),
                            ),
                        ),
                )
                .child(
                    row()
                        .child(self.cell("Fade in", Field::FadeIn, Some("ms"), window, cx))
                        .child(self.cell("Out", Field::FadeOut, Some("ms"), window, cx)),
                )
            })
            .children(menu)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(data: ClipData) -> Clip {
        Clip {
            id: "c1".into(),
            name: "Chords".into(),
            agent: false,
            track_id: "t1".into(),
            start_bar: 2.0,
            length_bars: 6.0,
            data,
        }
    }

    #[test]
    fn fields_show_one_based_bars_and_milliseconds() {
        let midi = clip(ClipData::Midi {
            notes: vec![],
            controllers: vec![],
        });
        assert_eq!(value_of(&midi, Field::Bar).unwrap(), "3");
        assert_eq!(value_of(&midi, Field::Length).unwrap(), "6");
        assert_eq!(value_of(&midi, Field::Gain), None);
        let mut audio = clip(ClipData::audio("s1", 0.0));
        if let ClipData::Audio {
            fade_in, gain_db, ..
        } = &mut audio.data
        {
            *fade_in = 0.25;
            *gain_db = -3.04;
        }
        assert_eq!(value_of(&audio, Field::FadeIn).unwrap(), "250");
        assert_eq!(value_of(&audio, Field::Gain).unwrap(), "-3");
        assert_eq!(num(0.0625), "0.0625");
        assert_eq!(num(-0.0), "0");
    }

    #[test]
    fn unchanged_or_unreadable_fields_send_nothing() {
        let c = clip(ClipData::audio("s1", 0.0));
        assert_eq!(command_for(&c, Field::Bar, "3"), None);
        assert_eq!(command_for(&c, Field::Bar, "x"), None);
        assert_eq!(command_for(&c, Field::Bar, "0"), None);
        assert_eq!(command_for(&c, Field::Name, "  "), None);
        assert_eq!(command_for(&c, Field::Length, "-1"), None);
        let (method, params) = command_for(&c, Field::Bar, "5").unwrap();
        assert_eq!(method, "clip.move");
        assert_eq!(params["startBar"], 4.0);
        let (method, params) = command_for(&c, Field::FadeOut, "120").unwrap();
        assert_eq!(method, "clip.setFades");
        assert!((params["fadeOutSeconds"].as_f64().unwrap() - 0.12).abs() < 1e-9);
        let (_, params) = command_for(&c, Field::Gain, "40").unwrap();
        assert_eq!(params["gainDb"], 24.0);
        assert_eq!(command_for(&c, Field::Gain, "0"), None);
        let midi = clip(ClipData::Midi {
            notes: vec![],
            controllers: vec![],
        });
        assert_eq!(command_for(&midi, Field::FadeIn, "100"), None);
    }
}
