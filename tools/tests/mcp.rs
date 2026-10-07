use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
};

struct Mcp {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
}
impl Mcp {
    fn start(dir: &Path, args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ryolune-mcp"))
            .args(args)
            .env("RYOLUNE_CONTROL", dir.join("absent-control.json"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let reader = BufReader::new(child.stdout.take().unwrap());
        Self { child, reader }
    }
    fn notify(&mut self, method: &str) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{}", json!({ "jsonrpc": "2.0", "method": method })).unwrap();
    }
    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(
            stdin,
            "{}",
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
        )
        .unwrap();
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line}"));
        assert_eq!(reply["id"], id);
        reply
    }
    fn tool(&mut self, id: u64, name: &str, arguments: Value) -> Value {
        self.request(
            id,
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )["result"]
            .clone()
    }
}
impl Drop for Mcp {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.wait();
    }
}

#[test]
fn stdio_server_speaks_mcp_over_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("song.ryolune");
    let mut mcp = Mcp::start(dir.path(), &["--file", song.to_str().unwrap()]);
    let init = mcp.request(
        1,
        "initialize",
        json!({ "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }),
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    assert!(init["result"]["instructions"]
        .as_str()
        .unwrap()
        .contains("Headless"));
    mcp.notify("notifications/initialized");
    assert_eq!(mcp.request(2, "ping", json!({}))["result"], json!({}));

    let tools = mcp.request(3, "tools/list", json!({}))["result"]["tools"].clone();
    let names: Vec<&str> = tools
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"track_add") && names.contains(&"clip_setNotes"),
        "{names:?}"
    );
    let add = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "track_add")
        .unwrap();
    assert_eq!(add["inputSchema"]["required"], json!(["kind"]));
    assert_eq!(add["annotations"]["readOnlyHint"], false);

    let track = mcp.tool(
        4,
        "track_add",
        json!({ "kind": "midi", "name": "Keys", "instrument": "Glass Keys" }),
    );
    assert_eq!(track["isError"], false);
    assert_eq!(track["structuredContent"]["name"], "Keys");
    assert!(track["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("saved"));
    let id = track["structuredContent"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let clip = mcp.tool(
        5,
        "clip_create",
        json!({ "trackId": id, "startBar": 0, "lengthBars": 1, "notes": [{ "start": 0, "length": 4, "pitch": 64 }] }),
    );
    assert_eq!(
        clip["structuredContent"]["agent"], true,
        "MCP edits are marked as agent-made"
    );

    let bad = mcp.tool(6, "track_add", json!({ "kind": "drums" }));
    assert_eq!(bad["isError"], true);
    assert!(bad["content"][0]["text"].as_str().unwrap().contains("midi"));

    let unknown = mcp.request(
        7,
        "tools/call",
        json!({ "name": "track_explode", "arguments": {} }),
    );
    assert_eq!(unknown["error"]["code"], -32602);

    let info = mcp.request(
        8,
        "resources/read",
        json!({ "uri": "ryolune://session/info" }),
    );
    let text = info["result"]["contents"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed["clipCount"], 1);
    assert_eq!(
        parsed["dirty"], false,
        "file mode saved after the last tool call"
    );

    let missing = mcp.request(9, "nonsense/method", json!({}));
    assert_eq!(missing["error"]["code"], -32601);
    drop(mcp);
    assert!(song.exists());
}

#[test]
fn without_the_app_the_server_falls_back_to_headless() {
    let dir = tempfile::tempdir().unwrap();
    let mut mcp = Mcp::start(dir.path(), &[]);
    let init = mcp.request(1, "initialize", json!({ "protocolVersion": "2099-01-01" }));
    assert_eq!(
        init["result"]["protocolVersion"], "2025-06-18",
        "unknown versions get the newest supported"
    );
    assert!(init["result"]["instructions"]
        .as_str()
        .unwrap()
        .contains("in memory"));
    let info = mcp.tool(2, "session_info", json!({}));
    assert_eq!(info["structuredContent"]["mode"], "headless");
    let play = mcp.tool(3, "transport_play", json!({}));
    assert_eq!(play["isError"], true);
}

#[test]
fn mcp_composes_routes_renders_and_reopens_a_complete_song() {
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("song.ryolune");
    let wav = dir.path().join("song.wav");
    let mut mcp = Mcp::start(dir.path(), &["--file", song.to_str().unwrap()]);
    let mut id = 0;
    let mut run = |name: &str, params: Value| -> Value {
        id += 1;
        let reply = mcp.tool(id, name, params);
        assert_eq!(reply["isError"], false, "{name}: {reply}");
        reply["structuredContent"].clone()
    };
    run("session_new", json!({}));
    run("session_rename", json!({"name":"MCP complete song"}));
    let keys = run(
        "track_add",
        json!({"kind":"midi","instrument":"Glass Keys"}),
    )["id"]
        .clone();
    let clip = run(
        "clip_create",
        json!({"trackId":keys,"startBar":0,"lengthBars":1,"notes":[{"start":0,"length":3,"pitch":60},{"start":0,"length":3,"pitch":64},{"start":0,"length":3,"pitch":67}]}),
    );
    run("clip_duplicate", json!({"clipId":clip["id"]}));
    run(
        "strip_setPlugin",
        json!({"trackId":"bus-a","slot":7,"pluginId":"stock:Space"}),
    );
    let meta = run("strip_parameters", json!({"trackId":"bus-a","slot":7}));
    run(
        "strip_setParameter",
        json!({"trackId":"bus-a","slot":7,"parameterId":meta["parameters"][0]["id"],"value":meta["parameters"][0]["default"]}),
    );
    run(
        "strip_setSendLevel",
        json!({"trackId":keys,"send":0,"levelDb":-12}),
    );
    run("master_setVolume", json!({"volume":0.65}));
    run("session_bounce", json!({"path":wav}));
    run("session_open", json!({"path":song}));
    let session = run("session_get", json!({}));
    assert_eq!(session["name"], "MCP complete song");
    assert_eq!(session["clips"].as_array().unwrap().len(), 2);
    assert_eq!(
        session["strips"]["bus-a"]["inserts"][7]["plugin"],
        "stock:Space"
    );
    drop(mcp);
    let audio = ryolune_engine::audio::decode(std::fs::read(wav).unwrap(), Some("wav")).unwrap();
    assert!(audio.frames.iter().flatten().all(|s| s.is_finite()));
    assert!(audio.frames.iter().flatten().any(|s| s.abs() > 0.01));
}

#[test]
fn mcp_reports_autosave_failure_and_preserves_memory_for_retry() {
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("song.ryolune");
    let mut mcp = Mcp::start(dir.path(), &["--file", song.to_str().unwrap()]);
    // Make the destination unwritable after the headless server has opened it.
    assert_eq!(mcp.tool(1, "session_info", json!({}))["isError"], false);
    std::fs::remove_file(&song).unwrap();
    std::fs::create_dir(&song).unwrap();
    let failed = mcp.tool(2, "session_rename", json!({"name":"Do not lose this"}));
    assert_eq!(failed["isError"], true);
    assert!(failed["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("autosave failed"));
    std::fs::remove_dir(&song).unwrap();
    assert_eq!(mcp.tool(3, "session_save", json!({}))["isError"], false);
    assert_eq!(
        ryolune_engine::document::load(&song).unwrap().0.name,
        "Do not lose this"
    );
}

#[test]
fn mcp_rejects_invalid_modes_and_jsonrpc_envelopes() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec!["--file"],
        vec!["--live", "--headless"],
        vec!["--file", "x.ryolune", "--live"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_ryolune-mcp"))
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
    let mut mcp = Mcp::start(dir.path(), &["--headless"]);
    writeln!(
        mcp.child.stdin.as_mut().unwrap(),
        r#"{{"id":1,"method":"tools/list"}}"#
    )
    .unwrap();
    let mut line = String::new();
    mcp.reader.read_line(&mut line).unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(reply["error"]["code"], -32600);
    assert_eq!(
        mcp.request(2, "tools/list", json!({"cursor":"invented"}))["error"]["code"],
        -32602
    );
    assert!(mcp.request(3, "tools/list", json!({}))["result"]["tools"].is_array());
}

#[test]
fn mcp_exposes_configured_export_and_atomic_stem_reports() {
    let dir = tempfile::tempdir().unwrap();
    let mut mcp = Mcp::start(dir.path(), &["--headless"]);
    let track = mcp.tool(
        1,
        "track_add",
        json!({"kind":"midi","instrument":"Glass Keys"}),
    )["structuredContent"]["id"]
        .clone();
    assert_eq!(mcp.tool(2,"clip_create",json!({"trackId":track,"startBar":0,"lengthBars":1,"notes":[{"start":0,"length":2,"pitch":67}]}))["isError"],false);
    let folder = dir.path().join("stems");
    let report=mcp.tool(3,"session_exportStems",json!({"directory":folder,"trackIds":[track],"sampleRate":96000,"format":"pcm16","startBeat":0,"endBeat":1,"tailSeconds":0,"includeEffects":false,"includeMaster":false,"dither":false}));
    assert_eq!(report["isError"], false, "{report}");
    assert_eq!(
        report["structuredContent"]["files"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(report["structuredContent"]["files"][0]["sampleRate"], 96000);
    assert!(folder.join("manifest.json").exists());
    assert_eq!(
        mcp.tool(4, "session_exportStems", json!({"directory":folder}))["isError"],
        true
    );
    let tools = mcp.request(5, "tools/list", json!({}));
    let names: Vec<_> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for name in [
        "session_importMidi",
        "session_exportMidi",
        "session_exportAudio",
        "session_exportStems",
    ] {
        assert!(names.contains(&name));
    }
}

#[test]
fn mcp_serves_prompts_resources_and_parity_tools() {
    let dir = tempfile::tempdir().unwrap();
    let mut mcp = Mcp::start(dir.path(), &["--headless"]);
    let init = mcp.request(1, "initialize", json!({ "protocolVersion": "2025-06-18" }));
    assert!(init["result"]["capabilities"]["prompts"].is_object());
    let prompts = mcp.request(2, "prompts/list", json!({}))["result"]["prompts"].clone();
    let names: Vec<&str> = prompts
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(&names[..3], ["compose", "mix-review", "see-the-window"]);
    // Every skill is a prompt too (lsuite's HARNESS.md part 2).
    assert!(
        names.contains(&"mixing") && names.contains(&"mastering"),
        "{names:?}"
    );
    let compose = mcp.request(
        3,
        "prompts/get",
        json!({ "name": "compose", "arguments": { "style": "boom bap", "bars": "16" } }),
    );
    let text = compose["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(text.contains("16 bars of boom bap") && text.contains("clip_create"));
    assert_eq!(
        mcp.request(4, "prompts/get", json!({ "name": "nope" }))["error"]["code"],
        -32602
    );
    let resources = mcp.request(5, "resources/list", json!({}))["result"]["resources"].clone();
    let uris: Vec<&str> = resources
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["uri"].as_str().unwrap())
        .collect();
    assert!(uris.contains(&"ryolune://settings") && uris.contains(&"ryolune://app"));
    let app = mcp.request(6, "resources/read", json!({ "uri": "ryolune://app" }));
    let parsed: Value =
        serde_json::from_str(app["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(parsed["mode"], "headless");
    let tools = mcp.request(7, "tools/list", json!({}))["result"]["tools"].clone();
    let by_name = |name: &str| {
        tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("{name} missing"))
    };
    assert_eq!(
        by_name("track_remove")["annotations"]["destructiveHint"],
        true
    );
    assert_eq!(
        by_name("track_setVolume")["annotations"]["destructiveHint"],
        false
    );
    assert!(
        by_name("settings_set")["inputSchema"]["properties"]["value"]
            .get("type")
            .is_none()
    );
    let view = mcp.tool(8, "view_set", json!({ "pixelsPerBar": 60 }));
    assert_eq!(view["structuredContent"]["pixelsPerBar"], 60.0);
    let screenshot = mcp.tool(9, "ui_screenshot", json!({}));
    assert_eq!(screenshot["isError"], true);
    assert!(screenshot["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("live mode"));
}

#[test]
fn mcp_starts_from_the_overview_and_takes_names_for_ids() {
    let dir = tempfile::tempdir().unwrap();
    let mut mcp = Mcp::start(dir.path(), &["--headless"]);
    let init = mcp.request(1, "initialize", json!({ "protocolVersion": "2025-06-18" }));
    assert!(init["result"]["instructions"]
        .as_str()
        .unwrap()
        .contains("**Orient.** `session.overview`"));
    let tools = mcp.request(2, "tools/list", json!({}))["result"]["tools"].clone();
    let overview = tools
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "session_overview")
        .cloned()
        .expect("session_overview is a tool");
    assert_eq!(overview["annotations"]["readOnlyHint"], true);
    assert!(overview["inputSchema"]["properties"]["maxClips"].is_object());
    for tool in [
        "ui_state",
        "strip_programs",
        "strip_setProgram",
        "strip_removeInsert",
    ] {
        assert!(
            tools.as_array().unwrap().iter().any(|t| t["name"] == tool),
            "{tool} missing"
        );
    }
    mcp.tool(3, "session_new", json!({ "demo": true }));
    let result = mcp.tool(4, "session_overview", json!({}));
    let o = &result["structuredContent"];
    assert_eq!(o["song"]["meter"], "4/4", "{result}");
    assert!(o["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"] == "Bass" && t["instrument"]["name"].is_string()));
    let muted = mcp.tool(
        5,
        "track_setMute",
        json!({ "trackId": "Bass", "muted": true }),
    );
    assert_eq!(muted["structuredContent"]["mute"], true, "{muted}");
    let wrong = mcp.tool(
        6,
        "track_setMute",
        json!({ "trackId": "Bas", "muted": true }),
    );
    assert_eq!(wrong["isError"], true);
    assert!(wrong["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Did you mean Bass?"));
    let set = mcp.tool(
        7,
        "strip_setParameter",
        json!({ "trackId": "Bass", "slot": 0, "parameter": "threshold", "text": "-24 dB" }),
    );
    assert_eq!(
        set["structuredContent"]["changed"][0]["value"], -24.0,
        "{set}"
    );
    let resource = mcp.request(
        8,
        "resources/read",
        json!({ "uri": "ryolune://session/overview" }),
    );
    let parsed: Value =
        serde_json::from_str(resource["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    let bass = parsed["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "Bass")
        .cloned()
        .unwrap();
    assert_eq!(bass["problems"][0], "muted", "{bass}");
}

#[test]
fn mcp_serves_the_harness_brief_skills_pictures_and_checkpoints() {
    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("song.ryolune");
    let mut mcp = Mcp::start(dir.path(), &["--file", song.to_str().unwrap()]);
    let init = mcp.request(1, "initialize", json!({ "protocolVersion": "2025-06-18" }));
    let instructions = init["result"]["instructions"].as_str().unwrap();
    assert!(instructions.contains("finish routine") && instructions.contains("harness.skill"));

    // Skills: a tool, a prompt with the request, and resources.
    let skills = mcp.tool(2, "harness_skills", json!({}));
    assert!(skills["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("mastering"));
    let prompt = mcp.request(
        3,
        "prompts/get",
        json!({ "name": "mastering", "arguments": { "request": "-14 LUFS please" } }),
    );
    let text = prompt["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(text.contains("-14 LUFS please") && text.contains("Limiter"));
    let resources = mcp.request(4, "resources/list", json!({}))["result"]["resources"].clone();
    assert!(resources
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["uri"] == "ryolune://skills/mixing"));
    let brief = mcp.request(5, "resources/read", json!({ "uri": "ryolune://brief" }));
    assert_eq!(brief["result"]["contents"][0]["mimeType"], "text/markdown");
    let skill = mcp.request(
        6,
        "resources/read",
        json!({ "uri": "ryolune://skills/drum-programming" }),
    );
    assert!(skill["result"]["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("36 kick"));

    // An edit: a checkpoint is taken before it, and the song's state follows the result.
    let made = mcp.tool(
        7,
        "clip_create",
        json!({ "trackId": "Drums", "startBar": 0, "lengthBars": 2,
                "notes": [{"start": 0, "length": 0.5, "pitch": 36}, {"start": 1, "length": 0.5, "pitch": 38}] }),
    );
    assert_eq!(made["isError"], false, "{made}");
    assert!(made["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Song now:"));
    let changes = mcp.tool(8, "harness_changes", json!({}));
    assert!(
        changes["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Added 1 clip"),
        "{changes}"
    );

    // Eyes: the picture comes back as MCP image content, and not as text.
    let look = mcp.tool(9, "harness_look", json!({ "fromBar": 0, "toBar": 2 }));
    assert_eq!(look["isError"], false, "{look}");
    let content = look["content"].as_array().unwrap();
    let image = content
        .iter()
        .find(|c| c["type"] == "image")
        .expect("an image block");
    assert_eq!(image["mimeType"], "image/png");
    assert!(image["data"].as_str().unwrap().len() > 1000);
    assert!(!content[0]["text"]
        .as_str()
        .unwrap()
        .contains(image["data"].as_str().unwrap()));
    assert!(
        look["structuredContent"]["loudness"]["integratedLufs"].is_number(),
        "{look}"
    );

    // One step back to before the edit.
    let reverted = mcp.tool(10, "harness_revert", json!({}));
    assert_eq!(
        reverted["structuredContent"]["reverted"], true,
        "{reverted}"
    );
    let clips = mcp.tool(11, "clip_list", json!({}));
    let text = clips["content"][0]["text"].as_str().unwrap();
    let listed: Value = serde_json::from_str(
        text.split("\n\n")
            .next()
            .unwrap()
            .split("\n(saved")
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(listed, json!([]), "{text}");
}
