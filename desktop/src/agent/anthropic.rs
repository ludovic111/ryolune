//! The Anthropic Messages API with streaming and tool use.

use super::{
    await_tool, bounded, http, read_line_limited, system_prompt, take_steering, tool_output,
    tool_specs, user_text, Event, Message, Part, ToolCall, Turn,
};
use ryolune_engine::{settings::Provider, Result};
use serde_json::{json, Value};
use std::{io::BufReader, sync::atomic::Ordering, sync::mpsc};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const VERSION: &str = "2023-06-01";
const LINE_LIMIT: usize = 4 * 1024 * 1024;

fn tools() -> Vec<Value> {
    tool_specs()
        .into_iter()
        .map(|(_, name, doc, schema)| json!({ "name": name, "description": doc, "input_schema": schema }))
        .collect()
}
fn wire(messages: &[Message]) -> Vec<Value> {
    messages
        .iter()
        .map(|m| {
            let content: Vec<Value> = m
                .parts
                .iter()
                .map(|part| match part {
                    Part::Text(text) => json!({ "type": "text", "text": text }),
                    Part::ToolUse { id, name, input } => {
                        json!({ "type": "tool_use", "id": id, "name": name, "input": input })
                    }
                    Part::ToolResult { id, output, is_error, .. } => json!({
                        "type": "tool_result", "tool_use_id": id, "content": output, "is_error": is_error
                    }),
                })
                .collect();
            json!({ "role": m.role, "content": content })
        })
        .collect()
}

/// Where the Messages API is and how to reach it: Anthropic with the person's key, or the
/// lsuite AI endpoint (the same API at `<server>/api/ai`) with the lsuite account's token.
struct Endpoint {
    url: String,
    key: String,
    /// Errors are named after it ("Anthropic API", "lsuite AI").
    label: &'static str,
    lsuite: bool,
}

pub(crate) fn run(turn: Turn) -> Result<()> {
    let key = turn
        .settings
        .api_key(Provider::Anthropic)
        .ok_or("No Anthropic API key. Add one in Settings > Agent or set ANTHROPIC_API_KEY.")?;
    let model = turn.settings.model();
    run_on(
        turn,
        Endpoint {
            url: ENDPOINT.into(),
            key,
            label: "Anthropic API",
            lsuite: false,
        },
        model,
    )
}

/// lsuite AI: the Anthropic provider on the subscription's endpoint, with the account that
/// every lsuite app shares. Never falls back to another provider.
pub(crate) fn run_lsuite(turn: Turn) -> Result<()> {
    let account = ryolune_engine::account::load().ok_or(
        "Sign in to lsuite AI first: Settings › Agent › lsuite AI › Sign in. No other setup is needed.",
    )?;
    let mut model = turn.settings.agent.model.trim().to_string();
    if model.is_empty() {
        let _ = turn.events.send(Event::Status(
            "Asking lsuite AI for your plan's models…".into(),
        ));
        model = lsuite_model(&account)?;
    }
    run_on(
        turn,
        Endpoint {
            url: format!("{}/v1/messages", account.ai_base()),
            key: account.token.clone(),
            label: "lsuite AI",
            lsuite: true,
        },
        model,
    )
}

/// The plan's model to use when none is chosen: the plan's default as the server names it,
/// else a Sonnet when the plan has one, else the first the server lists.
pub(crate) fn lsuite_model(account: &ryolune_engine::account::Account) -> Result<String> {
    use ryolune_engine::account::Failure;
    // The plan names its default (`defaultModel` in /api/account/me); else pick from the list.
    if let Ok(me) = ryolune_engine::account::me(&account.server, &account.token) {
        if let Some(model) = me["defaultModel"].as_str().filter(|m| !m.is_empty()) {
            return Ok(model.to_string());
        }
    }
    let models = ryolune_engine::account::models(account).map_err(|f| match f {
        Failure::Unauthorized => f.message(),
        other => format!("lsuite AI: {}", other.message()),
    })?;
    let ids: Vec<&str> = models.iter().filter_map(|m| m["id"].as_str()).collect();
    ids.iter()
        .find(|id| id.contains("sonnet"))
        .or_else(|| ids.first())
        .map(|id| id.to_string())
        .ok_or_else(|| "Your lsuite AI plan has no models. Manage plan to choose one.".into())
}

fn run_on(turn: Turn, endpoint: Endpoint, model: String) -> Result<()> {
    let mut history = turn.history.clone();
    history.push(Message {
        role: "user",
        parts: vec![Part::Text(user_text(&turn.prompt, &turn.session_summary))],
    });
    let agent = http();
    let tools = tools();
    let system = system_prompt(&turn.settings);
    let mut rounds = 0;
    loop {
        if turn.cancel.load(Ordering::Acquire) {
            let _ = turn.events.send(Event::Done {
                error: None,
                cancelled: true,
                history,
            });
            return Ok(());
        }
        let _ = turn
            .events
            .send(Event::Status(format!("Thinking with {model}…")));
        let mut body = json!({
            "model": model,
            "max_tokens": turn.settings.agent.max_output_tokens,
            "system": system,
            "tools": tools,
            "messages": wire(&history),
            "stream": true,
        });
        if !turn.settings.agent.reasoning_effort.is_empty() {
            body["output_config"] = json!({"effort":turn.settings.agent.reasoning_effort});
        }
        let mut response = agent
            .post(&endpoint.url)
            .header("x-api-key", &endpoint.key)
            .header("anthropic-version", VERSION)
            .header("content-type", "application/json")
            .send(body.to_string())
            .map_err(|e| format!("Could not reach the {}: {e}", endpoint.label))?;
        if response.status() != 200 {
            let status = response.status().as_u16();
            let text = response.body_mut().read_to_string().unwrap_or_default();
            if endpoint.lsuite {
                if ryolune_engine::account::allowance_error(status, &text) {
                    // One line, with Manage plan in the panel; never another provider.
                    return Err(ryolune_engine::account::ALLOWANCE_MESSAGE.into());
                }
                if status == 401 {
                    return Err(ryolune_engine::account::Failure::Unauthorized.message());
                }
            }
            return Err(format!(
                "{} error {status}: {}",
                endpoint.label,
                api_error(&text)
            ));
        }
        let mut reader = BufReader::new(response.body_mut().as_reader());
        let mut text = String::new();
        let mut blocks: Vec<(String, String, String)> = vec![]; // id, name, json
        let mut current: Option<usize> = None;
        let mut stop_reason = String::new();
        let mut data = String::new();
        let mut completed = false;
        loop {
            if turn.cancel.load(Ordering::Acquire) {
                let _ = turn.events.send(Event::Done {
                    error: None,
                    cancelled: true,
                    history,
                });
                return Ok(());
            }
            let Some(line) = read_line_limited(&mut reader, LINE_LIMIT)
                .map_err(|e| format!("Stream ended: {e}"))?
            else {
                break;
            };
            let line = line.trim_end();
            if let Some(payload) = line.strip_prefix("data:") {
                if data.len() + payload.len() > LINE_LIMIT {
                    return Err("Stream event is too large".into());
                }
                data.push_str(payload.trim());
                continue;
            }
            if !line.is_empty() {
                continue;
            }
            if data.is_empty() {
                continue;
            }
            let event: Value =
                serde_json::from_str(&data).map_err(|e| format!("Invalid stream event: {e}"))?;
            data.clear();
            match event["type"].as_str().unwrap_or("") {
                "message_start" => {
                    let usage = &event["message"]["usage"];
                    let _ = turn.events.send(Event::Usage {
                        input: usage["input_tokens"].as_u64().unwrap_or(0),
                        output: 0,
                    });
                }
                "content_block_start" => {
                    let block = &event["content_block"];
                    if block["type"] == "tool_use" {
                        if blocks.len() >= 128 {
                            return Err("Too many tool calls in one response".into());
                        }
                        blocks.push((
                            block["id"].as_str().unwrap_or("").into(),
                            block["name"].as_str().unwrap_or("").into(),
                            String::new(),
                        ));
                        current = Some(blocks.len() - 1);
                        let _ = turn.events.send(Event::Status(format!(
                            "Preparing {}…",
                            block["name"]
                                .as_str()
                                .unwrap_or("a tool")
                                .replacen('_', ".", 1)
                        )));
                    } else {
                        current = None;
                    }
                }
                "content_block_delta" => {
                    let delta = &event["delta"];
                    if let Some(t) = delta["text"].as_str() {
                        if text.len() + t.len() > 1024 * 1024 {
                            return Err("Agent response exceeds 1 MiB".into());
                        }
                        text.push_str(t);
                        let _ = turn.events.send(Event::Text {
                            text: t.into(),
                            replace: false,
                        });
                    } else if let (Some(index), Some(partial)) =
                        (current, delta["partial_json"].as_str())
                    {
                        if blocks[index].2.len() + partial.len() > 1024 * 1024 {
                            return Err("Tool arguments exceed 1 MiB".into());
                        }
                        blocks[index].2.push_str(partial);
                    }
                }
                "message_delta" => {
                    stop_reason = event["delta"]["stop_reason"].as_str().unwrap_or("").into();
                    let _ = turn.events.send(Event::Usage {
                        input: 0,
                        output: event["usage"]["output_tokens"].as_u64().unwrap_or(0),
                    });
                }
                "message_stop" => {
                    completed = true;
                    break;
                }
                "error" => {
                    let message = event["error"]["message"].as_str().unwrap_or("unknown");
                    if endpoint.lsuite
                        && ryolune_engine::account::allowance_error(429, &event.to_string())
                    {
                        return Err(ryolune_engine::account::ALLOWANCE_MESSAGE.into());
                    }
                    return Err(format!("{} error: {message}", endpoint.label));
                }
                _ => {}
            }
        }
        if !completed {
            return Err("The response stream ended before completion. Try again.".into());
        }
        if stop_reason == "max_tokens" {
            return Err("The response reached its token limit. Increase it in Agent settings; no incomplete tool was run.".into());
        }
        if !text.is_empty() {
            let _ = turn.events.send(Event::TextEnd);
        }
        let mut parts = vec![];
        if !text.is_empty() {
            parts.push(Part::Text(bounded(&text, super::TEXT_LIMIT)));
        }
        for (id, name, input) in &blocks {
            let input: Value = if input.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(input).map_err(|e| format!("Invalid tool arguments: {e}"))?
            };
            parts.push(Part::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input,
            });
        }
        if parts.is_empty() {
            parts.push(Part::Text(String::new()));
        }
        history.push(Message {
            role: "assistant",
            parts,
        });
        if stop_reason != "tool_use" || blocks.is_empty() {
            // Steering that came in while the answer was written: one more round for it.
            if let Some(steering) = take_steering(&turn.steer) {
                let _ = turn.events.send(Event::Steered);
                history.push(Message {
                    role: "user",
                    parts: vec![Part::Text(steering)],
                });
                continue;
            }
            let _ = turn.events.send(Event::Done {
                error: None,
                cancelled: false,
                history,
            });
            return Ok(());
        }
        rounds += 1;
        if rounds > turn.settings.agent.max_tool_rounds {
            let _ = turn.events.send(Event::Done {
                error: Some(format!(
                    "Stopped after {} tool rounds (Settings > Agent raises the limit).",
                    turn.settings.agent.max_tool_rounds
                )),
                cancelled: false,
                history,
            });
            return Ok(());
        }
        let mut results = vec![];
        for (id, name, input) in blocks {
            let input: Value = if input.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(&input).map_err(|e| format!("Invalid tool arguments: {e}"))?
            };
            let (tx, rx) = mpsc::sync_channel(1);
            turn.events
                .send(Event::ToolCall(ToolCall {
                    name: name.clone(),
                    args: input,
                    reply: tx,
                }))
                .map_err(|_| "The interface stopped listening".to_string())?;
            let result = await_tool(&rx, &turn.cancel);
            let (output, is_error) = tool_output(&result);
            results.push(Part::ToolResult {
                id,
                name,
                output,
                is_error,
            });
        }
        // Steering joins the tool results, so the next call reads it without losing them.
        if let Some(steering) = take_steering(&turn.steer) {
            let _ = turn.events.send(Event::Steered);
            results.push(Part::Text(steering));
        }
        history.push(Message {
            role: "user",
            parts: results,
        });
    }
}

fn api_error(text: &str) -> String {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| bounded(text, 600))
}

#[cfg(test)]
mod lsuite_tests {
    use super::*;
    use ryolune_engine::account::{self, mock, Account};

    fn events(rx: &mpsc::Receiver<Event>) -> (String, Option<String>) {
        let mut text = String::new();
        let mut error = None;
        while let Ok(event) = rx.recv_timeout(std::time::Duration::from_secs(10)) {
            match event {
                Event::Text { text: t, .. } => text.push_str(&t),
                Event::Done { error: e, .. } => {
                    error = e;
                    break;
                }
                _ => {}
            }
        }
        (text, error)
    }

    #[test]
    fn lsuite_ai_answers_on_the_plan_and_says_when_the_allowance_is_used_up() {
        // The account file is the test process's scratch one (no LSUITE_HOME to race on).
        let server = mock::start();
        account::save(&Account {
            format: 1,
            server: server.url.clone(),
            email: "ada@example.com".into(),
            name: "Ada".into(),
            plan: "pro".into(),
            token: mock::TOKEN.into(),
            signed_in_at: String::new(),
        })
        .unwrap();
        let mut settings = ryolune_engine::settings::Settings::default();
        assert_eq!(
            settings.agent.provider,
            Provider::Lsuite,
            "lsuite AI comes first"
        );
        settings.agent.max_tool_rounds = 2;

        let (tx, rx) = mpsc::sync_channel(64);
        let worker = std::thread::spawn(move || run_lsuite(Turn::test("Hi", settings, tx)));
        let (text, error) = events(&rx);
        worker.join().unwrap().unwrap();
        assert_eq!(text, "Hello from lsuite AI.");
        assert_eq!(error, None);
        let requests = server.requests.lock().unwrap().clone();
        // No model chosen: the plan's list picks a Sonnet, then the Messages API.
        assert!(
            requests.contains(&"GET /api/ai/v1/models".to_string()),
            "{requests:?}"
        );
        assert!(
            requests.contains(&"POST /api/ai/v1/messages".to_string()),
            "{requests:?}"
        );

        server
            .exhausted
            .store(true, std::sync::atomic::Ordering::Release);
        let mut settings = ryolune_engine::settings::Settings::default();
        settings.agent.model = "claude-sonnet-5".into();
        let (tx, _rx) = mpsc::sync_channel(64);
        let error = run_lsuite(Turn::test("Hi", settings, tx)).unwrap_err();
        assert_eq!(error, account::ALLOWANCE_MESSAGE);
        assert!(crate::ui::agent_panel::connection::allowance_used(&error));

        account::remove().unwrap();
        let (tx, _rx) = mpsc::sync_channel(64);
        let error = run_lsuite(Turn::test("Hi", Default::default(), tx)).unwrap_err();
        assert!(error.starts_with("Sign in to lsuite AI"), "{error}");
    }
}
