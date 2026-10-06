//! What the browser lists, built from the registry and nothing else: `plugin.list` and
//! `plugin.folders` for the instrument and effect tabs, `session.catalog` for the loops and
//! the session's audio sources for the files. Pure data, so the grouping, the search and
//! the rows the list shows are tested without a window.

use ryolune_engine::model::Session;
use serde_json::Value;
use std::collections::HashSet;

/// The four tabs, in the order they show. The id is what `view.set browserTab` takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Instruments,
    Loops,
    Plugins,
    Files,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Instruments, Tab::Loops, Tab::Plugins, Tab::Files];

    pub fn id(self) -> &'static str {
        match self {
            Tab::Instruments => "instruments",
            Tab::Loops => "loops",
            Tab::Plugins => "plugins",
            Tab::Files => "files",
        }
    }
    /// The document's tab; anything unknown is the first one.
    pub fn from_id(id: &str) -> Tab {
        Tab::ALL
            .into_iter()
            .find(|t| t.id() == id)
            .unwrap_or(Tab::Instruments)
    }
    pub fn label(self) -> &'static str {
        match self {
            Tab::Instruments => "Instr",
            Tab::Loops => "Loops",
            Tab::Plugins => "Plugins",
            Tab::Files => "Files",
        }
    }
    pub fn placeholder(self) -> &'static str {
        match self {
            Tab::Instruments => "Search instruments",
            Tab::Loops => "Search loops",
            Tab::Plugins => "Search plugins",
            Tab::Files => "Search files",
        }
    }
    /// What a double click (or Enter) does on this tab.
    pub fn hint(self) -> &'static str {
        match self {
            Tab::Instruments => "Double-click: load on the selected MIDI track, or add one",
            Tab::Loops => "Double-click: add a MIDI loop at the playhead",
            Tab::Plugins => "Double-click: insert on the selected track",
            Tab::Files => {
                "Double-click: place at the playhead · drop files on the window to import"
            }
        }
    }
    /// The tabs filed by sound folder, whose groups fold and whose rows take a star.
    pub fn plugins(self) -> bool {
        matches!(self, Tab::Instruments | Tab::Plugins)
    }
}

/// One row of `plugin.list`: a plugin with its formats and layouts folded in.
#[derive(Clone, Debug, PartialEq)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub vendor: String,
    /// stock, native, clap, vst3 or au.
    pub format: String,
    pub folder: String,
    pub favorite: bool,
    pub instrument: bool,
    pub effect: bool,
    /// The formats of the same plugin as (id, format), CLAP first; empty when only one.
    pub formats: Vec<(String, String)>,
}

impl Plugin {
    pub fn from_json(v: &Value) -> Option<Plugin> {
        let text = |key: &str| v[key].as_str().unwrap_or_default().to_string();
        Some(Plugin {
            id: v["id"].as_str()?.to_string(),
            name: text("name"),
            vendor: text("vendor"),
            format: text("format"),
            folder: text("folder"),
            favorite: v["favorite"].as_bool().unwrap_or(false),
            instrument: v["instrument"].as_bool().unwrap_or(false),
            effect: v["effect"].as_bool().unwrap_or(true),
            formats: v["formats"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|f| {
                            Some((
                                f["id"].as_str()?.to_string(),
                                f["format"].as_str()?.to_string(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
    }
    /// Format and vendor beside the name; ryolune's own stock plugins need neither.
    pub fn meta(&self) -> String {
        if self.format == "stock" {
            return String::new();
        }
        let format = match self.format.as_str() {
            "native" => "Rust",
            "clap" => "CLAP",
            "vst3" => "VST3",
            "au" => "AU",
            "lv2" => "LV2",
            "ladspa" => "LADSPA",
            other => other,
        };
        format!("{format} · {}", self.vendor)
    }
}

/// How the context menu names a format: "Load as VST3".
pub fn format_label(format: &str) -> &str {
    match format {
        "clap" => "CLAP",
        "vst3" => "VST3",
        "au" => "Audio Unit",
        "lv2" => "LV2",
        "ladspa" => "LADSPA",
        "native" => "ryolune plugin",
        other => other,
    }
}

/// One row of a group.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// What `view.set browserSelection` keeps for this row: the plugin or source id, or the
    /// loop's name.
    pub key: String,
    /// The plugin id or audio source id; loops have none.
    pub id: Option<String>,
    pub name: String,
    /// Format and vendor, bars, seconds.
    pub meta: String,
    /// The sound folder a plugin is filed under (search and "Move to" use it).
    pub folder: Option<String>,
    /// The family whose colour the swatch wears (`Theme::family`).
    pub family: String,
    /// Starred; `None` for rows that cannot be starred (loops, files).
    pub favorite: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupKind {
    Favourites,
    Recent,
    /// A sound folder: its header wears the family colour.
    Folder,
    /// The loops and files: a plain caps header.
    Plain,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub name: String,
    pub kind: GroupKind,
    pub items: Vec<Item>,
}

fn plugin_item(p: &Plugin) -> Item {
    Item {
        key: p.id.clone(),
        id: Some(p.id.clone()),
        name: p.name.clone(),
        meta: p.meta(),
        folder: Some(p.folder.clone()),
        family: p.folder.clone(),
        favorite: Some(p.favorite),
    }
}

/// The instrument or effect tab: Favourites, then the six most recent, then one group per
/// sound folder in `plugin.folders` order (folders it does not know last, by name).
pub fn plugin_groups(
    plugins: &[Plugin],
    order: &[String],
    recent: &[String],
    instruments: bool,
) -> Vec<Group> {
    let mine: Vec<&Plugin> = plugins
        .iter()
        .filter(|p| if instruments { p.instrument } else { p.effect })
        .collect();
    let mut folders: Vec<Group> = vec![];
    for p in &mine {
        match folders.iter_mut().find(|g| g.name == p.folder) {
            Some(group) => group.items.push(plugin_item(p)),
            None => folders.push(Group {
                name: p.folder.clone(),
                kind: GroupKind::Folder,
                items: vec![plugin_item(p)],
            }),
        }
    }
    let rank = |name: &str| order.iter().position(|o| o == name).unwrap_or(order.len());
    folders.sort_by(|a, b| rank(&a.name).cmp(&rank(&b.name)).then(a.name.cmp(&b.name)));
    let favourites: Vec<Item> = mine
        .iter()
        .filter(|p| p.favorite)
        .map(|p| plugin_item(p))
        .collect();
    let recents: Vec<Item> = recent
        .iter()
        .filter_map(|id| mine.iter().find(|p| p.id == *id))
        .take(6)
        .map(|p| plugin_item(p))
        .collect();
    let mut groups = vec![];
    if !favourites.is_empty() {
        groups.push(Group {
            name: "Favourites".into(),
            kind: GroupKind::Favourites,
            items: favourites,
        });
    }
    if !recents.is_empty() {
        groups.push(Group {
            name: "Recent".into(),
            kind: GroupKind::Recent,
            items: recents,
        });
    }
    groups.extend(folders);
    groups
}

/// The families the loop swatches take in turn.
const LOOP_FAMILIES: [&str; 8] = [
    "Drums",
    "Synths",
    "Pads",
    "Textures",
    "Keys",
    "Samplers",
    "Bass",
    "Distortion",
];

/// The bundled MIDI loops of `session.catalog`.
pub fn loop_groups(catalog: &Value) -> Vec<Group> {
    let items: Vec<Item> = catalog["loops"]
        .as_array()
        .map(|loops| {
            loops
                .iter()
                .filter_map(|l| l["name"].as_str().map(|name| (name, l["bars"].as_f64())))
                .enumerate()
                .map(|(i, (name, bars))| Item {
                    key: name.to_string(),
                    id: None,
                    name: name.to_string(),
                    meta: bars.map_or_else(String::new, |b| format!("{b} bars")),
                    folder: None,
                    family: LOOP_FAMILIES[i % LOOP_FAMILIES.len()].to_string(),
                    favorite: None,
                })
                .collect()
        })
        .unwrap_or_default();
    vec![Group {
        name: "ryolune".into(),
        kind: GroupKind::Plain,
        items,
    }]
}

/// The audio the session holds, by name.
pub fn file_groups(session: &Session) -> Vec<Group> {
    let mut sources: Vec<_> = session.sources.values().collect();
    sources.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    vec![Group {
        name: "Project audio".into(),
        kind: GroupKind::Plain,
        items: sources
            .into_iter()
            .map(|s| Item {
                key: s.id.clone(),
                id: Some(s.id.clone()),
                name: s.name.clone(),
                meta: format!("{:.1} s", s.duration_seconds),
                folder: None,
                family: "Samplers".into(),
                favorite: None,
            })
            .collect(),
    }]
}

/// Whether a row matches a search: its name, format and vendor, its folder, or the plain
/// words of its folder ("reverb" finds everything under Space & Time).
pub fn matches(item: &Item, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let folder = item.folder.as_deref().unwrap_or_default();
    item.name.to_lowercase().contains(&q)
        || item.meta.to_lowercase().contains(&q)
        || folder.to_lowercase().contains(&q)
        || ryolune_engine::control_plugins::folder_words(folder).contains(&q)
}

/// The groups with only the rows a search keeps; empty groups go.
pub fn filter(groups: &[Group], query: &str) -> Vec<Group> {
    groups
        .iter()
        .map(|g| Group {
            items: g
                .items
                .iter()
                .filter(|i| matches(i, query))
                .cloned()
                .collect(),
            ..g.clone()
        })
        .filter(|g| !g.items.is_empty())
        .collect()
}

/// Where the closed state of a folder is kept: per tab, so Synths can be open among the
/// instruments and Favourites closed among the effects.
pub fn folder_key(tab: Tab, name: &str) -> String {
    format!("{}/{name}", tab.id())
}

/// One line of the list. Every line has the same height, so the list is virtual.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// A group's header: a folding folder on the plugin tabs, a caps label elsewhere.
    Header {
        group: usize,
        open: bool,
    },
    Item {
        group: usize,
        item: usize,
    },
    /// Nothing matches the search.
    Empty,
    /// The tab's button at the end: Scan plugins, or Import audio.
    Action,
}

/// The lines of the list for these (already filtered) groups. A search opens every folder.
pub fn rows(groups: &[Group], tab: Tab, closed: &HashSet<String>, searching: bool) -> Vec<Row> {
    let mut rows = vec![];
    if groups.is_empty() {
        rows.push(Row::Empty);
    }
    for (g, group) in groups.iter().enumerate() {
        let open = !tab.plugins() || searching || !closed.contains(&folder_key(tab, &group.name));
        rows.push(Row::Header { group: g, open });
        if open {
            rows.extend((0..group.items.len()).map(|item| Row::Item { group: g, item }));
        }
    }
    if tab != Tab::Loops {
        rows.push(Row::Action);
    }
    rows
}

/// The folder names a plugin can be moved to: every sound folder of either plugin tab.
pub fn folder_names(instruments: &[Group], effects: &[Group]) -> Vec<String> {
    let mut names: Vec<String> = vec![];
    for g in instruments.iter().chain(effects) {
        if g.kind == GroupKind::Folder && !names.contains(&g.name) {
            names.push(g.name.clone());
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plugin(id: &str, name: &str, folder: &str, instrument: bool) -> Plugin {
        Plugin {
            id: id.into(),
            name: name.into(),
            vendor: "Acme".into(),
            format: if id.starts_with("stock:") {
                "stock"
            } else {
                "vst3"
            }
            .into(),
            folder: folder.into(),
            favorite: false,
            instrument,
            effect: !instrument,
            formats: vec![],
        }
    }

    #[test]
    fn plugin_groups_put_favourites_and_recents_first_then_folders_in_library_order() {
        let mut plugins = vec![
            plugin("stock:Grand Piano", "Grand Piano", "Keys", true),
            plugin("stock:ryolune Synth", "ryolune Synth", "Synths", true),
            plugin("vst3:diva", "Diva", "Synths", true),
            plugin("vst3:mine", "Mine", "My Folder", true),
            plugin("stock:Space", "Space", "Space & Time", false),
        ];
        plugins[2].favorite = true;
        let order = vec!["Synths".to_string(), "Keys".to_string()];
        let recent = vec!["stock:Space".to_string(), "stock:Grand Piano".to_string()];
        let groups = plugin_groups(&plugins, &order, &recent, true);
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            names,
            ["Favourites", "Recent", "Synths", "Keys", "My Folder"]
        );
        assert_eq!(groups[0].items[0].name, "Diva");
        // Recents only list plugins of the tab's kind.
        assert_eq!(groups[1].items.len(), 1);
        assert_eq!(groups[1].items[0].key, "stock:Grand Piano");
        assert_eq!(groups[2].items.len(), 2);
        assert_eq!(groups[2].kind, GroupKind::Folder);
        // Stock plugins carry no vendor line; others show format and vendor.
        assert_eq!(groups[3].items[0].meta, "");
        assert_eq!(groups[2].items[1].meta, "VST3 · Acme");
        let effects = plugin_groups(&plugins, &order, &recent, false);
        assert_eq!(effects.len(), 2, "Recent and Space & Time");
        assert_eq!(
            folder_names(&groups, &effects),
            ["Synths", "Keys", "My Folder", "Space & Time"]
        );
    }

    #[test]
    fn plugin_rows_parse_from_plugin_list() {
        let row = json!({"id": "clap:u-he.diva", "name": "Diva", "vendor": "u-he", "format": "clap",
            "folder": "Synths", "favorite": true, "instrument": true, "effect": false,
            "formats": [{"id": "clap:u-he.diva", "format": "clap"}, {"id": "vst3:abc", "format": "vst3"}]});
        let p = Plugin::from_json(&row).unwrap();
        assert!(p.favorite && p.instrument && !p.effect);
        assert_eq!(p.formats[1], ("vst3:abc".to_string(), "vst3".to_string()));
        assert_eq!(p.meta(), "CLAP · u-he");
        assert_eq!(format_label("au"), "Audio Unit");
        assert_eq!(format_label("ladspa"), "LADSPA");
        assert!(Plugin::from_json(&json!({"name": "no id"})).is_none());
    }

    #[test]
    fn search_matches_name_vendor_folder_and_folder_words() {
        let plugins = vec![
            plugin("stock:Space", "Space", "Space & Time", false),
            plugin("vst3:comp", "Glue", "Dynamics", false),
        ];
        let groups = plugin_groups(&plugins, &[], &[], false);
        let names = |q: &str| -> Vec<String> {
            filter(&groups, q)
                .iter()
                .flat_map(|g| g.items.iter().map(|i| i.name.clone()))
                .collect()
        };
        assert_eq!(names("reverb"), ["Space"], "folder words");
        assert_eq!(names("compressor"), ["Glue"]);
        assert_eq!(names("ACME"), ["Glue"], "vendor, any case");
        assert_eq!(names("dynam"), ["Glue"], "folder name");
        assert_eq!(names("  "), ["Glue", "Space"], "folders by name");
        assert!(filter(&groups, "zzz").is_empty());
    }

    #[test]
    fn rows_fold_closed_folders_unless_searching_and_end_with_the_tab_button() {
        let plugins = vec![
            plugin("stock:a", "A", "Synths", true),
            plugin("stock:b", "B", "Keys", true),
        ];
        let groups = plugin_groups(&plugins, &[], &[], true);
        let mut closed = HashSet::new();
        closed.insert(folder_key(Tab::Instruments, "Keys"));
        let r = rows(&groups, Tab::Instruments, &closed, false);
        assert_eq!(
            r,
            [
                Row::Header {
                    group: 0,
                    open: false
                },
                Row::Header {
                    group: 1,
                    open: true
                },
                Row::Item { group: 1, item: 0 },
                Row::Action,
            ]
        );
        // A search opens every folder; the same folder on another tab is its own.
        assert_eq!(rows(&groups, Tab::Instruments, &closed, true).len(), 5);
        assert_eq!(rows(&groups, Tab::Plugins, &closed, false).len(), 5);
        assert_eq!(rows(&[], Tab::Loops, &closed, false), [Row::Empty]);
    }

    #[test]
    fn loops_and_files_come_from_the_catalog_and_the_session() {
        let catalog = json!({"loops": [{"name": "Four on the floor", "bars": 2}, {"name": "Arp", "bars": 1}]});
        let loops = loop_groups(&catalog);
        assert_eq!(loops[0].items[0].meta, "2 bars");
        assert_eq!(loops[0].items[1].family, "Synths");
        assert_eq!(loops[0].items[1].key, "Arp");
        let session = ryolune_engine::store::empty();
        let files = file_groups(&session);
        assert_eq!(files[0].name, "Project audio");
        assert_eq!(files[0].items.len(), session.sources.len());
        assert_eq!(Tab::from_id("files"), Tab::Files);
        assert_eq!(Tab::from_id("nonsense"), Tab::Instruments);
    }
}
