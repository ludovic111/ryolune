//! Model discovery uses the connected account/server. No inference request is sent.
use super::{bounded, cli, read_line_limited};
use ryolune_engine::{
    settings::{Provider, Settings},
    Result,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    io::{BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Model {
    pub id: String,
    pub name: String,
    pub efforts: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Group {
    pub provider: &'static str,
    pub label: String,
    pub models: Vec<Model>,
    pub error: Option<String>,
}
const EFFORTS: &[&str] = &[
    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

pub(crate) fn discover(settings: &Settings) -> Vec<Group> {
    std::thread::scope(|scope| {
        let jobs: Vec<_> = Provider::ALL
            .into_iter()
            .filter(|p| match p {
                Provider::Codex => {
                    super::discover_codex(&settings.agent.codex_executable).is_file()
                }
                Provider::Claude => {
                    super::discover_claude(&settings.agent.claude_executable).is_file()
                }
                Provider::Compatible => !settings.agent.compatible_base_url.is_empty(),
                // A local server is asked only when it is the one in use: probing every
                // address on each open would list servers that are not even installed.
                Provider::Ollama | Provider::LmStudio => *p == settings.agent.provider,
                _ => settings.api_key(*p).is_some(),
            })
            .map(|provider| {
                (
                    provider,
                    scope.spawn(move || match provider {
                        Provider::Codex | Provider::Claude => cli_models(settings, provider),
                        _ => api_models(settings, provider),
                    }),
                )
            })
            .collect();
        jobs.into_iter()
            .map(|(provider, job)| {
                let outcome = job
                    .join()
                    .unwrap_or_else(|_| Err("Model discovery stopped unexpectedly".into()));
                let (models, error) = match outcome {
                    Ok(models) => (models, None),
                    Err(error) => (vec![], Some(error)),
                };
                Group {
                    provider: provider.key(),
                    label: if provider == Provider::Compatible {
                        settings.agent.compatible_base_url.clone()
                    } else {
                        provider.label().into()
                    },
                    models,
                    error,
                }
            })
            .collect()
    })
}

fn normalized(raw: &Value, provider: Provider) -> Option<Model> {
    let id = raw["model"]
        .as_str()
        .or_else(|| raw["value"].as_str())
        .or_else(|| raw["id"].as_str())?;
    // Gemini lists "models/gemini-…"; its chat endpoint takes the bare name.
    let id = if provider == Provider::Gemini {
        id.strip_prefix("models/").unwrap_or(id)
    } else {
        id
    };
    if id.is_empty() || id.len() > 200 || id.chars().any(char::is_control) {
        return None;
    }
    let name = raw["displayName"]
        .as_str()
        .or_else(|| raw["display_name"].as_str())
        .or_else(|| raw["name"].as_str())
        .unwrap_or(id);
    let levels = raw
        .get("supportedReasoningEfforts")
        .or_else(|| raw.get("supportedEffortLevels"))
        .or_else(|| raw.get("reasoning_efforts"));
    let mut efforts: Vec<String> = levels
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().or_else(|| v["reasoningEffort"].as_str()))
        .map(str::to_owned)
        .collect();
    if provider == Provider::Anthropic {
        if let Some(capabilities) = raw["capabilities"]["effort"].as_object() {
            efforts.extend(
                capabilities
                    .iter()
                    .filter(|(_, v)| v["supported"] == true)
                    .map(|(k, _)| k.clone()),
            );
        }
    }
    efforts.retain(|level| {
        !level.is_empty()
            && level.len() <= 40
            && level
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    });
    efforts.sort_by(|a, b| {
        EFFORTS
            .iter()
            .position(|v| *v == a)
            .unwrap_or(usize::MAX)
            .cmp(&EFFORTS.iter().position(|v| *v == b).unwrap_or(usize::MAX))
            .then_with(|| a.cmp(b))
    });
    efforts.dedup();
    Some(Model {
        id: id.into(),
        name: bounded(name, 200),
        efforts,
    })
}

fn api_models(settings: &Settings, provider: Provider) -> Result<Vec<Model>> {
    let base = match provider {
        Provider::Anthropic => "https://api.anthropic.com/v1".to_string(),
        _ => settings.base_url(provider).unwrap_or_default(),
    };
    let client: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into();
    let mut models = vec![];
    let mut cursor: Option<String> = None;
    for _ in 0..20 {
        let mut request = client.get(&format!("{base}/models"));
        if provider == Provider::Anthropic {
            request = request
                .query("limit", "1000")
                .header("anthropic-version", "2023-06-01");
        }
        if let Some(cursor) = &cursor {
            request = request.query(
                if provider == Provider::Anthropic {
                    "after_id"
                } else {
                    "after"
                },
                cursor,
            );
        }
        if let Some(key) = settings.api_key(provider) {
            request = if provider == Provider::Anthropic {
                request.header("x-api-key", &key)
            } else {
                request.header("authorization", &format!("Bearer {key}"))
            };
        }
        let mut response = request.call().map_err(|_| {
            "Could not reach the model catalogue. Check the server address and connection."
                .to_string()
        })?;
        if response.status() != 200 {
            return Err(format!(
                "Model catalogue returned HTTP {}. Check this connection in Settings.",
                response.status()
            ));
        }
        let body = response
            .body_mut()
            .with_config()
            .limit(4 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| e.to_string())?;
        let page: Value =
            serde_json::from_str(&body).map_err(|e| format!("Invalid model catalogue: {e}"))?;
        let data = page["data"]
            .as_array()
            .ok_or("Server did not return a models list")?;
        models.extend(data.iter().filter_map(|v| normalized(v, provider)));
        if page["has_more"] != true {
            return Ok(models);
        }
        let next = page["last_id"]
            .as_str()
            .ok_or("Model catalogue omitted its next cursor")?;
        if cursor.as_deref() == Some(next) {
            return Err("Model catalogue repeated its cursor".into());
        }
        cursor = Some(next.into());
    }
    Err("Model catalogue exceeded 20 pages; narrow the server catalogue".into())
}

fn cli_models(settings: &Settings, provider: Provider) -> Result<Vec<Model>> {
    let mut selected = settings.clone();
    selected.agent.provider = provider;
    if super::connection::check(&selected).state != "signedIn" {
        return Err("Connect this account in Agent settings to list its models".into());
    }

    let workspace = tempfile::tempdir().map_err(|e| e.to_string())?;
    let codex_home = if provider == Provider::Codex {
        Some(super::codex::isolated_home()?)
    } else {
        None
    };
    let mut command = Command::new(if provider == Provider::Codex {
        super::discover_codex(&settings.agent.codex_executable)
    } else {
        super::discover_claude(&settings.agent.claude_executable)
    });
    if let Some(home) = &codex_home {
        command
            .args(["app-server", "--listen", "stdio://"])
            .env("CODEX_HOME", home.path());
    } else {
        command.args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--strict-mcp-config",
            "--mcp-config",
            "{\"mcpServers\":{}}",
            "--setting-sources",
            "",
            "--tools",
            "",
            "--no-session-persistence",
        ]);
    }
    command
        .current_dir(workspace.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    cli::group(&mut command);
    let mut child = command
        .spawn()
        .map_err(|_| "Could not start the account companion".to_string())?;
    let result = (|| {
        let mut input = child.stdin.take().ok_or("Missing companion input")?;
        let output = child.stdout.take().ok_or("Missing companion output")?;
        let (tx, rx) = mpsc::sync_channel(32);
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            while let Ok(Some(line)) = read_line_limited(&mut reader, 2 * 1024 * 1024) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let result = (|| {
            let send = |input: &mut std::process::ChildStdin, value: Value| -> Result<()> {
                writeln!(input, "{value}")
                    .and_then(|_| input.flush())
                    .map_err(|e| e.to_string())
            };
            let codex = provider == Provider::Codex;
            send(
                &mut input,
                if codex {
                    json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"ryolune","version":env!("CARGO_PKG_VERSION")}}})
                } else {
                    json!({"type":"control_request","request_id":"models","request":{"subtype":"initialize"}})
                },
            )?;
            let start = Instant::now();
            let mut models = vec![];
            let mut cursor: Option<String> = None;
            let mut pages = 0;
            while start.elapsed() < Duration::from_secs(25) {
                let line = match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(line) => line,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => return Err("The companion closed model discovery".into()),
                };
                let event: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
                if event.get("error").is_some() {
                    return Err("The companion could not list models. Check your sign-in.".into());
                }
                if codex && event["id"] == 1 {
                    send(&mut input, json!({"method":"initialized"}))?;
                    send(
                        &mut input,
                        json!({"id":2,"method":"model/list","params":{}}),
                    )?;
                } else if codex && event["id"] == 2 {
                    let page = &event["result"];
                    models.extend(
                        page["data"]
                            .as_array()
                            .ok_or("Missing models")?
                            .iter()
                            .filter_map(|m| normalized(m, provider)),
                    );
                    let Some(next) = page["nextCursor"].as_str() else {
                        return Ok(models);
                    };
                    pages += 1;
                    if cursor.as_deref() == Some(next) || pages > 20 {
                        return Err("Invalid model pagination".into());
                    }
                    cursor = Some(next.into());
                    send(
                        &mut input,
                        json!({"id":2,"method":"model/list","params":{"cursor":next}}),
                    )?;
                } else if !codex
                    && event["type"] == "control_response"
                    && event["response"]["request_id"] == "models"
                {
                    let raw = event["response"]["response"]["models"].as_array().ok_or("Claude did not return a model catalogue. Update the companion and check your account.")?;
                    return Ok(raw.iter().filter_map(|m| normalized(m, provider)).collect());
                }
            }
            Err("Model discovery timed out. Check the account connection and retry.".into())
        })();
        cli::terminate_tree(&mut child);
        drop(rx);
        drop(input);
        let _ = reader.join();
        result
    })();
    cli::terminate_tree(&mut child);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_advertised_efforts_are_available() {
        let codex = normalized(&json!({"model":"future-model","displayName":"Future", "supportedReasoningEfforts":[{"reasoningEffort":"high"}]}), Provider::Codex).unwrap();
        assert_eq!(codex.efforts, ["high"]);
        let claude = normalized(&json!({"id":"new-claude","capabilities":{"effort":{"xhigh":{"supported":true},"max":{"supported":false}}}}), Provider::Anthropic).unwrap();
        assert_eq!(claude.efforts, ["xhigh"]);
        let local = normalized(&json!({"id":"qwen-local"}), Provider::Compatible).unwrap();
        assert!(local.efforts.is_empty());
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    #[test]
    #[ignore = "Requires connected accounts; performs model discovery without inference"]
    fn connected_catalog() {
        let groups = discover(&Settings::load());
        assert!(!groups.is_empty());
        for group in groups {
            println!(
                "{}: {} models, error: {:?}",
                group.provider,
                group.models.len(),
                group.error
            );
            for model in &group.models {
                println!("  {}: {:?}", model.id, model.efforts);
            }
        }
    }
}
