//! The Changes tab: every command an agent ran against this window (the built-in one, an
//! MCP client, the CLI), newest first, from the host's log (`agent.changes`). An edit can
//! be undone from there, which also undoes everything after it, and redone up to it
//! (`agent.revert`, `redo`); each shows its CLI form and, on demand, the full answer.

use super::AgentPanel;
use crate::{
    agent::Role,
    ui::{
        theme::{radius, size, Theme, FONT_MONO},
        widgets::{icon, Button},
    },
};
use gpui::{div, prelude::*, px, AnyElement, ClipboardItem, Context, FontWeight, SharedString};
use serde_json::json;

impl AgentPanel {
    pub(super) fn changes(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let turn_card = self.turn_card("changes-turn", cx);
        let theme = Theme::get(cx).clone();
        let daw = self.daw.clone();
        let app = &daw.read(cx).app;
        let busy = app.agents.runtime.running();
        let depth = app.store.undo_depth();
        let notices: Vec<String> = app
            .agents
            .runtime
            .transcript
            .iter()
            .filter(|e| e.role == Role::Notice)
            .map(|e| e.text.clone())
            .collect();
        let empty = app.agents.change_count() == 0;
        let mut column = div().flex().flex_col().gap(px(8.0)).py(px(8.0));
        for (n, text) in notices.into_iter().enumerate() {
            column = column.child(
                card(&theme)
                    .id(("notice", n))
                    .child(title("Details", &theme))
                    .child(
                        div()
                            .text_size(px(size::SM))
                            .line_height(px(18.0))
                            .text_color(theme.text_2)
                            .child(text),
                    ),
            );
        }
        if empty {
            column = column.child(super::chat::welcome(
                "Your activity will appear here",
                "When the agent edits your project, you can review the result and undo it here.",
                cx,
            ));
        } else {
            column = column.child(
                div()
                    .px(px(2.0))
                    .text_size(px(size::SM))
                    .line_height(px(18.0))
                    .text_color(theme.text_2)
                    .child("Undo from here also undoes later edits, including yours. Stop the agent before reviewing changes."),
            );
        }
        if let Some(turn) = turn_card {
            column = column.child(turn);
        }
        for row in app.agents.change_rows(depth) {
            let sequence = row.sequence;
            let open = self.open_changes.contains(&sequence);
            let undone = row.mutated && !row.applied;
            let detail = row.detail.to_string();
            let applied = row.applied;
            column = column.child(
                card(&theme)
                    .id(("change", sequence as usize))
                    .when(undone, |d| d.opacity(0.6))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(if row.succeeded {
                                        theme.text
                                    } else {
                                        theme.danger
                                    })
                                    .when(undone, |d| d.line_through())
                                    .child(row.title.to_string()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .font_family(FONT_MONO)
                                    .text_size(px(10.0))
                                    .text_color(if row.running {
                                        theme.accent_text
                                    } else {
                                        theme.text_3
                                    })
                                    .child(if row.running {
                                        "RUNNING"
                                    } else if !row.succeeded {
                                        "FAILED"
                                    } else if undone {
                                        "UNDONE"
                                    } else if row.mutated {
                                        "APPLIED"
                                    } else {
                                        ""
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .font_family(FONT_MONO)
                                    .text_size(px(size::XS))
                                    .line_height(px(15.0))
                                    .text_color(theme.text_3)
                                    .child(detail.clone()),
                            )
                            .child(
                                Button::icon(("copy-cli", sequence as usize), "copy")
                                    .ghost()
                                    .compact()
                                    .tooltip("Copy the CLI command")
                                    .on_click(move |_, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            detail.clone(),
                                        ))
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(8.0))
                            .when(row.mutated, |d| {
                                d.child(
                                    Button::new(
                                        ("revert", sequence as usize),
                                        if applied {
                                            "Undo from here"
                                        } else {
                                            "Redo to here"
                                        },
                                    )
                                    .compact()
                                    .disabled(busy)
                                    .tooltip(if applied {
                                        "Undo this change and all later edits"
                                    } else {
                                        "Redo through this change"
                                    })
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.daw.update(cx, |daw, cx| {
                                                daw.run(
                                                    "agent.revert",
                                                    json!({"sequence": sequence, "redo": !applied}),
                                                    cx,
                                                );
                                            })
                                        },
                                    )),
                                )
                            })
                            .child(
                                div()
                                    .id(("details", sequence as usize))
                                    .flex()
                                    .items_center()
                                    .gap(px(4.0))
                                    .text_size(px(size::SM))
                                    .text_color(theme.text_2)
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(theme.text))
                                    .child(icon(
                                        if open {
                                            "chevron-down"
                                        } else {
                                            "chevron-right"
                                        },
                                        8.0,
                                        theme.text_3,
                                    ))
                                    .child(if row.succeeded {
                                        "Details"
                                    } else {
                                        "Error details"
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !this.open_changes.remove(&sequence) {
                                            this.open_changes.insert(sequence);
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .when(open, |d| {
                        d.child(
                            div()
                                .id(("output", sequence as usize))
                                .max_h(px(200.0))
                                .overflow_y_scroll()
                                .p(px(8.0))
                                .rounded(px(radius::SM))
                                .bg(theme.well)
                                .font_family(FONT_MONO)
                                .text_size(px(10.5))
                                .line_height(px(15.0))
                                .text_color(theme.text_2)
                                .child(SharedString::from(row.output.to_string())),
                        )
                    }),
            );
        }
        column.into_any_element()
    }
}

impl AgentPanel {
    /// The last turn of the built-in agent, once it ended with changes: what it changed and
    /// one button that reverts (or restores) all of it (lsuite's HARNESS.md part 6).
    pub(super) fn turn_card(
        &mut self,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let theme = Theme::get(cx).clone();
        let daw = self.daw.clone();
        let app = &daw.read(cx).app;
        let turn = app.agents.turn.as_ref()?;
        if turn.running || turn.changes.is_empty() || app.agents.runtime.running() {
            return None;
        }
        let reverted = turn.reverted;
        let count = turn.changes.len();
        let shown: Vec<String> = turn.changes.iter().take(6).cloned().collect();
        let more = count.saturating_sub(shown.len());
        let mut list = div().flex().flex_col().gap(px(2.0));
        for line in shown {
            list = list.child(
                div()
                    .text_size(px(size::SM))
                    .line_height(px(17.0))
                    .text_color(theme.text_2)
                    .when(reverted, |d| d.line_through())
                    .child(format!("· {line}")),
            );
        }
        if more > 0 {
            list = list.child(
                div()
                    .text_size(px(size::SM))
                    .text_color(theme.text_3)
                    .child(format!("and {more} more")),
            );
        }
        Some(
            card(&theme)
                .id(id)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.text)
                                .child(if reverted {
                                    "Last turn reverted".to_string()
                                } else {
                                    format!(
                                        "This turn made {count} change{}",
                                        if count == 1 { "" } else { "s" }
                                    )
                                }),
                        )
                        .child(
                            Button::new(
                                (id, 1usize),
                                if reverted { "Redo turn" } else { "Revert turn" },
                            )
                            .compact()
                            .tooltip(if reverted {
                                "Bring back everything the agent did in its last turn"
                            } else {
                                "Undo everything the agent did in its last turn, in one step"
                            })
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.daw.update(cx, |daw, cx| {
                                        daw.run(
                                            "agent.revertTurn",
                                            json!({ "redo": reverted }),
                                            cx,
                                        );
                                    })
                                },
                            )),
                        ),
                )
                .child(list)
                .into_any_element(),
        )
    }
}

/// One entry: a raised slab in the panel.
fn card(theme: &Theme) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .p(px(10.0))
        .rounded(px(radius::MD))
        .bg(theme.control.opacity(0.55))
        .border_1()
        .border_color(theme.line)
        .text_size(px(size::BASE))
}

fn title(text: &str, theme: &Theme) -> gpui::Div {
    div()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text)
        .child(text.to_string())
}
