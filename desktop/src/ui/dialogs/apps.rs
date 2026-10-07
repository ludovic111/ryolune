//! The music apps ryolune brings songs from (`interop::apps::APPS`), as a grid of their real
//! logos: first-run setup ("Coming from") and File › Import from / Export for Another App.
//! Every app listed is one ryolune really opens songs from (DAWproject from Bitwig Studio,
//! Studio One, Cubase and REAPER; stems and MIDI from the others). Logos and their sources:
//! `desktop/assets/logos/NOTICE.md`.

use super::plugins::logo_tile;
use crate::ui::theme::{size, Theme};
use gpui::{div, prelude::*, px, App, SharedString, Window};
use ryolune_engine::interop::apps::APPS;
use std::rc::Rc;

/// An app's logo in `assets/logos`.
pub(crate) fn logo(id: &str) -> Option<&'static str> {
    Some(match id {
        "ableton" => "logos/daw-ableton-live.svg",
        "logic" => "logos/daw-logic-pro.svg",
        "fl" => "logos/daw-fl-studio.svg",
        "bitwig" => "logos/daw-bitwig-studio.svg",
        "reaper" => "logos/daw-reaper.svg",
        "cubase" => "logos/daw-cubase.svg",
        "studioone" => "logos/daw-studio-one.svg",
        "protools" => "logos/daw-pro-tools.svg",
        "garageband" => "logos/daw-garageband.svg",
        _ => return None,
    })
}

type Pick = Rc<dyn Fn(Option<usize>, &mut Window, &mut App)>;

/// Three columns of app tiles (logo and name) and "Another app"; the chosen one is inverted.
pub(crate) fn picker(
    id: &'static str,
    selected: Option<usize>,
    other: &'static str,
    on_pick: impl Fn(Option<usize>, &mut Window, &mut App) + 'static,
    cx: &App,
) -> gpui::Div {
    let theme = Theme::get(cx).clone();
    let on_pick: Pick = Rc::new(on_pick);
    let tile = |index: Option<usize>, name: &'static str, logo: Option<&'static str>| {
        let on = selected == index;
        let pick = on_pick.clone();
        div()
            .id(SharedString::from(format!(
                "{id}-{}",
                index.map_or("other".to_string(), |i| i.to_string())
            )))
            .w(gpui::relative(0.333))
            .p(px(2.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .h(px(36.0))
                    .px(px(6.0))
                    .border_1()
                    .border_color(if on { theme.accent_fill } else { theme.line })
                    .bg(if on {
                        theme.accent_fill
                    } else {
                        theme.bg_raised
                    })
                    .cursor_pointer()
                    .when(!on, |d| d.hover(|s| s.bg(theme.hover)))
                    .children(logo.map(|path| logo_tile(path, 24.0, cx)))
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(size::SM))
                            .text_color(if on { theme.text_on_accent } else { theme.text })
                            .when(on, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                            .child(name),
                    ),
            )
            .on_click(move |_, window, cx| pick(index, window, cx))
    };
    let mut grid = div().flex().flex_wrap().w_full();
    for (i, app) in APPS.iter().enumerate() {
        grid = grid.child(tile(Some(i), app.name, logo(app.id)));
    }
    grid.child(tile(None, other, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_app_ryolune_opens_has_its_logo_bundled() {
        for app in APPS {
            let path = logo(app.id).unwrap_or_else(|| panic!("{} has a logo", app.id));
            assert!(
                crate::ui::assets::Assets::get(path).is_some(),
                "{path} is bundled"
            );
            assert!(
                !app.opens.is_empty(),
                "{} is listed because ryolune opens it",
                app.id
            );
        }
        for format in ["clap", "vst3", "au"] {
            let path = format!("logos/format-{format}.svg");
            assert!(crate::ui::assets::Assets::get(&path).is_some(), "{path}");
        }
        assert!(crate::ui::assets::Assets::get("providers/lsuite.svg").is_some());
    }
}
