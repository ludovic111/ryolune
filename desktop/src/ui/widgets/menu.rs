//! Popup menus: the title bar's menus, context menus (right click) and selects. A menu is
//! glass tier 2. Arrow keys move, Enter picks, Escape or a click outside dismisses and
//! focus returns where it was.
//!
//! A view that opens menus keeps a [`MenuHost`] and renders `host.render()` as its last
//! child; `host.open(items, position, window, cx)` shows one.

use crate::ui::{
    actions,
    daw::Daw,
    theme::{radius, size, Theme, FONT_UI},
};
use gpui::{
    actions as gpui_actions, anchored, deferred, div, prelude::*, px, AnyElement, App, Context,
    DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding, Pixels, Point,
    SharedString, Subscription, Window,
};
use std::rc::Rc;

gpui_actions!(menu, [Next, Previous, Confirm, Cancel]);

pub fn bind(cx: &mut App) {
    let c = Some("PopupMenu");
    cx.bind_keys([
        KeyBinding::new("down", Next, c),
        KeyBinding::new("up", Previous, c),
        KeyBinding::new("enter", Confirm, c),
        KeyBinding::new("escape", Cancel, c),
    ]);
}

pub type Pick = Rc<dyn Fn(&mut Window, &mut App)>;

pub enum MenuItem {
    Item {
        label: SharedString,
        detail: Option<SharedString>,
        checked: Option<bool>,
        disabled: bool,
        /// A colour swatch before the label (a track colour, a family).
        swatch: Option<gpui::Hsla>,
        pick: Pick,
    },
    Separator,
    Header(SharedString),
}

impl MenuItem {
    pub fn new(
        label: impl Into<SharedString>,
        pick: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        MenuItem::Item {
            label: label.into(),
            detail: None,
            checked: None,
            disabled: false,
            swatch: None,
            pick: Rc::new(pick),
        }
    }
    pub fn detail(mut self, text: impl Into<SharedString>) -> Self {
        if let MenuItem::Item { detail, .. } = &mut self {
            *detail = Some(text.into());
        }
        self
    }
    pub fn checked(mut self, on: bool) -> Self {
        if let MenuItem::Item { checked, .. } = &mut self {
            *checked = Some(on);
        }
        self
    }
    pub fn disabled(mut self, off: bool) -> Self {
        if let MenuItem::Item { disabled, .. } = &mut self {
            *disabled = off;
        }
        self
    }
    pub fn swatch(mut self, color: gpui::Hsla) -> Self {
        if let MenuItem::Item { swatch, .. } = &mut self {
            *swatch = Some(color);
        }
        self
    }
    /// A row for a table action: its label, shortcut, check mark and whether it can run.
    pub fn action(id: &'static str, daw: &Entity<Daw>, cx: &App) -> Self {
        let state = daw.read(cx);
        let def = actions::def(id);
        let label = def.map_or(id, |d| d.label);
        let mut item = MenuItem::new(label, move |window, cx| {
            window.dispatch_action(Box::new(actions::Do { id }), cx);
        })
        .disabled(!actions::enabled(id, state));
        if let Some(on) = actions::checked(id, state) {
            item = item.checked(on);
        }
        if let Some(keys) = actions::shortcut_label(id) {
            item = item.detail(keys);
        }
        item
    }
    fn selectable(&self) -> bool {
        matches!(
            self,
            MenuItem::Item {
                disabled: false,
                ..
            }
        )
    }
}

pub struct PopupMenu {
    items: Vec<MenuItem>,
    focus: FocusHandle,
    hovered: Option<usize>,
    min_width: Pixels,
}

impl EventEmitter<DismissEvent> for PopupMenu {}
impl Focusable for PopupMenu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PopupMenu {
    pub fn new(items: Vec<MenuItem>, cx: &mut Context<Self>) -> Self {
        Self {
            items,
            focus: cx.focus_handle(),
            hovered: None,
            min_width: px(200.0),
        }
    }
    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let n = self.items.len();
        if n == 0 {
            return;
        }
        let mut i = self.hovered.unwrap_or(if forward { n - 1 } else { 0 });
        for _ in 0..n {
            i = if forward {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            };
            if self.items[i].selectable() {
                self.hovered = Some(i);
                break;
            }
        }
        cx.notify();
    }
    fn pick(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(MenuItem::Item {
            pick,
            disabled: false,
            ..
        }) = self.items.get(index)
        {
            let pick = pick.clone();
            cx.emit(DismissEvent);
            pick(window, cx);
        }
    }
}

impl Render for PopupMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let hovered = self.hovered;
        div()
            .id("popup-menu")
            .key_context("PopupMenu")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Next, _, cx| this.step(true, cx)))
            .on_action(cx.listener(|this, _: &Previous, _, cx| this.step(false, cx)))
            .on_action(cx.listener(|this, _: &Confirm, window, cx| {
                if let Some(i) = this.hovered {
                    this.pick(i, window, cx);
                }
            }))
            .on_action(cx.listener(|_, _: &Cancel, _, cx| cx.emit(DismissEvent)))
            .on_mouse_down_out(cx.listener(|_, _, _, cx| cx.emit(DismissEvent)))
            // Clicks on the menu stop here: nothing under it sees them.
            .occlude()
            .font_family(FONT_UI)
            .min_w(self.min_width)
            .max_h(px(560.0))
            .overflow_y_scroll()
            .py(px(4.0))
            .rounded(px(radius::MD))
            .bg(theme.glass(2))
            .border_1()
            .border_color(theme.glass_edge)
            .shadow(theme.float_shadow())
            .text_size(px(size::BASE))
            .children(self.items.iter().enumerate().map(|(i, item)| {
                match item {
                    MenuItem::Separator => div()
                        .my(px(4.0))
                        .h(px(1.0))
                        .bg(theme.line)
                        .into_any_element(),
                    MenuItem::Header(text) => div()
                        .px(px(12.0))
                        .pt(px(6.0))
                        .pb(px(2.0))
                        .text_size(px(size::XS))
                        .text_color(theme.text_3)
                        .child(text.clone())
                        .into_any_element(),
                    MenuItem::Item {
                        label,
                        detail,
                        checked,
                        disabled,
                        swatch,
                        ..
                    } => {
                        // The item under the pointer is inverted, paper on ink.
                        let on = hovered == Some(i) && !disabled;
                        let ink = if on {
                            theme.text_on_accent
                        } else {
                            theme.accent_text
                        };
                        div()
                            .id(i)
                            .mx(px(4.0))
                            .px(px(8.0))
                            .h(px(26.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .rounded(px(radius::SM))
                            .text_color(if *disabled {
                                theme.text_3
                            } else if on {
                                theme.text_on_accent
                            } else {
                                theme.text
                            })
                            .when(on, |d| d.bg(theme.accent_fill))
                            .when(!disabled, |d| {
                                d.cursor_pointer().on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.pick(i, window, cx)
                                    }),
                                )
                            })
                            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                                if *over {
                                    this.hovered = Some(i);
                                    cx.notify();
                                }
                            }))
                            .child(
                                div()
                                    .w(px(12.0))
                                    .flex_none()
                                    .when(checked == &Some(true), |d| {
                                        d.child(super::controls::icon("check", 10.0, ink))
                                    }),
                            )
                            .when_some(*swatch, |d, c| d.child(super::controls::dot(c, 8.0)))
                            .child(div().flex_1().whitespace_nowrap().child(label.clone()))
                            .when_some(detail.clone(), |d, text| {
                                d.child(
                                    div()
                                        .pl(px(16.0))
                                        .text_size(px(size::SM))
                                        .text_color(if on { ink } else { theme.text_3 })
                                        .child(text),
                                )
                            })
                            .into_any_element()
                    }
                }
            }))
    }
}

/// Keeps the open menu of a view, where it shows and the subscription that closes it.
#[derive(Default)]
pub struct MenuHost {
    open: Option<(Entity<PopupMenu>, Point<Pixels>, Subscription)>,
}

impl MenuHost {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
    pub fn open<V: 'static>(
        &mut self,
        items: Vec<MenuItem>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<V>,
    ) {
        let menu = cx.new(|cx| PopupMenu::new(items, cx));
        self.show(menu, position, window, cx);
    }
    pub fn show<V: 'static>(
        &mut self,
        menu: Entity<PopupMenu>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<V>,
    ) {
        let previous = window.focused(cx);
        let subscription =
            cx.subscribe_in(&menu, window, move |_, _, _: &DismissEvent, window, cx| {
                if let Some(previous) = &previous {
                    window.focus(previous);
                }
                cx.notify();
            });
        window.focus(&menu.focus_handle(cx));
        self.open = Some((menu, position, subscription));
        cx.notify();
    }
    pub fn close(&mut self) {
        self.open = None;
    }
    /// The open menu, anchored where it was asked, above everything. Drops a dismissed one.
    pub fn render(&mut self, window: &Window, cx: &App) -> Option<AnyElement> {
        let (menu, position, _) = self.open.as_ref()?;
        if !menu.focus_handle(cx).contains_focused(window, cx) {
            // Dismissed (or focus went elsewhere): forget it.
            self.open = None;
            return None;
        }
        Some(
            deferred(
                anchored()
                    .position(*position)
                    .snap_to_window_with_margin(px(8.0))
                    .child(menu.clone()),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// A select: a button showing the current choice that opens the choices as a menu.
pub fn select_button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::get(cx);
    div()
        .id(id.into())
        .flex()
        .items_center()
        .justify_between()
        .gap(px(6.0))
        .h(px(26.0))
        .px(px(8.0))
        .rounded(px(radius::SM))
        .bg(theme.control)
        .border_1()
        .border_color(theme.control_edge)
        .text_size(px(size::BASE))
        .text_color(theme.text)
        .cursor_pointer()
        .hover(|s| s.bg(theme.control_hover))
        .child(
            div()
                .whitespace_nowrap()
                .overflow_hidden()
                .child(label.into()),
        )
        .child(super::controls::icon("chevron-down", 9.0, theme.text_3))
}
