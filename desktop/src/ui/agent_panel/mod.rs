//! The agent panel at the right edge (glass tier 1): a 32 px rail when closed, 380 px open
//! with the header, the tabs (Chat, Generate, Changes, Takes) and the composer. The
//! conversation, the Changes log and the runtime live in the host (`crate::agents`,
//! `crate::agent`); the panel reads them through the `Daw` entity and acts only through
//! the registry (`agent.*`, `generate.*`, `take.*`, `ui.showPanel`), like the CLI, MCP
//! clients and the agent itself. What is local here is the window's: the message being
//! written, which tab shows, which details are open, the menus.

mod changes;
mod chat;
mod composer;
pub mod connection;
pub mod context;
pub mod external;
#[cfg(debug_assertions)]
mod fixture;
mod generate;
pub mod jobs;
pub mod markdown;
mod models;
pub mod slash;
pub mod steps;
mod takes;

pub use external::ExternalAgents;

use super::{
    daw::Daw,
    theme::{radius, size, with_alpha, Theme, FONT_MONO},
    widgets::{text_input, Button, InputEvent, MenuHost, TextInput},
};
use connection::Connection;
use gpui::{
    div, prelude::*, px, AnyElement, App, Context, Entity, FontWeight, ScrollHandle, Subscription,
    Task, Window,
};
use serde_json::json;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Chat,
    Generate,
    Changes,
    Takes,
}

pub struct AgentPanel {
    daw: Entity<Daw>,
    tab: Tab,
    /// The message being written. It outlives collapsing the panel.
    composer: Entity<TextInput>,
    /// Why the last message did not go; it stays in the box.
    composer_error: String,
    /// Send the selection along with the message.
    with_context: bool,
    slash_index: usize,
    confirm_clear: bool,
    /// Put the cursor in the message box once it is on screen.
    focus_composer: bool,

    /// What `agent.connection` said, checked again when the service or Settings change.
    connection: Option<Connection>,
    checking: bool,
    connection_error: String,
    connection_key: Option<(bool, &'static str)>,
    _connection_task: Option<Task<()>>,

    /// The body's scroll, shared by the tabs (each starts at the top, Chat at the end).
    scroll: ScrollHandle,
    /// Scroll to the end on the next frame, whatever the position (a sent message).
    force_follow: bool,
    /// What the chat showed last frame: (next entry id, last entry's length, streaming).
    seen: (u64, usize, bool),
    /// Steps whose command is shown: (item key, step index).
    open_steps: HashSet<(u64, usize)>,
    /// Changes whose output is shown, by sequence.
    open_changes: HashSet<u64>,
    markdown: chat::MarkdownCache,

    /// "Use another agent", shown from the welcome while no service is connected.
    external: Option<Entity<ExternalAgents>>,
    models: models::ModelMenu,
    generate: generate::Generate,
    takes: takes::Takes,
    menu: MenuHost,
    _subscriptions: Vec<Subscription>,
}

impl AgentPanel {
    pub fn new(daw: Entity<Daw>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let composer = cx.new(|cx| {
            TextInput::new(cx)
                .multiline()
                .placeholder("Describe your idea, or type / for commands…")
        });
        let subscriptions = vec![
            cx.observe(&daw, |_, _, cx| cx.notify()),
            cx.subscribe_in(&composer, window, Self::composer_event),
        ];
        #[allow(unused_mut)]
        let mut this = Self {
            models: models::ModelMenu::new(window, cx),
            generate: generate::Generate::new(window, cx),
            takes: takes::Takes::new(window, cx),
            daw,
            tab: Tab::Chat,
            composer,
            composer_error: String::new(),
            with_context: true,
            slash_index: 0,
            confirm_clear: false,
            focus_composer: false,
            connection: None,
            checking: false,
            connection_error: String::new(),
            connection_key: None,
            _connection_task: None,
            scroll: ScrollHandle::new(),
            force_follow: true,
            seen: (0, 0, false),
            open_steps: HashSet::new(),
            open_changes: HashSet::new(),
            markdown: Default::default(),
            external: None,
            menu: MenuHost::default(),
            _subscriptions: subscriptions,
        };
        #[cfg(debug_assertions)]
        this.apply_fixture(cx);
        this
    }

    /// Open the panel with the current selection as the subject of the next message: the
    /// ⌘⇧J action and "Ask Agent…" in the context menus. A message already started stays.
    pub fn ask_about_selection(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.daw.update(cx, |daw, cx| {
            daw.run(
                "ui.showPanel",
                json!({"panel": "agent", "visible": true}),
                cx,
            );
        });
        self.with_context = true;
        self.select_tab(Tab::Chat, cx);
        self.focus_composer = true;
        cx.notify();
    }

    fn select_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.scroll.set_offset(gpui::point(px(0.0), px(0.0)));
            if tab == Tab::Takes {
                self.takes.refresh(&self.daw, cx);
            }
            if tab == Tab::Generate {
                self.generate.refresh(&self.daw, cx);
            }
        }
        if tab == Tab::Chat {
            self.force_follow = true;
        }
        cx.notify();
    }

    fn draft(&self, cx: &App) -> String {
        self.composer.read(cx).text().to_string()
    }

    /// Replace the message (a starter, a slash command, the last request) and focus it.
    fn set_draft(&mut self, text: &str, cx: &mut Context<Self>) {
        self.composer
            .update(cx, |input, cx| input.set_text(text.to_string(), cx));
        self.composer_error.clear();
        self.slash_index = 0;
        self.focus_composer = true;
        cx.notify();
    }

    fn open_settings(&mut self, section: &str, cx: &mut Context<Self>) {
        self.daw.update(cx, |daw, cx| {
            daw.run(
                "ui.showPanel",
                json!({"panel": "settings", "section": section}),
                cx,
            );
        });
    }

    fn busy(&self, cx: &App) -> bool {
        self.daw.read(cx).app.agents.runtime.running()
    }

    fn ready(&self) -> bool {
        self.connection.as_ref().is_some_and(Connection::can_chat)
    }

    /// Ask `agent.connection` again. A check never sends a prompt.
    fn check_connection(&mut self, cx: &mut Context<Self>) {
        self.checking = true;
        self.connection = None;
        self.connection_error.clear();
        let answer = jobs::request(&self.daw, "agent.connection", json!({}), cx);
        self._connection_task = Some(cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                this.checking = false;
                match result {
                    Ok(value) => this.connection = Connection::from_json(&value),
                    Err(error) => this.connection_error = error,
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Check the connection when the service changes or Settings opens or closes.
    fn follow_connection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = &self.daw.read(cx).app;
        let key = (app.settings_ui.open, app.settings.agent.provider.key());
        if self.connection_key != Some(key) {
            self.connection_key = Some(key);
            cx.defer_in(window, |this, _, cx| this.check_connection(cx));
        }
    }

    /// Send the message with the selection, through `agent.send`.
    fn send(&mut self, cx: &mut Context<Self>) {
        let draft = self.draft(cx);
        if draft.trim().is_empty() || self.busy(cx) || !self.ready() || self.checking {
            return;
        }
        let chips = if self.with_context {
            context::selection_context(self.daw.read(cx).app.store.session())
        } else {
            vec![]
        };
        let prompt = context::with_selection(draft.trim(), &chips);
        let result = self.daw.update(cx, |daw, cx| {
            daw.request("agent.send", json!({ "prompt": prompt }), cx)
        });
        match result {
            Ok(_) => {
                self.composer.update(cx, |input, cx| input.set_text("", cx));
                self.composer_error.clear();
                self.force_follow = true;
                self.tab = Tab::Chat;
            }
            Err(error) => self.composer_error = error,
        }
        self.focus_composer = true;
        cx.notify();
    }

    fn choose_slash(&mut self, command: &'static slash::Slash, cx: &mut Context<Self>) {
        match slash::choose(command) {
            slash::Choice::Generate => {
                self.set_draft("", cx);
                self.select_tab(Tab::Generate, cx);
            }
            slash::Choice::Takes => {
                self.set_draft("", cx);
                self.select_tab(Tab::Takes, cx);
            }
            slash::Choice::Draft(prompt) => self.set_draft(prompt, cx),
        }
    }

    fn composer_event(
        &mut self,
        _: &Entity<TextInput>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let matches = slash::matches(&self.draft(cx));
        match event {
            InputEvent::Submit if !matches.is_empty() => {
                let command = matches[self.slash_index.min(matches.len() - 1)];
                self.choose_slash(command, cx);
            }
            InputEvent::Submit => self.send(cx),
            InputEvent::Cancel if !matches.is_empty() => self.set_draft("", cx),
            InputEvent::Cancel => {
                if self.models.open {
                    self.models.open = false;
                    cx.notify();
                }
            }
            InputEvent::Changed => {
                self.slash_index = 0;
                self.composer_error.clear();
                cx.notify();
            }
            InputEvent::Blur => {}
        }
    }

    /// Arrow keys move through the slash menu while it is open, instead of the caret.
    fn slash_step(&mut self, down: bool, cx: &mut Context<Self>) -> bool {
        let count = slash::matches(&self.draft(cx)).len();
        if count == 0 {
            return false;
        }
        self.slash_index = if down {
            (self.slash_index + 1) % count
        } else {
            (self.slash_index + count - 1) % count
        };
        cx.notify();
        true
    }

    fn header(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let app = &self.daw.read(cx).app;
        let busy = app.agents.runtime.running();
        let empty = app.agents.runtime.transcript.is_empty();
        let ready = self.ready();
        let status = match &self.connection {
            Some(connection) => connection::provider_name_of(&connection.provider),
            None if self.checking => "Connecting…".into(),
            None => "Not connected".into(),
        };
        // Titled like every area: the name, the connection in mono, the actions boxed at the
        // right.
        div()
            .h(px(crate::ui::theme::layout::TOOLBAR))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .pl(px(14.0))
            .pr(px(8.0))
            .border_b_1()
            .border_color(theme.line)
            .child(status_dot(ready, busy, &theme))
            .child(crate::ui::widgets::panel_title("Agent", cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(crate::ui::widgets::panel_info(status, cx)),
            )
            .child(crate::ui::widgets::group(
                [
                    Button::icon("agent-new", "plus")
                        .flush()
                        .disabled(busy || empty)
                        .tooltip("New conversation")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.select_tab(Tab::Chat, cx);
                            this.confirm_clear = true;
                            cx.notify();
                        }))
                        .into_any_element(),
                    Button::icon("agent-settings", "gear")
                        .flush()
                        .icon_size(13.0)
                        .tooltip("Agent settings")
                        .on_click(cx.listener(|this, _, _, cx| this.open_settings("agent", cx)))
                        .into_any_element(),
                    Button::icon("agent-collapse", "chevron-right")
                        .flush()
                        .tooltip("Collapse agent")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.daw.update(cx, |daw, cx| {
                                daw.run(
                                    "ui.showPanel",
                                    json!({"panel": "agent", "visible": false}),
                                    cx,
                                );
                            })
                        }))
                        .into_any_element(),
                ],
                cx,
            ))
            .into_any_element()
    }

    /// "Clear the conversation? Your music stays." under the header.
    fn confirm(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let busy = self.busy(cx);
        div()
            .flex_none()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.0))
            .px(px(16.0))
            .py(px(10.0))
            .bg(theme.accent_soft)
            .border_b_1()
            .border_color(theme.line)
            .text_size(px(size::SM))
            .text_color(theme.text)
            .child(
                div()
                    .w_full()
                    .child("Clear the conversation? Your music stays."),
            )
            .child(
                Button::new("clear-cancel", "Cancel")
                    .compact()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.confirm_clear = false;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("clear-confirm", "Clear conversation")
                    .compact()
                    .primary()
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.clear_conversation(cx))),
            )
            .into_any_element()
    }

    /// "Clear conversation" confirmed: `agent.clear`; the music stays.
    fn clear_conversation(&mut self, cx: &mut Context<Self>) {
        let cleared = self
            .daw
            .update(cx, |daw, cx| daw.request("agent.clear", json!({}), cx));
        if cleared.is_ok() {
            self.confirm_clear = false;
            self.open_steps.clear();
            self.markdown.clear();
        }
        cx.notify();
    }

    fn tabs(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let changes = self.daw.read(cx).app.agents.change_count();
        let tabs = [
            (Tab::Chat, "Chat".to_string(), "Conversation"),
            (
                Tab::Generate,
                "Generate".into(),
                "Make a sound from a description",
            ),
            (
                Tab::Changes,
                format!("Changes · {changes}"),
                "What agents changed, to review and undo",
            ),
            (Tab::Takes, "Takes A/B".into(), "Compare creative takes"),
        ];
        div()
            .flex_none()
            .flex()
            .mx(px(14.0))
            .mt(px(10.0))
            .mb(px(4.0))
            .p(px(2.0))
            .gap(px(2.0))
            .rounded(px(radius::SM))
            .bg(with_alpha(theme.bg_sunken, 0.35))
            .border_1()
            .border_color(theme.line_strong)
            .children(tabs.into_iter().map(|(tab, label, tip)| {
                let on = self.tab == tab;
                div()
                    .id(gpui::SharedString::from(label.clone()))
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(24.0))
                    .px(px(5.0))
                    .rounded(px(radius::SM - 1.0))
                    .text_size(px(size::SM))
                    .font_weight(FontWeight::SEMIBOLD)
                    .whitespace_nowrap()
                    .overflow_hidden()
                    // The open tab is inverted, paper on ink.
                    .text_color(if on {
                        theme.text_on_accent
                    } else {
                        theme.text_2
                    })
                    .when(on, |d| d.bg(theme.accent_fill))
                    .when(!on, |d| {
                        d.cursor_pointer()
                            .hover(|s| s.bg(theme.hover).text_color(theme.text))
                    })
                    .child(label)
                    .tooltip(move |_, cx| super::widgets::tip(tip.into(), cx))
                    .on_click(cx.listener(move |this, _, _, cx| this.select_tab(tab, cx)))
            }))
            .into_any_element()
    }

    /// The collapsed panel: a 32 px rail with the status light and a vertical label.
    fn rail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let busy = self.busy(cx);
        let label = format!("AGENT · {}", if busy { "WORKING" } else { "IDLE" });
        div()
            .id("agent-rail")
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(14.0))
            .gap(px(12.0))
            .bg(theme.glass(1))
            .border_l_1()
            .border_color(theme.line)
            .cursor_pointer()
            .hover(|s| s.bg(theme.hover))
            .tooltip(|_, cx| super::widgets::tip("Open agent panel".into(), cx))
            .child(status_dot(busy, busy, &theme))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .font_family(FONT_MONO)
                    .text_size(px(10.0))
                    .line_height(px(12.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.text_2)
                    .children(label.chars().map(|c| {
                        div().child(if c == ' ' {
                            "\u{2009}".to_string()
                        } else {
                            c.to_string()
                        })
                    })),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.daw.update(cx, |daw, cx| {
                    daw.run(
                        "ui.showPanel",
                        json!({"panel": "agent", "visible": true}),
                        cx,
                    );
                })
            }))
            .into_any_element()
    }
}

/// The status light: a square of ink when ready, ringed while working, hollow otherwise
/// (the word beside it says which, colour is never the only signal).
fn status_dot(lit: bool, working: bool, theme: &Theme) -> gpui::Div {
    div()
        .flex_none()
        .size(px(8.0))
        .bg(if lit { theme.accent } else { theme.bg_sunken })
        .border_1()
        .border_color(if lit { theme.accent } else { theme.text_3 })
        .when(working, |d| {
            d.shadow(vec![gpui::BoxShadow {
                color: theme.accent_ring,
                offset: gpui::point(px(0.0), px(0.0)),
                blur_radius: px(0.0),
                spread_radius: px(2.0),
            }])
        })
}

impl Render for AgentPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        self.follow_connection(window, cx);
        if !self.daw.read(cx).app.agents.open {
            return self.rail(cx);
        }
        if std::mem::take(&mut self.focus_composer) {
            let input = self.composer.clone();
            window.defer(cx, move |window, cx| {
                input.update(cx, |input, _| input.focus(window))
            });
        }
        let header = self.header(cx);
        let confirm = self.confirm_clear.then(|| self.confirm(cx));
        let tabs = self.tabs(cx);
        let body = match self.tab {
            Tab::Chat => self.chat(window, cx),
            Tab::Generate => self.render_generate(window, cx),
            Tab::Changes => self.changes(cx),
            Tab::Takes => self.render_takes(window, cx),
        };
        let composer =
            matches!(self.tab, Tab::Chat | Tab::Changes).then(|| self.composer_area(window, cx));
        let menu = self.menu.render(window, cx);
        div()
            .id("agent-panel")
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme.glass(1))
            .border_l_1()
            .border_color(theme.line)
            .text_size(px(size::BASE))
            .text_color(theme.text)
            // Arrow keys reach the slash menu before the message box moves its caret.
            .capture_action(cx.listener(|this, _: &text_input::Up, _, cx| {
                if this.slash_step(false, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &text_input::Down, _, cx| {
                if this.slash_step(true, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(header)
            .children(confirm)
            .child(tabs)
            .child(
                div()
                    .id(("agent-body", self.tab as usize))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .overflow_x_hidden()
                    .track_scroll(&self.scroll)
                    .px(px(14.0))
                    .pt(px(4.0))
                    .pb(px(8.0))
                    .child(body),
            )
            .children(composer)
            .children(menu)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
