use ryolune_engine::control::{self, wire, Headless, Host, COMMANDS};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

fn call(host: &mut dyn Host, name: &str, params: Value) -> Value {
    control::call(host, name, &params, false).unwrap_or_else(|e| panic!("{name} {params}: {e}"))
}
fn fail(host: &mut dyn Host, name: &str, params: Value) -> String {
    control::call(host, name, &params, false)
        .err()
        .unwrap_or_else(|| panic!("{name} {params} should fail"))
}

#[test]
fn registry_is_unique_introspectable_and_mcp_safe() {
    let mut names = HashSet::new();
    for spec in COMMANDS.iter() {
        assert!(names.insert(spec.name), "duplicate {}", spec.name);
        let (family, action) = spec.name.split_once('.').expect("family.action");
        assert!(!family.is_empty() && !action.is_empty() && !action.contains('.'));
        assert!(
            !spec.name.contains('_'),
            "{} must round-trip to an MCP tool name",
            spec.name
        );
        let tool = spec.name.replacen('.', "_", 1);
        assert_eq!(tool.replacen('_', ".", 1), spec.name);
        assert!(tool.len() <= 64 && tool.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        let schema = control::schema(spec);
        assert_eq!(schema["type"], "object");
        let mut params = HashSet::new();
        for p in spec.params {
            assert!(params.insert(p.name), "{} repeats {}", spec.name, p.name);
            assert!(
                schema["properties"][p.name]["type"].is_string() || p.kind == control::Kind::Any,
                "{} {}",
                spec.name,
                p.name
            );
            assert_eq!(
                schema["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(p.name)),
                p.required
            );
        }
        assert!(!spec.doc.is_empty());
    }
    assert_eq!(
        control::describe().as_array().unwrap().len(),
        COMMANDS.len()
    );
}

#[test]
fn plugin_discovery_is_filtered_paged_and_stock_catalog_stays_small() {
    let mut host = Headless::new();
    let catalog = call(&mut host, "session.catalog", json!({}));
    assert_eq!(catalog["plugins"].as_array().unwrap().len(), 35);
    assert!(catalog["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .all(|plugin| plugin["format"] == "stock"));
    assert!(
        serde_json::to_vec(&catalog).unwrap().len() < 12000,
        "a stock sound lookup must not dump installed third-party libraries"
    );
    let first = call(
        &mut host,
        "plugin.list",
        json!({"format":"stock","kind":"instrument","limit":3}),
    );
    assert_eq!(first["total"], 12);
    assert_eq!(first["plugins"].as_array().unwrap().len(), 3);
    assert_eq!(first["nextOffset"], 3);
    let last = call(
        &mut host,
        "plugin.list",
        json!({"format":"stock","kind":"instrument","limit":3,"offset":9}),
    );
    assert_eq!(last["plugins"].as_array().unwrap().len(), 3);
    assert!(last["nextOffset"].is_null());
    let found = call(
        &mut host,
        "plugin.list",
        json!({"format":"stock","query":"PIANO"}),
    );
    // The named plugin first; other Keys instruments follow, since the folder is for pianos.
    assert_eq!(found["plugins"][0]["id"], "stock:E-Piano Mk I");
    assert!(found["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["folder"] == "Keys"));
    for params in [
        json!({"limit":0}),
        json!({"limit":201}),
        json!({"offset":-1}),
        json!({"format":"aax"}),
        json!({"kind":"random"}),
    ] {
        assert!(!fail(&mut host, "plugin.list", params).is_empty());
    }
}

#[test]
fn compact_session_inspection_excludes_plugin_payloads_and_only_expands_notes_on_request() {
    let mut host = Headless::new();
    let track = host
        .store
        .session()
        .tracks
        .iter()
        .find(|track| track.kind == "midi")
        .unwrap()
        .id
        .clone();
    call(
        &mut host,
        "clip.create",
        json!({"trackId":track,"startBar":0,"lengthBars":1,"notes":[{"start":0,"length":1,"pitch":60}]}),
    );
    call(
        &mut host,
        "strip.setPlugin",
        json!({"trackId":track,"slot":0,"pluginId":"stock:Utility"}),
    );
    host.store
        .amend(|session| {
            session.strips.get_mut(&track).unwrap().inserts[0].blob = "YWJj".repeat(100000)
        })
        .unwrap();
    let summary = call(&mut host, "session.inspect", json!({}));
    assert_eq!(summary["clips"][0]["noteCount"], 1);
    assert!(summary["clips"][0].get("data").is_none());
    assert_eq!(
        summary["strips"][&track]["inserts"][0]["hasSavedState"],
        true
    );
    assert!(summary["strips"][&track]["inserts"][0]
        .get("blob")
        .is_none());
    assert!(serde_json::to_vec(&summary).unwrap().len() < 20000);
    let expanded = call(&mut host, "session.inspect", json!({"includeNotes":true}));
    assert_eq!(expanded["clips"][0]["data"]["notes"][0]["pitch"], 60);
    let complete = call(&mut host, "session.get", json!({}));
    assert_eq!(
        complete["strips"][&track]["inserts"][0]["blob"]
            .as_str()
            .unwrap()
            .len(),
        400000
    );
}

#[test]
fn every_registered_command_is_implemented() {
    let mut host = Headless::new();
    for spec in COMMANDS.iter() {
        // Scanner tests use isolated fixtures; never probe the user's installed plugins here.
        if spec.name == "plugin.scan" {
            continue;
        }
        let err = control::call(&mut host, spec.name, &json!({}), false).err();
        assert!(
            err.as_deref()
                .is_none_or(|e| !e.contains("not implemented")),
            "{}: {err:?}",
            spec.name
        );
    }
    let err = fail(&mut host, "track.ad", json!({}));
    assert!(err.contains("track.add"), "{err}");
}

#[test]
fn parity_commands_cover_view_regions_tracks_inserts_presets_and_settings() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("RYOLUNE_DATA_DIR", dir.path());
    std::env::set_var("RYOLUNE_SETTINGS", dir.path().join("settings.json"));
    let mut host = Headless::new();
    let track = call(&mut host, "track.add", json!({"kind":"midi","name":"Keys"}))["id"].clone();
    let clip = call(
        &mut host,
        "clip.create",
        json!({"trackId":track,"startBar":0,"lengthBars":1,"notes":[
            {"start":0.1,"length":0.9,"pitch":60},{"start":1.45,"length":1.0,"pitch":64},{"start":3.9,"length":0.5,"pitch":127}]}),
    )["id"]
        .clone();
    let quantized = call(
        &mut host,
        "clip.quantize",
        json!({"clipId":clip,"division":4,"lengths":true}),
    );
    assert_eq!(quantized["changedNotes"], 3);
    let notes = call(&mut host, "note.list", json!({"clipId":clip}));
    assert_eq!(notes[0]["start"], 0.0);
    assert_eq!(notes[1]["start"], 1.0);
    assert_eq!(notes[2]["start"], 3.0, "notes stay inside the clip");
    assert_eq!(notes[2]["length"], 1.0);
    call(
        &mut host,
        "clip.transpose",
        json!({"clipId":clip,"semitones":5}),
    );
    let notes = call(&mut host, "note.list", json!({"clipId":clip}));
    assert_eq!(notes[0]["pitch"], 65);
    assert_eq!(notes[2]["pitch"], 127, "pitches clamp at the top");
    assert!(fail(
        &mut host,
        "clip.transpose",
        json!({"clipId":clip,"semitones":99})
    )
    .contains("-48"));
    call(&mut host, "history.undo", json!({"steps":2}));
    let notes = call(&mut host, "note.list", json!({"clipId":clip}));
    assert_eq!(notes[0]["start"], 0.1);
    call(
        &mut host,
        "strip.setPlugin",
        json!({"trackId":track,"slot":0,"pluginId":"stock:Space"}),
    );
    call(
        &mut host,
        "strip.setPlugin",
        json!({"trackId":track,"slot":1,"pluginId":"stock:Echo"}),
    );
    let moved = call(
        &mut host,
        "strip.moveInsert",
        json!({"trackId":track,"from":1,"to":0}),
    );
    assert_eq!(moved["inserts"][0]["effect"], "Echo");
    assert_eq!(moved["inserts"][1]["effect"], "Space");
    let copy = call(&mut host, "track.duplicate", json!({"trackId":track}));
    assert_eq!(copy["name"], "Keys copy");
    assert_eq!(copy["index"], 4, "the copy sits right after the original");
    assert_eq!(copy["clipCount"], 1);
    let original_strip = call(&mut host, "strip.get", json!({"trackId":track}));
    let copied_strip = call(&mut host, "strip.get", json!({"trackId":copy["id"]}));
    assert_eq!(copied_strip["inserts"][0]["effect"], "Echo");
    assert_ne!(
        copied_strip["inserts"][0]["id"],
        original_strip["inserts"][0]["id"]
    );
    call(
        &mut host,
        "clip.select",
        json!({"clipId":clip,"noteId":notes[1]["id"]}),
    );
    assert_eq!(
        host.store.session().view.selected_note_id.as_deref(),
        notes[1]["id"].as_str()
    );
    let view = call(
        &mut host,
        "view.set",
        json!({"pixelsPerBar":96,"scrollBar":2,"editorMode":"step","editorClipId":clip}),
    );
    assert_eq!(view["pixelsPerBar"], 96.0);
    assert_eq!(view["editorMode"], "step");
    assert_eq!(view["editorClipId"], clip);
    assert!(fail(&mut host, "view.set", json!({"editorMode":"drums"})).contains("pianoRoll"));
    let described = call(
        &mut host,
        "plugin.describe",
        json!({"pluginId":"stock:Space"}),
    );
    assert_eq!(described["parameters"].as_array().unwrap().len(), 5);
    assert!(described["presets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p == "Cathedral"));
    let loaded = call(
        &mut host,
        "preset.load",
        json!({"trackId":track,"slot":1,"name":"Cathedral"}),
    );
    assert_eq!(loaded["preset"], "Cathedral");
    assert_eq!(loaded["inserts"][1]["params"]["0"], 95.0);
    assert!(fail(
        &mut host,
        "preset.load",
        json!({"trackId":track,"slot":0,"name":"Cathedral"})
    )
    .contains("No preset"));
    let saved = call(
        &mut host,
        "preset.save",
        json!({"trackId":track,"slot":1,"name":"Bigger hall"}),
    );
    assert_eq!(saved["preset"]["params"]["0"], 95.0);
    assert!(
        call(&mut host, "preset.list", json!({"pluginId":"stock:Space"}))["presets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "Bigger hall" && p["factory"] == false)
    );
    call(
        &mut host,
        "preset.delete",
        json!({"pluginId":"stock:Space","name":"Bigger hall"}),
    );
    assert!(fail(
        &mut host,
        "preset.delete",
        json!({"pluginId":"stock:Space","name":"Cathedral"})
    )
    .contains("Factory"));
    let set = call(
        &mut host,
        "settings.set",
        json!({"path":"agent.provider","value":"anthropic"}),
    );
    assert_eq!(set["value"], "anthropic");
    call(
        &mut host,
        "settings.set",
        json!({"path":"agent.anthropicApiKey","value":"sk-ant-1234567890"}),
    );
    let shown = call(&mut host, "settings.get", json!({"path":"agent"}));
    assert_eq!(shown["anthropicApiKey"], "••••7890");
    assert_eq!(shown["provider"], "anthropic");
    assert!(fail(
        &mut host,
        "settings.set",
        json!({"path":"agent.bogus","value":1})
    )
    .contains("Unknown setting"));
    call(&mut host, "settings.reset", json!({}));
    assert_eq!(
        call(&mut host, "settings.get", json!({"path":"agent.provider"})),
        json!("codex")
    );
    let info = call(&mut host, "app.info", json!({}));
    assert_eq!(info["mode"], "headless");
    assert!(info["commands"].as_u64().unwrap() > 100);
    assert!(call(&mut host, "session.snapshots", json!({}))["snapshots"].is_array());
    let devices = call(&mut host, "audio.devices", json!({}));
    assert!(devices["outputs"].is_array());
    let err = fail(&mut host, "ui.screenshot", json!({}));
    assert!(err.contains("needs the running ryolune app"), "{err}");
    assert!(fail(&mut host, "agent.send", json!({"prompt":"hi"})).contains("live mode"));
    std::env::remove_var("RYOLUNE_SETTINGS");
    std::env::remove_var("RYOLUNE_DATA_DIR");
}

#[test]
fn agent_permissions_gate_dangerous_commands() {
    use ryolune_engine::{control_app, settings::Permissions};
    let strict = Permissions {
        file_operations: false,
        transport: false,
        replace_session: false,
        settings: false,
        app_control: false,
        generation: false,
        plugins: false,
    };
    for name in [
        "session.save",
        "transport.play",
        "session.new",
        "settings.set",
        "app.quit",
        "generate.audio",
        "generate.delete",
    ] {
        assert!(
            control_app::denied_for_agent(name, &strict).is_some(),
            "{name}"
        );
    }
    for name in [
        "track.add",
        "clip.setNotes",
        "session.info",
        "view.set",
        "preset.load",
    ] {
        assert!(
            control_app::denied_for_agent(name, &strict).is_none(),
            "{name}"
        );
    }
    assert!(control_app::denied_for_agent("session.save", &Permissions::default()).is_none());
    assert!(control_app::denied_for_agent("session.new", &Permissions::default()).is_some());
}

#[test]
fn parameters_are_validated_before_the_store_changes() {
    let mut host = Headless::new();
    let revision = host.store.revision;
    let err = fail(
        &mut host,
        "track.add",
        json!({ "kind": "midi", "colour": "#ff0000" }),
    );
    assert!(
        err.contains("Unknown parameter `colour`") && err.contains("color"),
        "{err}"
    );
    let err = fail(&mut host, "track.add", json!({}));
    assert!(err.contains("needs `kind`"), "{err}");
    let err = fail(&mut host, "transport.setTempo", json!({ "bpm": "fast" }));
    assert!(err.contains("must be a number"), "{err}");
    let err = fail(&mut host, "transport.setTempo", json!({ "bpm": 1000 }));
    assert!(err.contains("20 and 400"), "{err}");
    let err = fail(
        &mut host,
        "track.add",
        json!({ "kind": "midi", "color": "red" }),
    );
    assert!(err.contains("#rrggbb"), "{err}");
    let err = fail(
        &mut host,
        "track.add",
        json!({ "kind": "audio", "instrument": "Riser" }),
    );
    assert!(err.contains("Only MIDI tracks"), "{err}");
    assert_eq!(
        host.store.revision, revision,
        "rejected commands leave no trace"
    );
    assert!(!host.store.dirty() && !host.store.can_undo());
}

#[test]
fn headless_host_builds_a_song_with_one_history() {
    let mut host = Headless::new();
    let info = call(&mut host, "session.info", json!({}));
    assert_eq!(info["mode"], "headless");
    assert_eq!(info["trackCount"], 3);
    let bass = call(
        &mut host,
        "track.add",
        json!({ "kind": "midi", "name": "Bass", "instrument": "Sub Bass 808" }),
    );
    assert_eq!(bass["instrument"], "Sub Bass 808");
    assert_eq!(bass["color"], control::TRACK_PALETTE[3]);
    let id = bass["id"].as_str().unwrap().to_string();
    let clip = call(
        &mut host,
        "clip.create",
        json!({
            "trackId": id, "startBar": 0, "lengthBars": 2,
            "notes": [
                { "start": 0, "length": 1, "pitch": 36 },
                { "start": 2, "length": 1, "pitch": 43, "velocity": 90 }
            ]
        }),
    );
    assert_eq!(clip["noteCount"], 2);
    assert_eq!(clip["name"], "Bass 1");
    let clip_id = clip["id"].as_str().unwrap().to_string();
    let note = call(
        &mut host,
        "note.add",
        json!({ "clipId": clip_id, "start": 4, "length": 0.5, "pitch": 48 }),
    );
    assert_eq!(note["velocity"], 100);
    assert_eq!(
        call(&mut host, "note.list", json!({ "clipId": clip_id }))
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let err = fail(
        &mut host,
        "note.add",
        json!({ "clipId": clip_id, "start": 0, "length": 1, "pitch": 200 }),
    );
    assert!(err.contains("0 and 127"), "{err}");
    let updated = call(
        &mut host,
        "note.update",
        json!({ "clipId": clip_id, "noteId": note["id"], "pitch": 50, "velocity": 64 }),
    );
    assert_eq!(
        (updated["pitch"].as_u64(), updated["velocity"].as_u64()),
        (Some(50), Some(64))
    );
    let undone = call(&mut host, "history.undo", json!({}));
    assert_eq!(undone["applied"], true);
    let notes = call(&mut host, "note.list", json!({ "clipId": clip_id }));
    assert_eq!(
        notes[2]["pitch"], 48,
        "undo reverts exactly the last command"
    );
    call(&mut host, "history.undo", json!({}));
    assert_eq!(
        call(&mut host, "clip.get", json!({ "clipId": clip_id }))["data"]["notes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let split = call(
        &mut host,
        "clip.split",
        json!({ "clipId": clip_id, "bar": 1 }),
    );
    assert_eq!(split["left"]["lengthBars"], 1.0);
    assert_eq!(split["right"]["startBar"], 1.0);
    assert_eq!(
        call(&mut host, "clip.list", json!({ "trackId": id }))
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let strip = call(
        &mut host,
        "strip.setInsert",
        json!({ "trackId": id, "slot": 1, "effect": "Space", "bypassed": true }),
    );
    assert_eq!(strip["inserts"][1]["state"], "bypassed");
    let strip = call(
        &mut host,
        "strip.setSendLevel",
        json!({ "trackId": id, "send": 0, "levelDb": -12 }),
    );
    assert_eq!(strip["sends"][0]["levelDb"], -12.0);
    let t = call(
        &mut host,
        "track.setVolume",
        json!({ "trackId": id, "volume": 0.5 }),
    );
    assert_eq!(t["volume"], 0.5);
    let err = fail(
        &mut host,
        "track.setVolume",
        json!({ "trackId": id, "volume": 2 }),
    );
    assert!(err.contains("0.0 and 1.0"), "{err}");
    let err = fail(&mut host, "transport.play", json!({}));
    assert!(err.contains("bounce"), "{err}");
    let t = call(&mut host, "transport.locate", json!({ "bar": 3 }));
    assert_eq!(t["positionBeats"], 12.0);
    assert_eq!(
        call(&mut host, "session.catalog", json!({}))["loops"]
            .as_array()
            .unwrap()
            .len(),
        9
    );
}

#[test]
fn file_mode_round_trips_audio_and_renders() {
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("song.ryolune");
    let mix = dir.path().join("mix.wav");
    let mut host = Headless::new();
    call(&mut host, "transport.setTempo", json!({ "bpm": 124 }));
    let drums = call(
        &mut host,
        "clip.addLoop",
        json!({ "name": "Four Floor 124", "startBar": 0 }),
    );
    assert_eq!(drums["kind"], "midi");
    let saved = call(&mut host, "session.save", json!({ "path": song }));
    assert_eq!(saved["dirty"], false);
    let mut host = Headless::open(&song).unwrap();
    assert_eq!(host.path.as_deref(), Some(song.as_path()));
    assert_eq!(host.store.session().transport.tempo, 124.0);
    let bounced = call(&mut host, "session.bounce", json!({ "path": mix }));
    assert!(bounced["seconds"].as_f64().unwrap() > 3.0);
    assert!(std::fs::metadata(&mix).unwrap().len() > 44);
    let imported = call(
        &mut host,
        "session.importAudio",
        json!({ "path": mix, "startBar": 4 }),
    );
    assert_eq!(imported["clip"]["kind"], "audio");
    assert_eq!(imported["clip"]["startBar"], 4.0);
    let tracks = call(&mut host, "track.list", json!({}));
    assert!(tracks
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["kind"] == "audio" && t["clipCount"] == 1));
    assert!(host.store.dirty());
    call(&mut host, "session.save", json!({}));
    let reopened = Headless::open(&song).unwrap();
    assert_eq!(reopened.store.session().sources.len(), 1);
    assert_eq!(reopened.library.len(), 1, "embedded audio comes back");
    let err = fail(&mut Headless::new(), "session.save", json!({}));
    assert!(err.contains("pass `path`"), "{err}");
}

#[test]
fn live_socket_serves_commands_in_order_with_auth() {
    let dir = tempfile::tempdir().unwrap();
    let discovery = dir.path().join("control.json");
    let server = wire::Server::start_at(discovery.clone(), || {}).unwrap();
    let port = server.port();
    let stop = Arc::new(AtomicBool::new(false));
    let worker = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut host = Headless::new();
            while !stop.load(Ordering::Relaxed) {
                if let Some(request) = server.recv_timeout(Duration::from_millis(20)) {
                    wire::serve(&mut host, request);
                }
            }
            host.store.session().clips.len()
        })
    };
    let mut client = wire::Client::connect_at(&discovery).unwrap();
    assert_eq!(client.pid, std::process::id());
    let info = client.call("session.info", &json!({}), false).unwrap();
    assert_eq!(info["mode"], "headless");
    let track = client
        .call("track.add", &json!({ "kind": "midi" }), false)
        .unwrap();
    let clip = client
        .call(
            "clip.create",
            &json!({ "trackId": track["id"], "startBar": 0, "lengthBars": 1, "notes": [{ "start": 0, "length": 1, "pitch": 60 }] }),
            true,
        )
        .unwrap();
    assert_eq!(clip["agent"], true, "agent flag travels with the request");
    let full = client
        .call("clip.get", &json!({ "clipId": clip["id"] }), false)
        .unwrap();
    assert_eq!(full["data"]["notes"][0]["agent"], true);
    let err = client
        .call("clip.get", &json!({ "clipId": "nope" }), false)
        .unwrap_err();
    assert!(err.contains("Unknown clip"), "{err}");
    let err = client.call("bogus", &json!({}), false).unwrap_err();
    assert!(err.contains("Unknown command"), "{err}");

    // A second client without the token is refused before any command runs.
    let mut raw = TcpStream::connect(("127.0.0.1", port)).unwrap();
    writeln!(raw, r#"{{"jsonrpc":"2.0","id":1,"method":"track.list"}}"#).unwrap();
    let mut line = String::new();
    BufReader::new(raw.try_clone().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.contains("Authenticate first"), "{line}");
    let mut raw = TcpStream::connect(("127.0.0.1", port)).unwrap();
    writeln!(
        raw,
        r#"{{"jsonrpc":"2.0","id":1,"method":"auth","params":{{"token":"wrong"}}}}"#
    )
    .unwrap();
    let mut line = String::new();
    BufReader::new(raw).read_line(&mut line).unwrap();
    assert!(line.contains("Invalid control token"), "{line}");

    stop.store(true, Ordering::Relaxed);
    assert_eq!(worker.join().unwrap(), 1);
    assert!(
        !discovery.exists(),
        "dropping the server removes its discovery file"
    );
    assert!(wire::Client::connect_at(&discovery).is_err());
}

#[test]
fn plugin_routing_parameters_state_and_master_round_trip() {
    let mut host = Headless::new();
    let track = call(&mut host, "track.add", json!({"kind":"midi"}))["id"].clone();
    call(
        &mut host,
        "track.setArmed",
        json!({"trackId":track,"armed":true}),
    );
    call(
        &mut host,
        "strip.setPlugin",
        json!({"trackId":track,"pluginId":"stock:Glass Keys"}),
    );
    call(
        &mut host,
        "strip.setPlugin",
        json!({"trackId":track,"slot":7,"pluginId":"stock:Space"}),
    );
    let metadata = call(
        &mut host,
        "strip.parameters",
        json!({"trackId":track,"slot":7}),
    );
    let parameter = metadata["parameters"][0].clone();
    let value = parameter["min"].as_f64().unwrap();
    call(
        &mut host,
        "strip.setParameter",
        json!({"trackId":track,"slot":7,"parameterId":parameter["id"],"value":value}),
    );
    let state = call(
        &mut host,
        "strip.getState",
        json!({"trackId":track,"slot":7}),
    );
    assert_eq!(state["params"][parameter["id"].to_string()], value);
    let revision = host.store.revision;
    assert!(fail(&mut host, "strip.setParameter", json!({"trackId":track,"slot":7,"parameterId":parameter["id"],"value":parameter["max"].as_f64().unwrap()+1.0})).contains("between"));
    assert_eq!(host.store.revision, revision);
    call(
        &mut host,
        "strip.setBypass",
        json!({"trackId":track,"slot":7,"bypassed":true}),
    );
    let bypassed = call(
        &mut host,
        "strip.getState",
        json!({"trackId":track,"slot":7}),
    );
    assert_eq!(bypassed["id"], state["id"]);
    assert_eq!(bypassed["params"], state["params"]);
    call(&mut host, "history.undo", json!({}));
    assert_eq!(
        call(
            &mut host,
            "strip.getState",
            json!({"trackId":track,"slot":7})
        )["state"],
        "active"
    );
    for bus in ["master", "bus-a", "bus-b"] {
        call(
            &mut host,
            "strip.setInsert",
            json!({"trackId":bus,"slot":7,"effect":"Space"}),
        );
        assert_eq!(
            call(&mut host, "strip.get", json!({"trackId":bus}))["inserts"][7]["pluginId"],
            "stock:Space"
        );
        assert!(fail(
            &mut host,
            "strip.setSendLevel",
            json!({"trackId":bus,"send":0,"levelDb":-6})
        )
        .contains("feedback"));
    }
    call(&mut host, "master.setVolume", json!({"volume":0.5}));
    assert_eq!(host.store.session().master_volume, 0.5);
    call(
        &mut host,
        "strip.setInstrument",
        json!({"trackId":track,"instrument":"ryolune Synth"}),
    );
    assert!(host.store.session().strips[track.as_str().unwrap()]
        .synth
        .is_none());
    let mut instance = ryolune_engine::host::instantiate("stock:Space", "Space", 48000).unwrap();
    let blob = ryolune_engine::host::encode_blob(&instance.editor.save().unwrap());
    call(
        &mut host,
        "strip.setState",
        json!({"trackId":track,"slot":7,"blob":blob}),
    );
    let saved = call(
        &mut host,
        "strip.getState",
        json!({"trackId":track,"slot":7}),
    );
    assert_eq!(saved["blob"], blob);
    assert!(saved.get("params").is_none());
    assert!(fail(
        &mut host,
        "strip.setState",
        json!({"trackId":track,"slot":7,"blob":"invalid!"})
    )
    .contains("Invalid plugin state"));
}

#[test]
fn live_wire_limits_clients_and_keeps_pending_requests_until_completion() {
    let dir = tempfile::tempdir().unwrap();
    let discovery = dir.path().join("control.json");
    let server = wire::Server::start_at(discovery.clone(), || {}).unwrap();
    let clients: Vec<_> = (0..16)
        .map(|_| wire::Client::connect_at(&discovery).unwrap())
        .collect();
    assert!(
        wire::Client::connect_at(&discovery).is_err(),
        "excess live clients are refused without allocating another worker"
    );
    drop(clients);
    let mut client = None;
    for _ in 0..100 {
        if let Ok(connection) = wire::Client::connect_at(&discovery) {
            client = Some(connection);
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut client = client.expect("closed connections release capacity");
    let (send, receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        send.send(client.call(
            "session.rename",
            &json!({"name":"Acknowledged after completion"}),
            false,
        ))
        .unwrap();
    });
    let request = server.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        receive.recv_timeout(Duration::from_millis(250)).is_err(),
        "no optimistic result while store work is pending"
    );
    let mut host = Headless::new();
    wire::serve(&mut host, request);
    assert_eq!(
        receive
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap()["name"],
        "Acknowledged after completion"
    );
    worker.join().unwrap();
}

#[test]
fn dropping_live_server_closes_authenticated_idle_sockets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("control.json");
    let server = wire::Server::start_at(path.clone(), || {}).unwrap();
    let discovery = wire::read_discovery(&path).unwrap();
    let mut socket = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    writeln!(
        socket,
        "{}",
        json!({"jsonrpc":"2.0","id":0,"method":"auth","params":{"token":discovery.token}})
    )
    .unwrap();
    let mut reader = BufReader::new(socket);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.contains("result"));
    drop(server);
    line.clear();
    assert_eq!(
        reader.read_line(&mut line).unwrap(),
        0,
        "shutdown wakes idle connection workers immediately"
    );
}

#[test]
fn plugin_library_files_plugins_in_sound_folders() {
    let mut host = Headless::new();
    let drums = call(&mut host, "plugin.list", json!({"folder":"drums"}));
    assert_eq!(drums["plugins"][0]["name"], "Drum Machine");
    assert_eq!(drums["plugins"][0]["folder"], "Drums");
    let folders = call(&mut host, "plugin.folders", json!({}));
    let names: Vec<&str> = folders["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    for expected in [
        "Synths",
        "Keys",
        "Bass",
        "Dynamics",
        "Space & Time",
        "Pitch",
    ] {
        assert!(
            names.contains(&expected),
            "{expected} missing from {names:?}"
        );
    }
}

#[test]
fn monitoring_is_a_track_setting_that_saves_undoes_and_stays_out_of_old_files() {
    fn monitor_of(host: &mut Headless, id: &Value) -> Value {
        call(host, "track.list", json!({}))
            .as_array()
            .unwrap()
            .iter()
            .find(|t| &t["id"] == id)
            .unwrap()["monitor"]
            .clone()
    }
    let mut host = Headless::new();
    let audio = call(&mut host, "track.add", json!({"kind":"audio"}))["id"].clone();
    let midi = call(&mut host, "track.add", json!({"kind":"midi"}))["id"].clone();
    assert_eq!(monitor_of(&mut host, &audio), "off");
    let saved_off = serde_json::to_value(host.store().session()).unwrap();
    assert!(
        saved_off["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t.get("monitor").is_none()),
        "off is the absence of the field, so files without monitoring are unchanged"
    );
    for mode in ["auto", "on", "off"] {
        let track = call(
            &mut host,
            "track.setMonitor",
            json!({"trackId":audio,"monitor":mode}),
        );
        assert_eq!(track["monitor"], mode);
    }
    call(
        &mut host,
        "track.setMonitor",
        json!({"trackId":audio,"monitor":"auto"}),
    );
    let text = serde_json::to_string(host.store().session()).unwrap();
    let reloaded: ryolune_engine::model::Session = serde_json::from_str(&text).unwrap();
    let id = audio.as_str().unwrap();
    assert_eq!(
        reloaded.tracks.iter().find(|t| t.id == id).unwrap().monitor,
        ryolune_engine::model::Monitor::Auto
    );
    call(&mut host, "history.undo", json!({}));
    assert_eq!(monitor_of(&mut host, &audio), "off");
    assert!(fail(
        &mut host,
        "track.setMonitor",
        json!({"trackId":audio,"monitor":"loud"})
    )
    .contains("off, auto or on"));
    assert!(fail(
        &mut host,
        "track.setMonitor",
        json!({"trackId":midi,"monitor":"on"})
    )
    .contains("Only audio tracks"));
}

#[test]
fn the_browser_the_piano_roll_and_zoom_to_fit_are_view_commands() {
    let mut host = Headless::new();
    let view = call(&mut host, "view.get", json!({}));
    assert_eq!(view["browserTab"], "instruments");
    assert_eq!(view["browserSelection"], "E-Piano Mk I");
    assert_eq!(
        view["editorLowPitch"],
        Value::Null,
        "the piano roll frames the clip itself"
    );
    assert!(view["laneWidth"].as_f64().unwrap() > 0.0);
    let saved = serde_json::to_value(host.store().session()).unwrap();
    assert!(
        saved["view"].get("editorLowPitch").is_none(),
        "automatic stays out of the file"
    );
    let view = call(
        &mut host,
        "view.set",
        json!({"browserTab":"plugins","browserSelection":"Channel EQ","editorLowPitch":36,"laneWidth":1000}),
    );
    assert_eq!(view["browserTab"], "plugins");
    assert_eq!(view["browserSelection"], "Channel EQ");
    assert_eq!(view["editorLowPitch"], 36);
    assert_eq!(view["laneWidth"], 1000.0);
    let cleared = call(
        &mut host,
        "view.set",
        json!({"browserSelection":"","editorLowPitch":-1}),
    );
    assert_eq!(cleared["browserSelection"], Value::Null);
    assert_eq!(cleared["editorLowPitch"], Value::Null);
    assert!(fail(&mut host, "view.set", json!({"browserTab":"presets"})).contains("instruments"));
    assert!(fail(&mut host, "view.set", json!({"editorLowPitch":120})).contains("0 and 108"));
    assert!(fail(&mut host, "view.set", json!({"laneWidth":5})).contains("laneWidth"));
    // The view is not document history.
    assert_eq!(call(&mut host, "history.info", json!({}))["canUndo"], false);

    // Fit: the song plus one bar across the lane, from the first bar; never beyond the zoom range.
    let track = call(&mut host, "track.add", json!({"kind":"midi"}))["id"].clone();
    call(
        &mut host,
        "clip.create",
        json!({"trackId":track,"startBar":20,"lengthBars":4}),
    );
    call(&mut host, "view.set", json!({"scrollBar":7}));
    let fit = call(&mut host, "view.fit", json!({}));
    assert_eq!(fit["scrollBar"], 0.0);
    assert_eq!(
        fit["pixelsPerBar"], 40.0,
        "1000 px over 24 bars and one spare"
    );
    call(&mut host, "view.set", json!({"laneWidth":20000}));
    assert_eq!(
        call(&mut host, "view.fit", json!({}))["pixelsPerBar"],
        480.0
    );
}

#[test]
fn the_clipboard_lives_in_the_host_so_any_client_can_paste() {
    let mut host = Headless::new();
    let midi = call(&mut host, "track.add", json!({"kind":"midi"}))["id"].clone();
    let other = call(&mut host, "track.add", json!({"kind":"midi"}))["id"].clone();
    let audio = call(&mut host, "track.add", json!({"kind":"audio"}))["id"].clone();
    assert!(fail(&mut host, "clip.paste", json!({})).contains("Nothing has been copied"));
    let clip = call(
        &mut host,
        "clip.create",
        json!({"trackId":midi,"startBar":2,"lengthBars":2,"name":"Riff",
               "notes":[{"start":0,"length":1,"pitch":60},{"start":1,"length":1,"pitch":64}]}),
    );
    let id = clip["id"].clone();
    // Copy: by id, or the selected clip.
    assert!(fail(&mut host, "clip.copy", json!({})).contains("Select a clip"));
    let copied = call(&mut host, "clip.copy", json!({"clipId":id}));
    assert_eq!(copied["copied"]["name"], "Riff");
    // Paste with nothing said: the selected track when its kind fits, at the playhead.
    call(&mut host, "track.select", json!({"trackId":other}));
    call(&mut host, "transport.locate", json!({"bar":8}));
    let pasted = call(&mut host, "clip.paste", json!({}));
    assert_eq!(pasted["trackId"], other);
    assert_eq!(pasted["startBar"], 8.0);
    assert_eq!(pasted["noteCount"], 2);
    assert_ne!(pasted["id"], id);
    // Pasting twice gives two independent clips with their own note ids.
    let again = call(&mut host, "clip.paste", json!({"trackId":midi,"bar":12}));
    let notes = |host: &mut Headless, id: &Value| {
        call(host, "clip.get", json!({"clipId":id}))["data"]["notes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let (a, b) = (
        notes(&mut host, &pasted["id"]),
        notes(&mut host, &again["id"]),
    );
    assert!(a.iter().all(|n| !b.contains(n)));
    // A MIDI clip does not go on an audio track; with an audio track selected it goes home.
    assert!(fail(&mut host, "clip.paste", json!({"trackId":audio})).contains("MIDI"));
    call(&mut host, "track.select", json!({"trackId":audio}));
    assert_eq!(
        call(&mut host, "clip.paste", json!({"bar":30}))["trackId"],
        midi
    );
    // Cut removes the clip in one undo step and keeps it on the clipboard.
    let before = call(&mut host, "clip.list", json!({}))
        .as_array()
        .unwrap()
        .len();
    call(&mut host, "clip.cut", json!({"clipId":id}));
    assert_eq!(
        call(&mut host, "clip.list", json!({}))
            .as_array()
            .unwrap()
            .len(),
        before - 1
    );
    assert_eq!(
        call(&mut host, "clip.paste", json!({"bar":40}))["name"],
        "Riff"
    );
    call(&mut host, "history.undo", json!({}));
    call(&mut host, "history.undo", json!({}));
    assert_eq!(
        call(&mut host, "clip.list", json!({}))
            .as_array()
            .unwrap()
            .len(),
        before
    );
}

#[test]
fn controller_points_are_edited_like_notes_and_follow_every_clip_edit() {
    let mut host = Headless::new();
    let track = call(&mut host, "track.add", json!({"kind":"midi"}))["id"].clone();
    let clip = call(
        &mut host,
        "clip.create",
        json!({"trackId":track,"startBar":0,"lengthBars":2}),
    )["id"]
        .clone();
    let plain = serde_json::to_value(host.store().session()).unwrap();
    let data = &plain["clips"].as_array().unwrap()[0]["data"];
    assert!(
        data.get("controllers").is_none(),
        "A clip without controllers is written exactly as before"
    );

    let mod_low = call(
        &mut host,
        "controller.add",
        json!({"clipId":clip,"kind":"cc","number":1,"time":0,"value":20}),
    );
    assert_eq!(mod_low["controllerCount"], 1);
    let mod_id = mod_low["controller"]["id"].clone();
    call(
        &mut host,
        "controller.add",
        json!({"clipId":clip,"kind":"cc","number":1,"time":6,"value":90}),
    );
    call(
        &mut host,
        "controller.add",
        json!({"clipId":clip,"kind":"bend","time":2,"value":4096}),
    );
    // Same lane, same time: the value changes, no second point.
    let again = call(
        &mut host,
        "controller.add",
        json!({"clipId":clip,"kind":"bend","time":2,"value":-4096}),
    );
    assert_eq!(again["controllerCount"], 3);
    let pedal = call(
        &mut host,
        "controller.add",
        json!({"clipId":clip,"kind":"cc","number":64,"time":1,"value":127}),
    )["controller"]["id"]
        .clone();
    let listed = call(&mut host, "controller.list", json!({"clipId":clip}));
    assert_eq!(listed["controllers"].as_array().unwrap().len(), 4);
    assert_eq!(listed["lanes"].as_array().unwrap().len(), 3);
    let times: Vec<f64> = listed["controllers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["time"].as_f64().unwrap())
        .collect();
    assert!(times.windows(2).all(|w| w[0] <= w[1]), "{times:?}");
    let bends = call(
        &mut host,
        "controller.list",
        json!({"clipId":clip,"kind":"bend"}),
    );
    assert_eq!(bends["controllers"][0]["value"], -4096);

    call(
        &mut host,
        "controller.update",
        json!({"clipId":clip,"controllerId":mod_id,"value":30,"time":0.5}),
    );
    call(
        &mut host,
        "controller.remove",
        json!({"clipId":clip,"controllerId":pedal}),
    );
    let before_draw = host.store().undo_depth();
    let drawn = call(
        &mut host,
        "controller.setPoints",
        json!({"clipId":clip,"kind":"cc","number":11,"from":2,"to":4,
            "points":[{"time":2,"value":10},{"time":3,"value":60},{"time":3,"value":70}]}),
    );
    assert_eq!(drawn["controller"]["count"], 2);
    assert_eq!(
        host.store().undo_depth(),
        before_draw + 1,
        "One drawn curve is one undo step"
    );
    call(&mut host, "history.undo", json!({}));
    assert!(call(
        &mut host,
        "controller.list",
        json!({"clipId":clip,"kind":"cc","number":11})
    )["controllers"]
        .as_array()
        .unwrap()
        .is_empty());
    call(&mut host, "history.redo", json!({}));

    // Validation leaves the clip alone.
    for (params, message) in [
        (
            json!({"clipId":clip,"kind":"cc","time":0,"value":1}),
            "needs `number`",
        ),
        (
            json!({"clipId":clip,"kind":"cc","number":1,"time":0,"value":128}),
            "0 to 127",
        ),
        (
            json!({"clipId":clip,"kind":"bend","time":0,"value":9000}),
            "-8192 to 8191",
        ),
        (
            json!({"clipId":clip,"kind":"bend","number":3,"time":0,"value":0}),
            "takes no `number`",
        ),
        (
            json!({"clipId":clip,"kind":"cc","number":1,"time":8,"value":1}),
            "before the clip's end",
        ),
        (
            json!({"clipId":clip,"kind":"cc","number":123,"time":0,"value":1}),
            "0-119",
        ),
        (
            json!({"clipId":clip,"kind":"wheel","time":0,"value":1}),
            "cc, bend, pressure or poly",
        ),
    ] {
        let error = fail(&mut host, "controller.add", params.clone());
        assert!(error.contains(message), "{params}: {error}");
    }

    // Agents' points are marked, as their notes are.
    let by_agent = control::call(
        &mut host,
        "controller.add",
        &json!({"clipId":clip,"kind":"pressure","time":7,"value":50}),
        true,
    )
    .unwrap();
    assert_eq!(by_agent["controller"]["agent"], true);

    // The file carries them and reads them back.
    let text = serde_json::to_string(host.store().session()).unwrap();
    let reloaded: ryolune_engine::model::Session = serde_json::from_str(&text).unwrap();
    assert_eq!(
        serde_json::to_value(&reloaded).unwrap()["clips"],
        serde_json::to_value(host.store().session()).unwrap()["clips"]
    );

    // Split at bar 1 (beat 4): the right half starts with the mod wheel at 30 and the bend
    // at -4096, the values in force at the cut, and keeps its own later points.
    let halves = call(&mut host, "clip.split", json!({"clipId":clip,"bar":1}));
    let right = halves["right"]["id"].clone();
    let points = call(&mut host, "controller.list", json!({"clipId":right}))["controllers"].clone();
    let summary: Vec<(String, f64, i64)> = points
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                format!("{}{}", p["kind"].as_str().unwrap(), p["number"]),
                p["time"].as_f64().unwrap(),
                p["value"].as_i64().unwrap(),
            )
        })
        .collect();
    assert!(summary.contains(&("cc1".into(), 0.0, 30)), "{summary:?}");
    assert!(
        summary.contains(&("bendnull".into(), 0.0, -4096)),
        "{summary:?}"
    );
    assert!(summary.contains(&("cc1".into(), 2.0, 90)), "{summary:?}");
    assert!(
        summary.contains(&("pressurenull".into(), 3.0, 50)),
        "{summary:?}"
    );
    let left = halves["left"]["id"].clone();
    let left_points =
        call(&mut host, "controller.list", json!({"clipId":left}))["controllers"].clone();
    assert!(left_points
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["time"].as_f64().unwrap() < 4.0));

    // Trimming the right half's left edge by two beats keeps positions and chases again.
    call(
        &mut host,
        "clip.trim",
        json!({"clipId":right,"startBar":1.5}),
    );
    let trimmed =
        call(&mut host, "controller.list", json!({"clipId":right}))["controllers"].clone();
    let mod_wheel: Vec<(f64, i64)> = trimmed
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["number"] == 1)
        .map(|p| (p["time"].as_f64().unwrap(), p["value"].as_i64().unwrap()))
        .collect();
    assert_eq!(mod_wheel, vec![(0.0, 90)]);

    // Duplicates get their own ids; transpose leaves controllers alone.
    let copy = call(&mut host, "clip.duplicate", json!({"clipId":right}))["id"].clone();
    let original_ids: HashSet<String> = trimmed
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect();
    let copied = call(&mut host, "controller.list", json!({"clipId":copy}))["controllers"].clone();
    assert_eq!(copied.as_array().unwrap().len(), original_ids.len());
    assert!(copied
        .as_array()
        .unwrap()
        .iter()
        .all(|p| !original_ids.contains(p["id"].as_str().unwrap())));
    call(
        &mut host,
        "clip.transpose",
        json!({"clipId":copy,"semitones":5}),
    );
    let after = call(&mut host, "controller.list", json!({"clipId":copy}))["controllers"].clone();
    assert_eq!(after, copied);
    assert!(fail(&mut host, "controller.list", json!({"clipId":"nope"})).contains("Unknown clip"));
}

#[test]
fn controller_lanes_are_per_midi_channel() {
    let mut host = Headless::new();
    let track = call(&mut host, "track.add", json!({"kind":"midi"}))["id"].clone();
    let clip = call(
        &mut host,
        "clip.create",
        json!({"trackId":track,"startBar":0,"lengthBars":1}),
    )["id"]
        .clone();
    call(
        &mut host,
        "controller.add",
        json!({"clipId":clip,"kind":"cc","number":1,"time":0,"value":20}),
    );
    let other = call(
        &mut host,
        "controller.add",
        json!({"clipId":clip,"kind":"cc","number":1,"channel":5,"time":0,"value":90}),
    );
    assert_eq!(
        other["controllerCount"], 2,
        "The same controller at the same time on another channel is another point"
    );
    assert_eq!(other["controller"]["channel"], 5);
    let listed = call(&mut host, "controller.list", json!({"clipId":clip}));
    let lanes = listed["lanes"].as_array().unwrap();
    assert_eq!(lanes.len(), 2);
    assert!(lanes[0].get("channel").is_none());
    assert_eq!(lanes[1]["channel"], 5);
    let only = call(
        &mut host,
        "controller.list",
        json!({"clipId":clip,"channel":5}),
    );
    assert_eq!(only["controllers"].as_array().unwrap().len(), 1);
    assert_eq!(only["controllers"][0]["value"], 90);
    // Replacing channel 5's lane leaves channel 0's alone.
    call(
        &mut host,
        "controller.setPoints",
        json!({"clipId":clip,"kind":"cc","number":1,"channel":5,"points":[]}),
    );
    let left = call(&mut host, "controller.list", json!({"clipId":clip}));
    assert_eq!(left["controllers"].as_array().unwrap().len(), 1);
    assert_eq!(left["controllers"][0]["value"], 20);
    let error = fail(
        &mut host,
        "controller.add",
        json!({"clipId":clip,"kind":"bend","channel":16,"time":0,"value":0}),
    );
    assert!(error.contains("0-15"), "{error}");
}

#[test]
fn polyphonic_pressure_is_a_lane_per_key() {
    let mut host = Headless::new();
    let track = call(&mut host, "track.add", json!({"kind":"midi"}))["id"].clone();
    let clip = call(
        &mut host,
        "clip.create",
        json!({"trackId":track,"startBar":0,"lengthBars":1}),
    )["id"]
        .clone();
    for key in [60, 64] {
        call(
            &mut host,
            "controller.add",
            json!({"clipId":clip,"kind":"poly","number":key,"time":0,"value":50}),
        );
    }
    let listed = call(&mut host, "controller.list", json!({"clipId":clip}));
    let lanes = listed["lanes"].as_array().unwrap();
    assert_eq!(lanes.len(), 2);
    assert_eq!(lanes[0]["kind"], "poly");
    assert_eq!(lanes[0]["name"], "Poly Pressure (key 60)");
    let session = serde_json::to_value(host.store().session()).unwrap();
    let data = &session["clips"].as_array().unwrap().last().unwrap()["data"];
    assert!(data.get("controllers").is_none());
    assert_eq!(data["polyPressure"].as_array().unwrap().len(), 2);
    let error = fail(
        &mut host,
        "controller.add",
        json!({"clipId":clip,"kind":"poly","time":0,"value":1}),
    );
    assert!(error.contains("the key 0-127"), "{error}");
}
