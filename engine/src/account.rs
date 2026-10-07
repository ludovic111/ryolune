//! lsuite AI: the one lsuite account every app on this computer shares (lsuite's AI.md).
//!
//! Signing in from any lsuite app signs in all of them: the account lives in
//! `~/.lsuite/account.json` (0600, `$LSUITE_HOME` replaces `~/.lsuite`), written atomically and
//! read whenever it is needed, because another app can change it at any time:
//!
//! ```json
//! { "format": 1, "server": "https://lsuite.xyz", "email": "…", "name": "…", "plan": "pro",
//!   "token": "lsk_…", "signedInAt": "2026-10-06T21:00:00Z" }
//! ```
//!
//! The token is a secret: it never leaves this module in a command result, a log line or a
//! document ([`Account::public`] masks it). The server is `https://lsuite.xyz`, or
//! `$LSUITE_ACCOUNT_SERVER` for a new sign-in (tests, a local demo server); a signed-in
//! account keeps talking to the server that issued its token.
//!
//! Signing in works like native apps do OAuth: [`sign_in_browser`] listens on
//! `127.0.0.1:<random port>`, opens `<server>/account/connect?app=…&port=…&state=…`, takes the
//! browser's `/callback?code=…&state=…`, checks `state` and trades the code for a token at
//! `POST /api/account/token`. CLIs and headless use paste the key the account page shows
//! instead ([`sign_in_with_key`]).
//!
//! Everything here blocks (the network has a 15-second budget per call): the window runs it
//! on a worker, the CLI and MCP in their own process.

use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub const FORMAT: u64 = 1;
pub const DEFAULT_SERVER: &str = "https://lsuite.xyz";
/// How long the browser has to come back with a code.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);
const NETWORK: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub format: u64,
    pub server: String,
    pub email: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub plan: String,
    pub token: String,
    #[serde(default)]
    pub signed_in_at: String,
}

impl Account {
    /// What may be shown: everything but the token, which is masked.
    pub fn public(&self) -> Value {
        json!({
            "server": self.server,
            "email": self.email,
            "name": self.name,
            "plan": self.plan,
            "signedInAt": self.signed_in_at,
            "token": mask(&self.token),
        })
    }
    /// The Anthropic-compatible endpoint the subscription serves (`<server>/api/ai`): the
    /// Anthropic provider and Claude Code reach the plan's models through it.
    pub fn ai_base(&self) -> String {
        format!("{}/api/ai", self.server)
    }
}

/// `~/.lsuite/account.json`.
pub fn path() -> PathBuf {
    crate::lsuite::home().join("account.json")
}

/// The signed-in account, if any. A file in another format, unreadable or without a token
/// counts as signed out (it is left alone for the app that wrote it).
pub fn load() -> Option<Account> {
    let text = std::fs::read_to_string(path()).ok()?;
    if text.len() > 64 * 1024 {
        return None;
    }
    let account: Account = serde_json::from_str(&text).ok()?;
    crate::diagnostics::add_secret(&account.token);
    (account.format == FORMAT && !account.token.trim().is_empty() && valid_server(&account.server))
        .then_some(account)
}

/// Write the account for every lsuite app, atomically and readable only by this user.
pub fn save(account: &Account) -> Result<()> {
    let path = path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(account).map_err(|e| e.to_string())?;
    crate::document::atomic_write(&path, |f| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        f.write_all(text.as_bytes()).map_err(|e| e.to_string())
    })?;
    crate::diagnostics::add_secret(&account.token);
    Ok(())
}

/// Forget the account on this computer (every lsuite app is signed out).
pub fn remove() -> Result<bool> {
    match std::fs::remove_file(path()) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("Could not remove the lsuite account file: {e}")),
    }
}

fn valid_server(server: &str) -> bool {
    (server.starts_with("https://") || server.starts_with("http://"))
        && server.len() < 300
        && !server.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The server a new sign-in goes to: `$LSUITE_ACCOUNT_SERVER`, else lsuite.xyz.
pub fn server() -> String {
    std::env::var("LSUITE_ACCOUNT_SERVER")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| valid_server(s))
        .unwrap_or_else(|| DEFAULT_SERVER.to_string())
}

/// Where the person manages their plan: `<server>/account`.
pub fn manage_url() -> String {
    format!(
        "{}/account",
        load().map_or_else(server, |account| account.server)
    )
}

fn mask(secret: &str) -> String {
    let tail: String = secret
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if secret.chars().count() <= 8 {
        "••••".into()
    } else {
        format!("••••{tail}")
    }
}

fn client() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_global(Some(NETWORK))
        .user_agent(format!("ryolune/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// What went wrong talking to the account server, in words a person can act on.
#[derive(Debug, PartialEq)]
pub enum Failure {
    /// The token was refused: signed out elsewhere, or revoked.
    Unauthorized,
    /// The server could not be reached.
    Offline(String),
    /// The server answered with an error.
    Server(String),
}
impl Failure {
    pub fn message(&self) -> String {
        match self {
            Failure::Unauthorized => {
                "Your lsuite sign-in has expired. Sign in again to use lsuite AI.".into()
            }
            Failure::Offline(detail) => {
                format!(
                    "Could not reach the lsuite account server ({detail}). Check your connection."
                )
            }
            Failure::Server(detail) => format!("The lsuite account server said: {detail}"),
        }
    }
}

fn read_json(
    mut response: ureq::http::Response<ureq::Body>,
) -> std::result::Result<Value, Failure> {
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_to_string()
        .unwrap_or_default();
    let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if status == 401 || status == 403 {
        return Err(Failure::Unauthorized);
    }
    if !(200..300).contains(&status) {
        let detail = value["error"]["message"]
            .as_str()
            .or_else(|| value["error"].as_str())
            .or_else(|| value["message"].as_str())
            .map(str::to_string)
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(Failure::Server(detail.chars().take(300).collect()));
    }
    if value.is_null() {
        return Err(Failure::Server("an answer that is not JSON".into()));
    }
    Ok(value)
}

fn offline(error: ureq::Error) -> Failure {
    Failure::Offline(match error {
        ureq::Error::Timeout(_) => "timed out".to_string(),
        ureq::Error::ConnectionFailed | ureq::Error::Io(_) => "connection failed".to_string(),
        other => other.to_string().chars().take(120).collect(),
    })
}

/// `GET /api/account/me`: email, name, plan, status, usage and models of the token's account.
pub fn me(server: &str, token: &str) -> std::result::Result<Value, Failure> {
    client()
        .get(&format!("{server}/api/account/me"))
        .header("authorization", &format!("Bearer {token}"))
        .call()
        .map_err(offline)
        .and_then(read_json)
}

/// `GET /api/ai/plans`: the plans, prices, models and allowances (the source of truth; the
/// apps keep no copy).
pub fn plans() -> Result<Value> {
    let server = load().map_or_else(server, |a| a.server);
    let plans = client()
        .get(&format!("{server}/api/ai/plans"))
        .call()
        .map_err(offline)
        .and_then(read_json)
        .map_err(|f| f.message())?;
    Ok(json!({ "server": server, "manageUrl": format!("{server}/account"), "plans": plans }))
}

/// The plan's model ids from `GET /api/ai/v1/models` (Anthropic's list format).
pub fn models(account: &Account) -> std::result::Result<Vec<Value>, Failure> {
    let value = client()
        .get(&format!("{}/v1/models", account.ai_base()))
        .header("x-api-key", &account.token)
        .header("anthropic-version", "2023-06-01")
        .call()
        .map_err(offline)
        .and_then(read_json)?;
    Ok(value["data"]
        .as_array()
        .or_else(|| value["models"].as_array())
        .cloned()
        .unwrap_or_default())
}

fn account_from(server: &str, token: &str, me: &Value) -> Result<Account> {
    let email = me["email"].as_str().unwrap_or("").trim().to_string();
    if email.is_empty() {
        return Err("The lsuite account server did not say which account this is".into());
    }
    Ok(Account {
        format: FORMAT,
        server: server.to_string(),
        email,
        name: me["name"].as_str().unwrap_or("").to_string(),
        plan: me["plan"].as_str().unwrap_or("").to_string(),
        token: token.to_string(),
        signed_in_at: crate::lsuite::now_rfc3339(),
    })
}

/// Sign in with the key the account page shows (`lsk_…`): checked with the server, then
/// written for every lsuite app.
pub fn sign_in_with_key(key: &str) -> Result<Value> {
    let key = key.trim();
    if key.is_empty() || key.len() > 512 || key.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(
            "Paste the whole key from your lsuite account page (it starts with lsk_).".into(),
        );
    }
    let server = server();
    let me = me(&server, key).map_err(|f| match f {
        Failure::Unauthorized => {
            "lsuite did not accept this key. Copy it again from your account page.".to_string()
        }
        other => other.message(),
    })?;
    save(&account_from(&server, key, &me)?)?;
    Ok(status_with(&server, Some(&me)))
}

/// Sign in through the browser: listen on the loopback, open the connect page with `open`,
/// wait for the callback (until `timeout`, or `cancel`), trade the code for a token.
pub fn sign_in_browser(
    app: &str,
    open: &dyn Fn(&str) -> Result<()>,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<Value> {
    let server = server();
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Could not wait for the browser on this computer: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let state = random_state()?;
    open(&format!(
        "{server}/account/connect?app={app}&port={port}&state={state}"
    ))?;
    let started = Instant::now();
    let code = loop {
        if cancel.load(Ordering::Acquire) {
            return Err("Sign-in cancelled.".into());
        }
        if started.elapsed() > timeout {
            return Err("Sign-in timed out: the browser did not come back. Try again.".into());
        }
        match listener.accept() {
            Ok((stream, _)) => match callback(stream, &state, app) {
                Some(Ok(code)) => break code,
                Some(Err(error)) => return Err(error),
                // A favicon or a stray request: keep waiting.
                None => continue,
            },
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Err(e) => return Err(format!("Waiting for the browser failed: {e}")),
        }
    };
    let reply = client()
        .post(&format!("{server}/api/account/token"))
        .header("content-type", "application/json")
        .send(json!({ "code": code }).to_string())
        .map_err(offline)
        .and_then(read_json)
        .map_err(|f| f.message())?;
    let token = reply["token"]
        .as_str()
        .filter(|t| !t.trim().is_empty())
        .ok_or("The lsuite account server gave no token")?;
    let me = if reply["account"].is_object() {
        reply["account"].clone()
    } else {
        me(&server, token).map_err(|f| f.message())?
    };
    save(&account_from(&server, token, &me)?)?;
    Ok(status_with(&server, Some(&me)))
}

/// Answer one request on the loopback. `None` when it is not the callback.
fn callback(stream: TcpStream, state: &str, app: &str) -> Option<Result<String>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    // The rest of the head, so the browser is not reset before it reads the answer.
    let mut header = String::new();
    while reader.read_line(&mut header).is_ok_and(|n| n > 2) && header.len() < 16 * 1024 {
        header.clear();
    }
    let target = line.split_whitespace().nth(1).unwrap_or("");
    let (route, query) = target.split_once('?').unwrap_or((target, ""));
    let mut stream = stream;
    if route != "/callback" {
        let _ = stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return None;
    }
    let param = |name: &str| {
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(k, _)| *k == name)
            .map(|(_, v)| percent_decode(v))
    };
    let outcome = match (param("code"), param("state"), param("error")) {
        (_, _, Some(error)) => Err(format!("Sign-in was not finished: {error}")),
        (Some(code), Some(got), _) if got == state && !code.is_empty() => Ok(code),
        (_, Some(_), _) => {
            Err("The browser's answer did not match this sign-in. Try again.".into())
        }
        _ => Err("The browser came back without a sign-in code. Try again.".into()),
    };
    let (title, text) = match &outcome {
        Ok(_) => (
            "Connected",
            format!("{app} is connected to your lsuite account. You can close this tab and go back to {app}."),
        ),
        Err(error) => ("Not connected", error.clone()),
    };
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>{title}</title><body style=\"font:16px system-ui;background:#050505;color:#f2f2f2;display:grid;place-items:center;height:90vh\"><div><h1 style=\"font-weight:600\">{title}</h1><p>{}</p></div></body>",
        html_escape(&text)
    );
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
    Some(outcome)
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = |b: u8| (b as char).to_digit(16);
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(high), Some(low)) => {
                        out.push((high * 16 + low) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn random_state() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Sign out: the server forgets the token (best effort; offline still signs out here) and
/// the account file goes, which signs out every lsuite app on this computer.
pub fn sign_out() -> Result<Value> {
    let Some(account) = load() else {
        remove()?;
        return Ok(status_with(&server(), None));
    };
    let _ = client()
        .post(&format!("{}/api/account/signout", account.server))
        .header("authorization", &format!("Bearer {}", account.token))
        .send_empty();
    remove()?;
    Ok(status_with(&account.server, None))
}

/// The account as the apps show it, asking the server for the plan and the allowance when
/// `check` is set. Never holds the token.
pub fn status(check: bool) -> Value {
    let Some(account) = load() else {
        return status_with(&server(), None);
    };
    if !check {
        let mut value = status_with(&account.server, None);
        fill_from_file(&mut value, &account);
        return value;
    }
    match me(&account.server, &account.token) {
        Ok(me) => {
            // Keep the file's plan in step for the apps that read it offline.
            if me["plan"].as_str().is_some_and(|p| p != account.plan) {
                let mut next = account.clone();
                next.plan = me["plan"].as_str().unwrap_or("").to_string();
                let _ = save(&next);
            }
            status_with(&account.server, Some(&me))
        }
        Err(failure) => {
            let mut value = status_with(&account.server, None);
            fill_from_file(&mut value, &account);
            value["state"] = json!(if failure == Failure::Unauthorized {
                "expired"
            } else {
                "offline"
            });
            value["message"] = json!(failure.message());
            value
        }
    }
}

fn fill_from_file(value: &mut Value, account: &Account) {
    value["signedIn"] = json!(true);
    value["state"] = json!("signedIn");
    value["email"] = json!(account.email);
    value["name"] = json!(account.name);
    value["plan"] = json!(account.plan);
    value["planName"] = json!(plan_name(&account.plan));
    value["summary"] = json!(plan_name(&account.plan));
    value["message"] = json!(format!("Signed in as {}.", account.email));
}

/// The status document: signed out when `me` is `None`.
fn status_with(server: &str, me: Option<&Value>) -> Value {
    let base = json!({
        "signedIn": false,
        "state": "signedOut",
        "server": server,
        "manageUrl": format!("{server}/account"),
        "accountFile": path(),
        "message": "No setup. Sign in and your agent works.",
    });
    let Some(me) = me else { return base };
    let mut value = base;
    let plan = me["plan"].as_str().unwrap_or("");
    value["signedIn"] = json!(true);
    value["state"] = json!("signedIn");
    for key in ["email", "name", "plan", "status", "usage", "models"] {
        if !me[key].is_null() {
            value[key] = me[key].clone();
        }
    }
    value["planName"] = json!(plan_name(plan));
    value["summary"] = json!(summary(me));
    value["exhausted"] = json!(exhausted(&me["usage"]));
    value["message"] = json!(if plan.is_empty() || plan == "free" {
        "Signed in. Choose a plan to use lsuite AI.".to_string()
    } else if exhausted(&me["usage"]) {
        "This month's allowance is used up. Manage plan to add more.".to_string()
    } else {
        format!(
            "Signed in as {}.",
            me["email"].as_str().unwrap_or("your account")
        )
    });
    value
}

/// "pro" → "Pro".
pub fn plan_name(plan: &str) -> String {
    let mut chars = plan.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "No plan".into(),
    }
}

fn exhausted(usage: &Value) -> bool {
    match (usage["used"].as_f64(), usage["limit"].as_f64()) {
        (Some(used), Some(limit)) if limit > 0.0 => used >= limit,
        _ => false,
    }
}

/// `Pro · 38 % used · resets 1 Nov`.
pub fn summary(me: &Value) -> String {
    let mut parts = vec![plan_name(me["plan"].as_str().unwrap_or(""))];
    let usage = &me["usage"];
    if let (Some(used), Some(limit)) = (usage["used"].as_f64(), usage["limit"].as_f64()) {
        if limit > 0.0 {
            parts.push(format!(
                "{} % used",
                ((used / limit) * 100.0).round().clamp(0.0, 100.0) as u32
            ));
        }
    }
    if let Some(day) = usage["resetsAt"].as_str().and_then(short_date) {
        parts.push(format!("resets {day}"));
    }
    parts.join(" · ")
}

/// `2026-11-01T00:00:00Z` → `1 Nov`.
fn short_date(text: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month: usize = text.get(5..7)?.parse().ok()?;
    let day: u32 = text.get(8..10)?.parse().ok()?;
    Some(format!("{day} {}", MONTHS.get(month.checked_sub(1)?)?))
}

/// Whether an error from the AI endpoint means the allowance ran out (the plan's monthly
/// credits). The agent says so in one line with Manage plan, and never switches provider.
pub fn allowance_error(status: u16, body: &str) -> bool {
    let lower = body.to_lowercase();
    status == 402
        || ((status == 429 || status == 403)
            && ["allowance", "credit", "quota", "plan limit"]
                .iter()
                .any(|w| lower.contains(w)))
}

/// The one line the agent shows when the allowance ran out.
pub const ALLOWANCE_MESSAGE: &str =
    "Your lsuite AI allowance for this month is used up. Manage plan to add more.";

/// A stand-in for the lsuite account server, for tests here, in the window and in other
/// apps that pin this crate: the AI.md routes on `127.0.0.1`, one plan (`pro`), one token
/// (`lsk_test_token`) and one code (`code-1`). `exhausted` makes the Messages endpoint
/// answer like a used-up allowance; otherwise it streams a one-line reply.
#[doc(hidden)]
pub mod mock {
    use serde_json::{json, Value};
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
    };

    pub const TOKEN: &str = "lsk_test_token_0123456789";
    pub const CODE: &str = "code-1";

    pub struct Server {
        pub url: String,
        /// Every request as `METHOD /path`, in order.
        pub requests: Arc<Mutex<Vec<String>>>,
        pub exhausted: Arc<AtomicBool>,
        pub signed_out: Arc<AtomicBool>,
    }

    pub fn start() -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the mock server");
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(vec![]));
        let exhausted = Arc::new(AtomicBool::new(false));
        let signed_out = Arc::new(AtomicBool::new(false));
        let (log, used, out) = (requests.clone(), exhausted.clone(), signed_out.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (log, used, out) = (log.clone(), used.clone(), out.clone());
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() {
                        return;
                    }
                    let mut length = 0usize;
                    let mut auth = String::new();
                    loop {
                        let mut header = String::new();
                        if reader.read_line(&mut header).unwrap_or(0) <= 2 {
                            break;
                        }
                        let lower = header.to_lowercase();
                        if let Some(v) = lower.strip_prefix("content-length:") {
                            length = v.trim().parse().unwrap_or(0);
                        }
                        if lower.starts_with("authorization:") || lower.starts_with("x-api-key:") {
                            auth = header
                                .split_once(':')
                                .map(|x| x.1)
                                .unwrap_or("")
                                .trim()
                                .to_string();
                        }
                    }
                    let mut body = vec![0; length];
                    let _ = reader.read_exact(&mut body);
                    let mut parts = line.split_whitespace();
                    let method = parts.next().unwrap_or("").to_string();
                    let path = parts.next().unwrap_or("").to_string();
                    log.lock().unwrap().push(format!("{method} {path}"));
                    let authorized = !out.load(Ordering::Acquire)
                        && auth.trim_start_matches("Bearer ").trim() == TOKEN;
                    let me = json!({"email":"ada@example.com","name":"Ada","plan":"pro","status":"active",
                        "usage":{"used":380,"limit":1000,"resetsAt":"2026-11-01T00:00:00Z"},
                        "models":["claude-sonnet-5","claude-opus-5"]});
                    let (status, kind, text): (u16, &str, String) = match (method.as_str(), path.split('?').next().unwrap_or("")) {
                        ("GET", "/api/ai/plans") => (200, "application/json", json!({"demo":true,"plans":[{"id":"plus"},{"id":"pro"}]}).to_string()),
                        ("GET", "/api/account/me") if authorized => (200, "application/json", me.to_string()),
                        ("POST", "/api/account/token") => {
                            let sent: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                            if sent["code"] == CODE {
                                (200, "application/json", json!({"token": TOKEN, "account": me}).to_string())
                            } else {
                                (400, "application/json", json!({"error":{"message":"Unknown code"}}).to_string())
                            }
                        }
                        ("POST", "/api/account/signout") => {
                            out.store(true, Ordering::Release);
                            (200, "application/json", "{}".into())
                        }
                        ("GET", "/api/ai/v1/models") if authorized => (200, "application/json", json!({"data":[
                            {"id":"claude-haiku-5","display_name":"Claude Haiku 5"},
                            {"id":"claude-sonnet-5","display_name":"Claude Sonnet 5"}]}).to_string()),
                        ("POST", "/api/ai/v1/messages") if authorized && used.load(Ordering::Acquire) => (402, "application/json",
                            json!({"type":"error","error":{"type":"allowance_exceeded","message":"Monthly allowance used"}}).to_string()),
                        ("POST", "/api/ai/v1/messages") if authorized => {
                            let events = [
                                json!({"type":"message_start","message":{"usage":{"input_tokens":12}}}),
                                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello from lsuite AI."}}),
                                json!({"type":"content_block_stop","index":0}),
                                json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}),
                                json!({"type":"message_stop"}),
                            ];
                            let text: String = events.iter().map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap())).collect();
                            (200, "text/event-stream", text)
                        }
                        (_, "/api/account/me" | "/api/ai/v1/models" | "/api/ai/v1/messages") => (401, "application/json", json!({"error":{"message":"Invalid token"}}).to_string()),
                        _ => (404, "text/plain", "not found".into()),
                    };
                    let mut stream = stream;
                    let _ = write!(stream, "HTTP/1.1 {status} OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
                });
            }
        });
        Server {
            url,
            requests,
            exhausted,
            signed_out,
        }
    }

    /// What a browser does with the connect page: follow it straight to the app's callback
    /// with the mock's code and the page's state (or another state, to test the check).
    pub fn browser(state_override: Option<&'static str>) -> impl Fn(&str) -> crate::Result<()> {
        move |url: &str| {
            let query = url.split_once('?').map_or("", |(_, q)| q).to_string();
            let param = |name: &str| {
                query
                    .split('&')
                    .find_map(|p| p.strip_prefix(&format!("{name}=")))
                    .unwrap_or("")
                    .to_string()
            };
            let port = param("port");
            let state = state_override.map_or_else(|| param("state"), str::to_string);
            std::thread::spawn(move || {
                let mut stream = std::net::TcpStream::connect(format!("127.0.0.1:{port}")).unwrap();
                let _ = write!(
                    stream,
                    "GET /callback?code={}&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                    super::mock::CODE
                );
                let mut answer = String::new();
                let _ = stream.read_to_string(&mut answer);
            });
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_read_like_the_contract() {
        let me = json!({"plan":"pro","usage":{"used":380,"limit":1000,"resetsAt":"2026-11-01T00:00:00Z"}});
        assert_eq!(summary(&me), "Pro · 38 % used · resets 1 Nov");
        assert_eq!(summary(&json!({"plan":"plus"})), "Plus");
        assert!(exhausted(&json!({"used":1000,"limit":1000})));
        assert!(!exhausted(&json!({"used":1,"limit":0})));
        assert_eq!(percent_decode("a%2Fb+c%zz"), "a/b c%zz");
        assert_eq!(mask("lsk_abcdefgh1234"), "••••1234");
        assert!(allowance_error(402, ""));
        assert!(allowance_error(
            429,
            "{\"error\":{\"message\":\"Monthly allowance used\"}}"
        ));
        assert!(!allowance_error(429, "rate limited"));
    }
}
