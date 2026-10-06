//! "Use another agent": connect an agent that lives outside ryolune (Claude Code, Codex,
//! Cursor, VS Code, Claude Desktop, Gemini CLI…) to this window over MCP. One ready
//! configuration per client, copied or installed from a link; they come from `agent.mcp`
//! and `agent.openClient`. A view of its own so Settings > Agent can show it:
//! `cx.new(|cx| ExternalAgents::new(daw, cx))`.

use crate::ui::{
    daw::Daw,
    theme::{radius, size, Theme, FONT_MONO},
    widgets::Button,
};
use gpui::{
    div, prelude::*, px, ClipboardItem, Context, Entity, FontWeight, SharedString, Task, Window,
};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub struct Client {
    pub id: String,
    pub name: String,
    pub how: String,
    pub text: String,
    pub file: Option<String>,
    pub link: bool,
}

/// What `agent.mcp` reports: whether outside agents can reach the window, and the clients.
pub fn clients_from(value: &Value) -> (bool, Vec<Client>) {
    let clients = value["clients"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some(Client {
                id: c["id"].as_str()?.to_string(),
                name: c["name"].as_str().unwrap_or("").to_string(),
                how: c["how"].as_str().unwrap_or("").to_string(),
                text: c["text"].as_str().unwrap_or("").to_string(),
                file: c["file"].as_str().map(str::to_string),
                link: c["link"].as_bool().unwrap_or(false),
            })
        })
        .collect();
    (value["bridgeEnabled"].as_bool().unwrap_or(false), clients)
}

/// "Claude Desktop (beta)" → "Claude Desktop", for "Add to …".
fn short_name(name: &str) -> &str {
    match name.find(" (") {
        Some(at) if name.ends_with(')') => &name[..at],
        _ => name,
    }
}

pub struct ExternalAgents {
    daw: Entity<Daw>,
    bridge: bool,
    clients: Vec<Client>,
    chosen: String,
    copied: Option<String>,
    error: String,
    loaded: bool,
    _copied: Option<Task<()>>,
}

impl ExternalAgents {
    pub fn new(daw: Entity<Daw>, cx: &mut Context<Self>) -> Self {
        cx.observe(&daw, |this, _, cx| {
            // The bridge switch lives in Settings > Control: follow it.
            let enabled = this.daw.read(cx).app.settings.control.enable_bridge;
            if this.loaded && enabled != this.bridge {
                this.load(cx);
            }
        })
        .detach();
        let mut this = Self {
            daw,
            bridge: false,
            clients: vec![],
            chosen: "claude-code".into(),
            copied: None,
            error: String::new(),
            loaded: false,
            _copied: None,
        };
        this.load(cx);
        this
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        match self
            .daw
            .update(cx, |daw, cx| daw.request("agent.mcp", json!({}), cx))
        {
            Ok(value) => {
                (self.bridge, self.clients) = clients_from(&value);
                self.loaded = true;
            }
            Err(error) => self.error = error,
        }
        cx.notify();
    }

    fn copy(&mut self, client: &Client, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(client.text.clone()));
        self.copied = Some(client.id.clone());
        self._copied = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1600))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.copied = None;
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Render for ExternalAgents {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let client = self
            .clients
            .iter()
            .find(|c| c.id == self.chosen)
            .or(self.clients.first())
            .cloned();
        let text = |s: String| {
            div()
                .text_size(px(size::SM))
                .line_height(px(18.0))
                .text_color(theme.text_2)
                .child(s)
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .text_size(px(size::BASE))
            .text_color(theme.text)
            .child(
                div()
                    .text_size(px(size::BASE + 1.0))
                    .font_weight(FontWeight::BOLD)
                    .child("Use another agent"),
            )
            .child(text("Claude Code, Codex, Cursor and any app that speaks MCP can work on this song from outside. They get the same commands as the built-in agent, and every change they make lands in Undo.".into()))
            .when(self.loaded && !self.bridge, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .p(px(10.0))
                        .rounded(px(radius::SM))
                        .bg(theme.warning.opacity(0.12))
                        .border_1()
                        .border_color(theme.warning.opacity(0.4))
                        .text_size(px(size::SM))
                        .child("The local connection is off, so outside agents cannot reach this window.")
                        .child(
                            Button::new("bridge-on", "Turn it on")
                                .compact()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.daw.update(cx, |daw, cx| {
                                        daw.run(
                                            "ui.showPanel",
                                            json!({"panel": "settings", "section": "control"}),
                                            cx,
                                        );
                                    })
                                })),
                        ),
                )
            })
            .when(!self.error.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(px(size::SM))
                        .text_color(theme.danger)
                        .child(SharedString::from(self.error.clone())),
                )
            })
            .when_some(client, |d, client| {
                let chosen = client.id.clone();
                let (copy_client, open_id) = (client.clone(), client.id.clone());
                d.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(4.0))
                        .children(self.clients.iter().map(|c| {
                            let on = c.id == chosen;
                            let id = c.id.clone();
                            div()
                                .id(SharedString::from(format!("client-{}", c.id)))
                                .px(px(9.0))
                                .h(px(24.0))
                                .flex()
                                .items_center()
                                .border_1()
                                .border_color(if on { theme.accent } else { theme.line })
                                .bg(if on { theme.accent_soft } else { theme.control.opacity(0.0) })
                                .text_size(px(size::SM))
                                .text_color(if on { theme.text } else { theme.text_2 })
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.hover))
                                .child(c.name.clone())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.chosen = id.clone();
                                    cx.notify();
                                }))
                        })),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .p(px(12.0))
                        .rounded(px(radius::MD))
                        .bg(theme.well)
                        .border_1()
                        .border_color(theme.line)
                        .child(text(client.how.clone()))
                        .when_some(client.file.clone(), |d, file| {
                            d.child(
                                div()
                                    .flex()
                                    .gap(px(6.0))
                                    .text_size(px(size::SM))
                                    .child(div().text_color(theme.text_3).child("File"))
                                    .child(div().font_family(FONT_MONO).child(file)),
                            )
                        })
                        .child(
                            div()
                                .id("client-config")
                                .max_h(px(220.0))
                                .overflow_y_scroll()
                                .p(px(8.0))
                                .rounded(px(radius::SM))
                                .bg(theme.bg_sunken)
                                .font_family(FONT_MONO)
                                .text_size(px(10.5))
                                .line_height(px(15.0))
                                .text_color(theme.text_2)
                                .child(SharedString::from(client.text.clone())),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(8.0))
                                .when(client.link, |d| {
                                    d.child(
                                        Button::new(
                                            "client-add",
                                            format!("Add to {}", short_name(&client.name)),
                                        )
                                        .primary()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let result = this.daw.update(cx, |daw, cx| {
                                                daw.request(
                                                    "agent.openClient",
                                                    json!({ "client": open_id }),
                                                    cx,
                                                )
                                            });
                                            if let Err(error) = result {
                                                this.error = error;
                                                cx.notify();
                                            }
                                        })),
                                    )
                                })
                                .child(
                                    Button::new(
                                        "client-copy",
                                        if self.copied.as_deref() == Some(client.id.as_str()) {
                                            "Copied"
                                        } else {
                                            "Copy"
                                        },
                                    )
                                    .with_icon("copy")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.copy(&copy_client, cx)
                                    })),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(size::XS))
                                .text_color(theme.text_3)
                                .child("Keep ryolune open while the agent works: it edits this window."),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::clients::{clients, Server};

    #[test]
    fn offers_a_ready_configuration_for_each_outside_agent() {
        let server = Server {
            command: "/Applications/ryolune.app/Contents/MacOS/ryolune-mcp".into(),
            discovery: "/tmp/control.json".into(),
        };
        let (bridge, list) = clients_from(&clients(&server, false));
        assert!(!bridge);
        assert!(list.len() >= 9);
        let cursor = list.iter().find(|c| c.id == "cursor").unwrap();
        assert!(cursor.link, "Cursor installs from its link");
        assert!(cursor.file.is_some());
        assert!(list.iter().all(|c| c.text.contains("ryolune-mcp")));
        assert_eq!(short_name("VS Code (Copilot)"), "VS Code");
        assert_eq!(short_name("Zed"), "Zed");
    }
}
