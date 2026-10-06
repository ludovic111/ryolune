//! The Settings window (⌘,) over `engine::settings::Settings`: a modal sheet with the
//! sections at the left (General, Audio & MIDI, Interface, Agent, Generation, Plugins,
//! Control, Updates, About) and the chosen one's rows at the right, each a label, what it
//! does and its control. Every change runs `settings.set`, which saves the file and applies
//! it at once (`apply_settings`); the section shown is `ui.showPanel section=`, so the CLI,
//! MCP and the agent open the same place.

pub mod agent;
pub mod diagnostics;
pub mod fields;
pub mod generation;
pub mod theme_picker;

use super::{
    daw::Daw,
    dialogs::modal::{self, Dismiss, ModalFocus},
    theme::{radius, size, Theme},
    widgets::{field, select_button, Button, InputEvent, MenuHost, MenuItem, Switch, TextInput},
};
use crate::settings::SECTION_KEYS;
use agent::AgentForm;
use fields::{Devices, Field, Kind, FIELDS, SIDEBAR};
use generation::GenerationForm;
use gpui::{
    div, prelude::*, px, AnyElement, App, Context, Entity, MouseButton, MouseDownEvent,
    PathPromptOptions, SharedString, Subscription, Window,
};
use serde_json::{json, Value};
use std::time::Duration;

/// How long a notice ("Saved", a sign-in result) stays.
const NOTICE: Duration = Duration::from_secs(8);

pub struct SettingsWindow {
    daw: Entity<Daw>,
    focus: ModalFocus,
    menu: MenuHost,
    agent: Entity<AgentForm>,
    generation: Entity<GenerationForm>,
    /// Text fields of the number settings, by path.
    numbers: Vec<(&'static str, Entity<TextInput>)>,
    devices: Devices,
    /// Closing was asked over unsaved agent connection edits.
    confirm_close: bool,
    /// The section and open state last seen, to load a section's data when it appears.
    seen: Option<usize>,
    /// Settings › Diagnostics: the log tail and crash reports, as last read.
    diag: diagnostics::DiagState,
    _subscriptions: Vec<Subscription>,
}

impl SettingsWindow {
    pub fn new(daw: Entity<Daw>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        modal::bind(cx);
        let mut subscriptions = vec![];
        let mut numbers = vec![];
        for f in FIELDS
            .iter()
            .filter(|f| matches!(f.kind, Kind::Number { .. }))
        {
            let input = cx.new(|cx| TextInput::new(cx).mono());
            let path = f.path;
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this: &mut Self, input, event, window, cx| match event {
                    InputEvent::Submit | InputEvent::Blur => {
                        let text = input.read(cx).text().trim().to_string();
                        this.commit_number(path, &text, cx);
                    }
                    InputEvent::Cancel => window.dispatch_action(Box::new(Dismiss), cx),
                    InputEvent::Changed => {}
                },
            ));
            numbers.push((path, input));
        }
        // Focus follows the window opening and closing, from wherever it is asked.
        subscriptions.push(cx.observe_in(&daw, window, |this, daw, window, cx| {
            let ui = &daw.read(cx).app.settings_ui;
            let (open, section) = (ui.open, ui.section);
            this.focus.sync(open, window, cx);
            let now = open.then_some(section);
            if now != this.seen {
                this.seen = now;
                if let Some(section) = now {
                    cx.defer_in(window, move |this, _, cx| this.load(section, cx));
                }
            }
        }));
        Self {
            agent: cx.new(|cx| AgentForm::new(daw.clone(), window, cx)),
            generation: cx.new(|cx| GenerationForm::new(daw.clone(), window, cx)),
            focus: ModalFocus::new(cx),
            menu: MenuHost::default(),
            numbers,
            devices: Devices::default(),
            confirm_close: false,
            seen: None,
            diag: Default::default(),
            _subscriptions: subscriptions,
            daw,
        }
    }

    /// Load what a section shows from the registry.
    fn load(&mut self, section: usize, cx: &mut Context<Self>) {
        match SECTION_KEYS.get(section).copied() {
            Some("audio") => self.refresh_devices(cx),
            Some("agent") => self.agent.update(cx, |form, cx| form.refresh(cx)),
            Some("generation") => self.generation.update(cx, |form, cx| form.refresh(cx)),
            Some("diagnostics") => self.load_diagnostics(cx),
            _ => {}
        }
    }

    fn refresh_devices(&mut self, cx: &mut Context<Self>) {
        let daw = self.daw.clone();
        modal::request_async(
            self,
            &daw,
            "audio.devices",
            json!({}),
            cx,
            |this, result, cx| {
                match result {
                    Ok(value) => this.devices = Devices::from_value(&value),
                    Err(error) => this.show_error(error, cx),
                }
                cx.notify();
            },
        );
    }

    fn show_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.daw.update(cx, |daw, cx| {
            daw.app.settings_ui.error = Some(error);
            cx.notify();
        });
    }

    /// `settings.set` one path; a refusal shows in the window.
    fn set(&mut self, path: &str, value: Value, cx: &mut Context<Self>) -> bool {
        let result = self.daw.update(cx, |daw, cx| {
            let result = daw.request("settings.set", json!({ "path": path, "value": value }), cx);
            if result.is_ok() {
                daw.app.settings_ui.error = None;
            }
            result
        });
        match result {
            Ok(_) => true,
            Err(error) => {
                self.show_error(error, cx);
                false
            }
        }
    }

    fn commit_number(&mut self, path: &'static str, text: &str, cx: &mut Context<Self>) {
        let saved = self.value(path, cx);
        let Ok(number) = text.parse::<i64>() else {
            self.show_error("Type a whole number.".into(), cx);
            self.sync_number(path, &saved, cx);
            return;
        };
        if saved.as_i64() != Some(number) && !self.set(path, json!(number), cx) {
            self.sync_number(path, &saved, cx);
        }
    }

    fn sync_number(&self, path: &str, value: &Value, cx: &mut Context<Self>) {
        if let Some((_, input)) = self.numbers.iter().find(|(p, _)| *p == path) {
            let text = value.to_string();
            input.update(cx, |input, cx| input.set_text(text, cx));
        }
    }

    /// The saved value of a path, secrets masked.
    fn value(&self, path: &str, cx: &App) -> Value {
        self.daw
            .read(cx)
            .app
            .settings
            .get(Some(path))
            .unwrap_or(Value::Null)
    }

    /// Escape or the close key: close, unless agent connection edits would be lost.
    fn dismiss(&mut self, _: &Dismiss, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_open() {
            return;
        }
        if self.agent.read(cx).unsaved(cx) && !self.confirm_close {
            self.confirm_close = true;
            cx.notify();
            return;
        }
        self.close(window, cx);
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_close = false;
        self.agent.update(cx, |form, cx| form.discard(cx));
        self.focus.sync(false, window, cx);
        self.daw.update(cx, |daw, cx| {
            daw.run(
                "ui.showPanel",
                json!({"panel": "settings", "visible": false}),
                cx,
            );
        });
    }

    fn select(
        &mut self,
        items: Vec<MenuItem>,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu.open(items, e.position, window, cx);
    }

    /// One preference row.
    fn row(
        &mut self,
        f: &'static Field,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let value = self.value(f.path, cx);
        let path = f.path;
        let description = (!f.description.is_empty()).then(|| SharedString::from(f.description));
        let control: AnyElement = match f.kind {
            Kind::Switch => {
                let on = value.as_bool().unwrap_or(false);
                let set = cx.listener(move |this, on: &bool, _, cx| {
                    this.set(path, json!(on), cx);
                });
                Switch::new(SharedString::from(path), on)
                    .on_toggle(move |on, window, cx| set(&on, window, cx))
                    .into_any_element()
            }
            Kind::Number { .. } => {
                let input = self
                    .numbers
                    .iter()
                    .find(|(p, _)| *p == path)
                    .map(|(_, i)| i.clone())
                    .expect("every number field has an input");
                let focused = input.read(cx).is_focused(window);
                if !focused {
                    let text = value.to_string();
                    input.update(cx, |input, cx| input.set_text(text, cx));
                }
                div()
                    .w(px(90.0))
                    .child(field(&input, focused, cx))
                    .into_any_element()
            }
            Kind::Choice(choices) => {
                let label = fields::choice_label(choices, &value, &self.devices);
                select_button(SharedString::from(path), label, cx)
                    .min_w(px(220.0))
                    .max_w(px(300.0))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                            let current = this.value(path, cx);
                            let entity = cx.entity().downgrade();
                            let items = fields::options(choices, &this.devices)
                                .into_iter()
                                .map(|(label, v)| {
                                    let on = v == current
                                        || v.as_f64()
                                            .zip(current.as_f64())
                                            .is_some_and(|(a, b)| (a - b).abs() < 1e-6);
                                    let entity = entity.clone();
                                    MenuItem::new(label, move |_, cx| {
                                        let v = v.clone();
                                        let _ = entity.update(cx, |this, cx| {
                                            this.set(path, v, cx);
                                        });
                                    })
                                    .checked(on)
                                })
                                .collect();
                            this.select(items, e, window, cx);
                        }),
                    )
                    .into_any_element()
            }
            Kind::Appearance => {
                let mode = self.value("interface.mode", cx);
                let entity = cx.entity().downgrade();
                return div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .pb(px(10.0))
                    .child(
                        div()
                            .text_size(px(size::BASE))
                            .text_color(Theme::get(cx).text)
                            .child(f.label),
                    )
                    .child(theme_picker::picker(
                        mode.as_str().unwrap_or("dark"),
                        move |mode, _, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                this.set("interface.mode", json!(mode), cx);
                            });
                        },
                        cx,
                    ))
                    .into_any_element();
            }
            Kind::Paths => return self.paths(f, &value, cx),
        };
        modal::field_row(f.label, description, control, cx).into_any_element()
    }

    /// A list of folders with Remove keys, and Add folder… with the system's chooser.
    fn paths(&mut self, f: &'static Field, value: &Value, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let paths: Vec<String> = value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        let path = f.path;
        let mut list = div().flex().flex_col().gap(px(4.0));
        for (i, folder) in paths.iter().enumerate() {
            let rest: Vec<String> = paths
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, p)| p.clone())
                .collect();
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(8.0))
                    .py(px(3.0))
                    .rounded(px(radius::SM))
                    .bg(theme.well)
                    .child(crate::ui::widgets::icon("folder", 11.0, theme.text_3))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(size::SM))
                            .text_color(theme.text_2)
                            .child(folder.clone()),
                    )
                    .child(
                        Button::icon(SharedString::from(format!("{path}-remove-{i}")), "close")
                            .ghost()
                            .compact()
                            .icon_size(8.0)
                            .tooltip(format!("Stop searching {folder}"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set(path, json!(rest.clone()), cx);
                            })),
                    ),
            );
        }
        let current = paths.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .py(px(8.0))
            .border_b_1()
            .border_color(theme.hairline)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(size::BASE))
                                    .text_color(theme.text)
                                    .child(f.label),
                            )
                            .child(modal::note(f.description, cx)),
                    )
                    .child(
                        Button::new(SharedString::from(format!("{path}-add")), "Add folder…")
                            .compact()
                            .with_icon("plus")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.add_folder(path, current.clone(), cx)
                            })),
                    ),
            )
            .child(list)
            .into_any_element()
    }

    fn add_folder(&mut self, path: &'static str, current: Vec<String>, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Add".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(folders))) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let mut next = current;
                for folder in folders {
                    let folder = folder.display().to_string();
                    if !next.contains(&folder) {
                        next.push(folder);
                    }
                }
                this.set(path, json!(next), cx);
            });
        })
        .detach();
    }

    /// What a section shows besides its rows.
    fn extras(&mut self, key: &str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = Theme::get(cx).clone();
        let app = &self.daw.read(cx).app;
        let version = crate::update::current_version();
        match key {
            "audio" => vec![div()
                .flex()
                .gap(px(8.0))
                .pt(px(4.0))
                .child(
                    Button::new("devices-refresh", "Refresh devices")
                        .compact()
                        .on_click(cx.listener(|this, _, _, cx| this.refresh_devices(cx))),
                )
                .child(Button::new("devices-reconnect", "Reconnect output").compact().on_click(
                    cx.listener(|this, _, _, cx| {
                        this.daw.update(cx, |daw, cx| {
                            daw.fire("audio.reconnect", cx);
                        })
                    }),
                ))
                .into_any_element()],
            "plugins" => {
                let scanning = app.scan_job.is_some();
                let count = app.catalog.len();
                vec![div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .pt(px(4.0))
                    .child(
                        Button::new("plugins-scan", if scanning { "Scanning…" } else { "Scan plugins" })
                            .disabled(scanning)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.daw.update(cx, |daw, cx| {
                                    daw.fire("plugin.scan", cx);
                                })
                            })),
                    )
                    .child(modal::note(format!("{count} plugins in the library"), cx))
                    .into_any_element()]
            }
            "control" => {
                let port = app.control.as_ref().map(|server| server.port());
                let discovery = app.discovery_path();
                vec![modal::note(
                    match port {
                        Some(port) => format!(
                            "Listening on 127.0.0.1:{port}. ryolune-cli and ryolune-mcp find it through {}.",
                            discovery.display()
                        ),
                        None => "Off: scripts and outside agents cannot reach this window.".into(),
                    },
                    cx,
                )
                .into_any_element()]
            }
            "updates" => {
                let updates = &app.updates;
                let checking = updates.checking.is_some();
                let installing = updates.installing.is_some();
                let available = updates.available.as_ref().map(|r| r.version.clone());
                let installed = updates.installed.is_some();
                vec![
                    modal::text(format!("This is ryolune {version}. Every update is free."), cx)
                        .into_any_element(),
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            Button::new(
                                "updates-check",
                                if checking { "Checking…" } else { "Check for updates" },
                            )
                            .disabled(checking || installing)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.daw.update(cx, |daw, cx| {
                                    daw.fire("app.checkUpdates", cx);
                                })
                            })),
                        )
                        .when_some(available.filter(|_| !installed), |d, version| {
                            d.child(
                                Button::new(
                                    "updates-install",
                                    if installing {
                                        format!("Installing {version}…")
                                    } else {
                                        format!("Install {version}")
                                    },
                                )
                                .primary()
                                .disabled(installing)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.daw.update(cx, |daw, cx| {
                                        daw.fire("app.installUpdate", cx);
                                    })
                                })),
                            )
                        })
                        .when(installed, |d| {
                            d.child(Button::new("updates-relaunch", "Restart now").primary().on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.daw.update(cx, |daw, cx| {
                                        daw.fire("app.relaunch", cx);
                                    })
                                }),
                            ))
                        })
                        .child(Button::new("updates-whats-new", "What's new").ghost().on_click(
                            cx.listener(|this, _, _, cx| {
                                this.daw.update(cx, |daw, cx| {
                                    daw.run("ui.showPanel", json!({"panel": "whatsNew"}), cx);
                                })
                            }),
                        ))
                        .into_any_element(),
                ]
            }
            "diagnostics" => self.diagnostics(cx),
            "about" => vec![
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(10.0))
                    .child(modal::heading("ryolune", cx))
                    .child(
                        div()
                            .font_family(super::theme::FONT_MONO)
                            .text_size(px(size::SM))
                            .text_color(theme.text_3)
                            .child(version),
                    )
                    .into_any_element(),
                modal::text(
                    "Digital audio workstation for macOS, Linux and Windows, part of lsuite, the free and open-source creative suite.",
                    cx,
                )
                .into_any_element(),
                modal::text(
                    "Free and open source, every update included. If ryolune earns a place in your music, you can donate to it, once or monthly. It unlocks nothing.",
                    cx,
                )
                .into_any_element(),
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.0))
                    .child(
                        Button::new("about-support", "Donate…")
                            .with_icon("heart")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.daw.update(cx, |daw, cx| {
                                    daw.run("app.openGuide", json!({"guide": "support"}), cx);
                                })
                            })),
                    )
                    .child(
                        Button::new("about-site", "lsuite.xyz/ryolune")
                            .ghost()
                            .with_icon("external")
                            .on_click(|_, _, cx| cx.open_url("https://lsuite.xyz/ryolune")),
                    )
                    .child(
                        Button::new("about-sdk", "Native plugin SDK")
                            .ghost()
                            .with_icon("external")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.daw.update(cx, |daw, cx| {
                                    daw.run("app.openGuide", json!({"guide": "plugins"}), cx);
                                })
                            })),
                    )
                    .into_any_element(),
                modal::note("MIT licence. Fonts: Manrope and IBM Plex Mono (OFL).", cx)
                    .into_any_element(),
            ],
            _ => vec![],
        }
    }

    fn sidebar(&self, current: &str, cx: &mut Context<Self>) -> gpui::Div {
        let theme = Theme::get(cx).clone();
        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(184.0))
            .gap(px(2.0))
            .p(px(10.0))
            .border_r_1()
            .border_color(theme.hairline)
            .children(SIDEBAR.iter().map(|(key, title)| {
                let on = *key == current;
                let key = *key;
                div()
                    .id(SharedString::from(format!("settings-{key}")))
                    .flex()
                    .items_center()
                    .h(px(30.0))
                    .px(px(10.0))
                    .rounded(px(radius::SM))
                    .text_size(px(size::BASE))
                    .text_color(if on { theme.text } else { theme.text_2 })
                    .when(on, |d| {
                        d.bg(theme.accent_soft).text_color(theme.accent_text)
                    })
                    .when(!on, |d| d.cursor_pointer().hover(|s| s.bg(theme.hover)))
                    .child(*title)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.daw.update(cx, |daw, cx| {
                            daw.run(
                                "ui.showPanel",
                                json!({"panel": "settings", "section": key}),
                                cx,
                            );
                        })
                    }))
            }))
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let ui = &self.daw.read(cx).app.settings_ui;
        let open = ui.open;
        let key = SECTION_KEYS[ui.section.min(SECTION_KEYS.len() - 1)];
        let notice = ui
            .notice
            .as_ref()
            .filter(|(_, at)| at.elapsed() < NOTICE)
            .map(|(text, _)| text.clone());
        let error = ui.error.clone();
        self.focus.sync(open, window, cx);
        if !self.agent.read(cx).unsaved(cx) {
            self.confirm_close = false;
        }
        let title = SIDEBAR.iter().find(|(k, _)| *k == key).map_or("", |s| s.1);

        let mut content = modal::body("settings-content")
            .flex_1()
            .min_w_0()
            .h_full()
            .pt(px(16.0))
            .px(px(24.0))
            .gap(px(4.0));
        if self.confirm_close {
            content = content.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .p(px(10.0))
                    .mb(px(8.0))
                    .rounded(px(radius::MD))
                    .bg(theme.well)
                    .border_1()
                    .border_color(theme.warning)
                    .child(super::widgets::icon("warning", 12.0, theme.warning))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(size::BASE))
                            .text_color(theme.text)
                            .child("Discard unsaved agent connection changes?"),
                    )
                    .child(
                        Button::new("close-keep", "Keep editing")
                            .compact()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirm_close = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("close-discard", "Discard changes")
                            .compact()
                            .danger()
                            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    ),
            );
        }
        if !matches!(key, "agent" | "generation" | "about") {
            content = content.child(div().pb(px(6.0)).child(modal::heading(title, cx)));
        }
        if let Some(error) = error {
            content = content.child(div().py(px(4.0)).child(modal::error_line(error, cx)));
        } else if let Some(notice) = notice {
            content = content.child(div().py(px(4.0)).child(modal::note(notice, cx)));
        }
        match key {
            "agent" => content = content.child(self.agent.clone()),
            "generation" => content = content.child(self.generation.clone()),
            _ => {
                let rows: Vec<AnyElement> = FIELDS
                    .iter()
                    .filter(|f| f.section == key)
                    .map(|f| self.row(f, window, cx))
                    .collect();
                content = content.children(rows);
                let extras = self.extras(key, cx);
                content = content.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .pt(px(10.0))
                        .children(extras),
                );
            }
        }

        let sidebar = self.sidebar(key, cx);
        let sheet = modal::sheet(
            "settings",
            "Settings",
            880.0,
            Some(Box::new(|window, cx| {
                window.dispatch_action(Box::new(Dismiss), cx)
            })),
            cx,
        )
        .h(px(640.0))
        .child(
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .border_t_1()
                .border_color(theme.hairline)
                .child(sidebar)
                .child(content),
        );
        modal::layer("settings-layer", &self.focus.handle, cx)
            .on_action(cx.listener(Self::dismiss))
            .child(sheet)
            .children(self.menu.render(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn closing_over_unsaved_agent_edits_asks_first(cx: &mut gpui::TestAppContext) {
        let dir = std::env::temp_dir().join(format!("ryolune-ui-test-{}", std::process::id()));
        std::env::set_var("RYOLUNE_SETTINGS", dir.join("settings.json"));
        let mut app = crate::app::Ryolune::from_session(ryolune_engine::store::empty(), None);
        app.settings.agent.provider = ryolune_engine::settings::Provider::OpenAi;
        app.open_settings(Some(3));
        let daw = cx.new(|_| Daw::new(app));
        cx.update(|cx| {
            cx.set_global(Theme::new(super::super::theme::Mode::Dark, true));
            crate::ui::actions::bind(cx);
        });
        let (settings, cx) =
            cx.add_window_view(|window, cx| SettingsWindow::new(daw.clone(), window, cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let agent = settings.read(cx).agent.clone();
            agent.update(cx, |form, cx| form.type_key("unsaved-test-key", cx));
        });
        cx.simulate_keystrokes("escape");
        cx.update(|_, cx| {
            assert!(settings.read(cx).confirm_close, "it asks before discarding");
            assert!(daw.read(cx).app.settings_ui.open);
            assert!(
                settings.read(cx).agent.read(cx).unsaved(cx),
                "the draft is kept"
            );
        });
        cx.simulate_keystrokes("escape");
        cx.update(|_, cx| {
            assert!(
                !daw.read(cx).app.settings_ui.open,
                "a second Escape discards"
            );
            assert!(!settings.read(cx).agent.read(cx).unsaved(cx));
            assert_eq!(daw.read(cx).app.settings.agent.openai_api_key, "");
        });
    }
}
