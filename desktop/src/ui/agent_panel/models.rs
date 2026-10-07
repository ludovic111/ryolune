//! The model picker in the composer's bar: the maker's mark, the model and the reasoning
//! effort; clicking opens the models the connected accounts and APIs report
//! (`agent.models`, fetched on open, nothing sent to a model), searchable, with only the
//! efforts each model advertises. "Use this model" saves provider, model and effort in one
//! `agent.configure`.

use super::{connection, jobs, AgentPanel};
use crate::ui::{
    daw::Daw,
    theme::{radius, size, Theme},
    widgets::{field, icon, select_button, Button, InputEvent, MenuItem, TextInput},
};
use gpui::{
    div, img, prelude::*, px, svg, AnyElement, App, Context, Entity, FontWeight, SharedString,
    Subscription, Task, Window,
};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub efforts: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub provider: String,
    pub label: String,
    pub models: Vec<Model>,
    pub error: Option<String>,
}

/// The groups `agent.models` reports.
pub fn groups_from(value: &Value) -> Vec<Group> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            Some(Group {
                provider: g["provider"].as_str()?.to_string(),
                label: g["label"].as_str().unwrap_or("").to_string(),
                models: g["models"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|m| {
                        Some(Model {
                            id: m["id"].as_str()?.to_string(),
                            name: m["name"].as_str().unwrap_or("").to_string(),
                            efforts: m["efforts"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|e| e.as_str().map(str::to_string))
                                .collect(),
                        })
                    })
                    .collect(),
                error: g["error"].as_str().map(str::to_string),
            })
        })
        .collect()
}

/// Whether a model answers a search: its name, id, service or the group's label.
pub fn model_matches(group: &Group, model: &Model, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || format!(
            "{} {} {} {}",
            model.name,
            model.id,
            connection::provider_name_of(&group.provider),
            group.label
        )
        .to_lowercase()
        .contains(&query)
}

pub struct ModelMenu {
    pub open: bool,
    search: Entity<TextInput>,
    /// The choice being made; saved only by "Use this model".
    provider: String,
    model: String,
    effort: String,
    groups: Vec<Group>,
    loading: bool,
    /// The catalog could not be fetched.
    error: String,
    /// `agent.configure` refused the choice.
    save_error: String,
    /// When a click outside closed the menu: the same click on the picker must not reopen it.
    dismissed: Option<std::time::Instant>,
    _task: Option<Task<()>>,
    _search: Subscription,
}

impl ModelMenu {
    pub fn new(window: &mut Window, cx: &mut Context<AgentPanel>) -> Self {
        let search = cx.new(|cx| TextInput::new(cx).placeholder("Search your models…"));
        let subscription = cx.subscribe_in(&search, window, |this, _, event, _, cx| match event {
            InputEvent::Cancel => {
                this.models.open = false;
                cx.notify();
            }
            _ => cx.notify(),
        });
        Self {
            open: false,
            search,
            provider: String::new(),
            model: String::new(),
            effort: String::new(),
            groups: vec![],
            loading: false,
            error: String::new(),
            save_error: String::new(),
            dismissed: None,
            _task: None,
            _search: subscription,
        }
    }

    fn selected(&self) -> Option<&Model> {
        self.groups
            .iter()
            .find(|g| g.provider == self.provider)?
            .models
            .iter()
            .find(|m| m.id == self.model)
    }

    /// Fetch the models from every connection, without asking any of them anything.
    pub(super) fn refresh(&mut self, daw: &Entity<Daw>, cx: &mut Context<AgentPanel>) {
        self.loading = true;
        self.error.clear();
        let answer = jobs::request(daw, "agent.models", json!({}), cx);
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                let menu = &mut this.models;
                menu.loading = false;
                match result {
                    Ok(value) => menu.groups = groups_from(&value),
                    Err(_) => {
                        menu.error =
                            "Could not load your models. Check your connections and retry.".into()
                    }
                }
                cx.notify();
            });
        }));
    }
}

/// A maker's mark on its white tile; a neutral symbol for a model nobody can name.
pub fn provider_logo(provider: &str, model: &str, size_px: f32, cx: &App) -> AnyElement {
    let theme = Theme::get(cx);
    let tile = div()
        .flex_none()
        .size(px(size_px))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius::SM))
        .bg(theme.logo_tile)
        .border_1()
        .border_color(theme.line);
    let inner = size_px * 0.68;
    if provider == "lsuite" && model.is_empty() {
        // lsuite's own mark is a tile already: the grain tile, edge to edge.
        return div()
            .flex_none()
            .size(px(size_px))
            .border_1()
            .border_color(theme.line)
            .child(img("providers/lsuite.svg").size(px(size_px - 2.0)))
            .into_any_element();
    }
    match connection::model_brand(provider, model) {
        Some((_, file)) if file.ends_with("-color") => tile
            .child(img(format!("providers/{file}.svg")).size(px(inner)))
            .into_any_element(),
        Some((_, file)) => tile
            .child(
                svg()
                    .path(format!("providers/{file}.svg"))
                    .size(px(inner))
                    .text_color(theme.logo_ink),
            )
            .into_any_element(),
        None => tile
            .child(icon("provider", inner, theme.logo_ink))
            .into_any_element(),
    }
}

impl AgentPanel {
    /// The bar's button: mark, model, effort.
    pub(super) fn model_button(&self, cx: &Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let app = &self.daw.read(cx).app;
        let provider = app.settings.agent.provider.key();
        let model = app.settings.model();
        let effort = app.settings.agent.reasoning_effort.clone();
        let busy = app.agents.runtime.running();
        let label = self
            .models
            .groups
            .iter()
            .find(|g| g.provider == provider)
            .and_then(|g| g.models.iter().find(|m| m.id == model))
            .map(|m| m.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| {
                if model.is_empty() {
                    "Service default".into()
                } else {
                    model.clone()
                }
            });
        let effort = if effort.is_empty() {
            "Auto".to_string()
        } else {
            connection::effort_name(&effort)
        };
        div()
            .id("model-picker")
            .flex()
            .flex_initial()
            .min_w_0()
            .max_w(gpui::relative(0.62))
            .items_center()
            .gap(px(6.0))
            .pl(px(3.0))
            .pr(px(8.0))
            .py(px(3.0))
            .text_size(px(size::SM))
            .when(self.models.open, |d| d.bg(theme.hover))
            .when(!busy, |d| d.cursor_pointer().hover(|s| s.bg(theme.hover)))
            .when(busy, |d| d.opacity(0.6))
            .child(provider_logo(provider, &model, 20.0, cx))
            .child(
                div()
                    .min_w_0()
                    .flex_shrink()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(theme.text)
                    .child(label),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(3.0))
                    .text_color(theme.text_2)
                    .child(effort)
                    .child(icon("chevron-down", 8.0, theme.text_3)),
            )
            .tooltip(|_, cx| crate::ui::widgets::tip("Choose agent model".into(), cx))
            .when(!busy, |d| {
                d.on_click(cx.listener(|this, _, window, cx| this.toggle_models(window, cx)))
            })
            .into_any_element()
    }

    fn toggle_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let just_dismissed = self
            .models
            .dismissed
            .take()
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(400));
        if self.models.open {
            self.models.open = false;
        } else if !just_dismissed {
            let settings = &self.daw.read(cx).app.settings;
            let (provider, model, effort) = (
                settings.agent.provider.key().to_string(),
                settings.model(),
                settings.agent.reasoning_effort.clone(),
            );
            let menu = &mut self.models;
            menu.open = true;
            menu.provider = provider;
            menu.model = model;
            menu.effort = effort;
            menu.save_error.clear();
            menu.search.update(cx, |input, cx| input.set_text("", cx));
            let daw = self.daw.clone();
            self.models.refresh(&daw, cx);
            let search = self.models.search.clone();
            search.update(cx, |input, _| input.focus(window));
        }
        cx.notify();
    }

    fn save_model(&mut self, cx: &mut Context<Self>) {
        let menu = &self.models;
        let params = json!({
            "provider": menu.provider,
            "model": menu.model.trim(),
            "reasoningEffort": menu.effort,
        });
        let result = self
            .daw
            .update(cx, |daw, cx| daw.request("agent.configure", params, cx));
        match result {
            Ok(_) => self.models.open = false,
            Err(error) => self.models.save_error = error,
        }
        cx.notify();
    }

    /// The menu above the composer card.
    pub(super) fn model_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let busy = self.busy(cx);
        let menu = &self.models;
        let query = menu.search.read(cx).text().to_string();
        let selected = menu.selected().cloned();
        let search_focused = menu.search.read(cx).is_focused(window);
        let option =
            |id: SharedString, provider: &str, model: &Model, on: bool, cx: &mut Context<Self>| {
                let (p, m) = (provider.to_string(), model.id.clone());
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(10.0))
                    .py(px(7.0))
                    .border_b_1()
                    .border_color(theme.hairline)
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme.accent_soft))
                    .hover(|s| s.bg(theme.hover))
                    .child(radio(on, &theme))
                    .child(provider_logo(provider, &model.id, 20.0, cx))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_color(theme.text)
                                    .child(if model.name.is_empty() {
                                        model.id.clone()
                                    } else {
                                        model.name.clone()
                                    }),
                            )
                            .when(model.name != model.id && !model.name.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_size(px(size::XS))
                                        .text_color(theme.text_3)
                                        .child(model.id.clone()),
                                )
                            }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let menu = &mut this.models;
                        if menu.provider != p || menu.model != m {
                            menu.provider = p.clone();
                            menu.model = m.clone();
                            menu.effort.clear();
                        }
                        cx.notify();
                    }))
            };

        let mut list = div()
            .id("model-list")
            .max_h(px(240.0))
            .overflow_y_scroll()
            .rounded(px(radius::SM))
            .bg(theme.well)
            .border_1()
            .border_color(theme.line);
        if selected.is_none() && query.trim().is_empty() {
            let current = Model {
                id: menu.model.clone(),
                name: if menu.model.is_empty() {
                    "Service default".into()
                } else {
                    menu.model.clone()
                },
                efforts: vec![],
            };
            let provider = menu.provider.clone();
            list = list.child(
                option("model-current".into(), &provider, &current, true, cx).child(
                    div()
                        .ml_auto()
                        .text_size(px(size::XS))
                        .text_color(theme.text_3)
                        .child("Current selection"),
                ),
            );
        }
        let mut any = false;
        let groups = self.models.groups.clone();
        let (chosen_provider, chosen_model) =
            (self.models.provider.clone(), self.models.model.clone());
        for group in groups.iter().filter(|g| !g.models.is_empty()) {
            let found: Vec<&Model> = group
                .models
                .iter()
                .filter(|m| model_matches(group, m, &query))
                .collect();
            if found.is_empty() {
                continue;
            }
            any = true;
            list = list.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(7.0))
                    .px(px(9.0))
                    .py(px(8.0))
                    .bg(theme.glass(3))
                    .border_b_1()
                    .border_color(theme.hairline)
                    .text_size(px(size::SM))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(provider_logo(&group.provider, "", 18.0, cx))
                    .child(connection::provider_name_of(&group.provider))
                    .child(
                        div()
                            .font_weight(FontWeight::NORMAL)
                            .text_color(theme.text_3)
                            .child(group.label.clone()),
                    ),
            );
            for model in found {
                let on = chosen_provider == group.provider && chosen_model == model.id;
                let id = SharedString::from(format!("model-{}-{}", group.provider, model.id));
                list = list.child(option(id, &group.provider, model, on, cx));
            }
        }
        if !query.trim().is_empty() && !any {
            list = list.child(
                div()
                    .p(px(10.0))
                    .text_color(theme.text_3)
                    .child("No matching models."),
            );
        }

        let menu = &self.models;
        let efforts = selected.as_ref().map_or(vec![], |m| m.efforts.clone());
        let effort_label = if menu.effort.is_empty() {
            "Provider default".to_string()
        } else if efforts.contains(&menu.effort) {
            connection::effort_name(&menu.effort)
        } else {
            format!("{} · saved", connection::effort_name(&menu.effort))
        };
        let note = |text: String| {
            div()
                .text_size(px(size::SM))
                .line_height(px(18.0))
                .text_color(theme.text_2)
                .child(text)
        };
        let loading = menu.loading;
        let mut notes: Vec<String> = vec![];
        if selected.as_ref().is_some_and(|m| m.efforts.is_empty()) {
            notes.push(
                "This provider does not advertise adjustable thinking modes for this model.".into(),
            );
        }
        notes.push(if loading {
            "Fetching models from your connections…".into()
        } else {
            "Models reported by your connected accounts and APIs.".into()
        });
        for group in menu.groups.iter().filter(|g| g.error.is_some()) {
            notes.push(format!(
                "{}: {}",
                connection::provider_name_of(&group.provider),
                group.error.clone().unwrap_or_default()
            ));
        }
        let failure = [menu.save_error.clone(), menu.error.clone()]
            .into_iter()
            .find(|e| !e.is_empty());

        div()
            .id("model-menu")
            .absolute()
            .left_0()
            .right_0()
            .bottom_full()
            .mb(px(6.0))
            .max_h(px(520.0))
            .overflow_y_scroll()
            .p(px(14.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .rounded(px(radius::LG))
            .bg(theme.glass(3))
            .border_1()
            .border_color(theme.glass_edge)
            .shadow(vec![gpui::BoxShadow {
                color: theme.glass_shadow,
                offset: gpui::point(px(0.0), px(12.0)),
                blur_radius: px(32.0),
                spread_radius: px(0.0),
            }])
            .text_size(px(size::BASE))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                if this.models.open && !this.menu.is_open() {
                    this.models.open = false;
                    this.models.dismissed = Some(std::time::Instant::now());
                    cx.notify();
                }
            }))
            .when(busy, |d| d.opacity(0.6))
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .child("Your connected models"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.0))
                    .child(
                        div()
                            .text_size(px(size::SM))
                            .text_color(theme.text_2)
                            .child("Find a model"),
                    )
                    .child(field(&self.models.search, search_focused, cx)),
            )
            .child(list)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.0))
                    .child(
                        div()
                            .text_size(px(size::SM))
                            .text_color(theme.text_2)
                            .child("Reasoning"),
                    )
                    .child(
                        select_button("effort-select", effort_label, cx).on_click(cx.listener(
                            move |this, e: &gpui::ClickEvent, window, cx| {
                                let current = this.models.effort.clone();
                                let mut options =
                                    vec![(String::new(), "Provider default".to_string())];
                                options.extend(
                                    efforts
                                        .iter()
                                        .map(|e| (e.clone(), connection::effort_name(e))),
                                );
                                if !current.is_empty() && !efforts.contains(&current) {
                                    options.push((
                                        current.clone(),
                                        format!("{} · saved", connection::effort_name(&current)),
                                    ));
                                }
                                let entity = cx.entity();
                                let items = options
                                    .into_iter()
                                    .map(|(value, label)| {
                                        let entity = entity.clone();
                                        let on = value == current;
                                        MenuItem::new(label, move |_, cx| {
                                            let value = value.clone();
                                            entity.update(cx, |this, cx| {
                                                this.models.effort = value;
                                                cx.notify();
                                            });
                                        })
                                        .checked(on)
                                    })
                                    .collect();
                                this.menu.open(items, e.position(), window, cx);
                            },
                        )),
                    ),
            )
            .children(notes.into_iter().map(note))
            .when_some(failure, |d, text| {
                d.child(
                    div()
                        .text_size(px(size::SM))
                        .text_color(theme.danger)
                        .child(text),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.0))
                    .child(
                        Button::new("models-refresh", "Refresh models")
                            .disabled(loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let daw = this.daw.clone();
                                this.models.refresh(&daw, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("models-use", "Use this model")
                            .primary()
                            .disabled(loading || busy)
                            .on_click(cx.listener(|this, _, _, cx| this.save_model(cx))),
                    ),
            )
            .child(
                div().flex().child(
                    Button::new("models-manage", "Manage connections")
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.models.open = false;
                            this.open_settings("agent", cx);
                        })),
                ),
            )
            .into_any_element()
    }
}

/// A radio mark: a ring, filled with the accent when chosen.
fn radio(on: bool, theme: &Theme) -> gpui::Div {
    div()
        .flex_none()
        .size(px(13.0))
        .border_1()
        .border_color(if on { theme.accent } else { theme.line_strong })
        .flex()
        .items_center()
        .justify_center()
        .when(on, |d| d.child(div().size(px(7.0)).bg(theme.accent)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_catalog_and_searches_it() {
        let groups = groups_from(&json!([
            {"provider":"codex","label":"Account","models":[{"id":"future-model","name":"Future model","efforts":["low","xhigh"]}],"error":null},
            {"provider":"compatible","label":"Local server","models":[{"id":"qwen-test","name":"Qwen test","efforts":[]}],"error":"slow"}
        ]));
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].models[0].efforts, ["low", "xhigh"]);
        assert_eq!(groups[1].error.as_deref(), Some("slow"));
        let (g, m) = (&groups[0], &groups[0].models[0]);
        assert!(model_matches(g, m, ""));
        assert!(model_matches(g, m, "FUTURE"));
        assert!(
            model_matches(g, m, "codex"),
            "the service's name finds its models"
        );
        assert!(model_matches(g, m, "account"));
        assert!(!model_matches(g, m, "qwen"));
    }
}
