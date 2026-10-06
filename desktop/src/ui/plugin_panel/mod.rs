//! Plugin windows: one floating panel per open window (`ui.openPluginWindow`). A stock
//! plugin shows its designed face (nameplate, a display drawn from what it does, a selector
//! per choice and a dial per continuous parameter); any other plugin a parameter list with a
//! search. Both carry the programs and presets menu, bypass, the key that opens the plugin's
//! own editor window, and an Automate key per parameter. Every change is a registry command
//! (`strip.setParameter`, `strip.setProgram`, `preset.*`, `strip.setBypass`), so the window
//! does what a script or the agent can do; a dial's drag is one undo step.
//!
//! Panels are glass tier 2, dragged by their header and closed with `ui.closePluginWindow`.

pub mod display;
pub mod params;
pub mod response;

use super::{
    automation::FocusLane,
    daw::Daw,
    theme::{radius, size, with_alpha, Theme, FONT_MONO},
    widgets::{
        self, caps, field, icon, Button, InputEvent, Knob, MenuHost, MenuItem, Phase, Segmented,
        Slider, TextInput,
    },
};
use crate::app::Ryolune;
use display::{Inks, Mark, Values};
use gpui::{
    canvas, div, point, prelude::*, px, uniform_list, AnyElement, App, Context, ElementId, Empty,
    Entity, Hsla, MouseButton, MouseDownEvent, Pixels, Point, SharedString, Subscription, Window,
};
use params::{Parameter, Typed};
use ryolune_engine::model::{Session, Strip};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Width of a stock plugin's face and of the generic parameter panel.
const FACE_W: f32 = 600.0;
const LIST_W: f32 = 540.0;
/// A row of the generic parameter list.
const ROW_H: f32 = 30.0;
/// The display's height, as the design's.
const DISPLAY_H: f32 = 148.0;

/// Where a plugin window's plugin sits now: its strip and insert slot (`None` for a MIDI
/// track's instrument). Windows are keyed by rack key, so a plugin moved up or down keeps its
/// window and the panel reads the slot it is in now, never the one it opened on.
pub fn locate(session: &Session, key: &str) -> Option<(String, Option<usize>)> {
    for (strip_id, strip) in &session.strips {
        if let Some(slot) = strip.inserts.iter().position(|i| i.id == key) {
            return Some((strip_id.clone(), Some(slot)));
        }
        if strip.synth.as_ref().is_some_and(|s| s.id == key) {
            return Some((strip_id.clone(), None));
        }
    }
    // A stock instrument nobody has touched has no insert yet: its key is derived.
    session
        .tracks
        .iter()
        .filter(|t| t.kind == "midi")
        .find(|t| {
            let strip = session
                .strips
                .get(&t.id)
                .cloned()
                .unwrap_or_else(Strip::default);
            strip.synth.is_none() && strip.synth_key(&t.id) == key
        })
        .map(|t| (t.id.clone(), None))
}

/// What a panel shows, read from the registry when the document moves.
#[derive(Clone, Debug, Default)]
struct Info {
    track: String,
    slot: Option<usize>,
    plugin_id: String,
    name: String,
    bypassed: bool,
    has_gui: bool,
    parameters: Vec<Parameter>,
}

impl Info {
    fn stock(&self) -> bool {
        self.plugin_id.starts_with("stock:")
    }
    /// The face's name: a stock plugin's own, else the insert's.
    fn title(&self) -> &str {
        self.plugin_id.strip_prefix("stock:").unwrap_or(&self.name)
    }
    /// `trackId` and `slot` as every strip command takes them.
    fn target(&self) -> Value {
        let mut v = json!({ "trackId": self.track });
        if let Some(slot) = self.slot {
            v["slot"] = json!(slot);
        }
        v
    }
    fn with(&self, extra: Value) -> Value {
        let mut v = self.target();
        if let (Some(v), Value::Object(extra)) = (v.as_object_mut(), extra) {
            v.extend(extra);
        }
        v
    }
    fn parameter(&self, id: u32) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.id == id)
    }
}

impl Values for Info {
    fn v(&self, name: &str, fallback: f64) -> f64 {
        self.parameters
            .iter()
            .find(|p| p.name == name)
            .map_or(fallback, |p| p.value)
    }
    fn has(&self, name: &str) -> bool {
        self.parameters.iter().any(|p| p.name == name)
    }
}

/// Read a window's plugin through `strip.parameters` (the window's loaded instance answers).
fn read(app: &mut Ryolune, key: &str) -> Result<Info, String> {
    let (track, slot) =
        locate(app.store.session(), key).ok_or("This plugin is no longer in the song")?;
    let mut info = Info {
        track,
        slot,
        ..Default::default()
    };
    let v = app.run_control_command(
        "strip.parameters",
        &info.with(json!({ "limit": 10000 })),
        false,
        "Interface",
    )?;
    info.plugin_id = v["pluginId"].as_str().unwrap_or_default().to_string();
    info.name = v["plugin"].as_str().unwrap_or_default().to_string();
    info.bypassed = v["bypassed"].as_bool().unwrap_or(false);
    info.has_gui = v["hasGui"].as_bool().unwrap_or(false);
    info.parameters = v["parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Parameter::from_json)
        .collect();
    Ok(info)
}

/// The sound folder the engine files a plugin under, which gives the face its colour.
fn folder_of(app: &Ryolune, plugin_id: &str) -> Option<String> {
    use ryolune_engine::control_plugins::{folder, AutoFolders};
    let d = app.catalog.iter().find(|d| d.id == plugin_id)?;
    Some(folder(
        d,
        &app.settings.plugins,
        &AutoFolders::new(&app.catalog),
    ))
}

/// One open plugin window.
struct Panel {
    origin: Point<Pixels>,
    info: Option<Info>,
    error: Option<String>,
    /// The store revision `info` was read at.
    revision: Option<u64>,
    folder: Option<(String, Option<String>)>,
    /// A dial or slider in a drag: its value is ours until release.
    dragging: Option<u32>,
    filter: Entity<TextInput>,
    preset_name: Entity<TextInput>,
    value: Entity<TextInput>,
    /// The parameter whose value is being typed.
    editing: Option<u32>,
    saving: bool,
    _subscriptions: Vec<Subscription>,
}

/// The payload of a header drag; the key tells panels apart.
#[derive(Clone)]
struct PanelMove(String);

pub struct PluginPanels {
    daw: Entity<Daw>,
    panels: HashMap<String, Panel>,
    /// Back to front: the last is drawn on top.
    order: Vec<String>,
    /// The header being dragged: where the pointer pressed and where the panel was.
    moving: Option<(String, Point<Pixels>, Point<Pixels>)>,
    menu: MenuHost,
    opened: usize,
}

impl PluginPanels {
    pub fn new(daw: Entity<Daw>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            daw,
            panels: HashMap::new(),
            order: vec![],
            moving: None,
            menu: MenuHost::default(),
            opened: 0,
        }
    }

    fn new_panel(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> Panel {
        let filter = cx.new(|cx| TextInput::new(cx).placeholder("Search parameters"));
        let preset_name = cx.new(|cx| TextInput::new(cx).placeholder("Preset name"));
        let value = cx.new(|cx| TextInput::new(cx).mono());
        let k = key.to_string();
        let subscriptions = vec![
            cx.subscribe_in(&filter, window, |_, _, _: &InputEvent, _, cx| cx.notify()),
            cx.subscribe_in(&preset_name, window, {
                let key = k.clone();
                move |this, _, event: &InputEvent, _, cx| match event {
                    InputEvent::Submit => this.save_preset(&key, cx),
                    InputEvent::Cancel => {
                        if let Some(p) = this.panels.get_mut(&key) {
                            p.saving = false;
                        }
                        cx.notify();
                    }
                    _ => {}
                }
            }),
            cx.subscribe_in(&value, window, {
                let key = k.clone();
                move |this, input, event: &InputEvent, _, cx| match event {
                    InputEvent::Submit | InputEvent::Blur => {
                        let text = input.read(cx).text().to_string();
                        this.commit_typed(&key, &text, cx);
                    }
                    InputEvent::Cancel => {
                        if let Some(p) = this.panels.get_mut(&key) {
                            p.editing = None;
                        }
                        cx.notify();
                    }
                    InputEvent::Changed => {}
                }
            }),
        ];
        // Cascade new windows from the upper left of the arrangement.
        let n = (self.opened % 8) as f32;
        self.opened += 1;
        Panel {
            origin: point(px(300.0 + 28.0 * n), px(120.0 + 28.0 * n)),
            info: None,
            error: None,
            revision: None,
            folder: None,
            dragging: None,
            filter,
            preset_name,
            value,
            editing: None,
            saving: false,
            _subscriptions: subscriptions,
        }
    }

    /// Keep one panel per open window, and re-read a panel when the document moved (an
    /// undo, automation, an agent's edit) unless one of its dials is in a drag.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (keys, revision) = {
            let app = &self.daw.read(cx).app;
            let mut keys: Vec<String> = app.plugins.windows.keys().cloned().collect();
            keys.sort();
            (keys, app.store.revision)
        };
        self.panels.retain(|k, _| keys.contains(k));
        self.order.retain(|k| keys.contains(k));
        for key in &keys {
            if !self.panels.contains_key(key) {
                let panel = self.new_panel(key, window, cx);
                self.panels.insert(key.clone(), panel);
                self.order.push(key.clone());
            }
            let stale = self
                .panels
                .get(key)
                .is_some_and(|p| p.revision != Some(revision) && p.dragging.is_none());
            if !stale {
                continue;
            }
            let (result, folder) = self.daw.update(cx, |daw, _| {
                let result = read(&mut daw.app, key);
                let folder = result
                    .as_ref()
                    .ok()
                    .map(|info| (info.plugin_id.clone(), folder_of(&daw.app, &info.plugin_id)));
                (result, folder)
            });
            let panel = self.panels.get_mut(key).expect("panel");
            panel.revision = Some(revision);
            match result {
                Ok(info) => {
                    if panel.folder.as_ref().map(|f| &f.0) != folder.as_ref().map(|f| &f.0) {
                        panel.folder = folder;
                    }
                    panel.info = Some(info);
                    panel.error = None;
                }
                Err(error) => panel.error = Some(error),
            }
        }
    }

    fn raise(&mut self, key: &str) {
        if self.order.last().map(String::as_str) != Some(key) {
            self.order.retain(|k| k != key);
            self.order.push(key.to_string());
        }
    }

    /// Run a registry command; a failure shows in the error dialog.
    fn run(&mut self, method: &str, params: Value, cx: &mut Context<Self>) -> Option<Value> {
        self.daw.update(cx, |daw, cx| daw.run(method, params, cx))
    }

    /// A dial or slider moved: set the parameter, one undo step per drag.
    fn set_position(
        &mut self,
        key: &str,
        id: u32,
        position: f32,
        phase: Phase,
        cx: &mut Context<Self>,
    ) {
        let Some(p) = self
            .panels
            .get(key)
            .and_then(|p| p.info.as_ref()?.parameter(id))
        else {
            return;
        };
        let value = p.value_at(position as f64);
        self.set_value(key, id, value, phase, cx);
    }

    fn set_value(&mut self, key: &str, id: u32, value: f64, phase: Phase, cx: &mut Context<Self>) {
        let Some(panel) = self.panels.get_mut(key) else {
            return;
        };
        let Some(info) = panel.info.as_ref() else {
            return;
        };
        if phase == Phase::Start {
            panel.dragging = Some(id);
            self.daw.update(cx, |daw, _| daw.gesture(true));
        }
        let changed = info.parameter(id).is_some_and(|p| p.value != value);
        if changed {
            let params = info.with(json!({ "parameterId": id, "value": value }));
            let result = self.run("strip.setParameter", params, cx);
            let revision = self.daw.read(cx).app.store.revision;
            if let Some(panel) = self.panels.get_mut(key) {
                // The answer carries the parameter as it now reads: patch it in rather than
                // reading every parameter again on each step of a drag.
                if let (Some(row), Some(info)) = (
                    result.as_ref().and_then(|r| r["changed"].get(0)),
                    panel.info.as_mut(),
                ) {
                    if let (Some(fresh), Some(p)) = (
                        Parameter::from_json(row),
                        info.parameters.iter_mut().find(|p| p.id == id),
                    ) {
                        p.value = fresh.value;
                        p.display = fresh.display;
                    }
                    panel.revision = Some(revision);
                }
            }
        }
        if phase == Phase::End {
            self.daw.update(cx, |daw, _| daw.gesture(false));
            if let Some(panel) = self.panels.get_mut(key) {
                panel.dragging = None;
                panel.revision = None;
            }
        }
        cx.notify();
    }

    /// A choice picked, a switch flipped: one command, one undo step.
    fn pick(&mut self, key: &str, id: u32, value: f64, cx: &mut Context<Self>) {
        self.set_value(key, id, value, Phase::Start, cx);
        self.set_value(key, id, value, Phase::End, cx);
    }

    fn start_typing(&mut self, key: &str, id: u32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.panels.get_mut(key) else {
            return;
        };
        let Some(p) = panel.info.as_ref().and_then(|i| i.parameter(id)) else {
            return;
        };
        let text = if p.is_choice() || !p.display.is_empty() {
            p.shown()
        } else {
            format!("{}", (p.value * 10_000.0).round() / 10_000.0)
        };
        panel.editing = Some(id);
        panel.value.update(cx, |input, cx| {
            input.set_text(text, cx);
            input.select_all_text(cx);
            input.focus(window);
        });
        cx.notify();
    }

    fn commit_typed(&mut self, key: &str, text: &str, cx: &mut Context<Self>) {
        let Some(panel) = self.panels.get_mut(key) else {
            return;
        };
        let Some(id) = panel.editing.take() else {
            return;
        };
        let Some(info) = panel.info.as_ref() else {
            return;
        };
        let Some(p) = info.parameter(id) else {
            return;
        };
        let params = match params::typed(p, text) {
            Some(Typed::Value(value)) => info.with(json!({ "parameterId": id, "value": value })),
            Some(Typed::Text(text)) => info.with(json!({ "parameterId": id, "text": text })),
            None => {
                cx.notify();
                return;
            }
        };
        self.run("strip.setParameter", params, cx);
        cx.notify();
    }

    /// Automate a parameter: its lane if it has one, else a new one; then show it.
    fn automate(&mut self, key: &str, id: u32, cx: &mut Context<Self>) {
        let Some(info) = self.panels.get(key).and_then(|p| p.info.clone()) else {
            return;
        };
        let Some(p) = info.parameter(id) else {
            return;
        };
        let lane = match &p.lane {
            Some(lane) => Some(lane.clone()),
            None => self
                .run(
                    "automation.create",
                    info.with(json!({ "target": "pluginParameter", "parameterId": id })),
                    cx,
                )
                .and_then(|r| r["lane"]["id"].as_str().map(str::to_string)),
        };
        if let Some(lane) = lane {
            cx.set_global(FocusLane(Some(lane)));
            self.run(
                "ui.showPanel",
                json!({ "panel": "automation", "visible": true }),
                cx,
            );
        }
    }

    fn save_preset(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(panel) = self.panels.get(key) else {
            return;
        };
        let name = panel.preset_name.read(cx).text().trim().to_string();
        let Some(info) = panel.info.clone() else {
            return;
        };
        if name.is_empty() {
            return;
        }
        if self
            .run("preset.save", info.with(json!({ "name": name })), cx)
            .is_some()
        {
            if let Some(panel) = self.panels.get_mut(key) {
                panel.saving = false;
                panel.preset_name.update(cx, |i, cx| i.set_text("", cx));
            }
        }
        cx.notify();
    }

    /// The programs and presets menu: the plugin's own programs, ryolune presets, save, and
    /// deleting a preset of yours.
    fn open_presets(
        &mut self,
        key: &str,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(info) = self.panels.get(key).and_then(|p| p.info.clone()) else {
            return;
        };
        let programs = self.daw.update(cx, |daw, _| {
            daw.app
                .run_control_command("strip.programs", &info.target(), false, "Interface")
        });
        let programs = match programs {
            Ok(v) => v,
            Err(error) => {
                if let Some(p) = self.panels.get_mut(key) {
                    p.error = Some(error);
                }
                cx.notify();
                return;
            }
        };
        let this = cx.entity().downgrade();
        let mut items = vec![];
        let list = programs["programs"].as_array().cloned().unwrap_or_default();
        let current = programs["current"].as_u64();
        if !list.is_empty() {
            items.push(MenuItem::Header("Programs".into()));
            for program in list.iter().take(512) {
                let index = program["index"].as_u64().unwrap_or(0);
                let name = program["name"].as_str().unwrap_or_default().to_string();
                let (this, info) = (this.clone(), info.clone());
                items.push(
                    MenuItem::new(name, move |_, cx| {
                        let params = info.with(json!({ "index": index }));
                        let _ =
                            this.update(cx, |this, cx| this.run("strip.setProgram", params, cx));
                    })
                    .checked(current == Some(index)),
                );
            }
        }
        let presets: Vec<(String, bool)> = programs["presets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                Some((
                    p["name"].as_str()?.to_string(),
                    p["factory"].as_bool().unwrap_or(false),
                ))
            })
            .collect();
        if !presets.is_empty() {
            items.push(MenuItem::Header("Presets".into()));
            for (name, factory) in &presets {
                let (this, info, name) = (this.clone(), info.clone(), name.clone());
                items.push(
                    MenuItem::new(name.clone(), move |_, cx| {
                        let params = info.with(json!({ "name": name }));
                        let _ = this.update(cx, |this, cx| this.run("preset.load", params, cx));
                    })
                    .detail(if *factory { "Factory" } else { "" }),
                );
            }
        }
        if !items.is_empty() {
            items.push(MenuItem::Separator);
        }
        {
            let (this, key) = (this.clone(), key.to_string());
            items.push(MenuItem::new("Save preset…", move |window, cx| {
                let _ = this.update(cx, |this, cx| {
                    if let Some(p) = this.panels.get_mut(&key) {
                        p.saving = true;
                        p.preset_name.update(cx, |i, _| i.focus(window));
                    }
                    cx.notify();
                });
            }));
        }
        let mine: Vec<&String> = presets.iter().filter(|p| !p.1).map(|p| &p.0).collect();
        if !mine.is_empty() {
            items.push(MenuItem::Header("Delete a preset".into()));
            for name in mine {
                let (this, plugin, name) = (this.clone(), info.plugin_id.clone(), name.clone());
                items.push(MenuItem::new(name.clone(), move |_, cx| {
                    let params = json!({ "pluginId": plugin, "name": name });
                    let _ = this.update(cx, |this, cx| this.run("preset.delete", params, cx));
                }));
            }
        }
        self.menu.open(items, at, window, cx);
    }

    /// A select's choices as a menu.
    fn open_choices(
        &mut self,
        key: &str,
        id: u32,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(p) = self
            .panels
            .get(key)
            .and_then(|p| p.info.as_ref()?.parameter(id).cloned())
        else {
            return;
        };
        let this = cx.entity().downgrade();
        let current = p.value.round() as usize;
        let items = p
            .labels
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let (this, key) = (this.clone(), key.to_string());
                MenuItem::new(label.clone(), move |_, cx| {
                    let _ = this.update(cx, |this, cx| this.pick(&key, id, i as f64, cx));
                })
                .checked(i == current)
            })
            .collect();
        self.menu.open(items, at, window, cx);
    }
}

/// An id unique to one control of one window.
fn cid(key: &str, what: &str, id: impl std::fmt::Display) -> ElementId {
    ElementId::Name(SharedString::from(format!("{key}/{what}/{id}")))
}

impl PluginPanels {
    fn header(
        &self,
        key: &str,
        panel: &Panel,
        info: Option<&Info>,
        family: Option<Hsla>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let this = cx.entity().downgrade();
        // v2: the chrome is ink, whatever the plugin's family.
        let trim = theme.accent;
        let _ = family;
        let bypassed = info.is_some_and(|i| i.bypassed);
        let native = self
            .daw
            .read(cx)
            .app
            .plugins
            .windows
            .get(key)
            .is_some_and(|w| w.native.is_some());
        let title = info.map_or("Plugin".to_string(), |i| i.title().to_string());
        let category = panel
            .folder
            .as_ref()
            .and_then(|f| f.1.clone())
            .unwrap_or_else(|| "Plugin".into());

        let mut tools = div().flex().items_center().gap(px(6.0));
        if let Some(info) = info {
            let (this2, key2) = (this.clone(), key.to_string());
            tools = tools.child(
                widgets::select_button(cid(key, "presets", 0), "Presets", cx)
                    .w(px(112.0))
                    .on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        let at = point(e.position.x - px(8.0), e.position.y + px(16.0));
                        let _ =
                            this2.update(cx, |this, cx| this.open_presets(&key2, at, window, cx));
                    }),
            );
            if info.slot.is_some() {
                let (this2, target) = (this.clone(), info.target());
                tools = tools.child(
                    Button::icon(cid(key, "bypass", 0), "power")
                        .icon_size(12.0)
                        .lit(!bypassed)
                        .lit_color(trim)
                        .tooltip(if bypassed { "Turn on" } else { "Bypass" })
                        .on_click(move |_, _, cx| {
                            let mut params = target.clone();
                            params["bypassed"] = json!(!bypassed);
                            let _ = this2
                                .update(cx, |this, cx| this.run("strip.setBypass", params, cx));
                        }),
                );
            }
            if info.has_gui && !info.stock() {
                let (this2, target) = (this.clone(), info.target());
                tools = tools.child(
                    Button::icon(cid(key, "native", 0), "window")
                        .icon_size(12.0)
                        .lit(native)
                        .tooltip(if native {
                            "Close the plugin's window"
                        } else {
                            "Open the plugin's window"
                        })
                        .on_click(move |_, _, cx| {
                            let mut params = target.clone();
                            params["native"] = json!(true);
                            let _ = this2
                                .update(cx, |this, cx| this.run("ui.openPluginWindow", params, cx));
                        }),
                );
            }
        }
        let close_key = key.to_string();
        let this2 = this.clone();
        tools = tools.child(
            Button::icon(cid(key, "close", 0), "close")
                .ghost()
                .icon_size(10.0)
                .tooltip("Close")
                .on_click(move |_, _, cx| {
                    let params = json!({ "id": close_key });
                    let _ =
                        this2.update(cx, |this, cx| this.run("ui.closePluginWindow", params, cx));
                }),
        );

        let down_key = key.to_string();
        let move_key = key.to_string();
        div()
            .id(cid(key, "header", 0))
            .debug_selector(|| "plugin-header".into())
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.0))
            .px(px(14.0))
            .pt(px(12.0))
            .pb(px(10.0))
            .border_b_2()
            .border_color(with_alpha(trim, if bypassed { 0.35 } else { 1.0 }))
            .cursor_grab()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    if let Some(panel) = this.panels.get(&down_key) {
                        this.moving = Some((down_key.clone(), e.position, panel.origin));
                    }
                    this.raise(&down_key);
                    cx.notify();
                }),
            )
            .on_drag(PanelMove(key.to_string()), |_, _, _, cx| cx.new(|_| Empty))
            .on_drag_move::<PanelMove>(cx.listener(
                move |this, e: &gpui::DragMoveEvent<PanelMove>, window, cx| {
                    if e.drag(cx).0 != move_key {
                        return;
                    }
                    let Some((key, from, origin)) = this.moving.clone() else {
                        return;
                    };
                    let view = window.viewport_size();
                    let to = origin + (e.event.position - from);
                    if let Some(panel) = this.panels.get_mut(&key) {
                        // Keep the header reachable.
                        panel.origin = point(
                            to.x.clamp(px(-(FACE_W - 120.0)), view.width - px(120.0)),
                            to.y.clamp(px(0.0), view.height - px(48.0)),
                        );
                    }
                    cx.notify();
                },
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .min_w_0()
                    .child(widgets::dot(
                        if bypassed { theme.text_3 } else { trim },
                        8.0,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(size::LG))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme.text)
                                    .line_height(px(20.0))
                                    .truncate()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .font_family(FONT_MONO)
                                    .text_size(px(10.5))
                                    .text_color(if family.is_some() && !bypassed {
                                        trim
                                    } else {
                                        theme.text_3
                                    })
                                    .child(if bypassed {
                                        format!("{} · BYPASSED", category.to_uppercase())
                                    } else {
                                        category.to_uppercase()
                                    }),
                            ),
                    ),
            )
            .child(tools)
            .into_any_element()
    }

    fn save_row(
        &self,
        key: &str,
        panel: &Panel,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let this = cx.entity().downgrade();
        let focused = panel.preset_name.read(cx).is_focused(window);
        let (k1, k2) = (key.to_string(), key.to_string());
        let this2 = this.clone();
        div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(14.0))
            .pt(px(10.0))
            .child(field(&panel.preset_name, focused, cx).flex_1())
            .child(
                Button::new(cid(key, "save", 0), "Save")
                    .primary()
                    .compact()
                    .on_click(move |_, _, cx| {
                        let _ = this.update(cx, |this, cx| this.save_preset(&k1, cx));
                    }),
            )
            .child(
                Button::new(cid(key, "cancel-save", 0), "Cancel")
                    .ghost()
                    .compact()
                    .on_click(move |_, _, cx| {
                        let _ = this2.update(cx, |this, cx| {
                            if let Some(p) = this.panels.get_mut(&k2) {
                                p.saving = false;
                            }
                            cx.notify();
                        });
                    }),
            )
            .into_any_element()
    }

    /// The display: what the plugin does, drawn from its parameters.
    fn display(&self, marks: Vec<Mark>, family: Option<Hsla>, cx: &App) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let inks = Inks::new(&theme, family);
        div()
            .h(px(DISPLAY_H))
            .w_full()
            .rounded(px(radius::MD))
            .overflow_hidden()
            .bg(theme.display)
            .border_1()
            .border_color(theme.hairline)
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                            display::paint(&marks, bounds, inks, window, cx)
                        })
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }

    /// One dial of a stock face: label, knob, value and the Automate key.
    fn dial(
        &self,
        key: &str,
        p: &Parameter,
        family: Option<Hsla>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let this = cx.entity().downgrade();
        let (k1, k2) = (key.to_string(), key.to_string());
        let id = p.id;
        let group = SharedString::from(format!("{key}/dial/{id}"));
        let mut knob = Knob::new(cid(key, "knob", id), p.position(p.value) as f32)
            .size(46.0)
            .default_value(p.position(p.default) as f32)
            .tooltip(format!(
                "{}: drag, Shift for fine, double-click to reset",
                p.name
            ))
            .on_change({
                let this = this.clone();
                move |v, phase, _, cx| {
                    let _ = this.update(cx, |this, cx| this.set_position(&k1, id, v, phase, cx));
                }
            });
        if p.bipolar() {
            knob = knob.bipolar().default_value(p.position(p.default) as f32);
        }
        if let Some(c) = family {
            knob = knob.color(c);
        }
        div()
            .group(group.clone())
            .relative()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.0))
            .min_w_0()
            .child(
                div()
                    .max_w_full()
                    .truncate()
                    .font_family(FONT_MONO)
                    .text_size(px(10.0))
                    .text_color(theme.text_3)
                    .child(p.name.to_uppercase()),
            )
            .child(div().debug_selector(|| format!("knob-{id}")).child(knob))
            .child(
                div()
                    .font_family(FONT_MONO)
                    .text_size(px(size::XS))
                    .text_color(theme.text_2)
                    .whitespace_nowrap()
                    .child(p.format(p.value)),
            )
            .when(p.automatable, |d| {
                d.child(
                    div()
                        .id(cid(key, "automate", id))
                        .absolute()
                        .top(px(14.0))
                        .right(px(2.0))
                        .size(px(16.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme.hover)
                        .font_family(FONT_MONO)
                        .text_size(px(8.5))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(if p.lane.is_some() {
                            theme.accent_text
                        } else {
                            theme.text_3
                        })
                        .opacity(if p.lane.is_some() { 1.0 } else { 0.0 })
                        .group_hover(group, |s| s.opacity(1.0))
                        .cursor_pointer()
                        .hover(|s| s.text_color(theme.accent_text))
                        .tooltip({
                            let name = p.name.clone();
                            move |_, cx| widgets::tip(format!("Automate {name}").into(), cx)
                        })
                        .child("A")
                        .on_click(move |_, _, cx| {
                            let _ = this.update(cx, |this, cx| this.automate(&k2, id, cx));
                        }),
                )
            })
            .into_any_element()
    }

    /// A choice of a stock face: a switch for Off/On, tabs for a few labels, else a select.
    fn choice(
        &self,
        key: &str,
        p: &Parameter,
        family: Option<Hsla>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let this = cx.entity().downgrade();
        let id = p.id;
        let index = p.value.round().max(0.0) as usize;
        let k = key.to_string();
        let control = if p.labels.len() == 2 && p.labels[0] == "Off" {
            let on = index == 1;
            let b = Button::new(cid(key, "switch", id), if on { "ON" } else { "OFF" })
                .compact()
                .lit(on)
                .on_click(move |_, _, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.pick(&k, id, if on { 0.0 } else { 1.0 }, cx)
                    });
                });
            let _ = family;
            b.into_any_element()
        } else if p.labels.len() <= 4 {
            Segmented::new(cid(key, "tabs", id), p.labels.clone(), index)
                .on_select(move |i, _, cx| {
                    let _ = this.update(cx, |this, cx| this.pick(&k, id, i as f64, cx));
                })
                .into_any_element()
        } else {
            widgets::select_button(cid(key, "select", id), p.format(p.value), cx)
                .min_w(px(120.0))
                .on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let at = point(e.position.x - px(8.0), e.position.y + px(14.0));
                    let _ = this.update(cx, |this, cx| this.open_choices(&k, id, at, window, cx));
                })
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(5.0))
            .child(caps(p.name.clone(), cx))
            .child(control)
            .into_any_element()
    }

    /// A stock plugin's face.
    fn face(
        &self,
        key: &str,
        info: &Info,
        family: Option<Hsla>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let picture = display::picture(info.title(), info);
        let choices: Vec<_> = info.parameters.iter().filter(|p| p.is_choice()).collect();
        let dials: Vec<_> = info.parameters.iter().filter(|p| !p.is_choice()).collect();
        // At most six to a row, rows as even as they come (seven dials are four and three).
        let rows = dials.len().div_ceil(6).max(1);
        let columns = dials.len().div_ceil(rows).max(1) as u16;
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .p(px(14.0))
            .when_some(picture, |d, marks| d.child(self.display(marks, family, cx)))
            .when(!choices.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(18.0))
                        .children(choices.iter().map(|p| self.choice(key, p, family, cx))),
                )
            })
            .when(!dials.is_empty(), |d| {
                d.child(
                    div()
                        .grid()
                        .grid_cols(columns)
                        .gap_x(px(6.0))
                        .gap_y(px(16.0))
                        .px(px(10.0))
                        .pt(px(16.0))
                        .pb(px(12.0))
                        .rounded(px(radius::MD))
                        .bg(theme.hover)
                        .border_1()
                        .border_color(theme.hairline)
                        .children(dials.iter().map(|p| self.dial(key, p, family, cx))),
                )
            })
            .into_any_element()
    }

    /// One row of the generic parameter list.
    fn row(
        &self,
        key: &str,
        panel: &Panel,
        p: &Parameter,
        family: Option<Hsla>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let this = cx.entity().downgrade();
        let id = p.id;
        let (k1, k2, k3) = (key.to_string(), key.to_string(), key.to_string());
        let control = if p.is_choice() {
            let this = this.clone();
            widgets::select_button(cid(key, "select", id), p.shown(), cx)
                .flex_1()
                .on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let at = point(e.position.x - px(8.0), e.position.y + px(14.0));
                    let _ = this.update(cx, |this, cx| this.open_choices(&k1, id, at, window, cx));
                })
                .into_any_element()
        } else {
            let this = this.clone();
            let mut slider = Slider::new(cid(key, "slider", id), p.position(p.value) as f32)
                .default_value(p.position(p.default) as f32);
            if let Some(c) = family {
                slider = slider.color(c);
            }
            slider
                .on_change(move |v, phase, _, cx| {
                    let _ = this.update(cx, |this, cx| this.set_position(&k1, id, v, phase, cx));
                })
                .into_any_element()
        };
        let value = if p.is_choice() {
            // The select already says it.
            div().w(px(104.0)).into_any_element()
        } else if panel.editing == Some(id) {
            let focused = panel.value.read(cx).is_focused(window);
            field(&panel.value, focused, cx)
                .w(px(104.0))
                .into_any_element()
        } else {
            let this = this.clone();
            div()
                .id(cid(key, "value", id))
                .w(px(104.0))
                .h(px(24.0))
                .px(px(6.0))
                .flex()
                .items_center()
                .justify_end()
                .rounded(px(radius::XS))
                .font_family(FONT_MONO)
                .text_size(px(size::SM))
                .text_color(theme.text_2)
                .truncate()
                .cursor_text()
                .hover(|s| s.bg(theme.hover))
                .tooltip(|_, cx| widgets::tip("Click to type a value".into(), cx))
                .child(p.shown())
                .on_click(move |_, window, cx| {
                    let _ = this.update(cx, |this, cx| this.start_typing(&k2, id, window, cx));
                })
                .into_any_element()
        };
        div()
            .w_full()
            .h(px(ROW_H))
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(14.0))
            .child(
                div()
                    .w(px(150.0))
                    .flex_none()
                    .truncate()
                    .text_size(px(size::BASE))
                    .text_color(theme.text)
                    .child(p.name.clone()),
            )
            .child(control)
            .child(value)
            .child(
                div()
                    .id(cid(key, "automate", id))
                    .size(px(20.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius::XS))
                    .font_family(FONT_MONO)
                    .text_size(px(10.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(if p.lane.is_some() {
                        theme.accent_text
                    } else {
                        theme.text_3
                    })
                    .when(p.automatable, |d| {
                        d.cursor_pointer()
                            .hover(|s| s.bg(theme.hover).text_color(theme.accent_text))
                            .tooltip({
                                let name = p.name.clone();
                                move |_, cx| widgets::tip(format!("Automate {name}").into(), cx)
                            })
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, |this, cx| this.automate(&k3, id, cx));
                            })
                    })
                    .when(!p.automatable, |d| d.opacity(0.3))
                    .child("A"),
            )
            .into_any_element()
    }

    /// Any other plugin: a search and every parameter as a slider, a select or a typed value.
    fn list(
        &self,
        key: &str,
        panel: &Panel,
        info: &Info,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let filter = panel.filter.read(cx).text().to_string();
        let shown: Vec<u32> = params::matching(&info.parameters, &filter)
            .iter()
            .map(|p| p.id)
            .collect();
        let count = shown.len();
        let focused = panel.filter.read(cx).is_focused(window);
        let key_owned = key.to_string();
        let list = uniform_list(
            cid(key, "list", 0),
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                let Some(panel) = this.panels.get(&key_owned) else {
                    return vec![];
                };
                let Some(info) = panel.info.as_ref() else {
                    return vec![];
                };
                let family = panel
                    .folder
                    .as_ref()
                    .and_then(|f| f.1.as_deref())
                    .map(|folder| Theme::get(cx).family(folder));
                range
                    .filter_map(|i| shown.get(i))
                    .filter_map(|id| info.parameter(*id))
                    .map(|p| this.row(&key_owned, panel, p, family, window, cx))
                    .collect()
            }),
        )
        .w_full()
        .h(px((count.max(1) as f32 * ROW_H).min(ROW_H * 12.0)));
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(14.0))
                    .py(px(10.0))
                    .child(icon("search", 12.0, theme.text_3))
                    .child(field(&panel.filter, focused, cx).flex_1())
                    .child(
                        div()
                            .font_family(FONT_MONO)
                            .text_size(px(size::XS))
                            .text_color(theme.text_3)
                            .whitespace_nowrap()
                            .child(if filter.trim().is_empty() {
                                format!("{} parameters", info.parameters.len())
                            } else {
                                format!("{count} of {}", info.parameters.len())
                            }),
                    ),
            )
            .child(if count == 0 {
                div()
                    .px(px(14.0))
                    .py(px(12.0))
                    .text_color(theme.text_3)
                    .child(if info.parameters.is_empty() {
                        "This plugin shows no parameters; open its own window."
                    } else {
                        "No parameter matches."
                    })
                    .into_any_element()
            } else {
                list.into_any_element()
            })
            .child(div().h(px(8.0)))
            .into_any_element()
    }

    fn panel(&self, key: &str, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let panel = self.panels.get(key)?;
        let theme = Theme::get(cx).clone();
        let info = panel.info.as_ref();
        let family = panel
            .folder
            .as_ref()
            .and_then(|f| f.1.as_deref())
            .map(|folder| theme.family(folder));
        let stock = info.is_some_and(Info::stock);
        let raise_key = key.to_string();
        let body = match info {
            Some(info) if stock => self.face(key, info, family, cx),
            Some(info) => self.list(key, panel, info, window, cx),
            None => div().h(px(40.0)).into_any_element(),
        };
        Some(
            widgets::surface(2, cx)
                .id(cid(key, "panel", 0))
                .occlude()
                .absolute()
                .left(panel.origin.x)
                .top(panel.origin.y)
                .w(px(if stock || info.is_none() {
                    FACE_W
                } else {
                    LIST_W
                }))
                .flex()
                .flex_col()
                .overflow_hidden()
                .capture_any_mouse_down(cx.listener(move |this, _, _, cx| {
                    if this.order.last() != Some(&raise_key) {
                        this.raise(&raise_key);
                        cx.notify();
                    }
                }))
                .child(self.header(key, panel, info, family, cx))
                .when(panel.saving, |d| {
                    d.child(self.save_row(key, panel, window, cx))
                })
                .when_some(panel.error.clone(), |d, error| {
                    d.child(
                        div()
                            .px(px(14.0))
                            .pt(px(10.0))
                            .text_size(px(size::SM))
                            .text_color(theme.danger)
                            .child(error),
                    )
                })
                .child(body)
                .into_any_element(),
        )
    }
}

impl Render for PluginPanels {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(window, cx);
        let order = self.order.clone();
        let panels: Vec<AnyElement> = order
            .iter()
            .filter_map(|key| self.panel(key, window, cx))
            .collect();
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .children(panels)
            .children(self.menu.render(window, cx))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::app::Ryolune;
    use ryolune_engine::store;

    fn app() -> Ryolune {
        Ryolune::from_session(store::empty(), None)
    }

    /// The demo-free song starts with Drums (Drum Machine), Bass (Analog Bass) and Vocals.
    fn bass(app: &Ryolune) -> String {
        app.store
            .session()
            .tracks
            .iter()
            .find(|t| t.kind == "midi" && t.name == "Bass")
            .map(|t| t.id.clone())
            .expect("a Bass track")
    }

    /// Load stock effects into a strip's first slots. Straight into the store: the registry's
    /// `strip.setPlugin` notes a recent plugin in the settings file, which a test must not touch.
    pub(crate) fn put_inserts(app: &mut Ryolune, track: &str, names: &[&str]) {
        use ryolune_engine::{model::Insert, store::Command};
        let mut strip = app
            .store
            .session()
            .strips
            .get(track)
            .cloned()
            .unwrap_or_default();
        for (slot, name) in names.iter().enumerate() {
            let insert = Insert::new(
                format!("insert-test-{slot}"),
                &format!("stock:{name}"),
                name,
            );
            if slot < strip.inserts.len() {
                strip.inserts[slot] = insert;
            } else {
                strip.inserts.push(insert);
            }
        }
        app.store
            .dispatch(Command::SetStrip {
                track: track.into(),
                strip,
            })
            .unwrap();
    }

    // The window used to keep the slot it opened on: after Move up/down it re-read another
    // plugin's parameters.
    #[test]
    fn a_window_reads_the_slot_its_plugin_sits_in_now() {
        let mut app = app();
        let track = bass(&app);
        put_inserts(&mut app, &track, &["Chorus", "Channel EQ"]);
        let eq = app.store.session().strips[&track].inserts[1].id.clone();
        assert_eq!(
            locate(app.store.session(), &eq),
            Some((track.clone(), Some(1)))
        );
        app.run_control_command(
            "strip.moveInsert",
            &json!({ "trackId": track, "from": 1, "to": 0 }),
            false,
            "test",
        )
        .unwrap();
        assert_eq!(
            locate(app.store.session(), &eq),
            Some((track.clone(), Some(0)))
        );
        let info = read(&mut app, &eq).unwrap();
        assert_eq!(info.title(), "Channel EQ");
        assert!(info.stock() && info.parameters.iter().any(|p| p.name == "Mid Gain"));
        assert!(display::picture(info.title(), &info).is_some());
        assert!(locate(app.store.session(), "nothing").is_none());
    }

    #[test]
    fn an_untouched_stock_instrument_is_found_by_its_derived_key() {
        let mut app = app();
        let track = bass(&app);
        let strip = app
            .store
            .session()
            .strips
            .get(&track)
            .cloned()
            .unwrap_or_default();
        let key = strip.synth_key(&track);
        assert_eq!(
            locate(app.store.session(), &key),
            Some((track.clone(), None))
        );
        let info = read(&mut app, &key).unwrap();
        assert!(info.stock() && info.slot.is_none());
        // A parameter set from the window lands in the document as one undo step.
        let p = info
            .parameters
            .iter()
            .find(|p| !p.is_choice())
            .unwrap()
            .clone();
        let value = p.value_at(0.25);
        app.run_control_command(
            "strip.setParameter",
            &info.with(json!({ "parameterId": p.id, "value": value })),
            false,
            "test",
        )
        .unwrap();
        // The instrument now has an insert of its own, under the same key.
        assert_eq!(locate(app.store.session(), &key), Some((track, None)));
        let again = read(&mut app, &key).unwrap();
        assert!((again.parameter(p.id).unwrap().value - value).abs() < 1e-9);
    }

    /// The real window: the panel draws, its header drags it, and a dial's drag is one undo
    /// step through the registry.
    #[gpui::test]
    fn a_panel_moves_by_its_header_and_a_dial_drag_is_one_undo_step(cx: &mut gpui::TestAppContext) {
        use gpui::{Modifiers, MouseButton};
        cx.update(|cx| {
            cx.set_global(Theme::new(crate::ui::theme::Mode::Dark, true));
            crate::ui::actions::bind(cx);
        });
        let mut app = app();
        let track = bass(&app);
        put_inserts(&mut app, &track, &["Channel EQ"]);
        let key = app.store.session().strips[&track].inserts[0].id.clone();
        app.open_plugin_window(&key);
        let daw = cx.new(|_| Daw::new(app));
        let (view, cx) =
            cx.add_window_view(|window, cx| PluginPanels::new(daw.clone(), window, cx));
        let (origin, gain) = view.read_with(cx, |view, _| {
            let panel = &view.panels[&key];
            let info = panel.info.as_ref().expect("read on first draw");
            let gain = info
                .parameters
                .iter()
                .find(|p| p.name == "Mid Gain")
                .unwrap()
                .clone();
            (panel.origin, gain)
        });
        // Drag the header.
        let header = cx.debug_bounds("plugin-header").expect("header drawn");
        let from = point(header.left() + px(120.0), header.top() + px(16.0));
        let to = point(from.x + px(100.0), from.y + px(50.0));
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            point(from.x + px(10.0), from.y + px(5.0)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        let moved = view.read_with(cx, |view, _| view.panels[&key].origin);
        assert_eq!(moved, point(origin.x + px(100.0), origin.y + px(50.0)));
        // Turn Mid Gain up: several steps, one undo step.
        let depth = daw.read_with(cx, |daw, _| daw.app.store.undo_depth());
        let knob = cx
            .debug_bounds(Box::leak(format!("knob-{}", gain.id).into_boxed_str()))
            .expect("knob drawn");
        let c = knob.center();
        cx.simulate_mouse_down(c, MouseButton::Left, Modifiers::default());
        for dy in [8.0, 20.0, 40.0] {
            cx.simulate_mouse_move(
                point(c.x, c.y - px(dy)),
                MouseButton::Left,
                Modifiers::default(),
            );
        }
        cx.simulate_mouse_up(
            point(c.x, c.y - px(40.0)),
            MouseButton::Left,
            Modifiers::default(),
        );
        let (after, value) = daw.read_with(cx, |daw, _| {
            let s = daw.app.store.session();
            (
                daw.app.store.undo_depth(),
                s.strips[&track].inserts[0].params.get(&gain.id).copied(),
            )
        });
        assert_eq!(after, depth + 1, "one undo step for the whole drag");
        assert!(
            value.is_some_and(|v| v > gain.value),
            "{value:?} > {}",
            gain.value
        );
        // The panel shows the value the document holds.
        let shown = view.read_with(cx, |view, _| {
            view.panels[&key]
                .info
                .as_ref()
                .unwrap()
                .parameter(gain.id)
                .unwrap()
                .value
        });
        assert_eq!(Some(shown), value);
        // A typed value commits once, clamped, as one more step.
        view.update_in(cx, |view, window, cx| {
            view.start_typing(&key, gain.id, window, cx);
            view.commit_typed(&key, "100", cx);
            assert_eq!(view.panels[&key].editing, None);
        });
        let (last, typed) = daw.read_with(cx, |daw, _| {
            let s = daw.app.store.session();
            (
                daw.app.store.undo_depth(),
                s.strips[&track].inserts[0].params[&gain.id],
            )
        });
        assert_eq!((last, typed), (depth + 2, gain.max));
    }

    #[test]
    fn display_labels_stay_readable_on_the_display_in_both_modes() {
        use crate::ui::theme::Mode;
        fn lum(c: Hsla) -> f32 {
            let c: gpui::Rgba = c.into();
            let l = |v: f32| {
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * l(c.r) + 0.7152 * l(c.g) + 0.0722 * l(c.b)
        }
        for mode in [Mode::Dark, Mode::Light] {
            let t = Theme::new(mode, true);
            let (a, b) = (lum(t.display_ink), lum(t.display));
            let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
            assert!(ratio >= 3.0, "{mode:?}: {ratio:.2}");
        }
    }
}
