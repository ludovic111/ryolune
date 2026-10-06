//! File › Open Recent…: the songs opened lately (`app.recent`), newest first, each with its
//! folder; a song that moved or was deleted says so and cannot be opened. Opening one asks
//! about unsaved changes first, then opens it like File › Open (`app.openRecent` does the
//! same for scripts). The menus have no submenus, so this is a sheet.

use super::{modal, Dialogs};
use crate::ui::{theme::size, theme::Theme, widgets::Button};
use gpui::{div, prelude::*, px, Context};
use std::path::PathBuf;

impl Dialogs {
    pub(super) fn recent_sheet(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let theme = Theme::get(cx).clone();
        let settings = &self.daw.read(cx).app.settings;
        let songs = ryolune_engine::control_interop::recent(settings);
        let mut body = modal::body("recent-body");
        if songs.is_empty() {
            body = body.child(modal::note(
                "No songs yet. Songs you open or save show here.",
                cx,
            ));
        }
        for (i, song) in songs.into_iter().enumerate() {
            let path = song["path"].as_str().unwrap_or_default().to_string();
            let name = song["name"].as_str().unwrap_or_default().to_string();
            let folder = song["folder"].as_str().unwrap_or_default().to_string();
            let exists = song["exists"].as_bool().unwrap_or(false);
            let daw = self.daw.clone();
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .py(px(8.0))
                    .border_b_1()
                    .border_color(theme.hairline)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(size::BASE))
                                    .text_color(if exists { theme.text } else { theme.text_3 })
                                    .child(name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(size::XS))
                                    .text_color(theme.text_3)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(if exists {
                                        folder
                                    } else {
                                        format!("Not found: {folder}")
                                    }),
                            ),
                    )
                    .child(
                        Button::new(("recent-open", i), "Open")
                            .compact()
                            .disabled(!exists)
                            .tooltip(format!("Open “{name}”"))
                            .on_click(move |_, _, cx| {
                                let path = PathBuf::from(&path);
                                daw.update(cx, |daw, cx| {
                                    daw.app.open_recent(path);
                                    cx.notify();
                                });
                            }),
                    ),
            );
        }
        let daw = self.daw.clone();
        modal::sheet(
            "recent",
            "Open recent",
            520.0,
            Some(Box::new(|window, cx| {
                window.dispatch_action(Box::new(modal::Dismiss), cx)
            })),
            cx,
        )
        .child(body)
        .child(
            modal::footer(cx)
                .child(
                    Button::new("recent-other", "Open other…").on_click(move |_, _, cx| {
                        daw.update(cx, |daw, cx| {
                            daw.app.interop.show_recent = false;
                            daw.app.request(crate::app::Intent::Open);
                            cx.notify();
                        })
                    }),
                )
                .child(
                    Button::new("recent-done", "Done")
                        .primary()
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(modal::Dismiss), cx)
                        }),
                ),
        )
    }
}
