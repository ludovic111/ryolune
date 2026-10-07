//! The Chat tab: the welcome while the agent is not connected or nothing was said yet, then
//! the conversation from the host's transcript. The person's messages sit in bubbles at the
//! right with the selection that went along as a caption; the agent's replies read at full
//! width as Markdown, with the steps it took between them. It follows the end while a reply
//! streams, unless the person scrolled up to read.

use super::{
    context, markdown,
    steps::{self, Item},
    AgentPanel,
};
use crate::{
    agent::{Entry, Role},
    ui::{
        theme::{radius, size, Theme, FONT_MONO},
        widgets::{icon, Button},
    },
};
use gpui::{div, prelude::*, px, AnyElement, Context, FontWeight, SharedString, Window};
use std::{collections::HashMap, rc::Rc};

/// Ideas that fill the message box; the person decides when to send.
const STARTERS: [(&str, &str); 3] = [
    (
        "Start a beat",
        "Add a four-bar drum groove at the current tempo. Keep the rest of my project.",
    ),
    (
        "Write some chords",
        "Add a warm four-bar chord progression that fits this project, on a new instrument track.",
    ),
    (
        "Help with my mix",
        "Inspect my mix and suggest three improvements. Explain them before changing anything.",
    ),
];

/// How far from the end still counts as following it.
const FOLLOW_SLACK: f32 = 80.0;

/// Parsed replies by entry id, so a frame re-parses only the reply that changed.
#[derive(Default)]
pub struct MarkdownCache(HashMap<u64, (usize, bool, Rc<Vec<markdown::Block>>)>);

impl MarkdownCache {
    pub fn clear(&mut self) {
        self.0.clear();
    }
    fn get(&mut self, id: u64, entry: &Entry) -> Rc<Vec<markdown::Block>> {
        let fresh = |e: &Entry| Rc::new(markdown::parse(&e.text));
        match self.0.get(&id) {
            Some((len, streaming, blocks))
                if *len == entry.text.len() && *streaming == entry.streaming =>
            {
                blocks.clone()
            }
            _ => {
                let blocks = fresh(entry);
                self.0
                    .insert(id, (entry.text.len(), entry.streaming, blocks.clone()));
                blocks
            }
        }
    }
    /// Forget replies trimmed from the transcript.
    fn retain_from(&mut self, first_id: u64) {
        self.0.retain(|id, _| *id >= first_id);
    }
}

pub(super) fn welcome(title: &str, text: impl Into<SharedString>, cx: &gpui::App) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .py(px(18.0))
        .px(px(2.0))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .when(!title.is_empty(), |d| {
            d.child(
                div()
                    .text_size(px(size::BASE + 1.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.text)
                    .child(title.to_string()),
            )
        })
        .child(
            div()
                .text_size(px(size::BASE))
                .line_height(px(21.0))
                .text_color(theme.text_2)
                .child(text.into()),
        )
}

impl AgentPanel {
    pub(super) fn chat(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let daw = self.daw.clone();
        let app = &daw.read(cx).app;
        let runtime = &app.agents.runtime;
        let busy = runtime.running();
        let ready = self.ready();

        // Follow the end while it grows, unless the person scrolled up to read.
        let last = runtime.transcript.last();
        let seen = (
            runtime.first_id + runtime.transcript.len() as u64,
            last.map_or(0, |e| e.text.len()),
            last.is_some_and(|e| e.streaming),
        );
        let max = self.scroll.max_offset().height;
        let near_end = f32::from(max + self.scroll.offset().y) < FOLLOW_SLACK;
        if std::mem::take(&mut self.force_follow) || (seen != self.seen && near_end) {
            self.scroll.scroll_to_bottom();
        }
        self.seen = seen;
        self.markdown.retain_from(runtime.first_id);

        let mut column = div().w_full().min_w_0().flex().flex_col();
        if !ready {
            let (title, text) = if self.checking {
                (
                    "Checking your agent…",
                    "ryolune is asking your AI service whether it is ready. Nothing is sent."
                        .to_string(),
                )
            } else {
                (
                    "Connect your music assistant",
                    [
                        self.connection_error.as_str(),
                        self.connection.as_ref().map_or("", |c| c.message.as_str()),
                    ]
                    .into_iter()
                    .find(|m| !m.is_empty())
                    .unwrap_or("Choose an AI service and connect your account. Then describe your ideas in your own words.")
                    .to_string(),
                )
            };
            let mut card = welcome(title, text, cx);
            if !self.checking {
                card = card.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.0))
                        .child(
                            Button::new("setup-agent", "Set up agent")
                                .primary()
                                .on_click(cx.listener(|this, _, _, cx| this.open_settings("agent", cx))),
                        )
                        .child(
                            Button::new("check-again", "Check again")
                                .on_click(cx.listener(|this, _, _, cx| this.check_connection(cx))),
                        )
                        .child(
                            Button::new(
                                "use-another-agent",
                                if self.external.is_some() {
                                    "Hide outside agents"
                                } else {
                                    "Use another agent"
                                },
                            )
                            .ghost()
                            .tooltip("Connect Claude Code, Codex, Cursor or any MCP client to this window")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.external = match this.external.take() {
                                    Some(_) => None,
                                    None => {
                                        let daw = this.daw.clone();
                                        Some(cx.new(|cx| super::ExternalAgents::new(daw, cx)))
                                    }
                                };
                                cx.notify();
                            })),
                        ),
                );
                if let Some(external) = &self.external {
                    card = card.child(div().mt(px(8.0)).child(external.clone()));
                }
            }
            column = column.child(card);
        }
        if runtime.transcript.is_empty() {
            let title = if ready {
                "What would you like to make?"
            } else {
                "A few ideas to get started"
            };
            // The empty conversation is the panel's hero: a dithered fade of light behind it
            // and one word set in negative.
            let hero = if ready {
                div()
                    .flex()
                    .flex_wrap()
                    .items_baseline()
                    .gap(px(5.0))
                    .text_size(px(size::XL))
                    .line_height(px(28.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.text)
                    .child("What would you like to")
                    .child(
                        div()
                            .px(px(5.0))
                            .bg(theme.accent_fill)
                            .text_color(theme.text_on_accent)
                            .child("make?"),
                    )
                    .into_any_element()
            } else {
                div().into_any_element()
            };
            column = column.child(
                div()
                    .relative()
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left(px(-14.0))
                            .child(crate::ui::grain::dither(380.0, 140.0, 0.2, window, cx)),
                    )
                    .when(ready, |d| d.child(div().pt(px(26.0)).child(hero))),
            );
            column = column.child(
                welcome(
                    if ready { "" } else { title },
                    "Ask for a beat, a melody or help with your mix. You can write in your own language.",
                    cx,
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .my(px(6.0))
                        .children(STARTERS.iter().map(|&(title, prompt)| {
                            div()
                                .id(title)
                                .flex()
                                .items_center()
                                .justify_between()
                                .px(px(12.0))
                                .py(px(10.0))
                                .rounded(px(radius::SM))
                                .bg(theme.control)
                                .border_1()
                                .border_color(theme.control_edge)
                                .text_size(px(size::BASE))
                                .text_color(theme.text)
                                .child(title)
                                .child(icon("arrow-up-right", 10.0, theme.accent_text))
                                .when(busy, |d| d.opacity(0.4))
                                .when(!busy, |d| {
                                    d.cursor_pointer()
                                        .hover(|s| s.bg(theme.control_hover))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_draft(prompt, cx)
                                        }))
                                })
                        })),
                )
                .child(
                    div()
                        .text_size(px(size::SM))
                        .text_color(theme.text_3)
                        .child("Suggestions fill your message. You choose when to send it."),
                ),
            );
        }

        let entries = &runtime.transcript;
        for item in steps::thread_items(entries, runtime.first_id) {
            column = column.child(match item {
                Item::Message { key, index } => {
                    let entry = &entries[index];
                    if entry.role == Role::User {
                        user_message(&entry.text, &theme)
                    } else {
                        let blocks = self.markdown.get(key, entry);
                        agent_message(&blocks, key, entry.streaming, cx)
                    }
                }
                Item::Steps { key, indices } => self.steps(key, &indices, entries, cx),
            });
        }
        if let Some(turn) = self.turn_card("chat-turn", cx) {
            column = column.child(div().mt(px(8.0)).child(turn));
        }
        column.into_any_element()
    }

    /// What the agent did between two messages. Edits are listed; a run of pure reading
    /// collapses to one line, because "looked at the project" six times is noise.
    fn steps(
        &mut self,
        key: u64,
        indices: &[usize],
        entries: &[Entry],
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let tools: Vec<_> = indices
            .iter()
            .filter_map(|&i| entries[i].tool.as_ref())
            .collect();
        let edits: Vec<(usize, _)> = tools
            .iter()
            .enumerate()
            .filter(|(_, t)| !steps::is_reading(&t.name) || !steps::tool_ok(t))
            .map(|(n, t)| (n, *t))
            .collect();
        let reads = tools.len() - edits.len();
        div()
            .min_w_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .mt(px(2.0))
            .mb(px(10.0))
            .pl(px(10.0))
            .border_l_2()
            .border_color(theme.line_strong)
            .text_size(px(size::SM))
            .when(reads > 0, |d| {
                d.child(
                    div()
                        .py(px(2.0))
                        .text_color(theme.text_3)
                        .child(if reads > 1 {
                            format!("Looked at your project · {reads} checks")
                        } else {
                            "Looked at your project".to_string()
                        }),
                )
            })
            .children(edits.into_iter().map(|(n, tool)| {
                let open = self.open_steps.contains(&(key, n));
                let ok = steps::tool_ok(tool);
                let pending = tool.result.is_none();
                let label = steps::describe_tool(&tool.name, &tool.args);
                let why = steps::tool_error(tool);
                let command = format!(
                    "{} {}",
                    tool.name.replacen('_', ".", 1),
                    serde_json::to_string_pretty(&tool.args).unwrap_or_default()
                );
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id(SharedString::from(format!("step-{key}-{n}")))
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .py(px(2.0))
                            .cursor_pointer()
                            .text_color(theme.text_2)
                            .hover(|s| s.text_color(theme.text))
                            .child(div().w(px(10.0)).flex_none().child(if pending {
                                div().size(px(5.0)).bg(theme.accent).into_any_element()
                            } else if ok {
                                icon("check", 9.0, theme.accent).into_any_element()
                            } else {
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme.danger)
                                    .child("!")
                                    .into_any_element()
                            }))
                            .child(
                                // One line: a long label ends in an ellipsis inside the panel.
                                div()
                                    .min_w_0()
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(label),
                            )
                            .when(!ok, |d| {
                                d.child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .whitespace_nowrap()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .text_color(theme.danger)
                                        .child(if why.is_empty() {
                                            "This step did not work".to_string()
                                        } else {
                                            why
                                        }),
                                )
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.open_steps.remove(&(key, n)) {
                                    this.open_steps.insert((key, n));
                                }
                                cx.notify();
                            })),
                    )
                    .when(open, |d| {
                        d.child(
                            div()
                                .id(SharedString::from(format!("step-cmd-{key}-{n}")))
                                .ml(px(16.0))
                                .mt(px(2.0))
                                .mb(px(6.0))
                                .max_h(px(140.0))
                                .overflow_y_scroll()
                                .font_family(FONT_MONO)
                                .text_size(px(size::XS))
                                .text_color(theme.text_3)
                                .child(command),
                        )
                    })
            }))
            .into_any_element()
    }
}

/// What the person typed, with the selection that went along shown as a quiet caption.
fn user_message(text: &str, theme: &Theme) -> AnyElement {
    let (words, selected) = context::split_context(text);
    div()
        .py(px(10.0))
        .flex()
        .flex_col()
        .items_end()
        .child(
            div()
                .max_w(gpui::relative(0.88))
                .px(px(12.0))
                .py(px(8.0))
                .rounded_tl(px(14.0))
                .rounded_tr(px(14.0))
                .rounded_bl(px(14.0))
                .rounded_br(px(4.0))
                .bg(theme.control)
                .border_1()
                .border_color(theme.control_edge)
                .text_size(px(size::BASE + 0.5))
                .line_height(px(21.0))
                .text_color(theme.text)
                .child(words.to_string())
                .when(!selected.is_empty(), |d| {
                    d.child(
                        div()
                            .mt(px(4.0))
                            .text_size(px(size::SM))
                            .line_height(px(16.0))
                            .text_color(theme.text_3)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(format!("about {}", context::caption(selected))),
                    )
                }),
        )
        .into_any_element()
}

/// A reply: the agent's caption in the accent, the Markdown, and a caret while it streams.
fn agent_message(
    blocks: &[markdown::Block],
    key: u64,
    streaming: bool,
    cx: &gpui::App,
) -> AnyElement {
    let theme = Theme::get(cx);
    div()
        .py(px(10.0))
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .font_family(FONT_MONO)
                .text_size(px(10.5))
                .text_color(theme.accent_text)
                .child("AGENT"),
        )
        .child(
            markdown::render(blocks, &format!("reply-{key}"), cx)
                .text_size(px(size::BASE + 1.0))
                .line_height(px(23.0)),
        )
        .when(streaming, |d| {
            d.child(
                div()
                    .w(px(6.0))
                    .h(px(13.0))
                    .rounded(px(radius::XS))
                    .bg(theme.accent),
            )
        })
        .into_any_element()
}
