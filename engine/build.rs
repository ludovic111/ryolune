//! Builds the release notes (`docs/releases/<version>.md`) into the engine, so the window's
//! What's New sheet and `app.whatsNew` read them without the repository at hand. Any file
//! added there is picked up: there is no list to keep in step.

use std::{env, fmt::Write, fs, path::PathBuf};

fn main() {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../docs/releases");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut releases: Vec<(String, PathBuf)> = fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .filter_map(|p| {
            let version = p.file_stem()?.to_str()?.to_string();
            Some((version, p.canonicalize().unwrap_or(p)))
        })
        .collect();
    releases.sort();
    let mut out = String::from(
        "/// Every `docs/releases/<version>.md`, as (version, Markdown), by file name.\npub(crate) const RELEASES: &[(&str, &str)] = &[\n",
    );
    for (version, path) in &releases {
        println!("cargo:rerun-if-changed={}", path.display());
        let _ = writeln!(
            out,
            "    ({version:?}, include_str!({:?})),",
            path.display().to_string()
        );
    }
    out.push_str("];\n");
    let target = PathBuf::from(env::var("OUT_DIR").unwrap()).join("releases.rs");
    fs::write(target, out).unwrap();
}
