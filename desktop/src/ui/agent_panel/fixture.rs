//! Debug builds only: show the panel in states a fresh profile never reaches, to check its
//! look without sending anything to a paid service. `RYOLUNE_AGENT_FIXTURE=1` fills the
//! conversation with a sample exchange (messages, steps, a failure, Markdown) and treats the
//! service as connected; `RYOLUNE_AGENT_TAB=generate|changes|takes` opens that tab;
//! `RYOLUNE_AGENT_TOP=1` keeps the conversation at its top; `RYOLUNE_AGENT_DRAFT=/mi` writes
//! the message box; `RYOLUNE_AGENT_MODELS=1` opens the model menu.

use super::{connection::Connection, AgentPanel, Tab};
use crate::agent::{Entry, Role, ToolRecord};
use gpui::Context;
use serde_json::json;

const REPLY: &str = "I'll write a sixteenth-note variation on **Bass verse** and add gentle bus compression to the drums.\n\n## What changed\n\n- Bass: a busier line on bars 13–20, same notes, *tighter* rhythm\n- Drums: `ryolune Comp` at 4:1, 2–3 dB of glue\n  - attack 10 ms\n1. Listen from bar 13\n2. Undo from **Changes** if it is too much\n\n> There was no “Drum glue” preset, so I set the compressor by hand.\n\n```\nstrip.setParameter ratio=4\n```\n\n| Track | Change |\n|---|---|\n| Bass | variation |\n| Drums | glue |\n\nMore in the [user guide](https://lsuite.xyz/ryolune).";

impl AgentPanel {
    pub(super) fn apply_fixture(&mut self, cx: &mut Context<Self>) {
        if let Ok(tab) = std::env::var("RYOLUNE_AGENT_TAB") {
            self.tab = match tab.as_str() {
                "generate" => Tab::Generate,
                "changes" => Tab::Changes,
                "takes" => Tab::Takes,
                _ => Tab::Chat,
            };
        }
        if let Ok(draft) = std::env::var("RYOLUNE_AGENT_DRAFT") {
            self.composer
                .update(cx, |input, cx| input.set_text(draft, cx));
            self.focus_composer = true;
        }
        if std::env::var_os("RYOLUNE_AGENT_MODELS").is_some() {
            self.models.open = true;
            let daw = self.daw.clone();
            self.models.refresh(&daw, cx);
        }
        // Start at the top of the conversation instead of its end.
        self.force_follow = std::env::var_os("RYOLUNE_AGENT_TOP").is_none();
        if std::env::var_os("RYOLUNE_AGENT_FIXTURE").is_none() {
            return;
        }
        let provider = self.daw.read(cx).app.settings.agent.provider.key();
        self.connection = Some(Connection {
            provider: provider.into(),
            state: "configured".into(),
            message: String::new(),
        });
        self.connection_key = Some((false, provider));
        let tool = |name: &str, args: serde_json::Value, ok: bool| Entry {
            role: Role::Tool,
            text: String::new(),
            tool: Some(ToolRecord {
                name: name.into(),
                args,
                result: Some(if ok {
                    Ok(json!({}))
                } else {
                    Err("No preset named “Drum glue”".into())
                }),
                sequence: None,
            }),
            streaming: false,
        };
        let text = |role, text: &str, streaming| Entry {
            role,
            text: text.into(),
            tool: None,
            streaming,
        };
        self.daw.update(cx, |daw, _| {
            daw.app.agents.runtime.transcript = vec![
                text(Role::User, "Give the second verse a busier bass line and glue the drums a little.\n\n[Selected in the window: region \"Bass verse\" (clipId c1, MIDI, bars 5 to 13); track \"Bass\" (trackId t2, instrument)]", false),
                tool("session_overview", json!({}), true),
                tool("clip.list", json!({}), true),
                tool("clip.duplicate", json!({"clipId": "c1"}), true),
                tool("clip.setNotes", json!({"clipId": "c2", "notes": [1, 2, 3, 4]}), true),
                tool("session.batch", json!({"commands": [{}, {}, {}]}), true),
                tool("preset.load", json!({"name": "Drum glue"}), false),
                text(Role::Assistant, REPLY, false),
                text(Role::User, "Nice. Now a short riser into the chorus?", false),
                text(Role::Assistant, "Sure: a **four-bar** riser on a new track, ending on", true),
                text(Role::Notice, "Claude Code used ryolune-mcp 0.12.0.", false),
            ];
        });
        if !self.force_follow {
            let runtime = &self.daw.read(cx).app.agents.runtime;
            let last = runtime.transcript.last();
            self.seen = (
                runtime.first_id + runtime.transcript.len() as u64,
                last.map_or(0, |e| e.text.len()),
                last.is_some_and(|e| e.streaming),
            );
        }
    }
}
