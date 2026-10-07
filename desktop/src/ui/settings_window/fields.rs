//! The plain preferences, row by row: which section shows each setting, what it is called,
//! what it does and which control edits it. Agent and Generation have their own forms. A
//! test keeps this table and `engine::settings::Settings` in step, so a new preference is
//! either shown here or deliberately left out.

use serde_json::{json, Value};

/// The sidebar, in the order the window shows it: (section key, title). The keys are
/// `SECTION_KEYS` (what `ui.showPanel section=` takes).
pub(crate) const SIDEBAR: [(&str, &str); 10] = [
    ("general", "General"),
    ("audio", "Audio & MIDI"),
    ("interface", "Interface"),
    ("agent", "Agent"),
    ("generation", "Generation"),
    ("plugins", "Plugins"),
    ("control", "Control"),
    ("updates", "Updates"),
    ("diagnostics", "Diagnostics"),
    ("about", "About"),
];

/// A list a choice is made from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Choices {
    Outputs,
    Inputs,
    MidiInputs,
    Buffer,
    CountIn,
    Scale,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Kind {
    Switch,
    /// A whole number typed in a field.
    Number {
        min: i64,
        max: i64,
    },
    Choice(Choices),
    /// Dark, light or auto for the one ryolune theme (`interface.mode`).
    Appearance,
    /// Folders, added with the folder chooser and removed one by one.
    Paths,
}

pub(crate) struct Field {
    pub section: &'static str,
    pub path: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub kind: Kind,
}

const fn f(
    section: &'static str,
    path: &'static str,
    label: &'static str,
    description: &'static str,
    kind: Kind,
) -> Field {
    Field {
        section,
        path,
        label,
        description,
        kind,
    }
}

pub(crate) const FIELDS: [Field; 22] = [
    f(
        "general",
        "general.recoveryIntervalSeconds",
        "Recovery snapshots",
        "Seconds between recovery copies while an edited session sits idle, 10 to 600.",
        Kind::Number { min: 10, max: 600 },
    ),
    f(
        "general",
        "general.confirmBeforeQuit",
        "Confirm before quitting",
        "Ask before ryolune closes, even when everything is saved.",
        Kind::Switch,
    ),
    f(
        "general",
        "general.reopenLastSession",
        "Reopen the last session",
        "Start where you left off.",
        Kind::Switch,
    ),
    f(
        "audio",
        "audio.outputDevice",
        "Output",
        "Where ryolune plays. System default follows the system's choice.",
        Kind::Choice(Choices::Outputs),
    ),
    f(
        "audio",
        "audio.inputDevice",
        "Input",
        "What audio tracks record and monitor.",
        Kind::Choice(Choices::Inputs),
    ),
    f(
        "audio",
        "audio.bufferFrames",
        "Buffer size",
        "Frames per buffer for output and input. Smaller is lower latency and more CPU.",
        Kind::Choice(Choices::Buffer),
    ),
    f(
        "audio",
        "audio.midiInput",
        "MIDI input",
        "The keyboard or controller ryolune plays and records.",
        Kind::Choice(Choices::MidiInputs),
    ),
    f(
        "audio",
        "audio.connectMidiOnStart",
        "Connect MIDI on start",
        "Open the MIDI input when ryolune starts.",
        Kind::Switch,
    ),
    f(
        "audio",
        "audio.countInBars",
        "Count-in",
        "Bars of click before a recording starts from a stopped transport.",
        Kind::Choice(Choices::CountIn),
    ),
    f(
        "audio",
        "audio.meterInputWhenArmed",
        "Meter the input when armed",
        "Open the input while an audio track is armed, so its level shows before the take.",
        Kind::Switch,
    ),
    f(
        "interface",
        "interface.appearance",
        "Appearance",
        "",
        Kind::Appearance,
    ),
    f(
        "interface",
        "interface.scale",
        "Interface size",
        "Scales every panel, text and control.",
        Kind::Choice(Choices::Scale),
    ),
    f(
        "interface",
        "interface.followPlayhead",
        "Follow the playhead",
        "Scroll the arrangement to keep the playhead in view while playing.",
        Kind::Switch,
    ),
    f(
        "interface",
        "interface.showTooltips",
        "Show tooltips",
        "Name controls and their shortcuts when the pointer rests on them.",
        Kind::Switch,
    ),
    f(
        "interface",
        "interface.agentPanelOpenOnStart",
        "Open the agent panel on start",
        "Show the conversation at the right edge when ryolune starts.",
        Kind::Switch,
    ),
    f(
        "plugins",
        "plugins.scanOnStart",
        "Scan on start",
        "Look for new and updated plugins each time ryolune starts.",
        Kind::Switch,
    ),
    f(
        "plugins",
        "plugins.extraClapPaths",
        "CLAP folders",
        "Searched besides the system's plugin folders.",
        Kind::Paths,
    ),
    f(
        "plugins",
        "plugins.extraVst3Paths",
        "VST3 folders",
        "Searched besides the system's plugin folders.",
        Kind::Paths,
    ),
    f(
        "plugins",
        "plugins.extraNativePaths",
        "Native plugin folders",
        "ryolune plugins built with the Rust SDK.",
        Kind::Paths,
    ),
    f(
        "control",
        "control.enableBridge",
        "Local connection",
        "Let ryolune-cli, ryolune-mcp and outside agents drive this window over a connection on this computer only. Codex and Claude Code need it.",
        Kind::Switch,
    ),
    f(
        "updates",
        "general.checkUpdatesOnStart",
        "Check for updates on start",
        "Ask GitHub for a newer release when ryolune starts, then every six hours while it is open.",
        Kind::Switch,
    ),
    f(
        "updates",
        "general.installUpdatesAutomatically",
        "Install updates automatically",
        "Download and verify a new release in the background; ryolune restarts into it when you choose.",
        Kind::Switch,
    ),
];

/// Settings kept on purpose out of the window: bookkeeping, the theme's mode (edited by the
/// Appearance picker) and what the browser edits itself (favourites, folders, recents).
#[cfg(test)]
pub(crate) const HIDDEN: [&str; 10] = [
    "general.lastSession",
    "general.recentSessions",
    "general.exportsCompleted",
    "general.supportAsked",
    "general.lastRunVersion",
    "interface.mode",
    "plugins.favorites",
    "plugins.folders",
    "plugins.recent",
    // The switches of the Plugins window (Mix › Plugins…).
    "plugins.disabled",
];

/// Devices to choose from, as `audio.devices` lists them.
#[derive(Clone, Default, Debug)]
pub(crate) struct Devices {
    pub outputs: Vec<String>,
    pub inputs: Vec<String>,
    pub midi_inputs: Vec<String>,
}

impl Devices {
    pub fn from_value(value: &Value) -> Self {
        let list = |key: &str| {
            value[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        };
        Self {
            outputs: list("outputs"),
            inputs: list("inputs"),
            midi_inputs: list("midiInputs"),
        }
    }
}

/// The choices of a list: (what the menu says, the value written).
pub(crate) fn options(choices: Choices, devices: &Devices) -> Vec<(String, Value)> {
    let names = |list: &[String], none: &str| {
        std::iter::once((none.to_string(), Value::Null))
            .chain(list.iter().map(|n| (n.clone(), json!(n))))
            .collect()
    };
    match choices {
        Choices::Outputs => names(&devices.outputs, "System default"),
        Choices::Inputs => names(&devices.inputs, "System default"),
        Choices::MidiInputs => names(&devices.midi_inputs, "None"),
        Choices::Buffer => std::iter::once(("System default".to_string(), Value::Null))
            .chain(
                [64, 128, 256, 512, 1024, 2048]
                    .into_iter()
                    .map(|n| (format!("{n} frames"), json!(n))),
            )
            .collect(),
        Choices::CountIn => (0..=4)
            .map(|n| {
                (
                    match n {
                        0 => "Off".to_string(),
                        1 => "1 bar".to_string(),
                        n => format!("{n} bars"),
                    },
                    json!(n),
                )
            })
            .collect(),
        Choices::Scale => [0.75, 0.85, 0.9, 1.0, 1.1, 1.25, 1.5]
            .into_iter()
            .map(|s: f64| (format!("{}%", (s * 100.0).round()), json!(s)))
            .collect(),
    }
}

/// What a select shows for the saved value.
pub(crate) fn choice_label(choices: Choices, value: &Value, devices: &Devices) -> String {
    let same = |a: &Value, b: &Value| match (a.as_f64(), b.as_f64()) {
        (Some(a), Some(b)) => (a - b).abs() < 1e-6,
        _ => a == b,
    };
    options(choices, devices)
        .into_iter()
        .find(|(_, v)| same(v, value))
        .map(|(label, _)| label)
        .unwrap_or_else(|| match value {
            Value::String(name) => name.clone(),
            Value::Number(n) if choices == Choices::Scale => {
                format!("{}%", (n.as_f64().unwrap_or(1.0) * 100.0).round())
            }
            other => other.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryolune_engine::settings::Settings;

    #[test]
    fn every_preference_is_shown_or_left_out_on_purpose() {
        let document = Settings::default().redacted();
        for section in ["general", "audio", "interface", "plugins", "control"] {
            for key in document[section].as_object().unwrap().keys() {
                let path = format!("{section}.{key}");
                let shown = FIELDS.iter().any(|f| f.path == path);
                let hidden = HIDDEN.contains(&path.as_str());
                assert!(shown ^ hidden, "{path} must be shown once or hidden");
            }
        }
        for field in &FIELDS {
            let (section, key) = field.path.split_once('.').unwrap();
            assert!(
                document[section].get(key).is_some(),
                "{} exists",
                field.path
            );
            assert!(SIDEBAR.iter().any(|(k, _)| *k == field.section));
        }
        for (key, _) in SIDEBAR {
            assert!(crate::settings::SECTION_KEYS.contains(&key));
        }
        assert_eq!(SIDEBAR.len(), crate::settings::SECTION_KEYS.len());
    }

    #[test]
    fn choices_name_the_saved_value() {
        let devices = Devices {
            outputs: vec!["Studio Display".into()],
            ..Default::default()
        };
        assert_eq!(
            choice_label(Choices::Outputs, &Value::Null, &devices),
            "System default"
        );
        assert_eq!(
            choice_label(Choices::Outputs, &json!("Studio Display"), &devices),
            "Studio Display"
        );
        assert_eq!(
            choice_label(Choices::Outputs, &json!("Gone"), &devices),
            "Gone"
        );
        assert_eq!(choice_label(Choices::Scale, &json!(1.0), &devices), "100%");
        assert_eq!(choice_label(Choices::Scale, &json!(1.75), &devices), "175%");
        assert_eq!(choice_label(Choices::CountIn, &json!(0), &devices), "Off");
        assert_eq!(
            choice_label(Choices::Buffer, &json!(256), &devices),
            "256 frames"
        );
    }
}
