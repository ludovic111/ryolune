//! Release notes built into this copy: every `docs/releases/<version>.md` (see `build.rs`).
//!
//! The window opens What's New once after an update, with every release since the version
//! that ran before (`general.lastRunVersion` in the settings, [`unseen`]), and on request
//! (Help › What's New, the palette, Settings › Updates). `app.whatsNew` returns the same text.
//! A test fails when the workspace version has no notes, so a release cannot forget them.

use serde::Serialize;

include!(concat!(env!("OUT_DIR"), "/releases.rs"));

/// The version of this copy.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// Every release's notes on the web.
pub const RELEASES_URL: &str = "https://github.com/ludovic111/ryolune/releases";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    /// The notes' Markdown without their `# ryolune X.Y.Z` heading.
    pub notes: String,
}

/// `0.13.0` (or `v0.13.0`) as numbers, to compare.
pub fn version_key(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.trim().trim_start_matches('v').split('.');
    let mut next = || -> Option<u64> {
        let part = parts.next()?;
        part.split(|c: char| !c.is_ascii_digit())
            .next()?
            .parse()
            .ok()
    };
    let major = next()?;
    let minor = next()?;
    Some((major, minor, next().unwrap_or(0)))
}

/// `a` is a later version than `b`.
pub fn newer(a: &str, b: &str) -> bool {
    matches!((version_key(a), version_key(b)), (Some(a), Some(b)) if a > b)
}

fn release(version: &str, text: &str) -> Release {
    let text = text.trim_start();
    let body = match text.strip_prefix("# ") {
        Some(rest) => rest.split_once('\n').map_or("", |(_, body)| body),
        None => text,
    };
    Release {
        version: version.to_string(),
        notes: body.trim().to_string(),
    }
}

/// Every release this copy carries up to itself, newest first.
pub fn all() -> Vec<Release> {
    let mut out: Vec<Release> = RELEASES
        .iter()
        .filter(|(v, _)| version_key(v).is_some() && !newer(v, CURRENT))
        .map(|(v, text)| release(v, text))
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(version_key(&r.version)));
    out
}

/// One release's notes (a leading `v` is fine).
pub fn find(version: &str) -> Option<Release> {
    let key = version_key(version)?;
    all()
        .into_iter()
        .find(|r| version_key(&r.version) == Some(key))
}

/// Releases after `since`, up to and including this copy, newest first.
pub fn since(since: &str) -> Vec<Release> {
    all()
        .into_iter()
        .filter(|r| newer(&r.version, since))
        .collect()
}

/// What the window shows by itself when it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unseen {
    /// The version that ran before; `None` when this profile predates the record (0.13 and
    /// earlier), and only this version's notes show.
    pub since: Option<String>,
}

/// Whether to open What's New once on this start: after an update (the last version that ran
/// is older than this one), or the first start of a profile that already existed before the
/// last version was recorded. Not on a fresh install, a second start or a downgrade.
pub fn unseen(last_run: Option<&str>, existing_profile: bool) -> Option<Unseen> {
    match last_run.map(str::trim).filter(|v| !v.is_empty()) {
        Some(last) => (newer(CURRENT, last) && !since(last).is_empty()).then(|| Unseen {
            since: Some(last.to_string()),
        }),
        None => (existing_profile && find(CURRENT).is_some()).then_some(Unseen { since: None }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_workspace_version_has_release_notes() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../docs/releases/{CURRENT}.md"));
        assert!(
            path.is_file(),
            "docs/releases/{CURRENT}.md is missing: every version in Cargo.toml needs its release notes"
        );
        let notes = find(CURRENT).expect("the notes are built in");
        assert!(!notes.notes.is_empty());
        assert!(!notes.notes.starts_with("# "), "the heading is dropped");
        assert_eq!(all()[0].version, CURRENT);
        assert!(find(&format!("v{CURRENT}")).is_some());
    }

    #[test]
    fn versions_compare_as_numbers() {
        assert!(newer("0.10.0", "0.9.0"));
        assert!(newer("v1.0", "0.13.2"));
        assert!(!newer("0.13.0", "0.13.0"));
        assert!(!newer("junk", "0.1.0"));
        assert_eq!(version_key("0.11.1"), Some((0, 11, 1)));
        assert_eq!(version_key("0.14.0-beta.1"), Some((0, 14, 0)));
    }

    #[test]
    fn release_notes_since_a_version_are_newest_first() {
        let shown = since("0.11.0");
        assert!(shown.iter().any(|r| r.version == "0.11.1"));
        assert!(shown.iter().all(|r| r.version != "0.11.0"));
        assert_eq!(shown[0].version, CURRENT);
        assert!(shown
            .windows(2)
            .all(|w| newer(&w[0].version, &w[1].version)));
        assert!(since(CURRENT).is_empty());
        assert_eq!(
            release("1.0.0", "# ryolune 1.0.0\n\nHello\n").notes,
            "Hello"
        );
    }

    #[test]
    fn whats_new_opens_once_after_an_update_only() {
        // A fresh install has nothing to compare with.
        assert_eq!(unseen(None, false), None);
        // A profile from before the record shows this version.
        assert_eq!(unseen(None, true), Some(Unseen { since: None }));
        // An update shows everything since.
        assert_eq!(
            unseen(Some("0.11.0"), true),
            Some(Unseen {
                since: Some("0.11.0".into())
            })
        );
        // The same version again, or a downgrade: nothing.
        assert_eq!(unseen(Some(CURRENT), true), None);
        assert_eq!(unseen(Some("99.0.0"), true), None);
    }
}
