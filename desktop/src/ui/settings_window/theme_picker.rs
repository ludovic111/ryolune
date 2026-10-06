//! Dark, Light or Auto for the one ryolune theme (lsuite v2: black and white), each shown as
//! the window it gives: a miniature painted with the real tokens of that mode. Auto is split
//! down the middle. The chosen card is outlined in ink and its name inverted.

use crate::ui::theme::{radius, size, Mode, Theme, FONT_MONO};
use gpui::{div, prelude::*, px, relative, AnyElement, App, Hsla, Window};
use std::rc::Rc;

pub(crate) const CHOICES: [(&str, &str, &str); 3] = [
    ("dark", "Dark", "White ink on black, for long sessions"),
    ("light", "Light", "Black ink on paper, for daylight"),
    ("auto", "Auto", "Follows the system"),
];

/// Regions in the miniature: (track palette index, left, width) as fractions of the lane.
const CLIPS: [(usize, f32, f32); 3] = [(0, 0.08, 0.46), (1, 0.08, 0.7), (2, 0.3, 0.52)];

/// A window in miniature, in one mode.
fn preview(mode: Mode) -> gpui::Div {
    let t = Theme::new(mode, true);
    let key = |color: Hsla| {
        div()
            .size(px(9.0))
            .rounded(px(radius::XS))
            .bg(color)
            .border_1()
            .border_color(t.control_edge)
    };
    div()
        .relative()
        .size_full()
        .bg(t.lane)
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.0))
                .h(px(22.0))
                .px(px(6.0))
                .bg(t.glass_1.opaque)
                .border_b_1()
                .border_color(t.line)
                .child(key(t.accent_fill))
                .child(key(t.control))
                .child(
                    div()
                        .px(px(4.0))
                        .rounded(px(radius::XS))
                        .bg(t.display)
                        .font_family(FONT_MONO)
                        .text_size(px(7.5))
                        .text_color(t.text_display)
                        .child("004·2·1"),
                )
                .child(
                    // A knob, round like the real one.
                    div()
                        .size(px(10.0))
                        .rounded_full()
                        .bg(t.knob)
                        .border_1()
                        .border_color(t.knob_edge),
                ),
        )
        .children(CLIPS.iter().map(|(track, left, width)| {
            div()
                .relative()
                .flex_1()
                .border_b_1()
                .border_color(t.hairline)
                .child(
                    div()
                        .absolute()
                        .top(px(3.0))
                        .bottom(px(3.0))
                        .left(relative(*left))
                        .w(relative(*width))
                        .rounded(px(radius::XS))
                        .bg(t.track("", *track).opacity(0.85)),
                )
        }))
        .child(
            div()
                .absolute()
                .top(px(22.0))
                .bottom_0()
                .left(relative(0.42))
                .w(px(1.0))
                .bg(t.accent),
        )
}

/// The three cards. `on_mode` receives `dark`, `light` or `auto`.
pub(crate) fn picker(
    mode: &str,
    on_mode: impl Fn(&'static str, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = Theme::get(cx).clone();
    let setting = if CHOICES.iter().any(|c| c.0 == mode) {
        mode
    } else {
        "dark"
    };
    let on_mode = Rc::new(on_mode);
    div()
        .flex()
        .gap(px(10.0))
        .children(CHOICES.iter().map(|(id, name, blurb)| {
            let selected = *id == setting;
            let on_mode = on_mode.clone();
            let id = *id;
            let miniature = div()
                .h(px(86.0))
                .w_full()
                .rounded(px(radius::SM))
                .overflow_hidden()
                .border_1()
                .border_color(theme.line);
            let miniature = match id {
                "auto" => miniature
                    .flex()
                    .child(
                        div()
                            .w(relative(0.5))
                            .h_full()
                            .overflow_hidden()
                            .child(preview(Mode::Dark)),
                    )
                    .child(
                        div()
                            .w(relative(0.5))
                            .h_full()
                            .overflow_hidden()
                            .child(preview(Mode::Light)),
                    ),
                "light" => miniature.child(preview(Mode::Light)),
                _ => miniature.child(preview(Mode::Dark)),
            };
            div()
                .id(id)
                .flex()
                .flex_col()
                .gap(px(4.0))
                .flex_1()
                .p(px(8.0))
                .rounded(px(radius::MD))
                .bg(theme.bg_raised)
                .border_1()
                .border_color(if selected {
                    theme.accent_fill
                } else {
                    theme.line
                })
                .cursor_pointer()
                .hover(|d| d.border_color(theme.line_strong))
                .child(miniature)
                .child(
                    div()
                        .mt(px(4.0))
                        .px(px(4.0))
                        .text_size(px(size::BASE))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(if selected {
                            theme.text_on_accent
                        } else {
                            theme.text
                        })
                        .when(selected, |d| d.bg(theme.accent_fill))
                        .child(*name),
                )
                .child(
                    div()
                        .text_size(px(size::SM))
                        .text_color(theme.text_3)
                        .child(*blurb),
                )
                .on_click(move |_, window, cx| on_mode(id, window, cx))
        }))
        .into_any_element()
}
