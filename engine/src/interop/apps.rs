//! The music apps people come to ryolune from, and how a song travels between each and
//! ryolune: the formats it exchanges (DAWproject where the app has it, MIDI and stems where it
//! does not), how to bring a song over and take it back in its own menu names, and where it
//! is installed. The first-run setup asks which one a person used, `session.formats` lists
//! them with the ones found on this computer, and the user guide's "Coming from another
//! app" section follows this table.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Mac,
    Windows,
    Linux,
}
impl Os {
    pub const fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// A place on one system. `~` is the home folder; `%APPDATA%`, `%LOCALAPPDATA%`,
/// `%PROGRAMFILES%` and `%PROGRAMDATA%` are Windows' folders; a last part ending in `*`
/// matches every entry starting with what comes before it (`Ableton Live*`). A path without
/// a slash is a program looked up on the `PATH`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Place {
    pub os: Os,
    pub path: &'static str,
}
const fn mac(path: &'static str) -> Place {
    Place { os: Os::Mac, path }
}
const fn win(path: &'static str) -> Place {
    Place {
        os: Os::Windows,
        path,
    }
}
const fn linux(path: &'static str) -> Place {
    Place {
        os: Os::Linux,
        path,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub id: &'static str,
    pub name: &'static str,
    pub vendor: &'static str,
    /// `interop::FORMATS` ids ryolune opens from it, best first.
    pub opens: &'static [&'static str],
    /// `interop::FORMATS` ids ryolune writes that it opens, best first.
    pub writes: &'static [&'static str],
    /// How to bring a song from it into ryolune, in its own menu names.
    pub bring: &'static str,
    /// How to take a ryolune song back to it.
    pub take: &'static str,
    /// Where it is installed: any one found means it is.
    pub detect: &'static [Place],
}

pub const APPS: &[App] = &[
    App {
        id: "ableton",
        name: "Ableton Live",
        vendor: "Ableton",
        opens: &["audio", "midi"],
        writes: &["package", "midi", "stems"],
        bring: "Live does not write DAWproject. In Live, choose File › Export Audio/Video…, set Rendered Track to All Individual Tracks and export: one WAV per track. For a MIDI clip, right-click it and choose Export MIDI Clip…. In ryolune, choose File › Import from Another App… and pick all the WAVs at once (each becomes a track), or the .mid.",
        take: "In ryolune, choose File › Export for Another App… › Ableton Live: a folder with the song as a MIDI file and one WAV per track (Stems). In Live, drag the WAVs into the Arrangement at bar 1, then the .mid onto a MIDI track, and set Live's tempo to the song's.",
        detect: &[
            mac("/Applications/Ableton Live*"),
            win("%PROGRAMDATA%/Ableton"),
        ],
    },
    App {
        id: "logic",
        name: "Logic Pro",
        vendor: "Apple",
        opens: &["audio", "midi"],
        writes: &["package", "midi", "stems"],
        bring: "Logic does not write DAWproject. In Logic, choose File › Export › All Tracks as Audio Files… (one file per track, from the project start), and select the MIDI regions and choose File › Export › Selection as MIDI File…. In ryolune, choose File › Import from Another App… and pick the audio files together, or the .mid.",
        take: "In ryolune, choose File › Export for Another App… › Logic Pro: a folder with the MIDI file and one WAV per track. In Logic, choose File › Import › MIDI File… for the notes and drag the WAVs into the tracks area at bar 1.",
        detect: &[
            mac("/Applications/Logic Pro.app"),
            mac("/Applications/Logic Pro X.app"),
        ],
    },
    App {
        id: "fl",
        name: "FL Studio",
        vendor: "Image-Line",
        opens: &["audio", "midi"],
        writes: &["package", "midi", "stems"],
        bring: "FL Studio does not write DAWproject. In FL Studio, choose File › Export › Wave file… and turn on Split mixer tracks (one WAV per mixer track); for notes, choose File › Export › MIDI file…. In ryolune, choose File › Import from Another App… and pick the WAVs together, or the .mid.",
        take: "In ryolune, choose File › Export for Another App… › FL Studio: a folder with the MIDI file and one WAV per track. In FL Studio, choose File › Import › MIDI file… for the notes and drag the WAVs into the Playlist at bar 1.",
        detect: &[
            mac("/Applications/FL Studio*"),
            win("%PROGRAMFILES%/Image-Line/FL Studio*"),
        ],
    },
    App {
        id: "bitwig",
        name: "Bitwig Studio",
        vendor: "Bitwig",
        opens: &["dawproject", "midi", "audio"],
        writes: &["dawproject", "package"],
        bring: "In Bitwig Studio (5.0.9 or later), export the project as a DAWproject from the File menu. In ryolune, choose File › Import from Another App… and pick the .dawproject: tracks, clips, notes, audio, the mix, markers and tempo come across, and your CLAP and VST3 plugins load when they are installed here.",
        take: "In ryolune, choose File › Export for Another App… › Bitwig Studio: a .dawproject. In Bitwig Studio, open it with File › Open… (or drop it on the window).",
        detect: &[
            mac("/Applications/Bitwig Studio.app"),
            win("%PROGRAMFILES%/Bitwig Studio"),
            linux("bitwig-studio"),
            linux("/opt/bitwig-studio"),
            linux("/var/lib/flatpak/app/com.bitwig.BitwigStudio"),
            linux("~/.local/share/flatpak/app/com.bitwig.BitwigStudio"),
        ],
    },
    App {
        id: "reaper",
        name: "REAPER",
        vendor: "Cockos",
        opens: &["audio", "midi", "dawproject"],
        writes: &["package", "midi", "dawproject"],
        bring: "REAPER does not write DAWproject itself. Choose File › Render…, set Source to Stems (selected tracks) with every track selected, and render one WAV per track; for notes, choose File › Export project MIDI…. In ryolune, choose File › Import from Another App… and pick the WAVs together, or the .mid. (The free ProjectConverter by Jürgen Moßgraber turns a .rpp into a .dawproject, which ryolune opens whole.)",
        take: "In ryolune, choose File › Export for Another App… › REAPER: a folder with the MIDI file and one WAV per track. In REAPER, choose Insert › Media file… for each, at the project start. (ProjectConverter can also turn a ryolune .dawproject into a .rpp.)",
        detect: &[
            mac("/Applications/REAPER.app"),
            mac("/Applications/REAPER64.app"),
            win("%PROGRAMFILES%/REAPER (x64)"),
            win("%PROGRAMFILES%/REAPER"),
            linux("reaper"),
            linux("/opt/REAPER"),
            linux("~/opt/REAPER"),
        ],
    },
    App {
        id: "cubase",
        name: "Cubase",
        vendor: "Steinberg",
        opens: &["dawproject", "midi", "audio"],
        writes: &["dawproject", "package"],
        bring: "In Cubase 14 or later, choose File › Export › DAWproject…. In ryolune, choose File › Import from Another App… and pick the .dawproject. With an older Cubase, choose File › Export › Audio Mixdown… with Channel Batch Export for stems, and File › Export › MIDI File… for the notes.",
        take: "In ryolune, choose File › Export for Another App… › Cubase: a .dawproject. In Cubase 14 or later, choose File › Import › DAWproject…. For an older Cubase, export for REAPER instead (MIDI and stems) and import those.",
        detect: &[
            mac("/Applications/Cubase*"),
            win("%PROGRAMFILES%/Steinberg/Cubase*"),
        ],
    },
    App {
        id: "studioone",
        name: "Studio One",
        vendor: "PreSonus",
        opens: &["dawproject", "midi", "audio"],
        writes: &["dawproject", "package"],
        bring: "In Studio One 6.5 or later, export the song as a DAWproject from the File menu. In ryolune, choose File › Import from Another App… and pick the .dawproject.",
        take: "In ryolune, choose File › Export for Another App… › Studio One: a .dawproject. In Studio One, open it with File › Open… or drop it on the Start page.",
        detect: &[
            mac("/Applications/Studio One*"),
            win("%PROGRAMFILES%/PreSonus/Studio One*"),
        ],
    },
    App {
        id: "protools",
        name: "Pro Tools",
        vendor: "Avid",
        opens: &["audio", "midi"],
        writes: &["package", "midi", "stems"],
        bring: "Pro Tools does not write DAWproject. Select every track, right-click a track name and choose Commit or Bounce (Track Bounce) to get one WAV per track; for notes, choose File › Export › MIDI…. In ryolune, choose File › Import from Another App… and pick the WAVs together, or the .mid.",
        take: "In ryolune, choose File › Export for Another App… › Pro Tools: a folder with the MIDI file and one WAV per track. In Pro Tools, choose File › Import › Audio… (to new tracks, at session start) and File › Import › MIDI….",
        detect: &[
            mac("/Applications/Pro Tools.app"),
            win("%PROGRAMFILES%/Avid/Pro Tools"),
        ],
    },
    App {
        id: "garageband",
        name: "GarageBand",
        vendor: "Apple",
        opens: &["audio", "midi"],
        writes: &["package", "midi"],
        bring: "GarageBand exports only the whole mix (Share › Export Song to Disk…). For separate tracks, solo each one and export it in turn, then pick all the files in ryolune's File › Import from Another App…. Opening the GarageBand project in Logic Pro gives MIDI and every track at once.",
        take: "In ryolune, choose File › Export for Another App… › GarageBand: a folder with the MIDI file and one WAV per track. Drag the .mid and the WAVs into GarageBand's tracks area at bar 1.",
        detect: &[mac("/Applications/GarageBand.app")],
    },
];

pub fn find(id: &str) -> Option<&'static App> {
    let id = id.trim().to_ascii_lowercase();
    APPS.iter()
        .find(|a| a.id == id || a.name.to_ascii_lowercase() == id)
}
pub fn unknown(id: &str) -> String {
    format!(
        "Unknown app `{id}`: use one of {}",
        APPS.iter().map(|a| a.id).collect::<Vec<_>>().join(", ")
    )
}

/// Whether the app is installed on this computer.
pub fn installed(app: &App) -> bool {
    app.detect
        .iter()
        .filter(|place| place.os == Os::current())
        .any(|place| exists(place.path))
}

fn env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Expand `~` and `%VAR%`, then look for the place (with a trailing `*` on its last part).
fn exists(pattern: &str) -> bool {
    if !pattern.contains('/') {
        return on_path(pattern);
    }
    let expanded = if let Some(rest) = pattern.strip_prefix("~/") {
        match env("HOME").or_else(|| env("USERPROFILE")) {
            Some(home) => home.join(rest),
            None => return false,
        }
    } else if let Some(rest) = pattern.strip_prefix('%') {
        let Some((var, rest)) = rest.split_once('%') else {
            return false;
        };
        match env(var) {
            Some(base) => base.join(rest.trim_start_matches('/')),
            None => return false,
        }
    } else {
        PathBuf::from(pattern)
    };
    let text = expanded.to_string_lossy();
    match text.strip_suffix('*') {
        None => expanded.exists(),
        Some(prefix) => {
            let prefix = Path::new(prefix);
            let (Some(parent), Some(start)) =
                (prefix.parent(), prefix.file_name().and_then(|s| s.to_str()))
            else {
                return false;
            };
            std::fs::read_dir(parent).is_ok_and(|entries| {
                entries
                    .flatten()
                    .any(|e| e.file_name().to_string_lossy().starts_with(start))
            })
        }
    }
}

fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(program);
        candidate.is_file() || (cfg!(windows) && candidate.with_extension("exe").is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_app_has_steps_and_formats_ryolune_knows() {
        let mut ids = std::collections::BTreeSet::new();
        for app in APPS {
            assert!(ids.insert(app.id), "{} twice", app.id);
            assert!(!app.bring.is_empty() && !app.take.is_empty(), "{}", app.id);
            assert!(!app.detect.is_empty(), "{} has nowhere to look", app.id);
            for id in app.opens {
                let format = crate::interop::format(id).unwrap();
                assert!(format.opens, "{} opens {id}", app.id);
            }
            for id in app.writes {
                let format = crate::interop::format(id).unwrap();
                assert!(format.writes, "{} writes {id}", app.id);
            }
            // Where the app opens DAWproject, it is what ryolune writes for it first.
            if app.opens.first() == Some(&"dawproject") {
                assert_eq!(app.writes.first(), Some(&"dawproject"), "{}", app.id);
            }
        }
        for id in [
            "ableton",
            "logic",
            "fl",
            "bitwig",
            "reaper",
            "cubase",
            "studioone",
            "protools",
            "garageband",
        ] {
            assert!(find(id).is_some(), "{id}");
        }
        assert_eq!(find("Bitwig Studio").unwrap().id, "bitwig");
    }

    #[test]
    fn places_expand_home_and_match_a_prefix() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Ableton Live 12 Suite.app")).unwrap();
        let pattern = format!("{}/Ableton Live*", dir.path().display());
        assert!(exists(&pattern));
        assert!(!exists(&format!("{}/Cubase*", dir.path().display())));
        assert!(!exists("/definitely/not/here/App.app"));
        assert!(!exists("%RYOLUNE_NO_SUCH_VARIABLE%/x"));
        assert!(!exists("no-such-program-ryolune-test"));
    }
}
