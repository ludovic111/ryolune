//! Logs, crash reports and release notes through the registry, as the CLI reads them with no
//! window open (the data folder is this test binary's scratch folder).
use ryolune_engine::{
    control::{self, Headless},
    diagnostics,
    host::scan::data_dir,
    release_notes,
};
use serde_json::{json, Value};

fn call(host: &mut Headless, name: &str, params: Value) -> Value {
    control::call(host, name, &params, false).unwrap_or_else(|e| panic!("{name} {params}: {e}"))
}

#[test]
fn logs_reports_and_release_notes_are_commands() {
    let data = data_dir();
    let logs = diagnostics::logs_dir(&data);
    let crashes = diagnostics::crashes_dir(&data);
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::create_dir_all(&crashes).unwrap();
    std::fs::write(
        logs.join("ryolune.log"),
        (0..300).map(|i| format!("line {i}\n")).collect::<String>(),
    )
    .unwrap();
    std::fs::write(logs.join("ryolune.1.log"), "previous run\n").unwrap();
    std::fs::write(
        crashes.join("crash-20261006-100000-42.txt"),
        "ryolune crash report\n\nPanic on thread 'main' at src/x.rs:1:1:\nboom\n",
    )
    .unwrap();
    let mut host = Headless::new();

    let tail = call(&mut host, "app.logs", json!({"lines": 2}));
    assert_eq!(tail["lines"], json!(["line 298", "line 299"]));
    assert_eq!(tail["files"].as_array().unwrap().len(), 2);
    let previous = call(&mut host, "app.logs", json!({"file": "ryolune.1.log"}));
    assert_eq!(previous["lines"], json!(["previous run"]));
    assert!(control::call(
        &mut host,
        "app.logs",
        &json!({"file": "../settings.json"}),
        false
    )
    .is_err());
    assert!(control::call(&mut host, "app.logs", &json!({"lines": 0}), false).is_err());

    let listed = call(&mut host, "app.crashReports", json!({}));
    let reports = listed["reports"].as_array().unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["kind"], "crash");
    assert_eq!(reports[0]["summary"], "boom");
    let one = call(
        &mut host,
        "app.crashReports",
        json!({"id": "crash-20261006-100000-42.txt"}),
    );
    assert!(one["text"].as_str().unwrap().contains("boom"));

    let report = call(&mut host, "app.diagnostics", json!({}));
    assert_eq!(report["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(report["counts"]["crashReports"], 1);
    assert_eq!(report["crashReports"][0]["summary"], "boom");
    assert!(report["paths"]["logs"].is_string());

    assert_eq!(
        call(&mut host, "app.clearCrashReports", json!({}))["deleted"],
        1
    );
    assert!(call(&mut host, "app.crashReports", json!({}))["reports"]
        .as_array()
        .unwrap()
        .is_empty());

    let current = call(&mut host, "app.whatsNew", json!({}));
    assert_eq!(current["releases"][0]["version"], release_notes::CURRENT);
    assert_eq!(current["releases"].as_array().unwrap().len(), 1);
    let one = call(&mut host, "app.whatsNew", json!({"version": "0.12.0"}));
    assert_eq!(one["releases"][0]["version"], "0.12.0");
    let since = call(&mut host, "app.whatsNew", json!({"since": "0.11.0"}));
    assert!(since["releases"].as_array().unwrap().len() >= 3);
    assert!(control::call(
        &mut host,
        "app.whatsNew",
        &json!({"version": "0.0.1"}),
        false
    )
    .is_err());

    // Opening a browser needs the window.
    assert!(control::call(&mut host, "app.reportProblem", &json!({}), false).is_err());
}
