//! Buttons, keys, tabs and switches, and the boxes tools are grouped in. Every control is a
//! value (`RenderOnce`) configured by the caller, which owns the state and passes a callback.
//! lsuite v2: square corners, a chosen thing inverted (paper on ink), red only for record.

use crate::ui::{
    assets,
    theme::{radius, size, with_alpha, Theme, FONT_MONO},
};
use gpui::{div, prelude::*, px, svg, App, ClickEvent, ElementId, Hsla, SharedString, Window};
use std::rc::Rc;

pub type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
pub type SelectHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;
pub type ToggleHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// An icon from `assets/icons`, tinted.
pub fn icon(name: &str, size_px: f32, color: Hsla) -> gpui::Svg {
    svg()
        .path(assets::icon(name))
        .size(px(size_px))
        .flex_none()
        .text_color(color)
}

/// A small label in caps, in the mono face: section titles, display captions.
pub fn caps(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .font_family(FONT_MONO)
        .text_size(px(10.5))
        .text_color(theme.text_3)
        .child(text.into().to_uppercase())
}

/// A section heading: caps in mono running into a hairline, like a drawing.
pub fn heading(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .min_w_0()
        .child(caps(text, cx).text_color(theme.text_2).flex_none())
        .child(div().flex_1().h(px(1.0)).bg(theme.line))
}

/// An area's title, as a sidebar titles itself ("Arrangement", "Mixer", "Inspector").
pub fn panel_title(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .flex_none()
        .whitespace_nowrap()
        .text_size(px(size::BASE))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(theme.text)
        .child(text.into())
}

/// What an area shows, in mono beside its title (`16 bars · 120 bpm`).
pub fn panel_info(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .font_family(FONT_MONO)
        .text_size(px(size::XS))
        .text_color(theme.text_3)
        .child(text.into())
}

/// Height of a [`group`] of tools.
pub const GROUP_H: f32 = 28.0;

/// Tools that belong together in one box, a hairline between each (`Button::flush` inside).
/// Toolbars are made of these: what goes together is boxed together.
pub fn group(items: impl IntoIterator<Item = gpui::AnyElement>, cx: &App) -> gpui::Div {
    let theme = Theme::get(cx);
    let mut d = div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(GROUP_H))
        .border_1()
        .border_color(theme.line_strong)
        .bg(with_alpha(theme.bg_sunken, 0.35));
    for (i, el) in items.into_iter().enumerate() {
        if i > 0 {
            d = d.child(div().flex_none().w(px(1.0)).h_full().bg(theme.line));
        }
        d = d.child(el);
    }
    d
}

/// A tool in a [`group`]: icon and label, or the icon alone where room is short (the tooltip
/// still names it).
pub fn tool(
    id: impl Into<ElementId>,
    icon: &'static str,
    label: &'static str,
    labelled: bool,
    tip: impl Into<SharedString>,
) -> Button {
    let b = if labelled {
        Button::new(id, label).with_icon(icon)
    } else {
        Button::icon(id, icon)
    };
    b.flush().tooltip(tip)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variant {
    /// No fill until hovered: toolbars, rows.
    Ghost,
    /// The default control: a raised face with a crisp edge.
    Raised,
    /// The primary action, filled with the accent.
    Primary,
    /// A destructive or record action.
    Danger,
}

/// A text and/or icon button.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<&'static str>,
    icon_size: f32,
    variant: Variant,
    /// Lit: on, selected, armed. Drawn with the accent (or the given colour).
    lit: bool,
    lit_color: Option<Hsla>,
    disabled: bool,
    compact: bool,
    full_width: bool,
    /// Inside a [`group`]: as tall as the group, no box of its own.
    flush: bool,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: Some(label.into()),
            icon: None,
            icon_size: 12.0,
            variant: Variant::Raised,
            lit: false,
            lit_color: None,
            disabled: false,
            compact: false,
            full_width: false,
            flush: false,
            tooltip: None,
            on_click: None,
        }
    }
    pub fn icon(id: impl Into<ElementId>, icon: &'static str) -> Self {
        Self {
            label: None,
            icon: Some(icon),
            ..Self::new(id, "")
        }
    }
    pub fn with_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn icon_size(mut self, size: f32) -> Self {
        self.icon_size = size;
        self
    }
    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }
    pub fn ghost(self) -> Self {
        self.variant(Variant::Ghost)
    }
    pub fn primary(self) -> Self {
        self.variant(Variant::Primary)
    }
    pub fn danger(self) -> Self {
        self.variant(Variant::Danger)
    }
    pub fn lit(mut self, lit: bool) -> Self {
        self.lit = lit;
        self
    }
    pub fn lit_color(mut self, color: Hsla) -> Self {
        self.lit_color = Some(color);
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }
    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }
    /// Sits in a [`group`]: quiet, as tall as the group, the group draws the box.
    pub fn flush(mut self) -> Self {
        self.flush = true;
        if self.variant == Variant::Raised {
            self.variant = Variant::Ghost;
        }
        self
    }
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        // Lit is chosen: inverted, paper on ink; a lit colour (record) fills with that colour.
        let lit_color = self.lit_color.unwrap_or(theme.accent_fill);
        let (bg, fg, edge, hover) = match (self.variant, self.lit) {
            (_, true) => (
                lit_color,
                theme.text_on_accent,
                lit_color,
                if self.lit_color.is_some() {
                    lit_color
                } else {
                    theme.accent_hover
                },
            ),
            (Variant::Ghost, false) => (
                gpui::transparent_black(),
                theme.text_2,
                gpui::transparent_black(),
                theme.hover,
            ),
            (Variant::Raised, false) => (
                theme.control,
                theme.text,
                theme.control_edge,
                theme.control_hover,
            ),
            (Variant::Primary, false) => (
                theme.accent_fill,
                theme.text_on_accent,
                theme.accent_fill,
                theme.accent_hover,
            ),
            (Variant::Danger, false) => (
                with_alpha(theme.danger, 0.12),
                theme.danger,
                with_alpha(theme.danger, 0.4),
                with_alpha(theme.danger, 0.2),
            ),
        };
        let h = if self.flush {
            GROUP_H - 2.0
        } else if self.compact {
            22.0
        } else {
            28.0
        };
        let icon_only = self.label.is_none();
        let disabled = self.disabled;
        let on_click = self.on_click.clone();
        let tooltip = self.tooltip.clone();
        let primary = self.variant == Variant::Primary && !self.lit && !disabled;
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .h(px(h))
            .when(icon_only, |d| d.w(px(h)))
            .when(!icon_only, |d| {
                d.px(px(if self.compact || self.flush {
                    8.0
                } else {
                    12.0
                }))
            })
            .when(self.full_width, |d| d.w_full())
            .rounded(px(radius::SM))
            .bg(bg)
            .when(!self.flush, |d| d.border_1().border_color(edge))
            // The primary action stands on a small hard shadow.
            .when(primary, |d| d.shadow(theme.chip_shadow()))
            .text_size(px(size::BASE))
            .text_color(fg)
            .when(primary || self.lit, |d| {
                d.font_weight(gpui::FontWeight::SEMIBOLD)
            })
            .when(disabled, |d| d.opacity(0.4))
            .when(!disabled, |d| {
                d.cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .active(|s| s.opacity(0.85))
            })
            .when_some(self.icon, |d, name| d.child(icon(name, self.icon_size, fg)))
            .when_some(self.label, |d, label| {
                d.child(div().whitespace_nowrap().child(label))
            })
            .when_some(on_click.filter(|_| !disabled), |d, f| {
                d.on_click(move |e, w, cx| f(e, w, cx))
            })
            .when_some(tooltip, |d, text| {
                d.tooltip(move |_, cx| tip(text.clone(), cx))
            })
    }
}

/// A plain tooltip view.
pub struct Tip(SharedString);
impl Render for Tip {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx);
        div()
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(radius::SM))
            .bg(theme.glass(2))
            .border_1()
            .border_color(theme.glass_edge)
            .shadow(theme.float_shadow())
            .text_size(px(size::SM))
            .text_color(theme.text)
            .font_family(crate::ui::theme::FONT_UI)
            .child(self.0.clone())
    }
}
pub fn tip(text: SharedString, cx: &mut App) -> gpui::AnyView {
    cx.new(|_| Tip(text)).into()
}

/// A channel key: M (mute), S (solo), R (record arm), A (monitor). Small, square, lit in its
/// own colour.
#[derive(IntoElement)]
pub struct Key {
    id: ElementId,
    label: SharedString,
    on: bool,
    color: Hsla,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
    size: f32,
}
impl Key {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        on: bool,
        color: Hsla,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            on,
            color,
            tooltip: None,
            on_click: None,
            size: 20.0,
        }
    }
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}
impl RenderOnce for Key {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let (bg, fg) = if self.on {
            (self.color, theme.text_on_accent)
        } else {
            (theme.control, theme.text_2)
        };
        let hover = if self.on {
            self.color
        } else {
            theme.control_hover
        };
        let tooltip = self.tooltip.clone();
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(self.size))
            .rounded(px(radius::XS))
            .bg(bg)
            .border_1()
            .border_color(if self.on {
                with_alpha(self.color, 0.9)
            } else {
                theme.control_edge
            })
            .font_family(FONT_MONO)
            .text_size(px(10.0))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(fg)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .child(self.label)
            .when_some(self.on_click, |d, f| {
                d.on_click(move |e, w, cx| f(e, w, cx))
            })
            .when_some(tooltip, |d, text| {
                d.tooltip(move |_, cx| tip(text.clone(), cx))
            })
    }
}

/// Tabs in one box; the chosen one is inverted (paper on ink).
#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    items: Vec<SharedString>,
    selected: usize,
    on_select: Option<SelectHandler>,
    full_width: bool,
}
impl Segmented {
    pub fn new(
        id: impl Into<ElementId>,
        items: impl IntoIterator<Item = impl Into<SharedString>>,
        selected: usize,
    ) -> Self {
        Self {
            id: id.into(),
            items: items.into_iter().map(Into::into).collect(),
            selected,
            on_select: None,
            full_width: false,
        }
    }
    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }
    pub fn on_select(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }
}
impl RenderOnce for Segmented {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let id = self.id.clone();
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .when(self.full_width, |d| d.w_full())
            .p(px(2.0))
            .gap(px(2.0))
            .rounded(px(radius::SM))
            .bg(with_alpha(theme.bg_sunken, 0.35))
            .border_1()
            .border_color(theme.line_strong)
            .children(self.items.into_iter().enumerate().map(|(i, label)| {
                let on = i == self.selected;
                let f = self.on_select.clone();
                div()
                    .id(child_id(&id, i))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(self.full_width, |d| d.flex_1())
                    .h(px(22.0))
                    .px(px(10.0))
                    .rounded(px(radius::SM))
                    .text_size(px(size::BASE))
                    .whitespace_nowrap()
                    .text_color(if on {
                        theme.text_on_accent
                    } else {
                        theme.text_2
                    })
                    .when(on, |d| {
                        d.bg(theme.accent_fill)
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                    })
                    .when(!on, |d| {
                        d.cursor_pointer()
                            .hover(|s| s.bg(theme.hover).text_color(theme.text))
                    })
                    .child(label)
                    .when_some(f, |d, f| d.on_click(move |_, w, cx| f(i, w, cx)))
            }))
    }
}

/// An on/off switch for settings.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    on: bool,
    on_toggle: Option<ToggleHandler>,
}
impl Switch {
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            on_toggle: None,
        }
    }
    pub fn on_toggle(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(f));
        self
    }
}
impl RenderOnce for Switch {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let on = self.on;
        let f = self.on_toggle.clone();
        // Square, like everything: a box with a square knob, lit (inverted) when on.
        div()
            .id(self.id)
            .flex_none()
            .w(px(30.0))
            .h(px(18.0))
            .p(px(2.0))
            .bg(if on { theme.accent_fill } else { theme.well })
            .border_1()
            .border_color(if on {
                theme.accent_fill
            } else {
                theme.line_strong
            })
            .cursor_pointer()
            .child(
                div()
                    .size(px(12.0))
                    .bg(if on {
                        theme.text_on_accent
                    } else {
                        theme.text_3
                    })
                    .when(on, |d| d.ml(px(12.0))),
            )
            .when_some(f, |d, f| d.on_click(move |_, w, cx| f(!on, w, cx)))
    }
}

/// An id under another: a tab of a tab bar, a row of a list.
pub fn child_id(parent: &ElementId, key: impl ToString) -> ElementId {
    ElementId::NamedChild(Box::new(parent.clone()), key.to_string().into())
}

/// A coloured square: a family swatch, a track colour, a status light (square, like every
/// corner in v2).
pub fn dot(color: Hsla, size_px: f32) -> gpui::Div {
    div().flex_none().size(px(size_px)).bg(color)
}
