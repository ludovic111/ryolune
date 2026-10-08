//! Command-line client for ryolune. Every command comes from the shared registry, so the CLI can
//! do exactly what the interface and an agent can do, on the running app or on a file.

use ryolune_engine::control;
use ryolune_tools::{coerce, merge, Backend};
use serde_json::Value;
use std::{path::PathBuf, process::ExitCode};

const USAGE: &str = "ryolune-cli — command-line client for ryolune

USAGE
  ryolune-cli [OPTIONS] <command> [--param value | param=value ...]
  ryolune-cli commands [--json]       list every command (JSON: full parameter schema)
  ryolune-cli help <command>          show one command's parameters
  ryolune-cli batch                   run JSON lines from stdin: {\"command\":\"track.add\",\"params\":{...}}
                                     one JSON result per line; stops at the first error unless --continue
  ryolune-cli doctor                  check the bridge, versions, companions, settings and plugin cache

OPTIONS
  --file <session.ryolune>   edit the file in this process and save after each change;
                            session.new creates it
  --live                    require the running ryolune app (the default when --file is absent)
  --params <json>           parameters as one JSON object, merged with --param values
  --agent                   mark created clips and notes as agent-made in the interface
  --compact                 single-line JSON output
  --continue                in batch mode, keep going after a failed line

EXAMPLES
  ryolune-cli session.info
  ryolune-cli track.add --kind midi --name Bass --instrument \"Sub Bass 808\"
  ryolune-cli clip.create --trackId track-1 --startBar 0 --lengthBars 2 \\
      --notes '[{\"start\":0,\"length\":1,\"pitch\":36},{\"start\":2,\"length\":1,\"pitch\":43}]'
  ryolune-cli --file song.ryolune session.new
  ryolune-cli --file song.ryolune session.bounce --path mix.wav

Bars and beats are zero-based; note times are beats relative to their clip.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage(message)) => {
            eprintln!("{message}\n\nRun `ryolune-cli --help` for usage.");
            ExitCode::from(2)
        }
        Err(Failure::Command(message)) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

enum Failure {
    Usage(String),
    Command(String),
}
impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure::Command(message)
    }
}

fn run(args: &[String]) -> Result<(), Failure> {
    if let Some(result) = ryolune_tools::scan_child(args) {
        return result.map_err(Failure::Command);
    }
    let mut file: Option<PathBuf> = None;
    let mut live = false;
    let mut params_json: Option<String> = None;
    let mut agent = false;
    let mut compact = false;
    let mut keep_going = false;
    let mut command: Option<String> = None;
    let mut pairs: Vec<(String, String)> = vec![];
    let mut i = 0;
    let next = |i: &mut usize, flag: &str| -> Result<String, Failure> {
        *i += 1;
        args.get(*i)
            .cloned()
            .ok_or_else(|| Failure::Usage(format!("{flag} needs a value")))
    };
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--file" | "-f" => file = Some(PathBuf::from(next(&mut i, arg)?)),
            "--live" => live = true,
            "--params" | "-p" => params_json = Some(next(&mut i, arg)?),
            "--agent" => agent = true,
            "--compact" | "-c" => compact = true,
            "--continue" => keep_going = true,
            "--version" | "-V" => {
                println!("ryolune-cli {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ if command.is_none() => {
                if arg.starts_with('-') {
                    return Err(Failure::Usage(format!("Unknown option `{arg}`")));
                }
                command = Some(arg.clone());
            }
            _ if matches!(command.as_deref(), Some("help" | "commands" | "doctor")) => {
                pairs.push((arg.clone(), String::new()))
            }
            _ => {
                if let Some(key) = arg.strip_prefix("--") {
                    if let Some((k, v)) = key.split_once('=') {
                        pairs.push((k.into(), v.into()));
                    } else {
                        pairs.push((key.into(), next(&mut i, arg)?));
                    }
                } else if let Some((k, v)) = arg.split_once('=') {
                    pairs.push((k.into(), v.into()));
                } else {
                    return Err(Failure::Usage(format!(
                        "Expected `--name value` or `name=value`, got `{arg}`"
                    )));
                }
            }
        }
        i += 1;
    }
    let Some(command) = command else {
        return Err(Failure::Usage("Missing command".into()));
    };
    if command == "doctor" {
        let report = ryolune_tools::doctor();
        if pairs.iter().any(|(k, _)| k == "--json") || compact {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).unwrap_or_default()
            );
        } else {
            for check in report["checks"].as_array().into_iter().flatten() {
                println!(
                    "{} {:<22} {}",
                    if check["ok"] == true { "ok " } else { "!! " },
                    check["check"].as_str().unwrap_or(""),
                    check["detail"].as_str().unwrap_or("")
                );
            }
        }
        return if report["ok"] == true {
            Ok(())
        } else {
            Err(Failure::Command("Some checks failed".into()))
        };
    }
    if command == "commands" {
        if pairs.iter().any(|(k, _)| k == "--json") {
            println!(
                "{}",
                serde_json::to_string_pretty(&control::describe()).unwrap_or_default()
            );
            return Ok(());
        }
        let mut family = "";
        for spec in control::COMMANDS.iter() {
            let this = spec.name.split('.').next().unwrap_or("");
            if this != family {
                family = this;
                println!("\n{}", family.to_uppercase());
            }
            let params: Vec<String> = spec
                .params
                .iter()
                .map(|p| {
                    if p.required {
                        format!("--{}", p.name)
                    } else {
                        format!("[--{}]", p.name)
                    }
                })
                .collect();
            println!("{:<28} {}", spec.name, params.join(" "));
            println!("{:<28} {}", "", spec.doc);
        }
        return Ok(());
    }
    if command == "help" {
        let Some((name, _)) = pairs.first() else {
            println!("{USAGE}");
            return Ok(());
        };
        return describe(name);
    }
    if command == "batch" {
        return batch(file.as_deref(), live, agent, keep_going);
    }
    if control::spec(&command).is_none() {
        // Let the registry produce its suggestion text.
        let mut backend = Backend::headless(None, false)?;
        backend.call(&command, &Value::Null, false)?;
        return Ok(());
    }
    let mut params: Vec<(String, Value)> = vec![];
    for (k, v) in &pairs {
        params.push((k.clone(), coerce(&command, k, v)?));
    }
    let params = merge(params_json.as_deref(), &params)?;

    control::validate_request(&command, &params)?;
    let mut backend = match (&file, live) {
        (Some(_), true) => {
            return Err(Failure::Usage("--file and --live are exclusive".into()));
        }
        (Some(path), false) => Backend::headless(Some(path), command == "session.new")?,
        (None, _) => Backend::live().map_err(|e| {
            Failure::Command(format!(
                "{e}\nStart the ryolune app for live control, or pass --file <session.ryolune> to edit a file."
            ))
        })?,
    };
    let result = ryolune_tools::printable(backend.call(&command, &params, agent)?);
    if let Some(path) = backend.autosave()? {
        eprintln!("saved {}", path.display());
    }
    let text = if compact {
        serde_json::to_string(&result)
    } else {
        serde_json::to_string_pretty(&result)
    }
    .map_err(|e| e.to_string())?;
    println!("{text}");
    Ok(())
}

/// JSON lines in, JSON lines out. One backend is kept for the whole run so file mode saves
/// once per line and live mode reuses its connection.
fn batch(
    file: Option<&std::path::Path>,
    live: bool,
    agent: bool,
    keep_going: bool,
) -> Result<(), Failure> {
    use std::io::{BufRead, Write};
    let mut backend = match (file, live) {
        (Some(_), true) => return Err(Failure::Usage("--file and --live are exclusive".into())),
        (Some(path), false) => Backend::headless(Some(path), true)?,
        (None, _) => Backend::live().map_err(|e| {
            Failure::Command(format!(
                "{e}\nStart the ryolune app for live control, or pass --file <session.ryolune> to edit a file."
            ))
        })?,
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut failed = false;
    for (index, line) in stdin.lock().lines().enumerate() {
        let line = line.map_err(|e| Failure::Command(e.to_string()))?;
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let result = ryolune_tools::parse_batch_line(&line).and_then(|(name, params)| {
            control::validate_request(&name, &params)?;
            let value = ryolune_tools::printable(backend.call(&name, &params, agent)?);
            let saved = backend.autosave()?;
            Ok((name, value, saved))
        });
        let reply = match result {
            Ok((name, value, saved)) => serde_json::json!({
                "line": index + 1, "command": name, "ok": true, "result": value,
                "saved": saved,
            }),
            Err(error) => {
                failed = true;
                serde_json::json!({ "line": index + 1, "ok": false, "error": error })
            }
        };
        let mut out = stdout.lock();
        writeln!(out, "{reply}").map_err(|e| Failure::Command(e.to_string()))?;
        out.flush().map_err(|e| Failure::Command(e.to_string()))?;
        if failed && !keep_going {
            return Err(Failure::Command("Batch stopped at the first error".into()));
        }
    }
    if failed {
        Err(Failure::Command("Some batch lines failed".into()))
    } else {
        Ok(())
    }
}

fn describe(name: &str) -> Result<(), Failure> {
    let Some(spec) = control::spec(name) else {
        return Err(Failure::Command(format!(
            "Unknown command `{name}`. Run `ryolune-cli commands`."
        )));
    };
    println!(
        "{}\n  {}\n  {}\n",
        spec.name,
        spec.doc,
        if spec.mutates {
            "Changes the session, transport or files."
        } else {
            "Read only."
        }
    );
    if spec.params.is_empty() {
        println!("  No parameters.");
    }
    for p in spec.params {
        println!(
            "  --{:<16} {:<8} {}{}",
            p.name,
            p.kind.schema_type(),
            if p.required { "" } else { "(optional) " },
            p.doc
        );
    }
    Ok(())
}
