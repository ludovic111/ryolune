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

pub(crate) fn run(turn: Turn) -> Result<()> {
    let key = turn
        .settings
        .api_key(Provider::Anthropic)
        .ok_or("No Anthropic API key. Add one in Settings > Agent or set ANTHROPIC_API_KEY.")?;
    let model = turn.settings.model();
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
            .post(ENDPOINT)
            .header("x-api-key", &key)
            .header("anthropic-version", VERSION)
            .header("content-type", "application/json")
            .send(body.to_string())
            .map_err(|e| format!("Could not reach the Anthropic API: {e}"))?;
        if response.status() != 200 {
            let text = response.body_mut().read_to_string().unwrap_or_default();
            return Err(format!(
                "Anthropic API error {}: {}",
                response.status(),
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
                    return Err(format!(
                        "Anthropic API error: {}",
                        event["error"]["message"].as_str().unwrap_or("unknown")
                    ));
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
