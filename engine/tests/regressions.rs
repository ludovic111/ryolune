//! Regression tests for bugs found in the 0.8 quality pass, one test per bug.
use ryolune_engine::control::{self, Headless, Host};
use serde_json::{json, Value};

fn call(host: &mut dyn Host, name: &str, params: Value) -> Value {
    control::call(host, name, &params, false).unwrap_or_else(|e| panic!("{name} {params}: {e}"))
}
fn fail(host: &mut dyn Host, name: &str, params: Value) -> String {
    control::call(host, name, &params, false)
        .err()
        .unwrap_or_else(|| panic!("{name} {params} should fail"))
}
fn midi_track(host: &mut dyn Host) -> String {
    call(host, "track.add", json!({"kind":"midi","name":"Keys"}))["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// `strip.setInsert slot=0 bypassed=true` without `effect` emptied the slot, throwing the
/// effect and its settings away instead of bypassing it.
#[test]
fn bypassing_an_insert_by_slot_keeps_the_effect() {
    let mut host = Headless::new();
    let track = midi_track(&mut host);
    call(
        &mut host,
        "strip.setInsert",
        json!({"trackId":track,"slot":0,"effect":"Chorus"}),
    );
    let strip = call(
        &mut host,
        "strip.setInsert",
        json!({"trackId":track,"slot":0,"bypassed":true}),
    );
    assert_eq!(strip["inserts"][0]["effect"], "Chorus");
    assert_eq!(strip["inserts"][0]["state"], "bypassed");
    let strip = call(
        &mut host,
        "strip.setInsert",
        json!({"trackId":track,"slot":0,"bypassed":false}),
    );
    assert_eq!(strip["inserts"][0]["state"], "active");
    assert!(fail(
        &mut host,
        "strip.setInsert",
        json!({"trackId":track,"slot":1,"bypassed":true}),
    )
    .contains("empty"));
    let strip = call(
        &mut host,
        "strip.setInsert",
        json!({"trackId":track,"slot":0}),
    );
    assert_eq!(strip["inserts"][0]["state"], "empty");
}

/// `rhythm.preview path=…` could overwrite the open session file with a WAV, and an agent
/// with file operations off could write anywhere through it.
#[test]
fn rhythm_preview_respects_the_session_file_and_agent_permissions() {
    use ryolune_engine::{control_app, settings::Permissions};
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("song.ryolune");
    let mut host = Headless::new();
    call(&mut host, "session.save", json!({ "path": song }));
    let lanes = json!([{"steps":4,"pulses":4,"rotation":0,"pitch":36,"velocity":100}]);
    let error = fail(
        &mut host,
        "rhythm.preview",
        json!({ "lanes": lanes, "bars": 1, "path": song }),
    );
    assert!(error.contains("open session file"), "{error}");
    assert!(std::fs::read_to_string(&song).unwrap().starts_with('{'));
    let spec = control::spec("rhythm.preview").unwrap();
    assert!(spec.mutates, "a command that writes files is not read-only");
    let off = Permissions {
        file_operations: false,
        ..Permissions::default()
    };
    let with_path = json!({ "lanes": lanes, "bars": 1, "path": "/tmp/x.wav" });
    assert!(control_app::denied_for_agent_request("rhythm.preview", &with_path, &off).is_some());
    assert!(control_app::denied_for_agent_request(
        "ui.screenshot",
        &json!({"path":"/tmp/x.png"}),
        &off
    )
    .is_some());
    let without = json!({ "lanes": lanes, "bars": 1 });
    assert!(control_app::denied_for_agent_request("rhythm.preview", &without, &off).is_none());
    assert!(control_app::denied_for_agent_request(
        "rhythm.preview",
        &with_path,
        &Permissions::default()
    )
    .is_none());
}

/// `strip.getState` inside `session.batch` captured plugin state as an undo step of its own
/// in the window, which ended the batch's step and broke its rollback.
#[test]
fn strip_get_state_is_not_batchable() {
    let mut host = Headless::new();
    let track = midi_track(&mut host);
    let error = fail(
        &mut host,
        "session.batch",
        json!({"commands":[{"command":"strip.getState","params":{"trackId":track}}]}),
    );
    assert!(error.contains("strip.getState"), "{error}");
}

/// A failed atomic `session.batch` rolled the document back but emptied Redo: its first edit
/// had cleared the redo history, and the rollback did not bring it back.
#[test]
fn a_failed_atomic_batch_keeps_redo() {
    let mut host = Headless::new();
    call(&mut host, "session.rename", json!({"name":"One"}));
    call(&mut host, "session.rename", json!({"name":"Two"}));
    call(&mut host, "history.undo", json!({}));
    assert_eq!(call(&mut host, "history.info", json!({}))["canRedo"], true);
    let error = fail(
        &mut host,
        "session.batch",
        json!({"commands":[
            {"command":"session.rename","params":{"name":"Batch"}},
            {"command":"track.remove","params":{"trackId":"missing"}}
        ]}),
    );
    assert!(error.contains("rolled back"), "{error}");
    assert_eq!(call(&mut host, "session.info", json!({}))["name"], "One");
    assert_eq!(call(&mut host, "history.info", json!({}))["canRedo"], true);
    call(&mut host, "history.redo", json!({}));
    assert_eq!(call(&mut host, "session.info", json!({}))["name"], "Two");
}

/// Removing the selected note (or replacing every note) left `selection.noteId` naming a
/// note that no longer existed.
#[test]
fn removing_the_selected_note_clears_the_note_selection() {
    let mut host = Headless::new();
    let track = midi_track(&mut host);
    let clip = call(
        &mut host,
        "clip.create",
        json!({"trackId":track,"startBar":0,"lengthBars":1,
               "notes":[{"start":0,"length":1,"pitch":60},{"start":1,"length":1,"pitch":62}]}),
    );
    let clip_id = clip["id"].as_str().unwrap().to_string();
    let notes = call(&mut host, "note.list", json!({"clipId":clip_id}));
    let first = notes[0]["id"].as_str().unwrap().to_string();
    let second = notes[1]["id"].as_str().unwrap().to_string();
    call(
        &mut host,
        "clip.select",
        json!({"clipId":clip_id,"noteId":first}),
    );
    // Editing another note keeps the selection.
    call(
        &mut host,
        "note.update",
        json!({"clipId":clip_id,"noteId":second,"pitch":64}),
    );
    assert_eq!(
        call(&mut host, "session.info", json!({}))["selection"]["noteId"],
        first.as_str()
    );
    call(
        &mut host,
        "note.remove",
        json!({"clipId":clip_id,"noteId":first}),
    );
    let selection = &call(&mut host, "session.info", json!({}))["selection"];
    assert!(selection["noteId"].is_null(), "{selection}");
    assert_eq!(selection["clipId"], clip_id.as_str());
}

/// A file where two slots share an insert id (hand-merged, or written by a buggy tool) made
/// both tracks run through one plugin instance; an out-of-range piano-roll scroll from a file
/// left the roll empty.
#[test]
fn loading_repairs_shared_insert_ids_and_the_piano_roll_scroll() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("merged.ryolune");
    let mut host = Headless::new();
    let a = midi_track(&mut host);
    let b = midi_track(&mut host);
    for track in [&a, &b] {
        call(
            &mut host,
            "strip.setInsert",
            json!({"trackId":track,"slot":0,"effect":"Chorus"}),
        );
    }
    call(&mut host, "session.save", json!({ "path": path }));
    let mut file: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let shared = file["session"]["strips"][&a]["inserts"][0]["id"].clone();
    file["session"]["strips"][&b]["inserts"][0]["id"] = shared;
    file["session"]["view"]["editorLowPitch"] = json!(250);
    std::fs::write(&path, file.to_string()).unwrap();
    let (session, _) = ryolune_engine::document::load(&path).unwrap();
    let store = ryolune_engine::store::Store::new(session).unwrap();
    let session = store.session();
    assert_ne!(
        session.strips[&a].inserts[0].id,
        session.strips[&b].inserts[0].id
    );
    let mut ids: Vec<&str> = session
        .strips
        .values()
        .flat_map(|s| s.inserts.iter().chain(s.synth.iter()))
        .map(|i| i.id.as_str())
        .collect();
    let count = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), count, "every slot has its own id");
    assert_eq!(session.view.editor_low_pitch, Some(108));
}

/// `clip.legato` failed with "Invalid MIDI note" when a shortened region left a note past its
/// end; names of 41 CJK characters were refused as longer than 120 "characters".
#[test]
fn legato_ignores_notes_past_the_region_and_names_count_characters() {
    let mut host = Headless::new();
    let track = midi_track(&mut host);
    let clip = call(
        &mut host,
        "clip.create",
        json!({"trackId":track,"startBar":0,"lengthBars":2,
               "notes":[{"start":0,"length":0.5,"pitch":60},{"start":6,"length":1,"pitch":62}]}),
    );
    let id = clip["id"].as_str().unwrap().to_string();
    call(
        &mut host,
        "clip.resize",
        json!({"clipId":id,"lengthBars":1}),
    );
    call(&mut host, "clip.legato", json!({"clipId":id}));
    let notes = call(&mut host, "note.list", json!({"clipId":id}));
    assert_eq!(notes[0]["length"], 4.0, "held to the region end");
    assert_eq!(notes[1]["start"], 6.0);
    assert_eq!(notes[1]["length"], 1.0);
    let name: String = "音".repeat(41);
    call(&mut host, "take.create", json!({ "name": name }));
    let lanes = json!([{"steps":4,"pulses":4,"rotation":0,"pitch":36,"velocity":100}]);
    call(
        &mut host,
        "rhythm.create",
        json!({ "lanes": lanes, "bars": 1, "name": "ドラム".repeat(40) }),
    );
}

/// Every save and export went through a temporary file created owner-only, so a re-saved
/// song or an exported mix became unreadable to other accounts (0600), and a symlinked
/// destination was replaced by a plain file.
#[cfg(unix)]
#[test]
fn saved_and_exported_files_keep_ordinary_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let mut host = Headless::new();
    let song = dir.path().join("song.ryolune");
    call(&mut host, "session.save", json!({ "path": song }));
    assert_eq!(mode(&song), 0o644);
    std::fs::set_permissions(&song, std::fs::Permissions::from_mode(0o640)).unwrap();
    call(&mut host, "session.rename", json!({"name":"Again"}));
    call(&mut host, "session.save", json!({}));
    assert_eq!(mode(&song), 0o640, "a re-save keeps the file's permissions");
    let midi = dir.path().join("song.mid");
    call(&mut host, "session.exportMidi", json!({ "path": midi }));
    assert_eq!(mode(&midi), 0o644);
    let target = dir.path().join("real.ryolune");
    let link = dir.path().join("link.ryolune");
    std::fs::write(&target, "").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    ryolune_engine::document::atomic_write(&link, |f| {
        use std::io::Write;
        f.write_all(b"new").map_err(|e| e.to_string())
    })
    .unwrap();
    assert!(link.is_symlink(), "the link stays a link");
    assert_eq!(std::fs::read(&target).unwrap(), b"new");
}

/// Clips are placed in bars and automation in beats: changing 4/4 to 3/4 moved every clip
/// but left the automation where it was, so a fade written for bar 5 landed in bar 6.
#[test]
fn a_new_meter_keeps_automation_on_its_bars() {
    let mut host = Headless::new();
    let track = midi_track(&mut host);
    call(
        &mut host,
        "clip.create",
        json!({"trackId":track,"startBar":4,"lengthBars":1}),
    );
    call(
        &mut host,
        "automation.create",
        json!({"target":"trackVolume","trackId":track,
               "points":[{"beat":16,"value":0.2},{"beat":20,"value":0.9}]}),
    );
    call(
        &mut host,
        "transport.setTimeSignature",
        json!({"numerator":3,"denominator":4}),
    );
    let lanes = call(&mut host, "automation.list", json!({}));
    let beats: Vec<f64> = lanes["lanes"][0]["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["beat"].as_f64().unwrap())
        .collect();
    // Bar 4 starts at beat 12 in 3/4: the point written at the clip's start stays there.
    assert_eq!(beats, [12.0, 15.0]);
    // One undo restores both the meter and the points.
    call(&mut host, "history.undo", json!({}));
    let lanes = call(&mut host, "automation.list", json!({}));
    assert_eq!(lanes["lanes"][0]["points"][0]["beat"], 16.0);
    assert_eq!(
        call(&mut host, "session.info", json!({}))["transport"]["timeSignature"]["numerator"],
        4
    );
}

/// Exporting MIDI at 90 BPM and importing it with its tempo set the song to 89.99995 BPM.
#[test]
fn a_midi_round_trip_keeps_the_tempo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("song.mid");
    let mut host = Headless::new();
    call(&mut host, "transport.setTempo", json!({"bpm": 90}));
    let track = midi_track(&mut host);
    call(
        &mut host,
        "clip.create",
        json!({"trackId":track,"startBar":0,"lengthBars":1,
               "notes":[{"start":0,"length":1,"pitch":60}]}),
    );
    call(&mut host, "session.exportMidi", json!({ "path": path }));
    let mut other = Headless::new();
    let report = call(
        &mut other,
        "session.importMidi",
        json!({ "path": path, "importTempo": true }),
    );
    assert_eq!(report["fileTempo"], 90.0);
    assert_eq!(
        call(&mut other, "session.info", json!({}))["transport"]["tempo"],
        90.0
    );
}

/// Stock instruments left no headroom: the Four Floor loop on the Drum Machine peaked at
/// +3 dBFS on its own, and a five-note E-piano chord at +5 dBFS, at unity gain.
#[test]
fn stock_loops_and_chords_do_not_clip_at_unity() {
    let dir = tempfile::tempdir().unwrap();
    let mut host = Headless::new();
    let loops = call(&mut host, "session.catalog", json!({}))["loops"].clone();
    for (index, l) in loops.as_array().unwrap().iter().enumerate() {
        let name = l["name"].as_str().unwrap();
        let track = call(
            &mut host,
            "track.add",
            json!({"kind":"midi","name":name,"instrument":l["instrument"]}),
        )["id"]
            .as_str()
            .unwrap()
            .to_string();
        call(
            &mut host,
            "clip.addLoop",
            json!({"trackId":track,"name":name,"startBar":0}),
        );
        let solo = call(
            &mut host,
            "session.exportStems",
            json!({"directory": dir.path().join(format!("stems-{index}")), "trackIds":[track], "includeMaster": false}),
        );
        let peak = solo["files"][0]["peak"].as_f64().unwrap();
        assert!(peak < 0.9, "{name} peaks at {peak}");
    }
    let keys = call(
        &mut host,
        "track.add",
        json!({"kind":"midi","name":"Chord","instrument":"E-Piano Mk I"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let notes: Vec<Value> = [48, 55, 60, 64, 67]
        .iter()
        .map(|p| json!({"start":0,"length":4,"pitch":p,"velocity":110}))
        .collect();
    call(
        &mut host,
        "clip.create",
        json!({"trackId":keys,"startBar":0,"lengthBars":1,"notes":notes}),
    );
    let chord = call(
        &mut host,
        "session.exportStems",
        json!({"directory": dir.path().join("chord"), "trackIds":[keys], "includeMaster": false}),
    );
    let peak = chord["files"][0]["peak"].as_f64().unwrap();
    assert!(peak < 1.0, "the chord peaks at {peak}");
}

/// `session.importMidi importTempo=true` changed the meter but left automation on its old
/// beats, so it slid off the clips it was written against.
#[test]
fn importing_midi_with_its_meter_keeps_automation_on_its_bars() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("waltz.mid");
    let mut waltz = Headless::new();
    call(
        &mut waltz,
        "transport.setTimeSignature",
        json!({"numerator":3,"denominator":4}),
    );
    let track = midi_track(&mut waltz);
    call(
        &mut waltz,
        "clip.create",
        json!({"trackId":track,"startBar":0,"lengthBars":1,
               "notes":[{"start":0,"length":1,"pitch":60}]}),
    );
    call(&mut waltz, "session.exportMidi", json!({ "path": path }));

    let mut host = Headless::new();
    let track = midi_track(&mut host);
    call(
        &mut host,
        "clip.create",
        json!({"trackId":track,"startBar":4,"lengthBars":1}),
    );
    call(
        &mut host,
        "automation.create",
        json!({"target":"trackVolume","trackId":track,
               "points":[{"beat":16,"value":0.2},{"beat":20,"value":0.9}]}),
    );
    call(
        &mut host,
        "session.importMidi",
        json!({ "path": path, "importTempo": true }),
    );
    let info = call(&mut host, "session.info", json!({}));
    assert_eq!(info["transport"]["timeSignature"]["numerator"], 3);
    let beats = |host: &mut Headless| -> Vec<f64> {
        call(host, "automation.list", json!({}))["lanes"][0]["points"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["beat"].as_f64().unwrap())
            .collect()
    };
    // Bar 4 starts at beat 12 in 3/4: the point written at the clip's start stays there.
    assert_eq!(beats(&mut host), [12.0, 15.0]);
    // One undo takes back the import, the meter and the moved points together.
    call(&mut host, "history.undo", json!({}));
    assert_eq!(beats(&mut host), [16.0, 20.0]);
    assert_eq!(
        call(&mut host, "session.info", json!({}))["transport"]["timeSignature"]["numerator"],
        4
    );
}

/// A short Ogg Vorbis file, whose only audio page is also its last, decoded a few
/// milliseconds past its true end: symphonia read the last packet's overrun as a start
/// delay and kept it.
#[test]
fn ogg_imports_end_on_the_exported_frame() {
    use ryolune_engine::{audio, export, store};
    let session = store::demo();
    let mut library = audio::Library::new();
    audio::prepare_sources(&session, &mut library).unwrap();
    let dir = tempfile::tempdir().unwrap();
    for end_beat in [0.5, 1.37, 4.0] {
        let options = export::ExportOptions {
            end_beat: Some(end_beat),
            tail_seconds: 0.0,
            format: export::SampleFormat::Float32,
            ..Default::default()
        };
        let (wav, ogg) = (dir.path().join("mix.wav"), dir.path().join("mix.ogg"));
        export::mix(&session, &library, &wav, &options).unwrap();
        let report = export::mix(&session, &library, &ogg, &options).unwrap();
        let decoded = audio::decode(std::fs::read(&ogg).unwrap(), Some("ogg")).unwrap();
        assert_eq!(
            decoded.frames.len() as u64,
            report.frames,
            "{end_beat} beats"
        );
        // What was cut is the tail: the start still lines up with the mix.
        let original = audio::decode(std::fs::read(&wav).unwrap(), Some("wav")).unwrap();
        let (mut signal, mut error) = (0f64, 0f64);
        for (a, b) in original.frames.iter().zip(&decoded.frames) {
            for c in 0..2 {
                signal += (a[c] as f64).powi(2);
                error += (a[c] as f64 - b[c] as f64).powi(2);
            }
        }
        let snr = 10.0 * (signal / error.max(1e-12)).log10();
        assert!(snr > 10.0, "{end_beat} beats: {snr:.1} dB");
    }
}

/// An undone `session.importAudio` still counted toward the 1 GiB decoded-audio budget: the
/// library keeps the buffer so a redo can bring it back, and the budget summed the library.
#[test]
fn undone_audio_imports_do_not_count_toward_the_import_budget() {
    use ryolune_engine::audio::{self, AudioBuffer};
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("loop.wav");
    let mut host = Headless::new();
    call(
        &mut host,
        "clip.addLoop",
        json!({"name":"Four Floor 124","startBar":0}),
    );
    call(&mut host, "session.bounce", json!({ "path": wav }));
    let mut host = Headless::new();
    let imported = call(&mut host, "session.importAudio", json!({ "path": wav }));
    let source = imported["source"]["id"].as_str().unwrap().to_string();
    // Stand in a buffer that fills the whole budget for the imported one. Zeroed pages are
    // never touched, so this costs no real memory.
    let full = AudioBuffer {
        sample_rate: 48000,
        frames: vec![[0.0; 2]; audio::MAX_LIBRARY_BYTES / 8],
        peaks: Vec::new(),
    };
    host.library.insert(source.clone(), Arc::new(full));
    let error = fail(&mut host, "session.importAudio", json!({ "path": wav }));
    assert!(error.contains("1 GiB"), "{error}");
    call(&mut host, "history.undo", json!({}));
    assert!(host.store.session().sources.is_empty());
    assert!(host.library.contains_key(&source), "kept for redo");
    assert_eq!(audio::session_bytes(host.store.session(), &host.library), 0);
    call(&mut host, "session.importAudio", json!({ "path": wav }));
    assert_eq!(host.store.session().sources.len(), 1);
}

/// A new song was meant to start with Drums on the Drum Machine and Bass on Analog Bass, but
/// track strips only exist once edited, so both tracks still played the default synth.
#[test]
fn a_new_song_starts_with_drums_and_bass_instruments() {
    let mut host = Headless::new();
    let tracks = call(&mut host, "track.list", json!({}));
    let instrument = |name: &str| {
        tracks
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("no {name} track"))["instrument"]
            .clone()
    };
    assert_eq!(instrument("Drums"), "Drum Machine");
    assert_eq!(instrument("Bass"), "Analog Bass");
    assert_eq!(instrument("Vocals"), Value::Null);
}

/// Agents write `value="-6 dB"`; the refusal now names the parameter that takes text.
#[test]
fn a_number_given_as_text_points_at_the_text_parameter() {
    let mut host = Headless::new();
    let err = fail(
        &mut host,
        "strip.setParameter",
        json!({"trackId":"Drums","parameter":"Level","value":"-6 dB"}),
    );
    assert!(err.contains("use `text`"), "{err}");
    let err = fail(&mut host, "transport.setTempo", json!({"tempo":"fast"}));
    assert!(!err.contains("text"), "{err}");
}

/// Lanes were named "drums · Parameter 3"; they now say what they automate.
#[test]
fn automation_lanes_get_readable_names() {
    let mut host = Headless::new();
    let volume = call(
        &mut host,
        "automation.create",
        json!({"target":"trackVolume","trackId":"Bass"}),
    );
    assert_eq!(volume["lane"]["name"], "Bass · Volume");
    let level = call(
        &mut host,
        "automation.create",
        json!({"target":"pluginParameter","trackId":"Drums","parameter":"Level"}),
    );
    assert_eq!(level["lane"]["name"], "Drums · Drum Machine · Level");
}

/// The agent's conversations belong to a song by its id: a file without one (every file before
/// it existed) gets the same id each time it is opened from the same place, keeps it once saved,
/// and restoring a creative take saved before the song had an id does not lose it.
#[test]
fn a_song_keeps_one_id_across_opening_saving_and_takes() {
    use ryolune_engine::{audio::Library, document, store};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Old song.ryolune");
    let mut old = serde_json::to_value(store::demo()).unwrap();
    old.as_object_mut().unwrap().remove("id");
    let file = json!({"format":"ryolune-session","version":1,"session":old,"audio":{}});
    std::fs::write(&path, file.to_string()).unwrap();
    let (first, _) = document::load(&path).unwrap();
    let (again, _) = document::load(&path).unwrap();
    assert!(first.id.starts_with("file-"));
    assert_eq!(
        first.id, again.id,
        "the same file finds the same conversations"
    );
    // Saved, the id is in the file and survives a move.
    document::save(&first, &Library::new(), &path).unwrap();
    let moved = dir.path().join("Moved.ryolune");
    std::fs::rename(&path, &moved).unwrap();
    let (loaded, _) = document::load(&moved).unwrap();
    assert_eq!(loaded.id, first.id);
    // An empty id is not written, so such files read and write as before.
    let mut fresh = store::empty();
    assert!(fresh.id.is_empty());
    assert!(serde_json::to_value(&fresh).unwrap().get("id").is_none());
    fresh.ensure_id();
    assert_eq!(fresh.id.len(), 36);
    // Takes made while the song had no id.
    let mut host = Headless::new();
    host.store.load(store::demo()).unwrap();
    let original = call(&mut host, "take.create", json!({"name":"Original"}));
    call(&mut host, "take.create", json!({"name":"Variation"}));
    host.store.amend(|s| s.id = "song-1".into()).unwrap();
    let takes = call(&mut host, "take.list", json!({}));
    let id = takes["takes"][0]["id"].as_str().unwrap().to_string();
    assert!(original.is_object());
    call(&mut host, "take.select", json!({"id": id}));
    assert_eq!(host.store.session().id, "song-1");
}

/// Exports to a folder that did not exist yet failed with "No such file or directory", and an
/// agent (eval `export-stems`, 0.16) has no other command that makes folders.
#[test]
fn exports_make_the_folders_they_are_given() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    let mut host = Headless::new();
    let track = midi_track(&mut host);
    call(
        &mut host,
        "clip.create",
        json!({"trackId":track,"startBar":0,"lengthBars":1,"notes":[{"start":0,"length":1,"pitch":60}]}),
    );
    call(
        &mut host,
        "session.exportAudio",
        json!({"path": out.join("mix.wav"), "sampleRate": 44100, "format": "pcm16", "tailSeconds": 0}),
    );
    assert!(out.join("mix.wav").is_file());
    let stems = call(
        &mut host,
        "session.exportStems",
        json!({"directory": out.join("deeper").join("stems"), "tailSeconds": 0}),
    );
    assert!(!stems["files"].as_array().unwrap().is_empty(), "{stems}");
    call(
        &mut host,
        "session.exportMidi",
        json!({"path": out.join("midi").join("song.mid")}),
    );
    assert!(out.join("midi").join("song.mid").is_file());
    // Stems still refuse a folder that already exists.
    assert!(fail(
        &mut host,
        "session.exportStems",
        json!({"directory": out.join("deeper").join("stems")}),
    )
    .contains("exist"));
}
