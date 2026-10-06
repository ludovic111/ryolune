//! The composer under the conversation: the status line (with Stop while the agent works),
//! errors in plain words with what to do next, and one raised card holding what the
//! message is about (the selection, as chips), the slash menu, the message box and its bar
//! (model and reasoning, the send hint, Send). Enter sends, Shift+Enter starts a new line.

use super::{connection, context, slash, steps, AgentPanel, Tab};
use crate::ui::{
    theme::{radius, size, Theme},
    widgets::{icon, Button},
};
use gpui::{div, prelude::*, px, AnyElement, Context, FontWeight, SharedString, Window};

impl AgentPanel {
    pub(super) fn composer_area(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let daw = self.daw.clone();
        let app = &daw.read(cx).app;
        let runtime = &app.agents.runtime;
        let running = runtime.running();
        let ready = self.ready();
        let error = if self.composer_error.is_empty() {
            runtime.last_error.clone().unwrap_or_default()
        } else {
            self.composer_error.clone()
        };
        let status = if !ready {
            "Connect a service to send"
        } else {
            steps::human_status(&runtime.status, running, !error.is_empty())
        };
        let draft = self.draft(cx);
        let last_request = runtime
            .transcript
            .iter()
            .rev()
            .find(|e| e.role == crate::agent::Role::User)
            .map(|e| context::split_context(&e.text).0.to_string());
        let chips = context::selection_context(app.store.session());
        let matches = slash::matches(&draft);
        let can_send = !draft.trim().is_empty() && !running && ready && !self.checking;
        let focused = self.composer.read(cx).is_focused(window);
        let model_button = self.model_button(cx);
        let model_menu = self.models.open.then(|| self.model_menu(window, cx));

        let status_row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .mx(px(2.0))
            .mb(px(8.0))
            .min_h(px(22.0))
            .text_size(px(size::SM))
            .text_color(theme.text_2)
            .child(status)
            .when(running, |d| {
                d.child(
                    Button::new("agent-stop", "Stop")
                        .compact()
                        .tooltip("Stop the agent after the current step")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.daw.update(cx, |daw, cx| {
                                daw.fire("agent.stop", cx);
                            })
                        })),
                )
            });

        let error_box = (!error.is_empty()).then(|| {
            let unsent = !self.composer_error.is_empty();
            div()
                .id("agent-error")
                .mb(px(8.0))
                .p(px(10.0))
                .max_h(px(160.0))
                .overflow_y_scroll()
                .rounded(px(radius::SM))
                .bg(theme.well)
                .border_1()
                .border_color(theme.danger.opacity(0.4))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(px(size::SM))
                        .line_height(px(18.0))
                        .text_color(theme.text)
                        .child(connection::error_message(&error, unsent)),
                )
                .child(
                    div()
                        .text_size(px(size::XS))
                        .text_color(theme.text_3)
                        .child(SharedString::from(error.clone())),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.0))
                        .child(
                            Button::new("error-settings", "Check agent settings")
                                .compact()
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.open_settings("agent", cx)),
                                ),
                        )
                        .when_some(
                            last_request.filter(|_| draft.is_empty() && !running),
                            |d, last| {
                                d.child(
                                    Button::new("error-edit", "Edit last request")
                                        .compact()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_draft(&last, cx)
                                        })),
                                )
                            },
                        )
                        .child(
                            Button::new("error-changes", "View changes")
                                .compact()
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.select_tab(Tab::Changes, cx)),
                                ),
                        ),
                )
        });

        let about = if chips.is_empty() {
            div()
                .text_color(theme.text_3)
                .child("The agent sees your whole project.")
                .into_any_element()
        } else {
            let on = self.with_context;
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(5.0))
                .child(
                    div()
                        .id("about-toggle")
                        .flex_none()
                        .text_color(theme.text_3)
                        .underline()
                        .text_decoration_color(theme.text_3)
                        .cursor_pointer()
                        .hover(|s| s.text_color(theme.text))
                        .child(if on { "About" } else { "Not about" })
                        .tooltip(move |_, cx| {
                            crate::ui::widgets::tip(
                                if on {
                                    "Your selection goes with the message. Click to send without it."
                                } else {
                                    "Click to send your selection with the message."
                                }
                                .into(),
                                cx,
                            )
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.with_context = !this.with_context;
                            cx.notify();
                        })),
                )
                .children(chips.into_iter().map(|chip| {
                    let detail: SharedString = chip.detail.into();
                    div()
                        .id(SharedString::from(format!("chip-{}", chip.label)))
                        .flex_none()
                        .max_w_full()
                        .px(px(7.0))
                        .py(px(1.0))
                        .rounded(px(radius::SM))
                        .border_1()
                        .border_color(if on { theme.accent } else { theme.line })
                        .text_color(if on { theme.text } else { theme.text_3 })
                        .when(!on, |d| d.line_through())
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(chip.label)
                        .tooltip(move |_, cx| crate::ui::widgets::tip(detail.clone(), cx))
                }))
                .into_any_element()
        };

        let slash_menu = (!matches.is_empty()).then(|| {
            let selected = self.slash_index.min(matches.len() - 1);
            div()
                .id("slash-menu")
                .max_h(px(235.0))
                .overflow_y_scroll()
                .p(px(4.0))
                .rounded(px(radius::MD))
                .bg(theme.glass(2))
                .border_1()
                .border_color(theme.glass_edge)
                .children(matches.into_iter().enumerate().map(|(i, command)| {
                    div()
                        .id(command.name)
                        .flex()
                        .gap(px(12.0))
                        .px(px(8.0))
                        .py(px(7.0))
                        .rounded(px(radius::SM))
                        .text_size(px(size::SM))
                        .cursor_pointer()
                        // The chosen command is inverted, paper on ink.
                        .when(i == selected, |d| d.bg(theme.accent_fill))
                        .when(i != selected, |d| d.hover(|s| s.bg(theme.hover)))
                        .child(
                            div()
                                .min_w(px(75.0))
                                .flex_none()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if i == selected {
                                    theme.text_on_accent
                                } else {
                                    theme.accent_text
                                })
                                .child(format!("/{}", command.name)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .text_color(if i == selected {
                                    theme.text_on_accent
                                } else {
                                    theme.text_2
                                })
                                .child(command.label),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.choose_slash(command, cx)))
                }))
        });

        let send = div()
            .id("agent-send")
            .flex_none()
            .size(px(30.0))
            .flex()
            .items_center()
            .justify_center()
            .when(can_send, |d| {
                d.bg(theme.accent_fill)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.accent_hover))
                    // The primary action stands on a small hard shadow.
                    .shadow(theme.chip_shadow())
            })
            .when(!can_send, |d| {
                d.bg(theme.control)
                    .border_1()
                    .border_color(theme.control_edge)
            })
            .child(icon(
                "send",
                12.0,
                if can_send {
                    theme.text_on_accent
                } else {
                    theme.text_3
                },
            ))
            .tooltip(|_, cx| crate::ui::widgets::tip("Send (Enter)".into(), cx))
            .when(can_send, |d| {
                d.on_click(cx.listener(|this, _, _, cx| this.send(cx)))
            });

        let card = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .pt(px(10.0))
            .px(px(10.0))
            .pb(px(8.0))
            .rounded(px(radius::LG))
            .bg(theme.glass(2))
            .border_1()
            .border_color(if focused {
                theme.accent_ring
            } else {
                theme.glass_edge
            })
            // The composer sits in viewfinder brackets, like the suite's other composers.
            .child(crate::ui::grain::brackets(
                10.0,
                -6.0,
                if focused { theme.text_2 } else { theme.text_3 },
            ))
            .child(
                div()
                    .text_size(px(size::SM))
                    .text_color(theme.text_2)
                    .child(about),
            )
            .children(slash_menu)
            .child(
                div()
                    .id("agent-message")
                    .min_h(px(60.0))
                    .max_h(px(200.0))
                    .overflow_y_scroll()
                    .px(px(4.0))
                    .text_size(px(size::BASE + 1.0))
                    .text_color(theme.text)
                    .cursor_text()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.composer.update(cx, |input, _| input.focus(window))
                    }))
                    .child(self.composer.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(model_button)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_size(px(size::XS))
                            .text_color(theme.text_3)
                            .child("Enter to send · Shift Enter for a new line"),
                    )
                    .child(send),
            )
            .children(model_menu);

        div()
            .flex_none()
            .px(px(12.0))
            .pt(px(8.0))
            .pb(px(12.0))
            .border_t_1()
            .border_color(theme.hairline)
            .child(status_row)
            .children(error_box)
            .child(card)
            .into_any_element()
    }
}
