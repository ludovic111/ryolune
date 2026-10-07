//! lsuite AI: the shared account file, sign-in by key and through the browser, sign-out,
//! and the `account.*` commands, against a stand-in for the lsuite server.

use ryolune_engine::{
    account::{self, mock},
    control::{self, Headless},
    control_app, settings,
};
use serde_json::json;
use std::{sync::atomic::AtomicBool, time::Duration};

#[test]
fn lsuite_account_signs_in_out_and_never_shows_the_token() {
    // One test: the server and the account folder are process-wide environment variables.
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("LSUITE_HOME", home.path());
    let server = mock::start();
    std::env::set_var("LSUITE_ACCOUNT_SERVER", &server.url);

    // Signed out: no network, the invitation.
    let status = account::status(true);
    assert_eq!(status["state"], "signedOut");
    assert_eq!(status["message"], "No setup. Sign in and your agent works.");
    assert!(server.requests.lock().unwrap().is_empty());

    // Headless, without a key: the browser needs the window, a key works.
    let mut host = Headless::new();
    let error = control::call(&mut host, "account.signIn", &json!({}), false).unwrap_err();
    assert!(error.contains("key=lsk_"), "{error}");
    let error = control::call(
        &mut host,
        "account.signIn",
        &json!({"key": "lsk_wrong_key_000"}),
        false,
    )
    .unwrap_err();
    assert!(error.contains("did not accept this key"), "{error}");
    assert!(account::load().is_none());

    let signed = control::call(
        &mut host,
        "account.signIn",
        &json!({"key": mock::TOKEN}),
        false,
    )
    .unwrap();
    assert_eq!(signed["signedIn"], true);
    assert_eq!(signed["summary"], "Pro · 38 % used · resets 1 Nov");
    assert!(!signed.to_string().contains(mock::TOKEN));
    let file = account::path();
    assert_eq!(file, home.path().join("account.json"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(stored["format"], 1);
    assert_eq!(stored["server"], server.url.as_str());
    assert_eq!(stored["email"], "ada@example.com");
    assert_eq!(stored["plan"], "pro");
    assert_eq!(stored["token"], mock::TOKEN);

    let status = control::call(&mut host, "account.status", &json!({}), false).unwrap();
    assert_eq!(status["planName"], "Pro");
    assert_eq!(status["usage"]["limit"], 1000);
    assert!(!status.to_string().contains(mock::TOKEN));
    let plans = control::call(&mut host, "account.plans", &json!({}), false).unwrap();
    assert_eq!(plans["plans"]["demo"], true);
    let models = account::models(&account::load().unwrap()).unwrap();
    assert_eq!(models.len(), 2);

    // Agents read the account but never sign in or out.
    let permissions = settings::Permissions::default();
    for name in ["account.signIn", "account.signOut", "account.manage"] {
        assert!(
            control_app::denied_for_agent(name, &permissions).is_some(),
            "{name}"
        );
    }
    assert!(control_app::denied_for_agent("account.status", &permissions).is_none());

    // Sign out: the server forgets the token, the file goes (every app is signed out).
    let out = control::call(&mut host, "account.signOut", &json!({}), false).unwrap();
    assert_eq!(out["signedIn"], false);
    assert!(!file.exists());
    assert!(server
        .requests
        .lock()
        .unwrap()
        .contains(&"POST /api/account/signout".to_string()));
    server
        .signed_out
        .store(false, std::sync::atomic::Ordering::Release);

    // The browser sign-in: loopback callback, state checked, code traded for a token.
    let cancel = AtomicBool::new(false);
    let browser = mock::browser(Some("forged"));
    let error = account::sign_in_browser("ryolune", &browser, &cancel, Duration::from_secs(10))
        .unwrap_err();
    assert!(error.contains("did not match"), "{error}");
    assert!(account::load().is_none());
    let browser = mock::browser(None);
    let signed =
        account::sign_in_browser("ryolune", &browser, &cancel, Duration::from_secs(10)).unwrap();
    assert_eq!(signed["email"], "ada@example.com");
    assert_eq!(account::load().unwrap().token, mock::TOKEN);
    assert!(server
        .requests
        .lock()
        .unwrap()
        .contains(&"POST /api/account/token".to_string()));

    // Revoked elsewhere: the status says to sign in again, the file stays for the person.
    server
        .signed_out
        .store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(account::status(true)["state"], "expired");

    // A cancelled sign-in stops waiting.
    let cancelled = AtomicBool::new(true);
    let error = account::sign_in_browser(
        "ryolune",
        &|_: &str| Ok(()),
        &cancelled,
        Duration::from_secs(5),
    )
    .unwrap_err();
    assert!(error.contains("cancelled"), "{error}");

    std::env::remove_var("LSUITE_ACCOUNT_SERVER");
    std::env::remove_var("LSUITE_HOME");
}
