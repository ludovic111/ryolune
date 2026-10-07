//! `docs/COMMANDS.md` is generated from the command registry, so the reference can never
//! drift from what the window, the CLI, MCP and the agent actually accept. When a command
//! changes, regenerate it:
//!
//! ```sh
//! RYOLUNE_BLESS=1 cargo test -p ryolune-tools --test command_docs
//! ```
use ryolune_engine::{control, control_app};
use serde_json::Value;
use std::fmt::Write;

fn reference() -> String {
    let commands = control::describe();
    let commands = commands.as_array().expect("describe() lists commands");
    let mut families: Vec<(&str, Vec<&Value>)> = Vec::new();
    for c in commands {
        let family = c["name"].as_str().unwrap().split('.').next().unwrap();
        match families.iter_mut().find(|(f, _)| *f == family) {
            Some((_, list)) => list.push(c),
            None => families.push((family, vec![c])),
        }
    }
    let mut out = String::new();
    out.push_str("# Command reference\n\n");
    out.push_str(
        "<!-- Generated from the command registry by tools/tests/command_docs.rs. \
         Do not edit by hand: run `RYOLUNE_BLESS=1 cargo test -p ryolune-tools --test command_docs`. -->\n\n",
    );
    let _ = writeln!(
        out,
        "ryolune has {} commands. The window, `ryolune-cli`, `ryolune-mcp` and the built-in agent all \
         run these same commands, with the same undo history. On the CLI a command is \
         `ryolune-cli <name> --param value`; in MCP it is the tool `<name>` with the dot replaced \
         by an underscore (`track.add` is `track_add`); the agent sees the same tools.\n",
        commands.len()
    );
    out.push_str(
        "Conventions: bars and beats are zero-based; note `start` and `length` are beats relative \
         to their clip; pitch 60 is C4; velocity is 1–127; a fader value of 0.75 is unity gain. \
         Strip commands accept a track id or `master`, `bus-a`, `bus-b`; insert slots are 0–7.\n\n",
    );
    out.push_str(
        "**Edits** marks a command that can change the song, the transport, settings or files \
         (it is one undo step when it changes the song). **Needs the app** marks a command only \
         the running window can serve; the others also work on a file (`ryolune-cli --file song.ryolune …`).\n\n",
    );
    out.push_str(
        "Names shared across the lsuite apps are accepted too, and run the ryolune command \
         beside them: ",
    );
    out.push_str(
        &control::ALIASES
            .iter()
            .map(|(alias, real)| format!("`{alias}` → `{real}`"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    out.push_str(".\n\n");
    out.push_str("## Families\n\n");
    for (family, list) in &families {
        let _ = writeln!(
            out,
            "- [{family}](#{family}) — {}",
            list.iter()
                .map(|c| format!("`{}`", c["name"].as_str().unwrap()))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    for (family, list) in &families {
        let _ = write!(out, "\n## {family}\n");
        for c in list {
            let name = c["name"].as_str().unwrap();
            let mut tags = Vec::new();
            if c["mutates"].as_bool().unwrap_or(false) {
                tags.push("Edits");
            }
            if control_app::is_live_only(name) {
                tags.push("Needs the app");
            }
            let _ = write!(out, "\n### `{name}`\n\n");
            if !tags.is_empty() {
                let _ = writeln!(out, "*{}*\n", tags.join(" · "));
            }
            let _ = writeln!(out, "{}", c["description"].as_str().unwrap_or("").trim());
            let params = c["params"].as_array().unwrap();
            if params.is_empty() {
                continue;
            }
            out.push_str("\n| Parameter | Type | Required | Description |\n|---|---|---|---|\n");
            for p in params {
                let _ = writeln!(
                    out,
                    "| `{}` | {} | {} | {} |",
                    p["name"].as_str().unwrap(),
                    p["type"].as_str().unwrap_or("any"),
                    if p["required"].as_bool().unwrap_or(false) {
                        "yes"
                    } else {
                        ""
                    },
                    p["description"]
                        .as_str()
                        .unwrap_or("")
                        .replace('|', "\\|")
                        .replace('\n', " ")
                );
            }
        }
    }
    out
}

#[test]
fn the_command_reference_matches_the_registry() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/COMMANDS.md");
    let fresh = reference();
    if std::env::var_os("RYOLUNE_BLESS").is_some() {
        std::fs::write(&path, &fresh).unwrap();
        return;
    }
    let current = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        current == fresh,
        "docs/COMMANDS.md is out of date: run `RYOLUNE_BLESS=1 cargo test -p ryolune-tools --test command_docs`"
    );
}

/// `docs/HARNESS.md`: the agent's brief and every skill, as the agents get them (lsuite's
/// HARNESS.md parts 1 and 2), generated from the same text.
fn harness_reference() -> String {
    use ryolune_engine::harness;
    let mut out = String::new();
    out.push_str("# The agent harness: brief and skills\n\n");
    out.push_str(
        "<!-- Generated from engine/src/harness by tools/tests/command_docs.rs. Do not edit by \
         hand: edit engine/src/harness/brief.md or skills/*.md, then run \
         `RYOLUNE_BLESS=1 cargo test -p ryolune-tools --test command_docs`. -->\n\n",
    );
    out.push_str(
        "Every ryolune agent works from this text: the built-in agent's system prompt is the \
         brief (with a paragraph about the panel), `ryolune-mcp` sends it as its `instructions`, \
         and `harness.brief` returns it. The skills are loaded with `harness.skill name=…`; over \
         MCP each is also a prompt and the resource `ryolune://skills/<name>`. See \
         [AI_CONTROL.md](AI_CONTROL.md#the-agent-harness) for the commands.\n\n",
    );
    out.push_str("| Skill | When |\n| --- | --- |\n");
    for s in harness::skills() {
        let _ = writeln!(out, "| [`{}`](#{}) | {} |", s.name, s.name, s.when);
    }
    out.push_str("\n---\n\n");
    // The brief's own headings move down a level under this page's.
    for line in harness::brief().lines() {
        if let Some(rest) = line.strip_prefix('#') {
            let _ = writeln!(out, "##{rest}");
        } else {
            let _ = writeln!(out, "{line}");
        }
    }
    for s in harness::skills() {
        let _ = writeln!(out, "\n---\n\n<a id=\"{}\"></a>\n", s.name);
        let _ = writeln!(
            out,
            "## Skill `{}`: {}\n\n*When:* {}\n",
            s.name, s.title, s.when
        );
        for line in s.body.lines() {
            if line.starts_with("# ") {
                continue;
            }
            if let Some(rest) = line.strip_prefix("## ") {
                let _ = writeln!(out, "### {rest}");
            } else {
                let _ = writeln!(out, "{line}");
            }
        }
    }
    out
}

#[test]
fn the_harness_reference_matches_the_brief_and_skills() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/HARNESS.md");
    let fresh = harness_reference();
    if std::env::var_os("RYOLUNE_BLESS").is_some() {
        std::fs::write(&path, &fresh).unwrap();
        return;
    }
    let current = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        current == fresh,
        "docs/HARNESS.md is out of date: run `RYOLUNE_BLESS=1 cargo test -p ryolune-tools --test command_docs`"
    );
}
