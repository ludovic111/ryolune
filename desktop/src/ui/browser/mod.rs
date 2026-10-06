//! The browser at the left edge: instruments, loops, plugins and files, filed by sound
//! folder. Glass tier 1.
//!
//! Everything it shows comes from the registry (`plugin.list`, `plugin.folders`,
//! `session.catalog`, the session's sources) and everything it changes goes through it:
//! the tab and the selected row are `view.set`, a star is `plugin.setFavorite`, filing is
//! `plugin.setFolder`, and using a row (double click, Enter, or a drag onto the
//! arrangement) loads a plugin with `strip.setPlugin`, adds a loop with `clip.addLoop` or
//! places audio with `clip.create`. The plugin list is read once and again only when the
//! library changes (a scan, a star, a folder, a recent), never per frame.

pub mod groups;

use super::{
    actions::{self, Do},
    daw::Daw,
    theme::{layout, radius, size, with_alpha, Theme, FONT_MONO},
    widgets::{
        self, icon, text_input, Button, InputEvent, MenuHost, MenuItem, Segmented, TextInput,
    },
};
use gpui::{
    div, prelude::*, px, relative, uniform_list, AnyElement, App, ClickEvent, Context, Entity,
    FocusHandle, Focusable, KeyBinding, MouseButton, MouseDownEvent, Pixels, Point, ScrollStrategy,
    SharedString, Subscription, UniformListScrollHandle, Window,
};
use groups::{Group, GroupKind, Item, Plugin, Row, Tab};
use serde_json::{json, Value};
use std::collections::HashSet;

gpui::actions!(browser, [SelectNext, SelectPrevious, UseSelection]);

/// Every line of the list is this tall, so the list is virtual (`uniform_list`).
const ROW_H: f32 = 26.0;

/// A browser row being dragged: the payload of GPUI's `on_drag`. A view that accepts it
/// (`.on_drop(|drag: &BrowserDrag, ..|)`, the arrangement for one) calls
/// [`BrowserDrag::apply`] with the track and bar it was dropped on, which runs the same
/// registry commands as a double click in the browser.
#[derive(Clone, Debug, PartialEq)]
pub enum BrowserDrag {
    /// An instrument (a `plugin.list` id): loads on a MIDI track, or on a new one.
    Instrument { plugin_id: String, name: String },
    /// An effect: goes into the first free insert slot of the track.
    Effect { plugin_id: String, name: String },
    /// A bundled MIDI loop by its `session.catalog` name.
    Loop { name: String },
    /// One of the session's audio sources.
    Audio { source_id: String, name: String },
}

impl BrowserDrag {
    pub fn name(&self) -> &str {
        match self {
            BrowserDrag::Instrument { name, .. }
            | BrowserDrag::Effect { name, .. }
            | BrowserDrag::Loop { name }
            | BrowserDrag::Audio { name, .. } => name,
        }
    }

    /// Use the item on `track` (the selected track when `None`) at `bar` (the playhead,
    /// snapped, when `None`). A track of the wrong kind gets a new track beside it for
    /// instruments, loops and audio. Whatever it takes is one undo step.
    pub fn apply(
        &self,
        daw: &mut Daw,
        track: Option<&str>,
        bar: Option<f64>,
        cx: &mut Context<Daw>,
    ) {
        let s = daw.app.store.session();
        let target = track
            .map(str::to_string)
            .or_else(|| s.view.selected_track_id.clone());
        let of_kind = |kind: &str| {
            target
                .clone()
                .filter(|id| s.tracks.iter().any(|t| t.id == *id && t.kind == kind))
        };
        let start = bar.unwrap_or_else(|| actions::playhead_bar(daw, true));
        match self {
            BrowserDrag::Instrument { plugin_id, name } => {
                let midi = of_kind("midi");
                one_step(daw, cx, |daw, cx| {
                    let track = match midi {
                        Some(id) => id,
                        None => new_track(daw, "midi", name, cx)?,
                    };
                    daw.run(
                        "strip.setPlugin",
                        json!({"trackId": track, "pluginId": plugin_id}),
                        cx,
                    )
                });
            }
            BrowserDrag::Effect { plugin_id, name } => {
                let Some(track) = target.filter(|id| s.tracks.iter().any(|t| t.id == *id)) else {
                    daw.app.error = Some(format!("Select a track to insert {name} on."));
                    cx.notify();
                    return;
                };
                daw.run(
                    "strip.setPlugin",
                    json!({"trackId": track, "pluginId": plugin_id, "firstFreeSlot": true}),
                    cx,
                );
            }
            BrowserDrag::Loop { name } => {
                let mut params = json!({"name": name, "startBar": start});
                if let Some(track) = of_kind("midi") {
                    params["trackId"] = json!(track);
                }
                daw.run("clip.addLoop", params, cx);
            }
            BrowserDrag::Audio { source_id, name } => {
                let Some(seconds) = s.sources.get(source_id).map(|src| src.duration_seconds) else {
                    return;
                };
                // The bars the audio covers from there, whatever the tempo does on the way.
                let length = s.seconds_bars(start, seconds).max(0.25);
                let audio = of_kind("audio");
                one_step(daw, cx, |daw, cx| {
                    let track = match audio {
                        Some(id) => id,
                        None => new_track(daw, "audio", name, cx)?,
                    };
                    daw.run(
                        "clip.create",
                        json!({"trackId": track, "sourceId": source_id, "name": name,
                            "startBar": start, "lengthBars": length}),
                        cx,
                    )
                });
            }
        }
    }
}

/// Run several commands as one undo step.
fn one_step(
    daw: &mut Daw,
    cx: &mut Context<Daw>,
    f: impl FnOnce(&mut Daw, &mut Context<Daw>) -> Option<Value>,
) {
    daw.gesture(true);
    f(daw, cx);
    daw.gesture(false);
}

fn new_track(daw: &mut Daw, kind: &str, name: &str, cx: &mut Context<Daw>) -> Option<String> {
    daw.run("track.add", json!({"kind": kind, "name": name}), cx)?["id"]
        .as_str()
        .map(str::to_string)
}

/// What follows the pointer while a row is dragged: its swatch and name on glass.
struct DragChip {
    name: SharedString,
    family: String,
    /// Where the row was grabbed, so the chip sits just below-right of the pointer.
    grab: Point<Pixels>,
}

impl Render for DragChip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx);
        div()
            .pl(self.grab.x + px(10.0))
            .pt(self.grab.y + px(6.0))
            .child(
                widgets::surface(2, cx)
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(10.0))
                    .h(px(ROW_H))
                    .font_family(super::theme::FONT_UI)
                    .text_size(px(size::BASE))
                    .text_color(theme.text)
                    .child(swatch(theme.family(&self.family)))
                    .child(self.name.clone()),
            )
    }
}

fn swatch(color: gpui::Hsla) -> gpui::Div {
    div()
        .flex_none()
        .size(px(8.0))
        .rounded(px(radius::XS / 2.0))
        .bg(color)
}

/// Whether a row is the document's selection. Songs saved before rows were keyed by id
/// select by name, so the name counts too.
fn is_selected(item: &Item, selection: Option<&String>) -> bool {
    selection.is_some_and(|s| *s == item.key || *s == item.name)
}

/// What a context-menu line does to the browser.
type MenuPick = Box<dyn Fn(&mut Browser, &mut Window, &mut Context<Browser>)>;

/// The plugin being filed under a folder whose name is being typed in its row.
struct Naming {
    plugin_id: String,
    input: Entity<TextInput>,
    _events: Subscription,
}

pub struct Browser {
    daw: Entity<Daw>,
    focus: FocusHandle,
    search: Entity<TextInput>,
    /// The tab the search field was last set up for: changing tabs clears the search.
    tab: Tab,
    query: String,
    plugins: Vec<Plugin>,
    instruments: Vec<Group>,
    effects: Vec<Group>,
    loops: Vec<Group>,
    /// What the plugin groups were built from: the catalog size and the library settings
    /// (stars, folders, recents). A change, or the end of a scan, reads the list again.
    library: Option<(usize, String)>,
    scanning: bool,
    /// Folded folders, by `groups::folder_key`, for this run of the app.
    closed: HashSet<String>,
    naming: Option<Naming>,
    menu: MenuHost,
    scroll: UniformListScrollHandle,
    /// The groups and lines of the list as last drawn, for the virtual list and the keys.
    visible: Vec<Group>,
    rows: Vec<Row>,
    /// What `visible` was filtered from (tab, search, plugin list generation, document
    /// revision for the files), so a frame that changes none of them reuses it.
    visible_key: Option<(Tab, String, u64, u64)>,
    /// Bumped each time the plugin list is read again.
    generation: u64,
    _subscriptions: Vec<Subscription>,
}

impl Browser {
    pub fn new(daw: Entity<Daw>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let c = Some("Browser");
        cx.bind_keys([
            KeyBinding::new("down", SelectNext, c),
            KeyBinding::new("up", SelectPrevious, c),
            KeyBinding::new("enter", UseSelection, c),
        ]);
        let tab = Tab::from_id(&daw.read(cx).app.store.session().view.browser_tab);
        let search = cx.new(|cx| TextInput::new(cx).placeholder(tab.placeholder()));
        let search_events =
            cx.subscribe_in(
                &search,
                window,
                |this, input, event, window, cx| match event {
                    InputEvent::Changed => {
                        this.query = input.read(cx).text().to_string();
                        cx.notify();
                    }
                    InputEvent::Submit => this.use_selection(window, cx),
                    InputEvent::Cancel => {
                        input.update(cx, |i, cx| i.set_text("", cx));
                        this.query.clear();
                        window.focus(&this.focus);
                        cx.notify();
                    }
                    InputEvent::Blur => {}
                },
            );
        let library = cx.observe(&daw, |this: &mut Self, _, cx| this.sync_library(cx));
        let catalog = daw
            .update(cx, |daw, cx| daw.request("session.catalog", json!({}), cx))
            .unwrap_or(Value::Null);
        let mut browser = Self {
            daw,
            focus: cx.focus_handle(),
            search,
            tab,
            query: String::new(),
            plugins: vec![],
            instruments: vec![],
            effects: vec![],
            loops: groups::loop_groups(&catalog),
            library: None,
            scanning: false,
            closed: HashSet::new(),
            naming: None,
            menu: MenuHost::default(),
            scroll: UniformListScrollHandle::new(),
            visible: vec![],
            visible_key: None,
            generation: 0,
            rows: vec![],
            _subscriptions: vec![search_events, library],
        };
        browser.sync_library(cx);
        browser
    }

    /// Read the plugin list again when the library changed or a background job (a scan)
    /// just finished.
    fn sync_library(&mut self, cx: &mut Context<Self>) {
        let app = &self.daw.read(cx).app;
        let key = (
            app.catalog.len(),
            serde_json::to_string(&app.settings.plugins).unwrap_or_default(),
        );
        let scanning = app.scan_job.is_some() || app.control_job.is_some();
        let finished = self.scanning && !scanning;
        self.scanning = scanning;
        if finished || self.library.as_ref() != Some(&key) {
            self.library = Some(key);
            self.refresh_plugins(cx);
        }
    }

    /// Page through `plugin.list` and `plugin.folders` and rebuild both plugin tabs.
    fn refresh_plugins(&mut self, cx: &mut Context<Self>) {
        let (plugins, folders) = self.daw.update(cx, |daw, cx| {
            let mut plugins = vec![];
            let mut offset = 0;
            loop {
                let Ok(page) =
                    daw.request("plugin.list", json!({"offset": offset, "limit": 200}), cx)
                else {
                    break;
                };
                if let Some(rows) = page["plugins"].as_array() {
                    plugins.extend(rows.iter().filter_map(Plugin::from_json));
                }
                match page["nextOffset"].as_u64() {
                    Some(next) if next > offset => offset = next,
                    _ => break,
                }
            }
            let folders = daw
                .request("plugin.folders", json!({}), cx)
                .unwrap_or(Value::Null);
            (plugins, folders)
        });
        let names = |key: &str, field: Option<&str>| -> Vec<String> {
            folders[key]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|v| match field {
                            Some(f) => v[f].as_str(),
                            None => v.as_str(),
                        })
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        };
        let order = names("folders", Some("name"));
        let recent = names("recent", None);
        self.instruments = groups::plugin_groups(&plugins, &order, &recent, true);
        self.effects = groups::plugin_groups(&plugins, &order, &recent, false);
        self.plugins = plugins;
        self.generation += 1;
        cx.notify();
    }

    /// The unfiltered groups of a tab.
    fn groups(&self, tab: Tab, cx: &App) -> Vec<Group> {
        match tab {
            Tab::Instruments => self.instruments.clone(),
            Tab::Plugins => self.effects.clone(),
            Tab::Loops => self.loops.clone(),
            Tab::Files => groups::file_groups(self.daw.read(cx).app.store.session()),
        }
    }

    fn selection<'a>(&self, cx: &'a App) -> Option<&'a String> {
        self.daw
            .read(cx)
            .app
            .store
            .session()
            .view
            .browser_selection
            .as_ref()
    }

    fn select(&mut self, key: &str, cx: &mut Context<Self>) {
        self.daw.update(cx, |daw, cx| {
            daw.run("view.set", json!({"browserSelection": key}), cx);
        });
    }

    fn item(&self, row: Row) -> Option<&Item> {
        match row {
            Row::Item { group, item } => self.visible.get(group)?.items.get(item),
            _ => None,
        }
    }

    /// Move the selection to the next or previous row of the list as drawn.
    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let selection = self.selection(cx).cloned();
        let items: Vec<(usize, &Item)> = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, row)| self.item(*row).map(|item| (i, item)))
            .collect();
        if items.is_empty() {
            return;
        }
        let current = items
            .iter()
            .position(|(_, item)| is_selected(item, selection.as_ref()));
        let next = match (current, forward) {
            (None, true) => 0,
            (None, false) => items.len() - 1,
            (Some(i), true) => (i + 1).min(items.len() - 1),
            (Some(i), false) => i.saturating_sub(1),
        };
        let (row, key) = (items[next].0, items[next].1.key.clone());
        self.scroll.scroll_to_item(row, ScrollStrategy::Top);
        self.select(&key, cx);
    }

    /// Enter: use the selected row, or select the first one when none is.
    fn use_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selection(cx).cloned();
        let item = self
            .rows
            .iter()
            .filter_map(|row| self.item(*row))
            .find(|item| is_selected(item, selected.as_ref()))
            .cloned();
        match item {
            Some(item) => self.use_item(&item, None, window, cx),
            None => self.step(true, cx),
        }
    }

    /// What using a row of the current tab does, as a drag payload.
    fn payload(&self, item: &Item) -> Option<BrowserDrag> {
        let name = item.name.clone();
        Some(match self.tab {
            Tab::Instruments => BrowserDrag::Instrument {
                plugin_id: item.id.clone()?,
                name,
            },
            Tab::Plugins => BrowserDrag::Effect {
                plugin_id: item.id.clone()?,
                name,
            },
            Tab::Loops => BrowserDrag::Loop { name },
            Tab::Files => BrowserDrag::Audio {
                source_id: item.id.clone()?,
                name,
            },
        })
    }

    /// Double click or Enter: use the row on the selected track at the playhead. `format`
    /// loads another format of the same plugin (the context menu's "Load as VST3").
    fn use_item(
        &mut self,
        item: &Item,
        format: Option<&str>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut drag) = self.payload(item) else {
            return;
        };
        if let (
            Some(id),
            BrowserDrag::Instrument { plugin_id, .. } | BrowserDrag::Effect { plugin_id, .. },
        ) = (format, &mut drag)
        {
            *plugin_id = id.to_string();
        }
        self.daw
            .update(cx, |daw, cx| drag.apply(daw, None, None, cx));
    }

    fn set_favorite(&mut self, plugin_id: &str, favorite: bool, cx: &mut Context<Self>) {
        self.daw.update(cx, |daw, cx| {
            daw.run(
                "plugin.setFavorite",
                json!({"pluginId": plugin_id, "favorite": favorite}),
                cx,
            );
        });
        self.sync_library(cx);
    }

    /// File a plugin under a folder, or back under its automatic one (`None`).
    fn set_folder(&mut self, plugin_id: &str, folder: Option<&str>, cx: &mut Context<Self>) {
        let mut params = json!({ "pluginId": plugin_id });
        if let Some(folder) = folder {
            params["folder"] = json!(folder);
        }
        self.daw
            .update(cx, |daw, cx| daw.run("plugin.setFolder", params, cx));
        self.sync_library(cx);
    }

    fn toggle_folder(&mut self, name: &str, cx: &mut Context<Self>) {
        let key = groups::folder_key(self.tab, name);
        if !self.closed.remove(&key) {
            self.closed.insert(key);
        }
        cx.notify();
    }

    /// "Move to new folder…": type the folder's name in place of the plugin's.
    fn start_naming(
        &mut self,
        plugin_id: String,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| TextInput::new(cx).placeholder(format!("New folder for {name}")));
        let events = cx.subscribe_in(&input, window, |this, input, event, window, cx| {
            match event {
                InputEvent::Submit => {
                    let folder = input.read(cx).text().trim().to_string();
                    if let Some(naming) = this.naming.take() {
                        if !folder.is_empty() {
                            this.set_folder(&naming.plugin_id, Some(&folder), cx);
                        }
                    }
                    window.focus(&this.focus);
                }
                InputEvent::Cancel | InputEvent::Blur => this.naming = None,
                InputEvent::Changed => {}
            }
            cx.notify();
        });
        // The menu hands focus back where it was as it closes; take it after that.
        let target = input.clone();
        cx.defer_in(window, move |_, window, cx| target.read(cx).focus(window));
        self.naming = Some(Naming {
            plugin_id,
            input,
            _events: events,
        });
        cx.notify();
    }

    /// Right click on a plugin row: load it (in another format too), star it, file it.
    fn open_menu(
        &mut self,
        key: String,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.tab.plugins() {
            return;
        }
        let Some(plugin) = self.plugins.iter().find(|p| p.id == key).cloned() else {
            return;
        };
        let item = self
            .groups(self.tab, cx)
            .iter()
            .flat_map(|g| g.items.iter())
            .find(|i| i.key == key)
            .cloned();
        let Some(item) = item else {
            return;
        };
        let this = cx.entity();
        let instruments = self.tab == Tab::Instruments;
        let verb = if instruments { "Load" } else { "Insert" };
        let on = |f: MenuPick| {
            let this = this.clone();
            move |window: &mut Window, cx: &mut App| this.update(cx, |b, cx| f(b, window, cx))
        };
        let mut items = vec![];
        {
            let item = item.clone();
            items.push(MenuItem::new(
                if instruments {
                    "Load on track"
                } else {
                    "Insert on track"
                },
                on(Box::new(move |b, w, cx| b.use_item(&item, None, w, cx))),
            ));
        }
        for (id, format) in plugin.formats.iter().filter(|(id, _)| *id != plugin.id) {
            let (item, id) = (item.clone(), id.clone());
            items.push(MenuItem::new(
                format!("{verb} as {}", groups::format_label(format)),
                on(Box::new(move |b, w, cx| {
                    b.use_item(&item, Some(&id), w, cx)
                })),
            ));
        }
        items.push(MenuItem::Separator);
        {
            let (id, favorite) = (plugin.id.clone(), plugin.favorite);
            items.push(MenuItem::new(
                if favorite {
                    "Remove from Favourites"
                } else {
                    "Add to Favourites"
                },
                on(Box::new(move |b, _, cx| b.set_favorite(&id, !favorite, cx))),
            ));
        }
        items.push(MenuItem::Separator);
        let theme = Theme::get(cx).clone();
        for folder in groups::folder_names(&self.instruments, &self.effects) {
            if folder == plugin.folder {
                continue;
            }
            let id = plugin.id.clone();
            let color = theme.family(&folder);
            items.push(
                MenuItem::new(
                    format!("Move to {folder}"),
                    on(Box::new(move |b, _, cx| {
                        b.set_folder(&id, Some(&folder), cx)
                    })),
                )
                .swatch(color),
            );
        }
        {
            let (id, name) = (plugin.id.clone(), plugin.name.clone());
            items.push(MenuItem::new(
                "Move to new folder…",
                on(Box::new(move |b, w, cx| {
                    b.start_naming(id.clone(), &name, w, cx)
                })),
            ));
        }
        {
            let id = plugin.id.clone();
            items.push(MenuItem::new(
                "Return to automatic folder",
                on(Box::new(move |b, _, cx| b.set_folder(&id, None, cx))),
            ));
        }
        self.menu.open(items, position, window, cx);
    }

    /// Audition the selected MIDI track's instrument on middle C.
    fn preview(&mut self, cx: &mut Context<Self>) {
        self.daw.update(cx, |daw, cx| {
            let s = daw.app.store.session();
            let track = s
                .tracks
                .iter()
                .find(|t| Some(&t.id) == s.view.selected_track_id.as_ref() && t.kind == "midi")
                .map(|t| t.id.clone());
            match track {
                Some(id) => {
                    daw.run(
                        "note.preview",
                        json!({"trackId": id, "pitch": 60, "velocity": 100}),
                        cx,
                    );
                }
                None => {
                    daw.app.error = Some("Select an instrument track to preview".into());
                    cx.notify();
                }
            }
        });
    }

    /// The footer's name: the selected row, else the selected track's instrument.
    fn preview_name(&self, cx: &App) -> String {
        let s = self.daw.read(cx).app.store.session();
        if let Some(key) = &s.view.browser_selection {
            let item = self
                .visible
                .iter()
                .flat_map(|g| g.items.iter())
                .find(|i| is_selected(i, Some(key)));
            if let Some(item) = item {
                return item.name.clone();
            }
            if let Some(p) = self.plugins.iter().find(|p| p.id == *key) {
                return p.name.clone();
            }
        }
        s.view
            .selected_track_id
            .as_ref()
            .and_then(|id| s.tracks.iter().find(|t| t.id == *id && t.kind == "midi"))
            .and_then(|t| s.strips.get(&t.id))
            .map(|strip| strip.instrument_name())
            .unwrap_or_else(|| "—".into())
    }

    fn render_rows(
        &mut self,
        range: std::ops::Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = Theme::get(cx).clone();
        let tab = self.tab;
        let selection = self.selection(cx).cloned();
        let scanning = self.scanning;
        let rows: Vec<(usize, Row)> = range
            .filter_map(|ix| Some((ix, *self.rows.get(ix)?)))
            .collect();
        rows.into_iter()
            .map(|(ix, row)| match row {
                Row::Empty => div()
                    .w_full()
                    .h(px(ROW_H))
                    .flex()
                    .items_center()
                    .px(px(8.0))
                    .text_size(px(size::BASE))
                    .text_color(theme.text_3)
                    .child("No matches")
                    .into_any_element(),
                Row::Action => {
                    let button = if tab == Tab::Files {
                        Button::new("browser-import", "Import audio…").on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(Do { id: "importAudio" }), cx)
                        })
                    } else {
                        Button::new(
                            "browser-scan",
                            if scanning {
                                "Scanning…"
                            } else {
                                "Scan plugins"
                            },
                        )
                        .disabled(scanning)
                        .on_click(|_, window, cx| {
                            window.dispatch_action(
                                Box::new(Do {
                                    id: "rescanPlugins",
                                }),
                                cx,
                            )
                        })
                    };
                    div()
                        .w_full()
                        .h(px(ROW_H))
                        .flex()
                        .items_end()
                        .px(px(2.0))
                        .child(button.compact().full_width())
                        .into_any_element()
                }
                Row::Header { group, open } => {
                    let Some(g) = self.visible.get(group) else {
                        return div().h(px(ROW_H)).into_any_element();
                    };
                    if !tab.plugins() {
                        return div()
                            .w_full()
                            .h(px(ROW_H))
                            .flex()
                            .items_end()
                            .px(px(8.0))
                            .pb(px(4.0))
                            .child(widgets::caps(g.name.clone(), cx))
                            .into_any_element();
                    }
                    let name = g.name.clone();
                    div()
                        .id(("browser-folder", ix))
                        .w_full()
                        .h(px(ROW_H))
                        .flex()
                        .items_center()
                        .gap(px(7.0))
                        .pl(px(4.0))
                        .pr(px(8.0))
                        .rounded(px(radius::SM))
                        .text_size(px(size::XS))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(theme.text_2)
                        .hover(|s| s.text_color(theme.text))
                        .cursor_pointer()
                        .child(icon(
                            if open {
                                "chevron-down"
                            } else {
                                "chevron-right"
                            },
                            9.0,
                            theme.text_3,
                        ))
                        .when(g.kind == GroupKind::Folder, |d| {
                            d.child(
                                div()
                                    .flex_none()
                                    .w(px(3.0))
                                    .h(px(11.0))
                                    .bg(theme.family(&g.name)),
                            )
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(g.name.to_uppercase()),
                        )
                        .child(
                            div()
                                .font_family(FONT_MONO)
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(theme.text_3)
                                .child(g.items.len().to_string()),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, _| window.focus(&this.focus)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_folder(&name, cx)))
                        .into_any_element()
                }
                Row::Item { .. } => {
                    let Some(item) = self.item(row).cloned() else {
                        return div().h(px(ROW_H)).into_any_element();
                    };
                    self.render_item(ix, item, selection.as_ref(), &theme, window, cx)
                }
            })
            .collect()
    }

    fn render_item(
        &self,
        ix: usize,
        item: Item,
        selection: Option<&String>,
        theme: &Theme,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = is_selected(&item, selection);
        let naming = self
            .naming
            .as_ref()
            .filter(|n| Some(&n.plugin_id) == item.id.as_ref())
            .map(|n| n.input.clone());
        let drag = self.payload(&item);
        let star = item.favorite;
        // The selected row is inverted, paper on ink; its marks follow the paper.
        let paper = theme.text_on_accent;
        let fam_keys = if selected { paper } else { theme.text };
        let (click_item, menu_key) = (item.clone(), item.key.clone());
        let mut row = div()
            .id(("browser-row", ix))
            .group("browser-row")
            .w_full()
            .h(px(ROW_H))
            .flex()
            .items_center()
            .gap(px(9.0))
            .px(px(8.0))
            .rounded(px(radius::SM))
            .text_size(px(size::BASE))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(if selected { paper } else { theme.text })
            .when(selected, |d| d.bg(theme.accent_fill))
            .when(!selected, |d| d.hover(|s| s.bg(theme.hover)))
            .child(swatch(if selected {
                paper
            } else {
                theme.family(&item.family)
            }))
            .child(match naming {
                Some(input) => div()
                    .flex_1()
                    .min_w_0()
                    .px(px(4.0))
                    .rounded(px(radius::XS))
                    .bg(theme.well)
                    .border_1()
                    .border_color(theme.accent_ring)
                    .child(input)
                    .into_any_element(),
                None => div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(item.name.clone())
                    .into_any_element(),
            })
            // The name is what people look for: vendor and format give way to it.
            .child(
                div()
                    .flex_none()
                    .max_w(relative(0.42))
                    .truncate()
                    .font_family(FONT_MONO)
                    .text_size(px(size::XS))
                    .font_weight(gpui::FontWeight::NORMAL)
                    .text_color(if selected {
                        with_alpha(paper, 0.7)
                    } else {
                        theme.text_3
                    })
                    .child(item.meta.clone()),
            )
            .when_some(star.zip(item.id.clone()), |d, (favorite, id)| {
                d.child(
                    div()
                        .id(("browser-star", ix))
                        .flex_none()
                        .p(px(2.0))
                        .rounded(px(radius::XS))
                        .cursor_pointer()
                        .when(!favorite, |d| {
                            d.opacity(0.0)
                                .group_hover("browser-row", |s| s.opacity(1.0))
                        })
                        .child(icon(
                            if favorite { "star-filled" } else { "star" },
                            11.0,
                            if favorite {
                                fam_keys
                            } else if selected {
                                paper
                            } else {
                                theme.text_3
                            },
                        ))
                        .tooltip(move |_, cx| {
                            widgets::tip(
                                if favorite {
                                    "Remove from Favourites"
                                } else {
                                    "Add to Favourites"
                                }
                                .into(),
                                cx,
                            )
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.set_favorite(&id, !favorite, cx);
                        })),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, _| window.focus(&this.focus)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.open_menu(menu_key.clone(), e.position, window, cx);
                }),
            )
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                if e.click_count() >= 2 {
                    this.use_item(&click_item, None, window, cx);
                } else {
                    this.select(&click_item.key, cx);
                }
            }));
        if let Some(drag) = drag {
            let family = item.family.clone();
            row = row.on_drag(drag, move |drag: &BrowserDrag, grab, _, cx| {
                let (name, family) = (drag.name().to_string().into(), family.clone());
                cx.new(|_| DragChip { name, family, grab })
            });
        }
        // Rows filed under a folder sit indented under its header.
        div()
            .w_full()
            .when(self.tab.plugins(), |d| d.pl(px(12.0)))
            .child(row)
            .into_any_element()
    }
}

impl Focusable for Browser {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Browser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let tab = Tab::from_id(&self.daw.read(cx).app.store.session().view.browser_tab);
        if tab != self.tab {
            // A new tab starts with an empty search.
            self.tab = tab;
            self.query.clear();
            self.naming = None;
            self.search.update(cx, |input, cx| {
                input.set_text("", cx);
                input.set_placeholder(tab.placeholder(), cx);
            });
        }
        let searching = !self.query.trim().is_empty();
        let revision = if tab == Tab::Files {
            self.daw.read(cx).app.store.revision
        } else {
            0
        };
        let key = (tab, self.query.clone(), self.generation, revision);
        if self.visible_key.as_ref() != Some(&key) {
            self.visible = groups::filter(&self.groups(tab, cx), &self.query);
            self.visible_key = Some(key);
        }
        self.rows = groups::rows(&self.visible, tab, &self.closed, searching);
        let search_focused = self.search.read(cx).is_focused(window);
        let preview_name = self.preview_name(cx);
        let daw = self.daw.clone();
        let menu = self.menu.render(window, cx);

        div()
            .id("browser")
            .key_context("Browser")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.step(true, cx)))
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.step(false, cx)))
            .on_action(
                cx.listener(|this, _: &UseSelection, window, cx| this.use_selection(window, cx)),
            )
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.glass(1))
            .border_r_1()
            .border_color(theme.line)
            // Titled like every area: its name, what it lists in mono.
            .child(
                div()
                    .h(px(layout::TOOLBAR))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(12.0))
                    .border_b_1()
                    .border_color(theme.line)
                    .child(widgets::panel_title("Browser", cx))
                    .child(widgets::panel_info(
                        {
                            let n: usize = self.visible.iter().map(|g| g.items.len()).sum();
                            format!("{n} {}", if n == 1 { "item" } else { "items" })
                        },
                        cx,
                    )),
            )
            .child(
                div().flex().px(px(10.0)).pt(px(10.0)).pb(px(8.0)).child(
                    Segmented::new(
                        "browser-tabs",
                        Tab::ALL.map(Tab::label),
                        Tab::ALL.iter().position(|t| *t == tab).unwrap_or(0),
                    )
                    .full_width()
                    .on_select(move |i, _, cx| {
                        daw.update(cx, |daw, cx| {
                            daw.run("view.set", json!({"browserTab": Tab::ALL[i].id()}), cx);
                        })
                    }),
                ),
            )
            .child(
                div()
                    .mx(px(10.0))
                    .mb(px(10.0))
                    .h(px(26.0))
                    .flex()
                    .items_center()
                    .gap(px(7.0))
                    .px(px(9.0))
                    .rounded(px(radius::SM))
                    .bg(theme.well)
                    .border_1()
                    .border_color(if search_focused {
                        theme.accent_ring
                    } else {
                        theme.hairline
                    })
                    .text_size(px(size::BASE))
                    .text_color(theme.text)
                    // Arrow keys in the search walk the list; Enter uses the selected row.
                    .capture_action(cx.listener(|this, _: &text_input::Down, _, cx| {
                        cx.stop_propagation();
                        this.step(true, cx);
                    }))
                    .capture_action(cx.listener(|this, _: &text_input::Up, _, cx| {
                        cx.stop_propagation();
                        this.step(false, cx);
                    }))
                    .child(icon("search", 11.0, theme.text_3))
                    .child(div().flex_1().min_w_0().child(self.search.clone())),
            )
            .child(
                div()
                    .id("browser-list")
                    .flex_1()
                    .min_h_0()
                    .px(px(6.0))
                    .tooltip(move |_, cx| widgets::tip(tab.hint().into(), cx))
                    .child(
                        uniform_list(
                            "browser-rows",
                            self.rows.len(),
                            cx.processor(|this, range, window, cx| {
                                this.render_rows(range, window, cx)
                            }),
                        )
                        .track_scroll(self.scroll.clone())
                        .size_full(),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.0))
                    .p(px(10.0))
                    .border_t_1()
                    .border_color(theme.line)
                    .text_size(px(size::SM))
                    .text_color(theme.text_2)
                    .child(
                        Button::icon("browser-preview", "play")
                            .compact()
                            .icon_size(9.0)
                            .tooltip("Audition the selected track's instrument")
                            .on_click(cx.listener(|this, _, _, cx| this.preview(cx))),
                    )
                    .child("Preview")
                    .child(
                        div()
                            .ml_auto()
                            .min_w_0()
                            .truncate()
                            .font_family(FONT_MONO)
                            .text_size(px(size::XS))
                            .text_color(theme.text_3)
                            .child(preview_name),
                    ),
            )
            .children(menu)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{app::Ryolune, ui::theme::Mode};
    use gpui::TestAppContext;
    use ryolune_engine::store;

    /// A window host on the demo song for GPUI tests, with a scratch profile: loading a
    /// plugin notes it under Recent, which saves the settings, and a test must never write
    /// over the person's own settings or read their plugin cache.
    pub(crate) fn test_daw(cx: &mut TestAppContext) -> Entity<Daw> {
        static PROFILE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
        PROFILE.get_or_init(|| {
            let dir = tempfile::tempdir().expect("scratch profile").keep();
            std::env::set_var("RYOLUNE_DATA_DIR", dir.join("data"));
            std::env::set_var("RYOLUNE_SETTINGS", dir.join("settings.json"));
            dir
        });
        cx.update(|cx| {
            cx.set_global(Theme::new(Mode::Dark, true));
            actions::bind(cx);
        });
        cx.new(|_| Daw::new(Ryolune::from_session(store::demo(), None)))
    }

    fn daw(cx: &mut TestAppContext) -> Entity<Daw> {
        test_daw(cx)
    }

    fn track_of_kind(daw: &Daw, kind: &str) -> Option<String> {
        let s = daw.app.store.session();
        s.tracks
            .iter()
            .find(|t| t.kind == kind)
            .map(|t| t.id.clone())
    }

    #[gpui::test]
    fn using_a_row_runs_registry_commands_in_one_undo_step(cx: &mut TestAppContext) {
        let daw = daw(cx);
        daw.update(cx, |daw, cx| {
            let audio = track_of_kind(daw, "audio")
                .or_else(|| new_track(daw, "audio", "Audio", cx))
                .unwrap();
            // An instrument dropped on an audio track gets a MIDI track of its own.
            let (tracks, undo) = (
                daw.app.store.session().tracks.len(),
                daw.app.store.undo_depth(),
            );
            BrowserDrag::Instrument {
                plugin_id: "stock:Glass Keys".into(),
                name: "Glass Keys".into(),
            }
            .apply(daw, Some(&audio), None, cx);
            assert_eq!(daw.app.error, None);
            let s = daw.app.store.session();
            assert_eq!(s.tracks.len(), tracks + 1);
            assert_eq!(daw.app.store.undo_depth(), undo + 1, "one undo step");
            let piano = s.tracks.last().unwrap().id.clone();
            assert_eq!(s.strips[&piano].instrument_name(), "Glass Keys");

            // A loop lands on the MIDI track at the bar it was dropped on.
            let catalog = daw.request("session.catalog", json!({}), cx).unwrap();
            let name = catalog["loops"][0]["name"].as_str().unwrap().to_string();
            BrowserDrag::Loop { name }.apply(daw, Some(&piano), Some(8.0), cx);
            let s = daw.app.store.session();
            assert!(s
                .clips
                .iter()
                .any(|c| c.track_id == piano && (c.start_bar - 8.0).abs() < 1e-9));

            // An effect goes into the first free insert slot.
            let inserts = |daw: &Daw| {
                daw.app.store.session().strips.get(&piano).map_or(0, |s| {
                    s.inserts.iter().filter(|i| i.state != "empty").count()
                })
            };
            let before = inserts(daw);
            BrowserDrag::Effect {
                plugin_id: "stock:Space".into(),
                name: "Space".into(),
            }
            .apply(daw, Some(&piano), None, cx);
            assert_eq!(inserts(daw), before + 1);

            // Audio dropped on a MIDI track gets an audio track, the clip covering the source.
            let source = daw.app.store.session().sources.values().next().cloned();
            if let Some(source) = source {
                let (tracks, undo) = (
                    daw.app.store.session().tracks.len(),
                    daw.app.store.undo_depth(),
                );
                BrowserDrag::Audio {
                    source_id: source.id.clone(),
                    name: source.name.clone(),
                }
                .apply(daw, Some(&piano), Some(2.0), cx);
                let s = daw.app.store.session();
                assert_eq!(s.tracks.len(), tracks + 1);
                assert_eq!(daw.app.store.undo_depth(), undo + 1);
                let clip = s.clips.last().unwrap();
                let bars = s.seconds_bars(2.0, source.duration_seconds).max(0.25);
                assert!((clip.length_bars - bars).abs() < 1e-6);
            }
        });
    }

    #[gpui::test]
    fn arrows_select_rows_and_enter_loads_on_the_selected_track(cx: &mut TestAppContext) {
        let daw = daw(cx);
        let (browser, cx) = cx.add_window_view(|window, cx| Browser::new(daw.clone(), window, cx));
        cx.run_until_parked();
        let midi = daw.read_with(cx, |daw, _| {
            daw.app
                .store
                .session()
                .view
                .selected_track_id
                .clone()
                .filter(|id| {
                    daw.app
                        .store
                        .session()
                        .tracks
                        .iter()
                        .any(|t| t.id == *id && t.kind == "midi")
                })
        });
        let Some(midi) = midi else {
            return;
        };
        cx.update(|window, cx| {
            daw.update(cx, |daw, cx| {
                daw.run("view.set", json!({"browserSelection": ""}), cx);
            });
            window.focus(&browser.read(cx).focus);
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("down");
        let first = browser.read_with(cx, |b, _| {
            b.rows.iter().find_map(|r| b.item(*r)).cloned().unwrap()
        });
        let selection = daw.read_with(cx, |d, _| {
            d.app.store.session().view.browser_selection.clone()
        });
        assert_eq!(selection.as_ref(), Some(&first.key));
        cx.simulate_keystrokes("enter");
        let instrument = daw.read_with(cx, |d, _| {
            d.app.store.session().strips[&midi].instrument_name()
        });
        assert_eq!(instrument, first.name);
    }

    #[gpui::test]
    fn a_tab_change_clears_the_search_and_filters_by_folder_words(cx: &mut TestAppContext) {
        let daw = daw(cx);
        let (browser, cx) = cx.add_window_view(|window, cx| Browser::new(daw.clone(), window, cx));
        cx.update(|window, cx| {
            daw.update(cx, |daw, cx| {
                daw.run("view.set", json!({"browserTab": "plugins"}), cx);
            });
            browser.read(cx).search.read(cx).focus(window);
        });
        cx.run_until_parked();
        cx.simulate_input("reverb");
        cx.run_until_parked();
        let names = browser.read_with(cx, |b, _| {
            b.visible
                .iter()
                .flat_map(|g| g.items.iter().map(|i| i.name.clone()))
                .collect::<Vec<_>>()
        });
        assert!(names.iter().any(|n| n == "Space"), "{names:?}");
        cx.update(|_, cx| {
            daw.update(cx, |daw, cx| {
                daw.run("view.set", json!({"browserTab": "loops"}), cx);
            })
        });
        cx.run_until_parked();
        let (query, text) = browser.read_with(cx, |b, cx| {
            (b.query.clone(), b.search.read(cx).text().to_string())
        });
        assert!(query.is_empty() && text.is_empty());
    }
}
