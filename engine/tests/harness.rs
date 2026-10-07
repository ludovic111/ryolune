//! The agent harness through the registry (lsuite's HARNESS.md): brief and skills, live
//! context, eyes and ears (`harness.look`, `harness.measure`) and checkpoints.
use ryolune_engine::control::{self, Headless, Host};
use serde_json::{json, Value};

fn call(h: &mut dyn Host, name: &str, args: Value) -> Value {
    control::call(h, name, &args, false).unwrap_or_else(|e| panic!("{name}: {e}"))
}
fn demo() -> Headless {
    let mut h = Headless::new();
    call(&mut h, "session.new", json!({"demo": true}));
    h
}

#[test]
fn the_brief_and_skills_are_served_by_name() {
    let mut h = Headless::new();
    let brief = call(&mut h, "harness.brief", json!({}));
    let words = brief["words"].as_u64().unwrap();
    assert!((800..=1600).contains(&words), "{words} words");
    assert!(brief["markdown"]
        .as_str()
        .unwrap()
        .contains("## The finish routine"));
    let skills = call(&mut h, "harness.skills", json!({}));
    let names: Vec<&str> = skills
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!((8..=15).contains(&names.len()));
    for name in names {
        let skill = call(&mut h, "harness.skill", json!({ "name": name }));
        assert!(
            skill["markdown"].as_str().unwrap().contains("## Checks"),
            "{name}"
        );
        assert!(
            brief["markdown"]
                .as_str()
                .unwrap()
                .contains(&format!("`{name}`")),
            "the brief's index lists {name}"
        );
    }
    let missing =
        control::call(&mut h, "harness.skill", &json!({"name": "juggling"}), false).unwrap_err();
    assert!(missing.contains("mixing"), "{missing}");
}

#[test]
fn measure_reads_loudness_peaks_and_bands_of_a_range_and_each_track() {
    let mut h = demo();
    let m = call(
        &mut h,
        "harness.measure",
        json!({"fromBar": 0, "toBar": 2, "tracks": true, "targetLufs": -14}),
    );
    let l = &m["loudness"];
    let integrated = l["integratedLufs"]
        .as_f64()
        .expect("the demo is not silent");
    assert!((-40.0..0.0).contains(&integrated), "{m}");
    assert!(l["truePeakDbtp"].as_f64().unwrap() >= l["samplePeakDbfs"].as_f64().unwrap() - 0.05);
    assert!(m["spectrum"]["bandsDb"]["bass"].is_number());
    let tracks = m["tracks"].as_array().unwrap();
    assert!(tracks.len() >= 2 && tracks.iter().all(|t| t["track"].is_string()));
    assert!(
        m["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f.as_str().unwrap().contains("target")),
        "the demo is far from -14 LUFS: {m}"
    );
    // Hot enough to clip: the findings name the fix.
    call(&mut h, "master.setVolume", json!({"volume": 1.0}));
    for t in h.store.session().tracks.clone() {
        call(
            &mut h,
            "track.setVolume",
            json!({"trackId": t.id, "volume": 1.0}),
        );
    }
    let hot = call(&mut h, "harness.measure", json!({"fromBar": 0, "toBar": 2}));
    if hot["loudness"]["clippedSamples"].as_u64().unwrap() > 0 {
        assert!(
            hot["findings"][0].as_str().unwrap().contains("0 dBFS"),
            "{hot}"
        );
    }
    let wide = control::call(
        &mut h,
        "harness.measure",
        &json!({"fromBar": 0, "toBar": 4000}),
        false,
    );
    assert!(wide.unwrap_err().contains("600 seconds"));
}

#[test]
fn look_draws_a_picture_of_the_range_with_its_numbers() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = demo();
    let path = dir.path().join("look.png");
    let look = call(
        &mut h,
        "harness.look",
        json!({"fromBar": 0, "toBar": 2, "path": path, "targetLufs": -14}),
    );
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"\x89PNG"));
    assert_eq!(look["image"]["mimeType"], "image/png");
    assert!(look["image"]["width"].as_u64().unwrap() >= 1000);
    assert!(look["image"]["data"].as_str().unwrap().len() > 1000);
    assert!(look["loudness"]["integratedLufs"].is_number());
    assert!(look["notes"]["count"].as_u64().unwrap() > 0);
    // The piano roll alone renders nothing.
    let notes = call(
        &mut h,
        "harness.look",
        json!({"view": "notes", "path": path}),
    );
    assert!(notes["loudness"].is_null() && notes["image"]["height"].as_u64().unwrap() > 300);
    // One track, soloed.
    let first = h.store.session().tracks[0].id.clone();
    let solo = call(
        &mut h,
        "harness.look",
        json!({"trackId": first, "view": "mix", "toBar": 1, "path": path}),
    );
    assert!(solo["track"].is_string());
    // Never over the song's own file.
    let song = dir.path().join("song.ryolune");
    call(&mut h, "session.save", json!({"path": song}));
    let refused = control::call(
        &mut h,
        "harness.look",
        &json!({"view": "notes", "path": song.with_extension("png")}),
        false,
    );
    assert!(refused.is_ok(), "a .png beside the song is fine");
    let refused = control::call(&mut h, "harness.look", &json!({"path": song}), false);
    assert!(refused.is_err());
}

#[test]
fn a_checkpoint_reverts_a_whole_job_in_one_undo_step() {
    let mut h = demo();
    let tracks_before = h.store.session().tracks.len();
    let cp = call(&mut h, "harness.checkpoint", json!({"label": "Agent turn"}));
    assert_eq!(cp["label"], "Agent turn");
    call(&mut h, "transport.setTempo", json!({"bpm": 140}));
    call(
        &mut h,
        "track.add",
        json!({"kind": "midi", "name": "Lead", "instrument": "Glass Keys"}),
    );
    call(
        &mut h,
        "clip.create",
        json!({"trackId": "Lead", "startBar": 0, "lengthBars": 1, "notes": [{"start": 0, "length": 1, "pitch": 72}]}),
    );
    let changes = call(&mut h, "harness.changes", json!({}));
    let text = changes["changes"].to_string();
    assert!(text.contains("Tempo") && text.contains("Lead"), "{text}");
    let listed = call(&mut h, "harness.checkpoints", json!({}));
    assert_eq!(
        listed[0]["changes"].as_u64().unwrap(),
        changes["changes"].as_array().unwrap().len() as u64
    );

    let reverted = call(&mut h, "harness.revert", json!({"checkpoint": cp["id"]}));
    assert_eq!(reverted["reverted"], true);
    assert_eq!(h.store.session().tracks.len(), tracks_before);
    assert_ne!(h.store.session().transport.tempo, 140.0);
    // The revert is one step: undo brings the whole job back.
    call(&mut h, "history.undo", json!({}));
    assert_eq!(h.store.session().transport.tempo, 140.0);
    assert_eq!(h.store.session().tracks.len(), tracks_before + 1);
    // Opening another song forgets the checkpoints.
    call(&mut h, "session.new", json!({}));
    assert!(control::call(&mut h, "harness.revert", &json!({}), false).is_err());
}

#[test]
fn the_live_context_says_what_changed_since_the_agents_last_step() {
    let mut h = demo();
    let context = call(&mut h, "harness.context", json!({}));
    assert!(context["song"].as_str().unwrap().contains("BPM"));
    assert!(!context["tracks"].as_array().unwrap().is_empty());
    assert!(context.get("changedSinceYourLastStep").is_none());
    h.store.set_mark("agent");
    // The person edits while the agent thinks.
    let first = h.store.session().tracks[0].id.clone();
    call(
        &mut h,
        "track.setMute",
        json!({"trackId": first, "muted": true}),
    );
    let context = call(&mut h, "harness.context", json!({"key": "agent"}));
    let changed = context["changedSinceYourLastStep"].to_string();
    assert!(changed.contains("muted"), "{context}");
    let text = ryolune_engine::harness::context_text(&context);
    assert!(
        text.contains("Changed since your last step") && text.contains("MUTED"),
        "{text}"
    );
    // Another agent's mark is its own.
    let other = call(&mut h, "harness.context", json!({"key": "mcp"}));
    assert!(other.get("changedSinceYourLastStep").is_none());
}
