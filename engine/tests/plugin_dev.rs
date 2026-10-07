//! Plugins asked of an agent (lsuite's PLUGINS.md), the whole recipe with a real compiler:
//! plugin.new from the SDK template, plugin.writeSource, plugin.build with a mistake and its
//! structured error, the fix, plugin.publishLocal into an lsuite bundle, loading and playing
//! it, a rebuild under a new library name, disable, remove. The example plugins in
//! `plugins/` go in as bundles through plugin.install and play too.

use ryolune_engine::{
    control::{self, Headless},
    host::{native, scan},
    plugin::{ParamChange, ProcessContext},
    plugin_dev,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn call(host: &mut Headless, name: &str, params: Value) -> Value {
    control::call(host, name, &params, false).unwrap_or_else(|e| panic!("{name} {params}: {e}"))
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Run a plugin on a block of a 440 Hz sine and return the peak of what comes out.
fn play(plugin_id: &str, changes: &[ParamChange]) -> f32 {
    let mut instance = native::instantiate(plugin_id, 48_000).unwrap();
    let mut processor = instance.processor.take().unwrap();
    let mut peak = 0f32;
    for block in 0..20 {
        let mut audio: Vec<[f32; 2]> = (0..256)
            .map(|i| {
                let t = (block * 256 + i) as f32 / 48_000.0;
                let s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
                [s, s]
            })
            .collect();
        processor.process(
            &mut audio,
            &[],
            if block == 0 { changes } else { &[] },
            &ProcessContext::default(),
        );
        for frame in &audio {
            assert!(frame[0].is_finite() && frame[1].is_finite());
            peak = peak.max(frame[0].abs());
        }
    }
    peak
}

#[test]
fn an_agent_builds_installs_and_rebuilds_a_plugin_without_a_restart() {
    if plugin_dev::cargo().is_none() {
        eprintln!("No cargo here: the plugin build test is skipped.");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("LSUITE_HOME", home.path());
    // Plugin builds share a target folder that outlives the test, so reruns take seconds.
    std::env::set_var(
        "RYOLUNE_PLUGIN_TARGET_DIR",
        workspace().join("target").join("plugin-builds"),
    );
    let mut host = Headless::new();

    let guide = call(&mut host, "plugin.guide", json!({}));
    assert!(guide["markdown"]
        .as_str()
        .unwrap()
        .contains("plugin.publishLocal"));
    let toolchain = call(&mut host, "plugin.toolchain", json!({}));
    assert_eq!(toolchain["ok"], true, "{toolchain}");

    let made = call(
        &mut host,
        "plugin.new",
        json!({"name": "Night Crush", "kind": "effect", "vendor": "Ada"}),
    );
    assert_eq!(made["pluginId"], "native:com.ada.nightcrush");
    let crate_dir = PathBuf::from(made["path"].as_str().unwrap());
    assert!(crate_dir.starts_with(home.path().join("plugins-src/ryolune")));
    assert!(crate_dir.join(".cargo/config.toml").is_file());
    let template = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();

    // Paths outside the crate are refused.
    for bad in ["../escape.rs", "/etc/passwd", "target/x.rs"] {
        let error = control::call(
            &mut host,
            "plugin.writeSource",
            &json!({"name": "Night Crush", "path": bad, "contents": "x"}),
            false,
        )
        .unwrap_err();
        assert!(
            error.contains("crate") || error.contains("build"),
            "{bad}: {error}"
        );
    }

    // A mistake comes back as {file, line, message}, not a wall of text.
    let broken = template.replace(
        "let drive = self.drive.step();",
        "let drive: f32 = self.drive.step() + missing_value;",
    );
    assert_ne!(broken, template);
    call(
        &mut host,
        "plugin.writeSource",
        json!({"name": "Night Crush", "path": "src/lib.rs", "contents": broken}),
    );
    let failed = call(&mut host, "plugin.build", json!({"name": "Night Crush"}));
    assert_eq!(failed["ok"], false, "{failed}");
    let error = &failed["errors"][0];
    assert_eq!(error["file"], "src/lib.rs", "{failed}");
    assert!(error["line"].as_u64().unwrap() > 0);
    assert!(
        error["message"].as_str().unwrap().contains("missing_value"),
        "{error}"
    );
    let refused = call(
        &mut host,
        "plugin.publishLocal",
        json!({"name": "Night Crush"}),
    );
    assert_eq!(refused["ok"], false);
    assert!(scan::lookup("native:com.ada.nightcrush").is_none());

    // Fixed: built, installed as a bundle, in the list, playing.
    let quieter = template.replace(
        "*sample += (wet - *sample) * self.mix;",
        "*sample += (wet - *sample) * self.mix;\n                *sample *= 0.5;",
    );
    call(
        &mut host,
        "plugin.writeSource",
        json!({"name": "night-crush", "path": "src/lib.rs", "contents": quieter}),
    );
    let published = call(
        &mut host,
        "plugin.publishLocal",
        json!({"name": "Night Crush"}),
    );
    assert_eq!(published["ok"], true, "{published}");
    let first_library = PathBuf::from(published["library"].as_str().unwrap());
    assert!(first_library.starts_with(home.path().join("plugins/ryolune/com.ada.nightcrush")));
    let manifest = plugin_dev::read_manifest(first_library.parent().unwrap()).unwrap();
    assert_eq!(
        manifest.library.this_platform(),
        first_library.file_name().unwrap().to_str().unwrap()
    );
    let listed = call(&mut host, "plugin.list", json!({"query": "Night Crush"}));
    assert_eq!(
        listed["plugins"][0]["id"], "native:com.ada.nightcrush",
        "{listed}"
    );
    let info = call(
        &mut host,
        "plugin.info",
        json!({"id": "native:com.ada.nightcrush"}),
    );
    assert_eq!(info["origin"]["kind"], "lsuite", "{info}");
    assert_eq!(info["enabled"], true);
    let dry = [ParamChange::now(1, 0.0)];
    let half = play("native:com.ada.nightcrush", &dry);
    assert!((half - 0.25).abs() < 0.02, "{half}");

    // Rebuilt: a new library under a new name, the old one retired.
    let louder = quieter.replace("*sample *= 0.5;", "*sample *= 1.0;");
    call(
        &mut host,
        "plugin.writeSource",
        json!({"name": "Night Crush", "path": "src/lib.rs", "contents": louder}),
    );
    let again = call(
        &mut host,
        "plugin.publishLocal",
        json!({"name": "Night Crush"}),
    );
    assert_eq!(
        again["ok"], true,
        "rebuilding a loaded plugin failed: {again}"
    );
    let second_library = PathBuf::from(again["library"].as_str().unwrap());
    assert_ne!(first_library, second_library);
    assert!(!first_library.exists());
    assert_eq!(
        scan::lookup("native:com.ada.nightcrush").unwrap().path,
        second_library.display().to_string()
    );
    let full = play("native:com.ada.nightcrush", &dry);
    assert!((full - 0.5).abs() < 0.02, "{full}");

    // The examples in plugins/ install as bundles too.
    let built = workspace()
        .join("target")
        .join("plugin-builds")
        .join("release");
    for (example, id, lib) in [
        (
            "bitcrusher",
            "org.ryolune.examples.bitcrusher",
            "ryolune_plugin_bitcrusher",
        ),
        (
            "chorus",
            "org.ryolune.examples.chorus",
            "ryolune_plugin_chorus",
        ),
    ] {
        let status = std::process::Command::new(plugin_dev::cargo().unwrap())
            .args([
                "build",
                "--release",
                "-p",
                &format!("ryolune-plugin-{example}"),
            ])
            .current_dir(workspace())
            .env(
                "CARGO_TARGET_DIR",
                workspace().join("target").join("plugin-builds"),
            )
            .status()
            .unwrap();
        assert!(status.success(), "{example} builds");
        let bundle = tempfile::tempdir().unwrap();
        let file = format!(
            "{}{lib}.{}",
            if cfg!(windows) { "" } else { "lib" },
            native::library_extension()
        );
        std::fs::copy(built.join(&file), bundle.path().join(&file)).unwrap();
        std::fs::copy(
            workspace()
                .join("plugins")
                .join(example)
                .join("plugin.toml"),
            bundle.path().join("plugin.toml"),
        )
        .unwrap();
        let installed = call(&mut host, "plugin.install", json!({"path": bundle.path()}));
        assert_eq!(installed["pluginId"], format!("native:{id}"), "{installed}");
        let peak = play(&format!("native:{id}"), &[]);
        assert!(peak > 0.05, "{example} lets sound through: {peak}");
    }

    // Off: out of the list and refused to agents' loads; back on; removed.
    call(
        &mut host,
        "plugin.disable",
        json!({"id": "native:com.ada.nightcrush"}),
    );
    let hidden = call(&mut host, "plugin.list", json!({"query": "Night Crush"}));
    assert_eq!(hidden["total"], 0, "{hidden}");
    let shown = call(
        &mut host,
        "plugin.list",
        json!({"query": "Night Crush", "includeDisabled": true}),
    );
    assert_eq!(shown["plugins"][0]["enabled"], false);
    call(
        &mut host,
        "plugin.enable",
        json!({"id": "native:com.ada.nightcrush"}),
    );
    let stock = control::call(
        &mut host,
        "plugin.remove",
        &json!({"id": "stock:Space"}),
        false,
    )
    .unwrap_err();
    assert!(stock.contains("only be turned off"), "{stock}");
    call(
        &mut host,
        "plugin.remove",
        json!({"id": "native:com.ada.nightcrush"}),
    );
    assert!(scan::lookup("native:com.ada.nightcrush").is_none());
    assert!(!home
        .path()
        .join("plugins/ryolune/com.ada.nightcrush")
        .exists());

    std::env::remove_var("LSUITE_HOME");
    std::env::remove_var("RYOLUNE_PLUGIN_TARGET_DIR");
}
