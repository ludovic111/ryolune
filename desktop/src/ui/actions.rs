//! Named window actions: the one table behind the menus, the keyboard shortcuts, the command
//! palette and the shortcut sheet. Each one resolves "the selection" or "the playhead" and
//! runs registry commands, so the CLI, MCP and the built-in agent can do the same
//! (`docs/agent-parity.json` maps every id to its commands; `engine/tests/agent_parity.rs`
//! checks it).

use super::daw::Daw;
use gpui::{App, Context, KeyBinding, Menu, MenuItem, SharedString};
use ryolune_engine::model::{ClipData, Session};
use serde_json::{json, Value};

/// The one GPUI action: run the table entry `id`.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = ryolune, no_json)]
pub struct Do {
    pub id: &'static str,
}

/// Where a shortcut applies: everywhere, or only while no text field has the keyboard.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Global,
    NotTyping,
}

pub struct ActionDef {
    pub id: &'static str,
    pub label: &'static str,
    /// GPUI keystrokes; `secondary` is Cmd on macOS and Ctrl elsewhere.
    pub keys: &'static [&'static str],
    pub scope: Scope,
}

const fn a(id: &'static str, label: &'static str, keys: &'static [&'static str]) -> ActionDef {
    ActionDef {
        id,
        label,
        keys,
        scope: if keys.is_empty() {
            Scope::Global
        } else {
            Scope::NotTyping
        },
    }
}
/// A shortcut with a modifier: it works while typing too.
const fn g(id: &'static str, label: &'static str, keys: &'static [&'static str]) -> ActionDef {
    ActionDef {
        id,
        label,
        keys,
        scope: Scope::Global,
    }
}

pub const ACTIONS: &[ActionDef] = &[
    // History and transport.
    g("undo", "Undo", &["secondary-z"]),
    g("redo", "Redo", &["secondary-shift-z"]),
    a("togglePlay", "Play / Stop", &["space"]),
    a("stop", "Stop", &["0"]),
    a("record", "Record", &["r"]),
    a("cycle", "Cycle", &["c"]),
    a("returnToStart", "Go to Beginning", &["enter"]),
    a("rewind", "Rewind One Bar", &[","]),
    a("forward", "Forward One Bar", &["."]),
    a("metronome", "Metronome Click", &["k"]),
    // Editing.
    a("deleteSelection", "Delete", &["backspace", "delete"]),
    g("duplicateClip", "Duplicate Clip", &["secondary-d"]),
    g(
        "splitAtPlayhead",
        "Split Clip at Playhead",
        &["secondary-t"],
    ),
    a("openInEditor", "Open in Editor", &["e"]),
    // Text fields bind these keys too, and win while they have focus.
    a("copy", "Copy", &["secondary-c"]),
    a("cut", "Cut", &["secondary-x"]),
    a("paste", "Paste at Playhead", &["secondary-v"]),
    a("quantizeRegion", "Quantize Region Notes", &[]),
    g("transposeUp", "Transpose Up a Semitone", &["alt-up"]),
    g("transposeDown", "Transpose Down a Semitone", &["alt-down"]),
    g(
        "transposeOctaveUp",
        "Transpose Up an Octave",
        &["alt-shift-up"],
    ),
    g(
        "transposeOctaveDown",
        "Transpose Down an Octave",
        &["alt-shift-down"],
    ),
    // Tracks.
    g("addAudioTrack", "New Audio Track", &["secondary-alt-a"]),
    g("addMidiTrack", "New MIDI Track", &["secondary-alt-s"]),
    a("addBusTrack", "New Bus", &[]),
    a("groupSelectedTrack", "Route Track to a New Bus", &[]),
    g("duplicateTrack", "Duplicate Track", &["secondary-shift-d"]),
    g(
        "removeSelectedTrack",
        "Delete Track",
        &["secondary-backspace"],
    ),
    a("muteSelectedTrack", "Mute Track", &["m"]),
    a("soloSelectedTrack", "Solo Track", &["s"]),
    a("armSelectedTrack", "Record-Arm Track", &["a"]),
    a(
        "cycleMonitorSelectedTrack",
        "Input Monitoring: Off / Auto / On",
        &["i"],
    ),
    // View.
    g("zoomIn", "Zoom In", &["secondary-="]),
    g("zoomOut", "Zoom Out", &["secondary--"]),
    a("zoomToFit", "Zoom to Fit Session", &["z"]),
    a("followPlayhead", "Follow Playhead", &["f"]),
    a("toggleMixer", "Mixer", &["x"]),
    a("toggleControllerLane", "Controller Lane", &["l"]),
    a("toggleTempoTrack", "Tempo Track", &["shift-t"]),
    a("toggleAutomation", "Automation", &[]),
    g("commandPalette", "Command Palette…", &["secondary-p"]),
    g("showShortcuts", "Shortcuts and Help", &["secondary-/"]),
    a("editorPianoRoll", "Piano Roll", &[]),
    a("editorScore", "Score", &[]),
    a("editorStep", "Step", &[]),
    a("toolPointer", "Pointer Tool", &["1"]),
    a("toolPencil", "Pencil Tool", &["2"]),
    a("toolScissors", "Scissors Tool", &["3"]),
    // Markers.
    a("addMarker", "Add Marker at Playhead", &["shift-m"]),
    a("previousMarker", "Go to Previous Marker", &["shift-b"]),
    a("nextMarker", "Go to Next Marker", &["shift-n"]),
    a("cycleSection", "Cycle Section at Playhead", &["shift-c"]),
    // Agent.
    g("toggleAgentPanel", "Agent Panel", &["secondary-j"]),
    g(
        "askAgent",
        "Ask Agent About Selection…",
        &["secondary-shift-j"],
    ),
    a("stopAgent", "Stop Current Agent Action", &[]),
    g("musicalTyping", "Musical Typing", &["secondary-k"]),
    // File.
    g("newSession", "New Session", &["secondary-n"]),
    g("openSession", "Open…", &["secondary-o"]),
    a("openDemo", "Open Demo", &[]),
    g("save", "Save", &["secondary-s"]),
    g("saveAs", "Save As…", &["secondary-shift-s"]),
    g("importAudio", "Import Audio…", &["secondary-i"]),
    a("importMidi", "Import MIDI…", &[]),
    g("exportAudio", "Export Audio…", &["secondary-b"]),
    a("exportMidi", "Export MIDI…", &[]),
    a("recoverSession", "Recover Session…", &[]),
    g("settings", "Settings…", &["secondary-,"]),
    g("quit", "Quit ryolune", &["secondary-q"]),
    // Mix.
    a("saveRecoveredTake", "Save Recovered Take…", &[]),
    a("reconnectOutput", "Reconnect Output", &[]),
    a("audioSettings", "Audio and MIDI Devices…", &[]),
    a("rescanPlugins", "Rescan Plugins", &[]),
    a("showMaster", "Show Master Strip", &[]),
    a("showBusA", "Show Reverb Bus (A)", &[]),
    a("showBusB", "Show Delay Bus (B)", &[]),
    // Edit tools on the selected region.
    a("humanize", "Humanize Timing and Velocity", &[]),
    a("crescendo", "Velocity Crescendo", &[]),
    a("diminuendo", "Velocity Diminuendo", &[]),
    a("legato", "Legato Notes", &[]),
    a("reverseMidi", "Reverse MIDI Phrase", &[]),
    a("fitMajor", "Fit to C Major", &[]),
    a("fitMinor", "Fit to C Minor", &[]),
    a("repeatRegion", "Repeat Region × 4", &[]),
    // Help.
    a("agentSettings", "Agent Settings…", &[]),
    a("checkUpdates", "Check for Updates…", &[]),
    a("pluginGuide", "Native Plugin SDK…", &[]),
    a("support", "Support ryolune…", &[]),
    // Other apps, recent songs and the first-run setup.
    a("openRecent", "Open Recent…", &[]),
    a("importFromApp", "Import from Another App…", &[]),
    a("exportForApp", "Export for Another App…", &[]),
    a("firstRunSetup", "Set Up ryolune…", &[]),
];

pub fn def(id: &str) -> Option<&'static ActionDef> {
    ACTIONS.iter().find(|d| d.id == id)
}

/// The window's menus, in the title bar and in the macOS menu bar. `None` is a separator.
pub const MENUS: &[(&str, &[Option<&str>])] = &[
    (
        "File",
        &[
            Some("newSession"),
            Some("openSession"),
            Some("openRecent"),
            Some("openDemo"),
            None,
            Some("save"),
            Some("saveAs"),
            Some("importAudio"),
            Some("importMidi"),
            Some("exportAudio"),
            Some("exportMidi"),
            Some("importFromApp"),
            Some("exportForApp"),
            None,
            Some("recoverSession"),
            None,
            Some("settings"),
            None,
            Some("quit"),
        ],
    ),
    (
        "Edit",
        &[
            Some("undo"),
            Some("redo"),
            None,
            Some("cut"),
            Some("copy"),
            Some("paste"),
            Some("duplicateClip"),
            Some("splitAtPlayhead"),
            Some("deleteSelection"),
            None,
            Some("quantizeRegion"),
            Some("humanize"),
            Some("crescendo"),
            Some("diminuendo"),
            Some("legato"),
            Some("reverseMidi"),
            Some("fitMajor"),
            Some("fitMinor"),
            Some("repeatRegion"),
            None,
            Some("transposeUp"),
            Some("transposeDown"),
            Some("transposeOctaveUp"),
            Some("transposeOctaveDown"),
        ],
    ),
    (
        "Track",
        &[
            Some("addMidiTrack"),
            Some("addAudioTrack"),
            Some("addBusTrack"),
            Some("groupSelectedTrack"),
            Some("duplicateTrack"),
            Some("removeSelectedTrack"),
            None,
            Some("muteSelectedTrack"),
            Some("soloSelectedTrack"),
            Some("armSelectedTrack"),
            Some("cycleMonitorSelectedTrack"),
            None,
            Some("showMaster"),
            Some("showBusA"),
            Some("showBusB"),
        ],
    ),
    (
        "Mix",
        &[
            Some("saveRecoveredTake"),
            Some("reconnectOutput"),
            Some("audioSettings"),
            Some("musicalTyping"),
            None,
            Some("rescanPlugins"),
        ],
    ),
    (
        "Agent",
        &[
            Some("toggleAgentPanel"),
            Some("askAgent"),
            Some("stopAgent"),
            Some("agentSettings"),
        ],
    ),
    (
        "View",
        &[
            Some("commandPalette"),
            Some("toggleMixer"),
            Some("toggleControllerLane"),
            Some("toggleTempoTrack"),
            Some("toggleAutomation"),
            None,
            Some("followPlayhead"),
            Some("zoomToFit"),
            Some("zoomIn"),
            Some("zoomOut"),
            None,
            Some("addMarker"),
            Some("previousMarker"),
            Some("nextMarker"),
            Some("cycleSection"),
        ],
    ),
    (
        "Help",
        &[
            Some("showShortcuts"),
            Some("checkUpdates"),
            Some("pluginGuide"),
            Some("firstRunSetup"),
            None,
            Some("support"),
        ],
    ),
];

/// The shortcut text a menu or the palette shows: ⌘⇧Z on macOS, Ctrl+Shift+Z elsewhere.
pub fn shortcut_label(id: &str) -> Option<String> {
    label_for(def(id)?.keys.first()?, cfg!(target_os = "macos"))
}

/// One keystroke as text, in macOS symbols or spelled out for Windows and Linux.
pub fn label_for(keys: &str, mac: bool) -> Option<String> {
    let mut out = String::new();
    let parts: Vec<&str> = keys.split('-').collect();
    // "secondary--" is secondary + "-".
    let (mods, key) = if keys.ends_with("--") {
        (&parts[..parts.len() - 2], "-")
    } else {
        (&parts[..parts.len() - 1], *parts.last()?)
    };
    // macOS writes modifiers in the order ⌃⌥⇧⌘.
    let rank = |m: &&str| match *m {
        "ctrl" => 0,
        "alt" => 1,
        "shift" => 2,
        _ => 3,
    };
    let mut mods = mods.to_vec();
    if mac {
        mods.sort_by_key(rank);
    }
    for m in mods {
        out.push_str(match (m, mac) {
            ("secondary" | "cmd", true) => "⌘",
            ("secondary" | "ctrl", false) => "Ctrl+",
            ("ctrl", true) => "⌃",
            ("shift", true) => "⇧",
            ("shift", false) => "Shift+",
            ("alt", true) => "⌥",
            ("alt", false) => "Alt+",
            _ => "",
        });
    }
    out.push_str(&match key {
        "space" => "Space".to_string(),
        "enter" => if mac { "↩" } else { "Enter" }.to_string(),
        "backspace" => if mac { "⌫" } else { "Backspace" }.to_string(),
        "delete" => "Del".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        other => other.to_uppercase(),
    });
    Some(out)
}

pub fn bind(cx: &mut App) {
    super::widgets::bind(cx);
    let mut bindings = vec![];
    for def in ACTIONS {
        let context = match def.scope {
            Scope::Global => None,
            Scope::NotTyping => Some("!TextInput"),
        };
        for keys in def.keys {
            bindings.push(KeyBinding::new(keys, Do { id: def.id }, context));
        }
    }
    cx.bind_keys(bindings);
}

/// The macOS menu bar carries the same menus as the title bar, so the system's shortcuts,
/// Services and window menu work as on any Mac app.
pub fn set_menus(cx: &mut App) {
    let item = |id: &'static str| {
        let label = def(id).map_or(id, |d| d.label);
        MenuItem::action(label, Do { id })
    };
    let mut menus = vec![Menu {
        name: "ryolune".into(),
        items: vec![
            MenuItem::action("About ryolune", Do { id: "pluginGuide" }),
            MenuItem::separator(),
            item("settings"),
            item("checkUpdates"),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", gpui::SystemMenuType::Services),
            MenuItem::separator(),
            item("quit"),
        ],
    }];
    for (title, entries) in MENUS {
        menus.push(Menu {
            name: SharedString::from(*title),
            items: entries
                .iter()
                .filter(|e| !matches!(e, Some("settings" | "quit")))
                .map(|e| match e {
                    Some(id) => item(id),
                    None => MenuItem::separator(),
                })
                .collect(),
        });
    }
    cx.set_menus(menus);
}

fn selected_clip(s: &Session) -> Option<&ryolune_engine::model::Clip> {
    let id = s.view.selected_clip_id.as_ref()?;
    s.clips.iter().find(|c| &c.id == id)
}
fn selected_track(s: &Session) -> Option<&ryolune_engine::model::Track> {
    let id = s.view.selected_track_id.as_ref()?;
    s.tracks.iter().find(|t| &t.id == id)
}
fn selected_note(s: &Session) -> Option<(String, &ryolune_engine::model::Note)> {
    let note = s.view.selected_note_id.as_ref()?;
    let clip_id = s.view.editor_clip_id.as_ref()?;
    let clip = s.clips.iter().find(|c| &c.id == clip_id)?;
    let ClipData::Midi { notes, .. } = &clip.data else {
        return None;
    };
    notes
        .iter()
        .find(|n| &n.id == note)
        .map(|n| (clip_id.clone(), n))
}
fn is_midi(clip: Option<&ryolune_engine::model::Clip>) -> bool {
    clip.is_some_and(|c| matches!(c.data, ClipData::Midi { .. }))
}
/// The playhead as a bar, snapped to the grid for "at the playhead" edits.
pub fn playhead_bar(daw: &Daw, snap: bool) -> f64 {
    let s = daw.app.store.session();
    let bar = daw.app.position / s.beats_per_bar();
    if !snap {
        return bar;
    }
    super::format::snap_bars(bar, s.transport.snap_division, s.beats_per_bar())
}

/// Whether the action can run now.
pub fn enabled(id: &str, daw: &Daw) -> bool {
    let app = &daw.app;
    let s = app.store.session();
    let bar = app.position / s.beats_per_bar();
    let marker_here = s.markers.iter().any(|m| (m.bar - bar).abs() < 1e-6);
    match id {
        "undo" => app.store.can_undo(),
        "redo" => app.store.can_redo(),
        "deleteSelection" => selected_note(s).is_some() || selected_clip(s).is_some(),
        "duplicateClip" | "copy" | "cut" | "repeatRegion" => selected_clip(s).is_some(),
        "splitAtPlayhead" => {
            selected_clip(s).is_some_and(|c| bar > c.start_bar && bar < c.start_bar + c.length_bars)
        }
        "openInEditor" | "quantizeRegion" | "humanize" | "crescendo" | "diminuendo" | "legato"
        | "reverseMidi" | "fitMajor" | "fitMinor" => is_midi(selected_clip(s)),
        "transposeUp" | "transposeDown" | "transposeOctaveUp" | "transposeOctaveDown" => {
            selected_note(s).is_some() || is_midi(selected_clip(s))
        }
        "groupSelectedTrack" => selected_track(s).is_some_and(|t| t.kind != "bus"),
        "duplicateTrack" | "removeSelectedTrack" | "muteSelectedTrack" | "soloSelectedTrack" => {
            selected_track(s).is_some()
        }
        "armSelectedTrack" => selected_track(s).is_some_and(|t| t.kind != "bus"),
        "cycleMonitorSelectedTrack" => selected_track(s).is_some_and(|t| t.kind == "audio"),
        "stopAgent" => app.agents.runtime.running(),
        "addMarker" => !marker_here,
        "previousMarker" => s.markers.iter().any(|m| m.bar < bar - 1e-6),
        "nextMarker" => s.markers.iter().any(|m| m.bar > bar + 1e-6),
        "cycleSection" => s.markers.iter().any(|m| m.bar <= bar + 1e-6),
        "saveRecoveredTake" => app.unplaced_recording.is_some(),
        _ => true,
    }
}

/// Whether a toggle action is on, for menu check marks.
pub fn checked(id: &str, daw: &Daw) -> Option<bool> {
    let app = &daw.app;
    let s = app.store.session();
    let track = selected_track(s);
    Some(match id {
        "record" => app.record_enabled,
        "cycle" => s.transport.cycle,
        "metronome" => s.transport.metronome,
        "muteSelectedTrack" => track.is_some_and(|t| t.mute),
        "soloSelectedTrack" => track.is_some_and(|t| t.solo),
        "armSelectedTrack" => track.is_some_and(|t| t.armed),
        "cycleMonitorSelectedTrack" => {
            track.is_some_and(|t| t.monitor != ryolune_engine::model::Monitor::Off)
        }
        "followPlayhead" => s.view.follow_playhead,
        "toggleAgentPanel" => app.agents.open,
        "toggleMixer" => app.show_mixer,
        "toggleControllerLane" => app.show_controllers,
        "toggleTempoTrack" => app.show_tempo,
        "toggleAutomation" => app.show_automation,
        "musicalTyping" => app.musical_typing,
        "editorPianoRoll" => s.view.editor_mode == "pianoRoll",
        "editorScore" => s.view.editor_mode == "score",
        "editorStep" => s.view.editor_mode == "step",
        "toolPointer" => app.tool == 0,
        "toolPencil" => app.tool == 1,
        "toolScissors" => app.tool == 2,
        _ => return None,
    })
}

fn panel(daw: &mut Daw, name: &str, visible: bool, cx: &mut Context<Daw>) {
    daw.run(
        "ui.showPanel",
        json!({"panel": name, "visible": visible}),
        cx,
    );
}
fn settings_section(daw: &mut Daw, section: &str, cx: &mut Context<Daw>) {
    daw.run(
        "ui.showPanel",
        json!({"panel": "settings", "visible": true, "section": section}),
        cx,
    );
}
/// New tracks go right below the selected one.
fn add_track(daw: &mut Daw, kind: &str, cx: &mut Context<Daw>) {
    let s = daw.app.store.session();
    let index = s
        .view
        .selected_track_id
        .as_ref()
        .and_then(|id| s.tracks.iter().position(|t| &t.id == id))
        .map(|i| i + 1);
    if let Some(track) = daw.run("track.add", json!({ "kind": kind }), cx) {
        if let (Some(index), Some(id)) = (index, track["id"].as_str()) {
            daw.run("track.move", json!({"trackId": id, "index": index}), cx);
        }
    }
}
fn zoom_around_playhead(daw: &mut Daw, factor: f64, cx: &mut Context<Daw>) {
    let app = &daw.app;
    let s = app.store.session();
    let bar = app.position / s.beats_per_bar();
    let zoom = app.zoom as f64;
    let px = (bar - app.scroll) * zoom;
    let width = app.lane_width.max(1.0);
    let next = (zoom * factor).clamp(12.0, 480.0);
    let (anchor_bar, anchor_px) = if (0.0..=width).contains(&px) {
        (bar, px)
    } else {
        (app.scroll + width / 2.0 / zoom, width / 2.0)
    };
    daw.run(
        "view.set",
        json!({"pixelsPerBar": next, "scrollBar": (anchor_bar - anchor_px / next).max(0.0)}),
        cx,
    );
}
fn transpose(daw: &mut Daw, semitones: i32, cx: &mut Context<Daw>) {
    let s = daw.app.store.session();
    if let Some((clip, note)) = selected_note(s) {
        let pitch = (note.pitch as i32 + semitones).clamp(0, 127);
        let note = note.id.clone();
        daw.run(
            "note.update",
            json!({"clipId": clip, "noteId": note, "pitch": pitch}),
            cx,
        );
    } else if let Some(clip) = selected_clip(s).map(|c| c.id.clone()) {
        daw.run(
            "clip.transpose",
            json!({"clipId": clip, "semitones": semitones}),
            cx,
        );
    }
}
fn on_clip(daw: &mut Daw, method: &str, mut params: Value, cx: &mut Context<Daw>) {
    let Some(clip) = selected_clip(daw.app.store.session()).map(|c| c.id.clone()) else {
        return;
    };
    params["clipId"] = json!(clip);
    daw.run(method, params, cx);
}
fn on_track(
    daw: &mut Daw,
    method: &str,
    f: impl FnOnce(&ryolune_engine::model::Track) -> Value,
    cx: &mut Context<Daw>,
) {
    let Some(params) = selected_track(daw.app.store.session()).map(|t| {
        let mut p = f(t);
        p["trackId"] = json!(t.id);
        p
    }) else {
        return;
    };
    daw.run(method, params, cx);
}

/// Run a table action. Window-only effects (the palette, the composer) are the caller's:
/// this returns `false` for them so the workspace handles them.
pub fn perform(id: &str, daw: &mut Daw, cx: &mut Context<Daw>) -> bool {
    if !enabled(id, daw) {
        return true;
    }
    let s = daw.app.store.session();
    match id {
        "undo" => {
            daw.fire("history.undo", cx);
        }
        "redo" => {
            daw.fire("history.redo", cx);
        }
        "togglePlay" => {
            let method = if daw.app.playing {
                "transport.stop"
            } else {
                "transport.play"
            };
            daw.fire(method, cx);
        }
        "stop" => {
            daw.fire("transport.stop", cx);
        }
        "record" => {
            let enabled = !daw.app.record_enabled;
            daw.run("transport.punch", json!({ "enabled": enabled }), cx);
        }
        "cycle" => {
            let enabled = !s.transport.cycle;
            daw.run("transport.setCycle", json!({ "enabled": enabled }), cx);
        }
        "returnToStart" => {
            daw.fire("transport.returnToStart", cx);
        }
        "rewind" | "forward" => {
            let bars = if id == "rewind" { -1.0 } else { 1.0 };
            let beats = (daw.app.position + bars * s.beats_per_bar()).max(0.0);
            daw.run("transport.locate", json!({ "beats": beats }), cx);
        }
        "metronome" => {
            let enabled = !s.transport.metronome;
            daw.run("transport.setMetronome", json!({ "enabled": enabled }), cx);
        }
        "deleteSelection" => {
            if let Some((clip, note)) = selected_note(s) {
                let note = note.id.clone();
                daw.run("note.remove", json!({"clipId": clip, "noteId": note}), cx);
            } else {
                on_clip(daw, "clip.remove", json!({}), cx);
            }
        }
        "duplicateClip" => on_clip(daw, "clip.duplicate", json!({}), cx),
        "splitAtPlayhead" => {
            let bar = playhead_bar(daw, true);
            let clip = selected_clip(daw.app.store.session());
            if clip.is_some_and(|c| bar > c.start_bar && bar < c.start_bar + c.length_bars) {
                on_clip(daw, "clip.split", json!({ "bar": bar }), cx);
            }
        }
        "openInEditor" => {
            if let Some(clip) = selected_clip(s).map(|c| c.id.clone()) {
                daw.run("view.set", json!({ "editorClipId": clip }), cx);
            }
        }
        "copy" => on_clip(daw, "clip.copy", json!({}), cx),
        "cut" => on_clip(daw, "clip.cut", json!({}), cx),
        "paste" => {
            let bar = playhead_bar(daw, true);
            daw.run("clip.paste", json!({ "bar": bar }), cx);
        }
        "quantizeRegion" => on_clip(daw, "clip.quantize", json!({}), cx),
        "transposeUp" => transpose(daw, 1, cx),
        "transposeDown" => transpose(daw, -1, cx),
        "transposeOctaveUp" => transpose(daw, 12, cx),
        "transposeOctaveDown" => transpose(daw, -12, cx),
        "addAudioTrack" => add_track(daw, "audio", cx),
        "addMidiTrack" => add_track(daw, "midi", cx),
        "addBusTrack" => add_track(daw, "bus", cx),
        "groupSelectedTrack" => {
            if let Some(track) = selected_track(s).map(|t| t.id.clone()) {
                daw.run("track.group", json!({ "trackIds": [track] }), cx);
            }
        }
        "duplicateTrack" => {
            if let Some(track) = selected_track(s).map(|t| t.id.clone()) {
                if let Some(copy) = daw.run("track.duplicate", json!({ "trackId": track }), cx) {
                    if let Some(id) = copy["id"].as_str() {
                        daw.run("track.select", json!({ "trackId": id }), cx);
                    }
                }
            }
        }
        "removeSelectedTrack" => on_track(daw, "track.remove", |_| json!({}), cx),
        "muteSelectedTrack" => on_track(daw, "track.setMute", |t| json!({"muted": !t.mute}), cx),
        "soloSelectedTrack" => on_track(daw, "track.setSolo", |t| json!({"solo": !t.solo}), cx),
        "armSelectedTrack" => on_track(daw, "track.setArmed", |t| json!({"armed": !t.armed}), cx),
        "cycleMonitorSelectedTrack" => on_track(
            daw,
            "track.setMonitor",
            |t| {
                use ryolune_engine::model::Monitor;
                let next = match t.monitor {
                    Monitor::Off => "auto",
                    Monitor::Auto => "on",
                    Monitor::On => "off",
                };
                json!({ "monitor": next })
            },
            cx,
        ),
        "zoomIn" => zoom_around_playhead(daw, std::f64::consts::SQRT_2, cx),
        "zoomOut" => zoom_around_playhead(daw, std::f64::consts::FRAC_1_SQRT_2, cx),
        "zoomToFit" => {
            daw.fire("view.fit", cx);
        }
        "followPlayhead" => {
            let follow = !s.view.follow_playhead;
            daw.run("view.set", json!({ "followPlayhead": follow }), cx);
        }
        "toggleMixer" => {
            let v = !daw.app.show_mixer;
            panel(daw, "mixer", v, cx)
        }
        "toggleControllerLane" => {
            let v = !daw.app.show_controllers;
            panel(daw, "controllers", v, cx)
        }
        "toggleTempoTrack" => {
            let v = !daw.app.show_tempo;
            panel(daw, "tempo", v, cx)
        }
        "toggleAutomation" => {
            let v = !daw.app.show_automation;
            panel(daw, "automation", v, cx)
        }
        "toggleAgentPanel" => {
            let v = !daw.app.agents.open;
            panel(daw, "agent", v, cx)
        }
        "commandPalette" => panel(daw, "palette", true, cx),
        "showShortcuts" => panel(daw, "help", true, cx),
        "editorPianoRoll" => {
            daw.run("view.set", json!({"editorMode": "pianoRoll"}), cx);
        }
        "editorScore" => {
            daw.run("view.set", json!({"editorMode": "score"}), cx);
        }
        "editorStep" => {
            daw.run("view.set", json!({"editorMode": "step"}), cx);
        }
        "toolPointer" => {
            daw.run("ui.setTool", json!({"tool": "pointer"}), cx);
        }
        "toolPencil" => {
            daw.run("ui.setTool", json!({"tool": "pencil"}), cx);
        }
        "toolScissors" => {
            daw.run("ui.setTool", json!({"tool": "scissors"}), cx);
        }
        "addMarker" => {
            let bar = playhead_bar(daw, true);
            daw.run("marker.add", json!({ "bar": bar }), cx);
        }
        "previousMarker" => {
            daw.fire("marker.previous", cx);
        }
        "nextMarker" => {
            daw.fire("marker.next", cx);
        }
        "cycleSection" => {
            daw.fire("marker.cycleSection", cx);
        }
        "stopAgent" => {
            daw.fire("agent.stop", cx);
        }
        "musicalTyping" => {
            let enabled = !daw.app.musical_typing;
            daw.run("ui.musicalTyping", json!({ "enabled": enabled }), cx);
        }
        "newSession" => {
            daw.app.request(crate::app::Intent::New);
            cx.notify();
        }
        "openSession" => {
            daw.app.request(crate::app::Intent::Open);
            cx.notify();
        }
        "openDemo" => {
            daw.app.request(crate::app::Intent::Demo);
            cx.notify();
        }
        "save" | "saveAs" => {
            daw.app.save(id == "saveAs");
            cx.notify();
        }
        "importAudio" => {
            daw.app.import(None);
            cx.notify();
        }
        "importMidi" => {
            daw.app.import_midi_dialog();
            cx.notify();
        }
        "exportAudio" => {
            daw.app.open_export_dialog();
            cx.notify();
        }
        "exportMidi" => {
            daw.app.export_midi_dialog();
            cx.notify();
        }
        "openRecent" => {
            daw.app.interop.show_recent = true;
            cx.notify();
        }
        "importFromApp" | "exportForApp" => {
            daw.app.app_dialog(id == "importFromApp");
            cx.notify();
        }
        "firstRunSetup" => {
            daw.app.show_onboarding();
            cx.notify();
        }
        "recoverSession" => panel(daw, "recovery", true, cx),
        "settings" => panel(daw, "settings", true, cx),
        "quit" => {
            daw.fire("app.quit", cx);
        }
        "saveRecoveredTake" => {
            // The window asks where to write it; scripts give `session.saveRecoveredTake` a path.
            daw.app.save_recovered_take();
            cx.notify();
        }
        "reconnectOutput" => {
            daw.fire("audio.reconnect", cx);
        }
        "audioSettings" => settings_section(daw, "audio", cx),
        "agentSettings" => settings_section(daw, "agent", cx),
        "rescanPlugins" => {
            daw.fire("plugin.scan", cx);
        }
        "showMaster" => panel(daw, "master", true, cx),
        "showBusA" => panel(daw, "bus-a", true, cx),
        "showBusB" => panel(daw, "bus-b", true, cx),
        "humanize" => on_clip(
            daw,
            "clip.humanize",
            json!({"timingMs": 10, "velocity": 8, "seed": 1}),
            cx,
        ),
        "crescendo" => on_clip(daw, "clip.velocityRamp", json!({"from": 55, "to": 110}), cx),
        "diminuendo" => on_clip(daw, "clip.velocityRamp", json!({"from": 110, "to": 55}), cx),
        "legato" => on_clip(daw, "clip.legato", json!({}), cx),
        "reverseMidi" => on_clip(daw, "clip.reverseMidi", json!({}), cx),
        "fitMajor" => on_clip(
            daw,
            "clip.fitScale",
            json!({"root": 0, "scale": "major"}),
            cx,
        ),
        "fitMinor" => on_clip(
            daw,
            "clip.fitScale",
            json!({"root": 0, "scale": "minor"}),
            cx,
        ),
        "repeatRegion" => on_clip(daw, "clip.repeat", json!({"count": 3}), cx),
        "checkUpdates" => {
            daw.fire("app.checkUpdates", cx);
        }
        "pluginGuide" => {
            daw.run("app.openGuide", json!({"guide": "plugins"}), cx);
        }
        "support" => {
            daw.run("app.openGuide", json!({"guide": "support"}), cx);
        }
        // Window-only: the workspace opens the composer.
        "askAgent" => return false,
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_every_menu_entry_is_an_action() {
        let mut seen = std::collections::BTreeSet::new();
        for d in ACTIONS {
            assert!(seen.insert(d.id), "duplicate action {}", d.id);
        }
        for (_, entries) in MENUS {
            for id in entries.iter().flatten() {
                assert!(def(id).is_some(), "menu entry {id} is not an action");
            }
        }
    }

    #[test]
    fn every_shortcut_parses_and_is_bound_once() {
        let mut keys = std::collections::BTreeSet::new();
        for d in ACTIONS {
            for k in d.keys {
                assert!(gpui::Keystroke::parse(k).is_ok(), "{k}");
                assert!(keys.insert(*k), "{k} is bound twice");
            }
        }
        assert_eq!(label_for("secondary-shift-z", true).unwrap(), "⇧⌘Z");
        assert_eq!(
            label_for("secondary-shift-z", false).unwrap(),
            "Ctrl+Shift+Z"
        );
        assert_eq!(label_for("secondary--", true).unwrap(), "⌘-");
    }
}
