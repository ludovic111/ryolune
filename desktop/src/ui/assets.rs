//! Files bundled into the binary: fonts, icons, provider logos and the logos of the plugin
//! formats and music apps ryolune works with (`desktop/assets`, sources in
//! `assets/logos/NOTICE.md`). Icons
//! are ryolune's own drawings and a few from Lucide (ISC, `icons/LICENSE.lucide.txt`); the
//! mark is written by `scripts/gen-mark.py`.

use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

#[derive(rust_embed::RustEmbed)]
#[folder = "assets"]
#[include = "icons/*.svg"]
#[include = "providers/*.svg"]
#[include = "logos/*.svg"]
#[include = "fonts/*.ttf"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(Self::get(path).map(|file| file.data))
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Self::iter()
            .filter(|p| p.starts_with(path))
            .map(|p| SharedString::from(p.to_string()))
            .collect())
    }
}

/// The two OFL families every lsuite app ships (design system v2): Chakra Petch for the
/// interface, IBM Plex Mono for numbers, time and labels in caps.
pub fn fonts() -> Vec<Cow<'static, [u8]>> {
    Assets::iter()
        .filter(|p| p.starts_with("fonts/") && p.ends_with(".ttf"))
        .filter_map(|p| Assets::get(&p).map(|f| f.data))
        .collect()
}

/// An icon's path in the bundle, for `svg().path(..)`.
pub fn icon(name: &str) -> SharedString {
    format!("icons/{name}.svg").into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fonts_and_icons_are_bundled() {
        // Chakra Petch 400-700 and IBM Plex Mono 400-600.
        assert_eq!(fonts().len(), 7);
        for name in [
            "play",
            "stop",
            "record",
            "cycle",
            "ring",
            "mark",
            "chevron-down",
        ] {
            assert!(
                Assets::get(&format!("icons/{name}.svg")).is_some(),
                "{name}"
            );
        }
    }
}
