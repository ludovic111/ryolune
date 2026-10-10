//! Live control over a loopback socket.
//!
//! The running desktop app listens on 127.0.0.1 and writes `{port, token, pid}` to a discovery
//! file that only the current user can read. Clients send newline-delimited JSON-RPC 2.0: first
//! `auth` with the token, then any command name from the registry. Connection threads forward
//! each request to the thread that owns the store and block until it answers, so commands are
//! applied in order on the same thread as the interface, never concurrently with it.

use super::{call, Host};
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};

pub const VERSION: u32 = 1;
/// One request or reply line, including a `clip.setNotes` with thousands of notes.
pub const MAX_LINE: usize = 64 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CLIENTS: usize = 16;
const MAX_PENDING: usize = 16;
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
const IDLE_TIMEOUT: Duration = Duration::from_secs(3600);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Discovery {
    pub version: u32,
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

/// `$RYOLUNE_CONTROL`, else `~/.ryolune/control.json`.
pub fn discovery_path() -> PathBuf {
    if let Some(p) = std::env::var_os("RYOLUNE_CONTROL").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    if let Some(sandbox) = crate::host::scan::test_sandbox() {
        return sandbox.join("control.json");
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join(".ryolune").join("control.json")
}
pub fn read_discovery(path: &Path) -> Result<Discovery> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "ryolune is not running (no control file at {}): {e}",
            path.display()
        )
    })?;
    let d: Discovery =
        serde_json::from_str(&text).map_err(|e| format!("Invalid control file: {e}"))?;
    if d.version != VERSION {
        return Err(format!(
            "ryolune control protocol {} is not supported by this client ({VERSION})",
            d.version
        ));
    }
    Ok(d)
}

fn token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    (0..4u64)
        .map(|i| {
            // RandomState keys come from the operating system's randomness.
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(i);
            h.write_u128(nanos);
            h.write_u32(std::process::id());
            format!("{:016x}", h.finish())
        })
        .collect()
}
fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// One command waiting for the store's thread.
pub struct Request {
    pub method: String,
    pub params: Value,
    pub agent: bool,
    reply: mpsc::SyncSender<Result<Value>>,
}
impl Request {
    pub fn respond(self, result: Result<Value>) {
        let _ = self.reply.send(result);
    }
}
/// Answer a request with the registry.
pub fn serve(host: &mut dyn Host, request: Request) {
    let result = call(host, &request.method, &request.params, request.agent);
    request.respond(result);
}

/// Listener owned by the process that owns the store. Dropping it removes the discovery file.
pub struct Server {
    path: PathBuf,
    token: String,
    port: u16,
    receiver: mpsc::Receiver<Request>,
    stopping: Arc<AtomicBool>,
    clients: Arc<Mutex<HashMap<u64, TcpStream>>>,
}
impl Server {
    /// `wake` runs on a connection thread after each request is queued; the desktop uses it to
    /// request a repaint so the store thread notices without polling quickly.
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Result<Self> {
        Self::start_at(discovery_path(), wake)
    }
    pub fn start_at(path: PathBuf, wake: impl Fn() + Send + Sync + 'static) -> Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let token = token();
        write_discovery(
            &path,
            &Discovery {
                version: VERSION,
                port,
                token: token.clone(),
                pid: std::process::id(),
            },
        )?;
        let (sender, receiver) = mpsc::sync_channel(MAX_PENDING);
        let stopping = Arc::new(AtomicBool::new(false));
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let (stop, expected) = (stopping.clone(), token.clone());
        let clients = Arc::new(Mutex::new(HashMap::<u64, TcpStream>::new()));
        let active_clients = clients.clone();
        std::thread::Builder::new()
            .name("ryolune-control".into())
            .spawn(move || {
                let mut next_client = 0u64;
                for stream in listener.incoming() {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    if !stream.peer_addr().is_ok_and(|a| a.ip().is_loopback()) {
                        continue;
                    }
                    let mut active = active_clients.lock().unwrap_or_else(|e| e.into_inner());
                    if active.len() >= MAX_CLIENTS || stop.load(Ordering::Relaxed) {
                        continue;
                    }
                    let Ok(handle) = stream.try_clone() else {
                        continue;
                    };
                    let id = next_client;
                    next_client += 1;
                    active.insert(id, handle);
                    drop(active);
                    let (sender, expected, wake, stopping) =
                        (sender.clone(), expected.clone(), wake.clone(), stop.clone());
                    let guard = ConnectionGuard {
                        clients: active_clients.clone(),
                        id,
                    };
                    let _ = std::thread::Builder::new()
                        .name("ryolune-control-client".into())
                        .spawn(move || {
                            let _guard = guard;
                            connection(stream, &expected, &sender, &wake, &stopping)
                        });
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            path,
            token,
            port,
            receiver,
            stopping,
            clients,
        })
    }
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Every request queued so far, in arrival order.
    pub fn drain(&self) -> Vec<Request> {
        self.receiver.try_iter().collect()
    }
    /// Block until a request arrives or the timeout passes (headless hosts and tests).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Request> {
        self.receiver.recv_timeout(timeout).ok()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        for stream in self
            .clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            let _ = stream.shutdown(Shutdown::Both);
        }
        // Wake the blocking accept so the listener thread exits.
        let _ = TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, self.port)),
            Duration::from_millis(200),
        );
        if read_discovery(&self.path).is_ok_and(|d| same_token(&d.token, &self.token)) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
fn write_discovery(path: &Path, discovery: &Discovery) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string(discovery).map_err(|e| e.to_string())?;
    crate::document::atomic_write(path, |f| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        f.write_all(text.as_bytes()).map_err(|e| e.to_string())
    })
}

struct ConnectionGuard {
    clients: Arc<Mutex<HashMap<u64, TcpStream>>>,
    id: u64,
}
impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

fn read_frame(reader: &mut BufReader<TcpStream>, limit: usize) -> Option<String> {
    let mut line = String::new();
    let mut limited = reader.by_ref().take(limit as u64);
    match limited.read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) if line.len() >= limit => None,
        Ok(_) => Some(line),
    }
}
fn error_frame(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
fn connection(
    stream: TcpStream,
    expected: &str,
    sender: &mpsc::SyncSender<Request>,
    wake: &Arc<dyn Fn() + Send + Sync>,
    stopping: &AtomicBool,
) {
    let _ = stream.set_read_timeout(Some(AUTH_TIMEOUT));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_nodelay(true);
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut authenticated = false;
    while let Some(line) = read_frame(&mut reader, if authenticated { MAX_LINE } else { 4096 }) {
        if line.trim().is_empty() {
            continue;
        }
        let frame: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = writeln!(
                    out,
                    "{}",
                    error_frame(Value::Null, -32700, &format!("Parse error: {e}"))
                );
                continue;
            }
        };
        let id = frame.get("id").cloned().unwrap_or(Value::Null);
        if !frame.is_object()
            || frame.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || frame.get("id").is_none()
            || (!id.is_null() && !id.is_string() && !id.is_number())
            || frame
                .get("method")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        {
            if writeln!(
                out,
                "{}",
                error_frame(Value::Null, -32600, "Invalid JSON-RPC 2.0 request with id")
            )
            .is_err()
            {
                return;
            }
            continue;
        }
        let method = frame.get("method").and_then(Value::as_str).unwrap_or("");
        let params = frame.get("params").cloned().unwrap_or(Value::Null);
        let response = if method == "auth" {
            let given = params.get("token").and_then(Value::as_str).unwrap_or("");
            if same_token(given, expected) {
                authenticated = true;
                let _ = reader.get_ref().set_read_timeout(Some(IDLE_TIMEOUT));
                json!({ "jsonrpc": "2.0", "id": id, "result": { "ok": true, "app": "ryolune", "version": env!("CARGO_PKG_VERSION"), "protocol": VERSION } })
            } else {
                let _ = writeln!(out, "{}", error_frame(id, -32001, "Invalid control token"));
                return;
            }
        } else if !authenticated {
            let _ = writeln!(out, "{}", error_frame(id, -32001, "Authenticate first"));
            return;
        } else if method.is_empty() {
            error_frame(id, -32600, "Request needs a `method`")
        } else {
            let (reply, done) = mpsc::sync_channel(1);
            let request = Request {
                method: method.into(),
                params,
                agent: frame.get("agent").and_then(Value::as_bool).unwrap_or(false),
                reply,
            };
            match sender.try_send(request) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(_)) => {
                    if writeln!(
                        out,
                        "{}",
                        error_frame(id, -32003, "ryolune control queue is full; retry later")
                    )
                    .is_err()
                    {
                        return;
                    }
                    continue;
                }
                Err(mpsc::TrySendError::Disconnected(_)) => return,
            }
            wake();
            let result = loop {
                match done.recv_timeout(Duration::from_millis(200)) {
                    Err(mpsc::RecvTimeoutError::Timeout) if !stopping.load(Ordering::Relaxed) => {
                        continue
                    }
                    result => break result,
                }
            };
            match result {
                Ok(Ok(result)) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Ok(Err(message)) => error_frame(id, -32000, &message),
                Err(_) => error_frame(id, -32002, "ryolune dropped the request"),
            }
        };
        if writeln!(out, "{response}").is_err() {
            return;
        }
    }
}

/// Client side of the live protocol.
pub struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    next_id: u64,
    pub pid: u32,
    pub app_version: String,
}
impl Client {
    pub fn connect() -> Result<Self> {
        let path = discovery_path();
        match Self::connect_at(&path) {
            // Started by another lsuite app without ryolune's environment: the window's
            // lsuite entry says where its control file is.
            Err(error)
                if !path.exists()
                    && std::env::var_os("RYOLUNE_CONTROL").is_none_or(|p| p.is_empty()) =>
            {
                match crate::lsuite::entry("ryolune")
                    .and_then(|e| e["running"]["controlFile"].as_str().map(PathBuf::from))
                    .filter(|other| other != &path)
                {
                    Some(other) => Self::connect_at(&other).map_err(|_| error),
                    None => Err(error),
                }
            }
            result => result,
        }
    }
    pub fn connect_at(path: &Path) -> Result<Self> {
        let d = read_discovery(path)?;
        let stream = TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, d.port)),
            CONNECT_TIMEOUT,
        )
        .map_err(|e| {
            format!(
                "ryolune is not running (port {} refused: {e}). Delete {} if the app has quit.",
                d.port,
                path.display()
            )
        })?;
        stream.set_nodelay(true).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(AUTH_TIMEOUT))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| e.to_string())?;
        let writer = stream.try_clone().map_err(|e| e.to_string())?;
        let mut client = Self {
            reader: BufReader::new(stream),
            writer,
            next_id: 1,
            pid: d.pid,
            app_version: String::new(),
        };
        let hello = client.exchange(
            json!({ "jsonrpc": "2.0", "id": 0, "method": "auth", "params": { "token": d.token } }),
        )?;
        client
            .reader
            .get_ref()
            .set_read_timeout(None)
            .map_err(|e| e.to_string())?;
        client.app_version = hello
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .into();
        Ok(client)
    }
    /// Run a registry command in the app. Command failures come back as `Err(message)`;
    /// a lost connection is reported with a "Lost connection" prefix.
    pub fn call(&mut self, method: &str, params: &Value, agent: bool) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut frame = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if agent {
            frame["agent"] = json!(true);
        }
        self.exchange(frame)
    }
    fn exchange(&mut self, frame: Value) -> Result<Value> {
        let text = serde_json::to_string(&frame).map_err(|e| e.to_string())?;
        if text.len() >= MAX_LINE {
            return Err("Request exceeds 64 MiB".into());
        }
        writeln!(self.writer, "{text}").map_err(|e| format!("Lost connection to ryolune: {e}"))?;
        let mut line = String::new();
        let mut limited = self.reader.by_ref().take(MAX_LINE as u64);
        match limited.read_line(&mut line) {
            Ok(0) => return Err("Lost connection to ryolune: the app closed the socket".into()),
            Err(e) => return Err(format!("Lost connection to ryolune: {e}")),
            Ok(_) => {}
        }
        if line.len() >= MAX_LINE {
            return Err("Lost connection to ryolune: reply exceeds 64 MiB".into());
        }
        let response: Value = serde_json::from_str(&line)
            .map_err(|e| format!("Lost connection to ryolune: invalid reply: {e}"))?;
        if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || response.get("id") != frame.get("id")
            || response.get("result").is_some() == response.get("error").is_some()
        {
            return Err("Lost connection to ryolune: mismatched JSON-RPC reply".into());
        }
        if let Some(err) = response.get("error") {
            return Err(err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Unknown error")
                .to_string());
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }
}
