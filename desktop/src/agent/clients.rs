//! Ready configurations for the agents that can drive ryolune from outside over MCP:
//! Claude Code, Codex, Gemini CLI, Cursor, VS Code, Claude Desktop, Windsurf, opencode, Zed
//! and any other MCP client. Each one runs `ryolune-mcp --live`, which reaches this window
//! through the loopback bridge (Settings > Control). `agent.mcp` serves these to the window,
//! the CLI and MCP; `agent.openClient` opens the clients that install from a link.
use serde_json::{json, Value};
use std::path::Path;

/// `ryolune-mcp` and how to reach this window.
pub(crate) struct Server {
    pub command: String,
    pub discovery: String,
}

impl Server {
    pub(crate) fn current(discovery: &Path) -> Self {
        Self {
            command: super::cli::companion("ryolune-mcp"),
            discovery: discovery.display().to_string(),
        }
    }
    fn env(&self) -> Value {
        json!({ "RYOLUNE_CONTROL": self.discovery })
    }
    /// The `{command, args, env}` block most clients share.
    fn block(&self) -> Value {
        json!({ "command": self.command, "args": ["--live"], "env": self.env() })
    }
    fn mcp_servers(&self) -> String {
        pretty(&json!({ "mcpServers": { "ryolune": self.block() } }))
    }
    /// The standard `.mcp.json` text, for a folder an agent works in (zenith's threads).
    pub(crate) fn project_file(&self) -> String {
        self.mcp_servers()
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// Quote a path for a shell command line when it needs it.
fn quoted(path: &str) -> String {
    if path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-:\\".contains(c))
    {
        path.into()
    } else {
        format!("\"{}\"", path.replace('"', "\\\""))
    }
}

fn percent(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The install link of a client that has one.
pub(crate) fn link(server: &Server, client: &str) -> Option<String> {
    use base64::Engine;
    match client {
        "cursor" => Some(format!(
            "cursor://anysphere.cursor-deeplink/mcp/install?name=ryolune&config={}",
            percent(&base64::engine::general_purpose::STANDARD.encode(server.block().to_string()))
        )),
        "vscode" => {
            let mut config = server.block();
            config["name"] = json!("ryolune");
            config["type"] = json!("stdio");
            Some(format!(
                "vscode:mcp/install?{}",
                percent(&config.to_string())
            ))
        }
        _ => None,
    }
}

fn claude_desktop_config() -> &'static str {
    if cfg!(target_os = "macos") {
        "~/Library/Application Support/Claude/claude_desktop_config.json"
    } else if cfg!(windows) {
        "%APPDATA%\\Claude\\claude_desktop_config.json"
    } else {
        "~/.config/Claude/claude_desktop_config.json"
    }
}

/// Every client, in the order the window lists them.
pub(crate) fn clients(server: &Server, bridge_enabled: bool) -> Value {
    let command = quoted(&server.command);
    let env = format!("RYOLUNE_CONTROL={}", quoted(&server.discovery));
    let client = |id: &str, name: &str, how: &str, text: String, file: Option<&str>| {
        json!({
            "id": id,
            "name": name,
            "how": how,
            "text": text,
            "file": file,
            "link": link(server, id).is_some(),
        })
    };
    json!({
        "bridgeEnabled": bridge_enabled,
        "command": server.command,
        "args": ["--live"],
        "env": server.env(),
        "clients": [
            client("claude-code", "Claude Code", "Run this once in a terminal.",
                format!("claude mcp add ryolune --scope user -e {env} -- {command} --live"), None),
            client("codex", "Codex CLI", "Run this once in a terminal.",
                format!("codex mcp add ryolune --env {env} -- {command} --live"), None),
            client("cursor", "Cursor", "Click Add to Cursor, or paste this in the file below.",
                server.mcp_servers(), Some("~/.cursor/mcp.json")),
            client("vscode", "VS Code (Copilot)", "Click Add to VS Code, or paste this in the file below.",
                pretty(&json!({ "servers": { "ryolune": {
                    "type": "stdio", "command": server.command, "args": ["--live"], "env": server.env()
                } } })),
                Some(".vscode/mcp.json, or your user mcp.json")),
            client("claude-desktop", "Claude Desktop", "Paste this in the file below, then restart Claude.",
                server.mcp_servers(), Some(claude_desktop_config())),
            client("gemini", "Gemini CLI", "Add this to the file below.",
                server.mcp_servers(), Some("~/.gemini/settings.json")),
            client("windsurf", "Windsurf", "Add this to the file below.",
                server.mcp_servers(), Some("~/.codeium/windsurf/mcp_config.json")),
            client("opencode", "opencode", "Add this to the file below.",
                pretty(&json!({ "mcp": { "ryolune": {
                    "type": "local", "command": [server.command, "--live"], "environment": server.env()
                } } })),
                Some("~/.config/opencode/opencode.json")),
            client("zed", "Zed", "Add this to Zed's settings.",
                pretty(&json!({ "context_servers": { "ryolune": server.block() } })),
                Some("~/.config/zed/settings.json")),
            client("other", "Any other MCP client", "Most clients take this standard block.",
                server.mcp_servers(), None),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_client_points_at_the_live_server_and_links_carry_the_same_config() {
        let server = Server {
            command: "/Applications/ryolune.app/Contents/MacOS/ryolune-mcp".into(),
            discovery: "/Users/me/Library/Application Support/ryolune/control.json".into(),
        };
        let all = clients(&server, true);
        let list = all["clients"].as_array().unwrap();
        assert!(list.len() >= 9);
        for c in list {
            let text = c["text"].as_str().unwrap();
            assert!(text.contains("ryolune-mcp"), "{}", c["id"]);
            assert!(text.contains("--live"), "{}", c["id"]);
            assert!(text.contains("control.json"), "{}", c["id"]);
        }
        let claude = list.iter().find(|c| c["id"] == "claude-code").unwrap();
        // A discovery path with spaces is quoted for the shell.
        assert!(claude["text"]
            .as_str()
            .unwrap()
            .contains("\"/Users/me/Library/Application Support/ryolune/control.json\""));
        let cursor = link(&server, "cursor").unwrap();
        assert!(cursor
            .starts_with("cursor://anysphere.cursor-deeplink/mcp/install?name=ryolune&config="));
        assert!(!cursor.contains(' ') && !cursor.contains('+'));
        let vscode = link(&server, "vscode").unwrap();
        assert!(vscode.starts_with("vscode:mcp/install?%7B"));
        assert!(link(&server, "zed").is_none());
    }
}
