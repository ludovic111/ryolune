#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod account;
mod agent;
mod agents;
mod app;
mod control;
mod conversations;
mod diagnostics;
mod discovery;
mod export;
mod generate;
mod interop;
mod native;
mod plugins;
mod recovery;
mod settings;
mod ui;
mod update;

/// Asks the window to run its next tick soon. Called from any thread: the control bridge,
/// workers and the agent use it so a waiting request is answered at once.
pub(crate) type Wake = std::sync::Arc<dyn Fn() + Send + Sync>;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let mut screenshot = None;
    let mut control = true;
    let mut show_agents = false;
    let mut check_updates = std::env::var_os("RYOLUNE_NO_UPDATE").is_none_or(|v| v.is_empty());
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("ryolune {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--validate" => {
                let file = args
                    .next()
                    .ok_or("Usage: ryolune --validate session.ryolune")?;
                let (s, _) = ryolune_engine::document::load(std::path::Path::new(&file))?;
                println!(
                    "Valid session: {} ({} tracks, {} clips)",
                    s.name,
                    s.tracks.len(),
                    s.clips.len()
                );
                return Ok(());
            }
            "--bounce" => {
                let file = args
                    .next()
                    .ok_or("Usage: ryolune --bounce session.ryolune output.wav")?;
                let target = args.next().ok_or("Missing WAV output path")?;
                let (s, library) = ryolune_engine::document::load(std::path::Path::new(&file))?;
                ryolune_engine::render::bounce(&s, &library, std::path::Path::new(&target), 48000)?;
                println!("Exported {target}");
                return Ok(());
            }
            "--scan-plugin" => {
                // Child process used by the scanner: probe one bundle and print JSON.
                let format = args.next().ok_or("Missing plugin format")?;
                let bundle = args.next().ok_or("Missing plugin path")?;
                let format = ryolune_engine::plugin::Format::parse(&format!("{format}:x"))
                    .map(|(f, _)| f)
                    .ok_or("Unknown plugin format")?;
                let result: std::result::Result<Vec<ryolune_engine::plugin::Descriptor>, String> =
                    ryolune_engine::host::scan::probe(format, std::path::Path::new(&bundle));
                println!("{}", serde_json::to_string(&result)?);
                return Ok(());
            }
            "--scan-plugins" => {
                let cache =
                    ryolune_engine::host::scan::scan_all(|path| eprintln!("Scanning {path}"));
                for d in cache.descriptors() {
                    println!(
                        "{:<5} {:<40} {:<24} {}",
                        d.format.label(),
                        d.name,
                        d.vendor,
                        d.id
                    );
                }
                for e in cache.entries.iter().filter(|e| e.error.is_some()) {
                    eprintln!("{}: {}", e.path, e.error.clone().unwrap_or_default());
                }
                return Ok(());
            }
            "--plugins" => {
                for d in ryolune_engine::host::scan::installed() {
                    println!(
                        "{:<6} {:<40} {:<24} {}",
                        d.format.label(),
                        d.name,
                        d.vendor,
                        d.id
                    );
                }
                return Ok(());
            }
            "--screenshot" => {
                screenshot = Some(std::path::PathBuf::from(
                    args.next().ok_or("Missing screenshot path")?,
                ))
            }
            "--no-control" => control = false,
            "--agents" => show_agents = true,
            "--update" => {
                match update::check()? {
                    None => println!("ryolune {} is up to date", update::current_version()),
                    Some(release) => {
                        println!("Downloading ryolune {}…", release.version);
                        let target = update::install(&release)?;
                        println!(
                            "Installed ryolune {} at {}",
                            release.version,
                            target.display()
                        );
                    }
                }
                return Ok(());
            }
            "--no-update-check" => check_updates = false,
            "--release-keygen" => {
                let out = std::path::PathBuf::from(
                    args.next()
                        .ok_or("Usage: ryolune --release-keygen <secret-key-file>")?,
                );
                let public = update::write_keypair(&out)?;
                println!("Public key (put it in desktop/assets/update-signing.pub):\n{public}");
                println!("Secret key written to {} (keep it in the RYOLUNE_SIGNING_KEY GitHub secret, never in the repository)", out.display());
                return Ok(());
            }
            "--sign-release" => {
                let key = args
                    .next()
                    .ok_or("Usage: ryolune --sign-release <secret-key-file> <SHA256SUMS>")?;
                let file = args.next().ok_or("Missing the file to sign")?;
                let path =
                    update::sign_file(std::path::Path::new(&key), std::path::Path::new(&file))?;
                println!("Wrote {}", path.display());
                return Ok(());
            }
            "--verify-release" => {
                let file = args
                    .next()
                    .ok_or("Usage: ryolune --verify-release <SHA256SUMS>")?;
                update::verify_file(std::path::Path::new(&file))?;
                println!("Signature valid for {file}");
                return Ok(());
            }
            "--help" | "-h" => {
                println!("ryolune — native Rust DAW\n  ryolune [session.ryolune]\n  ryolune --validate session.ryolune\n  ryolune --bounce session.ryolune output.wav\n  ryolune --scan-plugins\n  ryolune --plugins\n  ryolune --agents       open the Agents tab\n  ryolune --no-control   disable CLI / MCP connections\n  ryolune --screenshot image.png\n  ryolune --update            install the latest GitHub release\n  ryolune --no-update-check   skip the startup update check (or set RYOLUNE_NO_UPDATE=1)\n  ryolune --release-keygen <file>          create a release signing key pair\n  ryolune --sign-release <key> <file>      write <file>.sig for a release\n  ryolune --verify-release <file>          check <file>.sig against the built-in public key");
                return Ok(());
            }
            _ => {
                if arg.starts_with('-') {
                    return Err(format!("Unknown option: {arg}").into());
                }
                path = Some(std::path::PathBuf::from(arg));
            }
        }
    }
    // The window process logs to `<data>/logs`, writes crash reports and notices a previous
    // run that ended without quitting; the one-shot modes above only print.
    let started = ryolune_engine::diagnostics::init(&ryolune_engine::host::scan::data_dir());
    update::cleanup();
    ui::run(
        path,
        screenshot,
        control,
        check_updates,
        show_agents,
        started.unclean,
    );
    ryolune_engine::diagnostics::clean_exit();
    Ok(())
}
