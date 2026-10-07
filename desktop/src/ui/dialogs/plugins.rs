//! The Plugins window (lsuite's PLUGINS.md), four parts in one sheet:
//!
//! - **Stock**: the instruments and effects ryolune ships, each with its line, and its switch.
//! - **Installed**: what was found on this computer (lsuite plugins, CLAP, VST3, Audio
//!   Units), each with its format's logo, vendor and version, its switch always in view, and
//!   Remove for lsuite plugins; Rescan.
//! - **Formats**: what ryolune loads, with the format's or maker's logo and where it looks.
//! - **Build with your agent**: one field. Sending it starts the agent on the plugin recipe
//!   (`plugin.guide` … `plugin.publishLocal`); the plugin then appears in Installed, loaded,
//!   without a restart. Also says whether Rust is installed.
//!
//! Everything goes through the registry: `plugin.enable` / `plugin.disable`, `plugin.remove`,
//! `plugin.scan`, `plugin.toolchain`, `agent.send` (`ui.showPanel panel=plugins` opens it).

use super::{modal, Dialogs};
use crate::ui::{
    theme::{size, Theme, FONT_MONO},
    widgets::{field, Button, InputEvent, Segmented, Switch, TextInput},
};
use gpui::{
    div, img, prelude::*, px, uniform_list, AnyElement, App, Context, Entity, SharedString,
    Subscription, Window,
};
use ryolune_engine::plugin::{Descriptor, Format};
use serde_json::json;

pub(crate) const PARTS: [&str; 4] = ["Stock", "Installed", "Formats", "Build with your agent"];
/// The parts as `ui.showPanel panel=plugins section=…` names them.
const SECTIONS: [&str; 4] = ["stock", "installed", "formats", "build"];
const ROW: f32 = 46.0;

/// One plugin as the window lists it.
#[derive(Clone)]
pub(crate) struct Row {
    id: String,
    name: String,
    detail: String,
    format: Format,
    lsuite: bool,
    removable: bool,
}

pub(crate) struct PluginsForm {
    describe: Entity<TextInput>,
    /// Installed: which format is shown (0 all, then lsuite, CLAP, VST3, AU).
    filter: usize,
    rows: Vec<Row>,
    /// What the rows were made from: the catalog's size and ends.
    rows_key: (usize, String, String),
    asked_toolchain: bool,
    _subscriptions: Vec<Subscription>,
}

/// The light tile a maker's logo sits on (black marks read in dark mode too).
pub(crate) fn logo_tile(path: &'static str, size_px: f32, cx: &App) -> AnyElement {
    let theme = Theme::get(cx);
    div()
        .flex_none()
        .size(px(size_px))
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.logo_tile)
        .border_1()
        .border_color(theme.line)
        .child(img(path).size(px(size_px * 0.66)))
        .into_any_element()
}

fn format_logo(format: Format, lsuite: bool, size_px: f32, cx: &App) -> AnyElement {
    match format {
        Format::Clap => logo_tile("logos/format-clap.svg", size_px, cx),
        Format::Vst3 => logo_tile("logos/format-vst3.svg", size_px, cx),
        Format::AudioUnit => logo_tile("logos/format-au.svg", size_px, cx),
        _ if lsuite || format == Format::Native => {
            crate::ui::agent_panel::provider_logo("lsuite", "", size_px, cx)
        }
        _ => crate::ui::agent_panel::provider_logo("lsuite", "", size_px, cx),
    }
}

fn format_name(format: Format, lsuite: bool) -> &'static str {
    match format {
        Format::Clap => "CLAP",
        Format::Vst3 => "VST3",
        Format::AudioUnit => "Audio Unit",
        Format::Native if lsuite => "lsuite plugin",
        Format::Native => "ryolune native",
        _ => "Stock",
    }
}

const FILTERS: [&str; 5] = ["All", "lsuite", "CLAP", "VST3", "AU"];

fn rows_of(catalog: &[Descriptor]) -> Vec<Row> {
    let mut rows: Vec<Row> = catalog
        .iter()
        .filter(|d| d.format.prefix() != "stock")
        .map(|d| {
            let bundle = (d.format == Format::Native)
                .then(|| ryolune_engine::plugin_dev::bundle_of(std::path::Path::new(&d.path)))
                .flatten();
            let lsuite = bundle.is_some();
            let version = bundle
                .as_ref()
                .map(|(_, m)| m.version.clone())
                .unwrap_or_default();
            let mut detail = vec![];
            if !d.vendor.is_empty() {
                detail.push(d.vendor.clone());
            }
            if !version.is_empty() {
                detail.push(version);
            }
            detail.push(format_name(d.format, lsuite).to_string());
            detail.push(if d.instrument { "instrument" } else { "effect" }.into());
            Row {
                id: d.id.clone(),
                name: d.name.clone(),
                detail: detail.join(" · "),
                format: d.format,
                lsuite,
                removable: d.format == Format::Native
                    && ryolune_engine::plugin_dev::removable(std::path::Path::new(&d.path)),
            }
        })
        .collect();
    // lsuite plugins first (the ones the person made), then by name.
    rows.sort_by(|a, b| {
        b.lsuite
            .cmp(&a.lsuite)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    rows
}

impl PluginsForm {
    pub fn new(window: &mut Window, cx: &mut Context<Dialogs>) -> Self {
        let describe = cx.new(|cx| {
            TextInput::new(cx)
                .multiline()
                .placeholder("Describe the plugin you want: “a bitcrusher with a mix knob”, “a warm tape saturation”, “a simple FM bell”…")
        });
        let subscriptions = vec![cx.subscribe_in(
            &describe,
            window,
            |this: &mut Dialogs, _, event, window, cx| match event {
                InputEvent::Changed => cx.notify(),
                InputEvent::Submit => this.build_with_agent(cx),
                InputEvent::Cancel => window.dispatch_action(Box::new(modal::Dismiss), cx),
                InputEvent::Blur => {}
            },
        )];
        Self {
            describe,
            filter: 0,
            rows: vec![],
            rows_key: (usize::MAX, String::new(), String::new()),
            asked_toolchain: false,
            _subscriptions: subscriptions,
        }
    }

    fn refresh_rows(&mut self, catalog: &[Descriptor]) {
        let key = (
            catalog.len(),
            catalog.first().map(|d| d.path.clone()).unwrap_or_default(),
            catalog.last().map(|d| d.path.clone()).unwrap_or_default(),
        );
        if key != self.rows_key {
            self.rows = rows_of(catalog);
            self.rows_key = key;
        }
    }

    fn shown_rows(&self) -> Vec<Row> {
        self.rows
            .iter()
            .filter(|r| match self.filter {
                1 => r.lsuite || r.format == Format::Native,
                2 => r.format == Format::Clap,
                3 => r.format == Format::Vst3,
                4 => r.format == Format::AudioUnit,
                _ => true,
            })
            .cloned()
            .collect()
    }
}

/// The request the agent gets: the person's words and the recipe.
pub(crate) fn recipe_prompt(description: &str) -> String {
    format!(
        "Build me a ryolune plugin: {}\n\nFollow ryolune's plugin recipe: read plugin.guide, check plugin.toolchain, then plugin.new, write the code with plugin.writeSource, run plugin.build until it is green (fix it from the errors), then plugin.publishLocal. Finally load it on the selected track and check that it sounds right.",
        description.trim()
    )
}

impl Dialogs {
    fn plugin_switch(&self, id: &str, on: bool) -> Switch {
        let daw = self.daw.clone();
        let id = id.to_string();
        Switch::new(SharedString::from(format!("plugin-on-{id}")), on).on_toggle(
            move |on, _, cx| {
                let id = id.clone();
                daw.update(cx, |daw, cx| {
                    daw.run(
                        if on {
                            "plugin.enable"
                        } else {
                            "plugin.disable"
                        },
                        json!({ "id": id }),
                        cx,
                    );
                })
            },
        )
    }

    /// Rows of the Installed list, for the virtual list.
    fn plugin_rows(
        &mut self,
        range: std::ops::Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = Theme::get(cx).clone();
        let rows = self.plugins.shown_rows();
        let disabled = self.daw.read(cx).app.settings.plugins.disabled.clone();
        let busy = self.daw.read(cx).app.plugin_job.is_some();
        rows.get(range.clone())
            .unwrap_or(&[])
            .iter()
            .map(|row| {
                let on = !disabled.contains(&row.id);
                let remove_id = row.id.clone();
                let daw = self.daw.clone();
                div()
                    .w_full()
                    .h(px(ROW))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .pr(px(4.0))
                    .border_b_1()
                    .border_color(theme.hairline)
                    .child(format_logo(row.format, row.lsuite, 26.0, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(size::BASE))
                                    .text_color(if on { theme.text } else { theme.text_3 })
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(row.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(size::XS))
                                    .text_color(theme.text_3)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(row.detail.clone()),
                            ),
                    )
                    .when(row.removable, |d| {
                        d.child(
                            Button::new(
                                SharedString::from(format!("plugin-remove-{}", row.id)),
                                "Remove",
                            )
                            .compact()
                            .ghost()
                            .disabled(busy)
                            .tooltip("Delete this plugin you installed")
                            .on_click(move |_, _, cx| {
                                let id = remove_id.clone();
                                daw.update(cx, |daw, cx| {
                                    daw.run("plugin.remove", json!({ "id": id }), cx);
                                })
                            }),
                        )
                    })
                    .child(self.plugin_switch(&row.id, on))
                    .into_any_element()
            })
            .collect()
    }

    fn stock_part(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = Theme::get(cx).clone();
        let app = &self.daw.read(cx).app;
        let disabled = app.settings.plugins.disabled.clone();
        let stock: Vec<Descriptor> = app
            .catalog
            .iter()
            .filter(|d| d.format.prefix() == "stock")
            .cloned()
            .collect();
        let mut part = div().flex().flex_col().gap(px(4.0));
        for (title, instrument) in [("Instruments", true), ("Effects", false)] {
            part = part.child(section_title(title, cx));
            let mut grid = div().flex().flex_wrap();
            for d in stock.iter().filter(|d| d.instrument == instrument) {
                let on = !disabled.contains(&d.id);
                let line = ryolune_engine::stock::description(&d.name).unwrap_or("");
                grid = grid.child(
                    div()
                        .w(relative_half())
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .py(px(6.0))
                        .pr(px(14.0))
                        .border_b_1()
                        .border_color(theme.hairline)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(size::BASE))
                                        .text_color(if on { theme.text } else { theme.text_3 })
                                        .child(d.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(size::XS))
                                        .text_color(theme.text_3)
                                        .child(line.to_string()),
                                ),
                        )
                        .child(self.plugin_switch(&d.id, on)),
                );
            }
            part = part.child(grid);
        }
        part
    }

    fn installed_part(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let theme = Theme::get(cx).clone();
        let catalog = self.daw.read(cx).app.catalog.clone();
        self.plugins.refresh_rows(&catalog);
        let scanning = {
            let app = &self.daw.read(cx).app;
            app.scan_job.is_some() || app.control_job.is_some()
        };
        let count = self.plugins.shown_rows().len();
        let filter = self.plugins.filter;
        let this = cx.entity().downgrade();
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .child(
                Segmented::new("plugins-filter", FILTERS, filter).on_select(move |i, _, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.plugins.filter = i;
                        cx.notify();
                    });
                }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(size::XS))
                            .font_family(FONT_MONO)
                            .text_color(theme.text_3)
                            .child(format!("{count} PLUGINS")),
                    )
                    .child(
                        Button::new(
                            "plugins-rescan",
                            if scanning { "Scanning…" } else { "Rescan" },
                        )
                        .compact()
                        .with_icon("cycle")
                        .disabled(scanning)
                        .tooltip("Look in the plugin folders again; rebuilt lsuite plugins reload")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.daw.update(cx, |daw, cx| {
                                daw.fire("plugin.scan", cx);
                            })
                        })),
                    ),
            );
        let _ = window;
        let list = if count == 0 {
            div()
                .py(px(24.0))
                .child(modal::note(
                    "No plugins found here yet. Install CLAP, VST3 or Audio Unit plugins the usual way and press Rescan, or build one with your agent.",
                    cx,
                ))
                .into_any_element()
        } else {
            uniform_list(
                "plugins-installed",
                count,
                cx.processor(|this, range, window, cx| this.plugin_rows(range, window, cx)),
            )
            .w_full()
            .h(px((count as f32 * ROW).min(ROW * 8.0)))
            .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(header)
            .child(list)
    }

    fn formats_part(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = Theme::get(cx).clone();
        let catalog = &self.daw.read(cx).app.catalog;
        let count = |f: Format| catalog.iter().filter(|d| d.format == f).count();
        let folders = |f: Format| -> Vec<String> {
            ryolune_engine::host::scan::directories(f)
                .into_iter()
                .map(|p| p.display().to_string())
                .collect()
        };
        let mut formats: Vec<(AnyElement, &str, &str, Vec<String>, usize)> = vec![
            (
                crate::ui::agent_panel::provider_logo("lsuite", "", 30.0, cx),
                "lsuite plugins",
                "Plugins written in Rust on ryolune's SDK, by you or your agent. They load like ryolune's own and reload when rebuilt.",
                folders(Format::Native),
                count(Format::Native),
            ),
            (
                logo_tile("logos/format-clap.svg", 30.0, cx),
                "CLAP",
                "The open plugin standard (free-audio). Note expressions, sample-accurate automation.",
                folders(Format::Clap),
                count(Format::Clap),
            ),
            (
                logo_tile("logos/format-vst3.svg", 30.0, cx),
                "VST3",
                "Steinberg's plugin format, with its own editor windows and programs.",
                folders(Format::Vst3),
                count(Format::Vst3),
            ),
        ];
        if cfg!(target_os = "macos") {
            formats.push((
                logo_tile("logos/format-au.svg", 30.0, cx),
                "Audio Units",
                "Apple's plugin format, from the system's registry, with factory presets.",
                vec!["Registered with macOS (~/Library/Audio/Plug-Ins/Components, /Library/Audio/Plug-Ins/Components)".into()],
                count(Format::AudioUnit),
            ));
        }
        let mut part = div().flex().flex_col();
        for (logo, name, line, places, n) in formats {
            part = part.child(
                div()
                    .flex()
                    .gap(px(12.0))
                    .py(px(10.0))
                    .border_b_1()
                    .border_color(theme.hairline)
                    .child(logo)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(size::BASE))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(theme.text)
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(size::XS))
                                            .font_family(FONT_MONO)
                                            .text_color(theme.text_3)
                                            .child(format!("{n} FOUND")),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(size::SM))
                                    .text_color(theme.text_2)
                                    .child(line),
                            )
                            .children(places.into_iter().take(6).map(|p| {
                                div()
                                    .text_size(px(size::XS))
                                    .font_family(FONT_MONO)
                                    .text_color(theme.text_3)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(p)
                            })),
                    ),
            );
        }
        part.child(
            div().pt(px(8.0)).child(
                Button::new("plugins-folders", "Other folders…")
                    .ghost()
                    .compact()
                    .tooltip("Settings › Plugins: more folders to scan")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.daw.update(cx, |daw, cx| {
                            daw.run(
                                "ui.showPanel",
                                json!({"panel": "plugins", "visible": false}),
                                cx,
                            );
                            daw.run(
                                "ui.showPanel",
                                json!({"panel": "settings", "section": "plugins"}),
                                cx,
                            );
                        })
                    })),
            ),
        )
    }

    fn build_part(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let theme = Theme::get(cx).clone();
        if !self.plugins.asked_toolchain {
            self.plugins.asked_toolchain = true;
            let daw = self.daw.clone();
            cx.defer(move |cx| {
                daw.update(cx, |daw, cx| {
                    daw.request("plugin.toolchain", json!({}), cx).ok();
                })
            });
        }
        let app = &self.daw.read(cx).app;
        let toolchain = app.plugin_toolchain.clone();
        let allowed = app.settings.agent.permissions.plugins;
        let job = app.plugin_job.clone();
        let last = app.plugin_result.clone();
        let running = app.agents.runtime.running();
        let text = self.plugins.describe.read(cx).text().trim().to_string();
        let focused = self.plugins.describe.read(cx).is_focused(window);
        let rust = match &toolchain {
            None => ("Checking for Rust…".to_string(), None),
            Some(t) if t["ok"] == true => (
                format!(
                    "Rust is installed · {}",
                    t["version"].as_str().unwrap_or("")
                ),
                None,
            ),
            Some(t) => (
                "Rust is not installed: the agent needs it to build plugins.".to_string(),
                Some(t["installHint"].as_str().unwrap_or("").to_string()),
            ),
        };
        let rust_missing = rust.1.is_some();
        let mut part = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(modal::text(
                "Say what you want to hear. Your agent writes the plugin in Rust on ryolune's SDK, builds it, and it appears in Installed, loaded, without restarting. You follow along in the agent panel.",
                cx,
            ))
            .child(
                field(&self.plugins.describe, focused, cx).min_h(px(84.0)),
            )
            .child(modal::field_row(
                "Let the agent build and install plugins",
                Some("Settings › Agent › plugins. Pressing Build allows it.".into()),
                Switch::new("plugins-permission", allowed).on_toggle({
                    let daw = self.daw.clone();
                    move |on, _, cx| {
                        daw.update(cx, |daw, cx| {
                            daw.run(
                                "settings.set",
                                json!({"path": "agent.permissions.plugins", "value": on}),
                                cx,
                            );
                        })
                    }
                }),
                cx,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        Button::new("plugins-build", "Build with your agent")
                            .primary()
                            .with_icon("sparkle")
                            .disabled(text.is_empty() || running || rust_missing)
                            .tooltip(if running {
                                "The agent is busy; wait for it or stop it"
                            } else {
                                "Opens the agent panel on the plugin recipe"
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.build_with_agent(cx))),
                    )
                    .when_some(job, |d, job| {
                        d.child(modal::note(
                            match job.as_str() {
                                "plugin.build" => "Building…",
                                "plugin.publishLocal" => "Building and installing…",
                                _ => "Working…",
                            },
                            cx,
                        ))
                    }),
            );
        part = part.child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .p(px(10.0))
                .bg(theme.well)
                .border_1()
                .border_color(theme.line)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(size::SM))
                                .text_color(theme.text_2)
                                .child(rust.0),
                        )
                        .child(
                            Button::new("plugins-recheck", "Check again")
                                .compact()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.daw.update(cx, |daw, cx| {
                                        daw.app.plugin_toolchain = None;
                                        daw.request("plugin.toolchain", json!({}), cx).ok();
                                    })
                                })),
                        ),
                )
                .when_some(rust.1, |d, hint| {
                    d.child(modal::note(hint, cx)).child(
                        div().flex().child(
                            Button::new("plugins-rustup", "Install Rust…")
                                .compact()
                                .with_icon("external")
                                .tooltip("Opens rustup.rs; nothing is installed until you run it")
                                .on_click(|_, _, cx| cx.open_url("https://rustup.rs")),
                        ),
                    )
                }),
        );
        if let Some((method, result)) = last {
            let line = match (&method[..], &result) {
                ("plugin.publishLocal", Ok(v)) if v["ok"] == true => Some(format!(
                    "{} is installed and loaded.",
                    v["plugins"][0]["name"].as_str().unwrap_or("The plugin")
                )),
                ("plugin.publishLocal" | "plugin.build", Ok(v))
                    if v["ok"] == false || v["build"]["ok"] == false =>
                {
                    let errors = if v["errors"].is_array() {
                        &v["errors"]
                    } else {
                        &v["build"]["errors"]
                    };
                    let n = errors.as_array().map_or(0, Vec::len);
                    Some(format!(
                        "The last build has {n} error{}: the agent fixes them from plugin.build.",
                        if n == 1 { "" } else { "s" }
                    ))
                }
                (_, Err(e)) => Some(e.clone()),
                _ => None,
            };
            if let Some(line) = line {
                part = part.child(modal::note(line, cx));
            }
        }
        part.child(
            div().flex().gap(px(8.0)).child(
                Button::new("plugins-sources", "Show sources")
                    .ghost()
                    .compact()
                    .with_icon("folder")
                    .tooltip("~/.lsuite/plugins-src/ryolune: the crates your agent wrote")
                    .on_click(|_, _, _| {
                        let dir = ryolune_engine::plugin_dev::sources_dir();
                        let _ = std::fs::create_dir_all(&dir);
                        crate::settings::reveal(&dir);
                    }),
            ),
        )
    }

    /// Start the agent on the recipe with the person's words. Pressing Build is the person
    /// asking for a plugin, so it also allows the agent to build and install one.
    pub(super) fn build_with_agent(&mut self, cx: &mut Context<Self>) {
        let text = self.plugins.describe.read(cx).text().trim().to_string();
        if text.is_empty() {
            return;
        }
        let started = self.daw.update(cx, |daw, cx| {
            if !daw.app.settings.agent.permissions.plugins {
                daw.run(
                    "settings.set",
                    json!({"path": "agent.permissions.plugins", "value": true}),
                    cx,
                );
            }
            daw.run(
                "ui.showPanel",
                json!({"panel": "plugins", "visible": false}),
                cx,
            );
            daw.run("agent.send", json!({ "prompt": recipe_prompt(&text) }), cx)
                .is_some()
        });
        if started {
            self.plugins
                .describe
                .update(cx, |input, cx| input.set_text(String::new(), cx));
        }
        cx.notify();
    }

    pub(super) fn plugins_sheet(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let part = self.daw.read(cx).app.show_plugins.unwrap_or(1).min(3);
        if part != 3 {
            self.plugins.asked_toolchain = false;
        }
        let daw = self.daw.clone();
        let tabs = Segmented::new("plugins-parts", PARTS, part).on_select(move |i, _, cx| {
            daw.update(cx, |daw, cx| {
                daw.run(
                    "ui.showPanel",
                    json!({"panel": "plugins", "section": SECTIONS[i.min(3)]}),
                    cx,
                );
            })
        });
        let content = match part {
            0 => self.stock_part(cx),
            1 => self.installed_part(window, cx),
            2 => self.formats_part(cx),
            _ => self.build_part(window, cx),
        };
        modal::sheet("plugins", "Plugins", 760.0, Some(super::close(cx)), cx)
            .child(div().px(px(20.0)).pb(px(10.0)).child(tabs))
            .child(modal::body("plugins-body").child(content))
            .child(
                modal::footer(cx).child(Button::new("plugins-done", "Done").primary().on_click(
                    cx.listener(|this, _, window, cx| this.dismiss(&modal::Dismiss, window, cx)),
                )),
            )
    }
}

fn relative_half() -> gpui::DefiniteLength {
    gpui::relative(0.5)
}

/// A section title in caps, mono, running into a hairline.
fn section_title(title: &str, cx: &App) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .pt(px(10.0))
        .pb(px(2.0))
        .child(
            div()
                .text_size(px(size::XS))
                .font_family(FONT_MONO)
                .text_color(theme.text_3)
                .child(title.to_uppercase()),
        )
        .child(div().flex_1().h(px(1.0)).bg(theme.hairline))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recipe_names_every_step() {
        let prompt = recipe_prompt(" a bitcrusher ");
        assert!(prompt.starts_with("Build me a ryolune plugin: a bitcrusher\n"));
        for step in [
            "plugin.guide",
            "plugin.toolchain",
            "plugin.new",
            "plugin.writeSource",
            "plugin.build",
            "plugin.publishLocal",
        ] {
            assert!(prompt.contains(step), "{step}");
        }
    }
}
