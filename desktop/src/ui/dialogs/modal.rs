//! What every modal shares: the scrim layer that takes the keyboard and the pointer, the
//! glass sheet (tier 3) with its title and close key, the footer of actions, focus that
//! returns where it was when the modal closes, and registry calls whose answer arrives on a
//! later frame. The dialogs and the Settings window are built from these.
//!
//! Keys: Escape dismisses the topmost modal ([`Dismiss`]), Enter takes its primary action
//! ([`Accept`]). While a modal has the keyboard the window's shortcuts are held back (the
//! session must not play or change under a question), except Quit.

use crate::ui::{
    actions::Do,
    daw::Daw,
    theme::{radius, size, Theme, FONT_MONO},
    widgets::{icon, surface, Button},
};
use gpui::{
    div, prelude::*, px, relative, App, Context, Entity, FocusHandle, Global, KeyBinding,
    MouseButton, SharedString, Window,
};
use ryolune_engine::Result;
use serde_json::Value;
use std::{sync::mpsc, time::Duration};

gpui::actions!(modal, [Dismiss, Accept]);

/// Window shortcuts that still run while a modal is open.
const THROUGH: [&str; 1] = ["quit"];

/// What the close key of a sheet runs.
pub(crate) type CloseHandler = Box<dyn Fn(&mut Window, &mut App)>;

struct Bound;
impl Global for Bound {}

/// Bind Escape and Enter for modals. Called when the first modal view is made, after the
/// window's own shortcuts, so on a tie (Enter is also Go to Beginning) the modal wins.
pub(crate) fn bind(cx: &mut App) {
    if cx.has_global::<Bound>() {
        return;
    }
    cx.set_global(Bound);
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, Some("Modal")),
        KeyBinding::new("enter", Accept, Some("Modal")),
    ]);
}

/// The full-window layer under a modal: the scrim, the keyboard (key context `Modal`) and
/// every pointer event, so nothing behind it can be clicked or scrolled.
pub(crate) fn layer(id: &'static str, focus: &FocusHandle, cx: &App) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::get(cx);
    div()
        .id(id)
        .key_context("Modal")
        .track_focus(focus)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .bg(theme.scrim)
        .flex()
        .items_center()
        .justify_center()
        .capture_action(|action: &Do, _, cx| {
            if !THROUGH.contains(&action.id) {
                cx.stop_propagation();
            }
        })
}

/// A modal sheet: glass tier 3 inside viewfinder brackets, a title bar with the close key,
/// then the caller's children.
/// `on_close` runs for the close key (the layer handles Escape).
pub(crate) fn sheet(
    id: &'static str,
    title: impl Into<SharedString>,
    width: f32,
    on_close: Option<CloseHandler>,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let theme = Theme::get(cx);
    let title: SharedString = title.into();
    surface(3, cx)
        .id(id)
        .w(px(width))
        .max_w(relative(0.92))
        .max_h(relative(0.88))
        .relative()
        .flex()
        .flex_col()
        // A click on the sheet is not a click on the scrim.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        // Framed like a viewfinder, the marks just outside the sheet.
        .child(crate::ui::grain::brackets(14.0, -9.0, theme.text_3))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.0))
                .pl(px(20.0))
                .pr(px(10.0))
                .pt(px(12.0))
                .pb(px(6.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(size::MD))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child(title.clone()),
                )
                .when_some(on_close, |d, f| {
                    d.child(
                        Button::icon(SharedString::from(format!("{id}-close")), "close")
                            .ghost()
                            .icon_size(10.0)
                            .tooltip(format!("Close {title} (Esc)"))
                            .on_click(move |_, window, cx| f(window, cx)),
                    )
                }),
        )
}

/// The sheet's body: padded, scrolling when taller than the window.
pub(crate) fn body(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(px(12.0))
        .px(px(20.0))
        .pt(px(4.0))
        .pb(px(16.0))
        .min_h_0()
        .flex_shrink()
        .overflow_y_scroll()
}

/// The row of actions at the bottom of a sheet, right aligned, primary last.
pub(crate) fn footer(cx: &App) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_end()
        .gap(px(8.0))
        .px(px(20.0))
        .py(px(12.0))
        .border_t_1()
        .border_color(theme.hairline)
}

/// Body text of a dialog.
pub(crate) fn text(content: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .text_size(px(size::BASE))
        .line_height(relative(1.45))
        .text_color(theme.text_2)
        .child(content.into())
}

/// A quiet note under a form: hints, privacy, what happens next.
pub(crate) fn note(content: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .text_size(px(size::SM))
        .line_height(relative(1.4))
        .text_color(theme.text_3)
        .child(content.into())
}

/// An error line inside a form.
pub(crate) fn error_line(content: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .flex()
        .items_start()
        .gap(px(6.0))
        .text_size(px(size::SM))
        .line_height(relative(1.4))
        .text_color(theme.danger)
        .child(div().pt(px(2.0)).child(icon("warning", 11.0, theme.danger)))
        .child(div().flex_1().min_w_0().child(content.into()))
}

/// Text in a sunken well, in the mono face: paths, reports, configuration to copy.
pub(crate) fn well(content: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .px(px(10.0))
        .py(px(8.0))
        .rounded(px(radius::SM))
        .bg(theme.well)
        .border_1()
        .border_color(theme.hairline)
        .font_family(FONT_MONO)
        .text_size(px(size::SM))
        .line_height(relative(1.45))
        .text_color(theme.text_2)
        .child(content.into())
}

/// A settings or form row: the label and what it does at the left, the control at the right.
pub(crate) fn field_row(
    label: impl Into<SharedString>,
    description: Option<SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.0))
        .min_h(px(44.0))
        .py(px(6.0))
        .border_b_1()
        .border_color(theme.hairline)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(size::BASE))
                        .text_color(theme.text)
                        .child(label.into()),
                )
                .when_some(description, |d, text| {
                    d.child(
                        div()
                            .text_size(px(size::SM))
                            .line_height(relative(1.35))
                            .text_color(theme.text_3)
                            .child(text),
                    )
                }),
        )
        .child(div().flex_none().flex().items_center().child(control))
}

/// A form field stacked under its label, for text that needs the width.
pub(crate) fn stacked(
    label: impl Into<SharedString>,
    hint: Option<SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .child(
            div()
                .text_size(px(size::SM))
                .text_color(theme.text_2)
                .child(label.into()),
        )
        .child(control)
        .when_some(hint, |d, text| d.child(note(text, cx)))
}

/// A heading inside a sheet or a settings section.
pub(crate) fn heading(title: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .text_size(px(size::LG))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(theme.text)
        .child(title.into())
}

/// Keeps a modal's focus: takes the keyboard when the modal opens and gives it back to
/// what had it when the modal closes.
pub(crate) struct ModalFocus {
    pub handle: FocusHandle,
    open: bool,
    previous: Option<FocusHandle>,
}

impl ModalFocus {
    pub fn new(cx: &mut App) -> Self {
        Self {
            handle: cx.focus_handle(),
            open: false,
            previous: None,
        }
    }
    /// Call with whether the modal shows now; it moves focus on a change.
    pub fn sync(&mut self, open: bool, window: &mut Window, cx: &mut App) {
        if open && !self.open {
            self.previous = window
                .focused(cx)
                .filter(|focused| !self.handle.contains(focused, window));
            window.focus(&self.handle);
        } else if open && window.focused(cx).is_none() {
            // A click on the scrim or a closed menu left nothing focused: keep the keys here.
            window.focus(&self.handle);
        } else if !open && self.open {
            let ours = window
                .focused(cx)
                .is_none_or(|focused| self.handle.contains(&focused, window));
            if let Some(previous) = self.previous.take().filter(|_| ours) {
                window.focus(&previous);
            }
        }
        self.open = open;
    }
}

/// Run a registry command from a modal and hand its answer to `done`, now or, for commands
/// that answer on a later frame (a connection check, the model list, an update check), when
/// the job finishes. The window never waits.
pub(crate) fn request_async<V: 'static>(
    view: &mut V,
    daw: &Entity<Daw>,
    method: &str,
    params: Value,
    cx: &mut Context<V>,
    done: impl FnOnce(&mut V, Result<Value>, &mut Context<V>) + 'static,
) {
    let result = daw.update(cx, |daw, cx| daw.request(method, params, cx));
    let running = result
        .as_ref()
        .is_ok_and(|value| value["status"] == "running");
    if !running {
        done(view, result, cx);
        return;
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let attached = daw.update(cx, |daw, _| {
        daw.app
            .attach_reply(crate::control::Reply::Channel(tx))
            .is_ok()
    });
    if !attached {
        done(view, result, cx);
        return;
    }
    cx.spawn(async move |this, cx| {
        let answer = loop {
            match rx.try_recv() {
                Ok(answer) => break answer,
                Err(mpsc::TryRecvError::Disconnected) => {
                    break Err("The window stopped waiting for this answer".into())
                }
                Err(mpsc::TryRecvError::Empty) => {
                    cx.background_executor()
                        .timer(Duration::from_millis(40))
                        .await
                }
            }
        };
        let _ = this.update(cx, |view, cx| done(view, answer, cx));
    })
    .detach();
}
