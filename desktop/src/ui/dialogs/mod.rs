//! Dialogs over the window: an error, the unsaved-changes question (New, Open, Quit,
//! Relaunch, Recover), the feedback warning for monitoring, an update, Export (audio, stems,
//! MIDI) and MIDI import, Recovery and the shortcut sheet. One shows at a time, the most
//! urgent first; closing it shows the next, so an export that fails returns to its form
//! after the error.
//!
//! Every dialog is a glass sheet over the scrim ([`modal`]): Escape or the close key
//! dismisses it, Enter takes its primary action and focus returns where it was. What the
//! dialogs change goes through registry commands; the export form edits the host's draft
//! (`Ryolune::export`), whose file chooser runs the `session.export*` / `session.importMidi`
//! commands.

pub mod export_sheet;
pub mod help;
pub mod modal;
pub mod onboarding;
pub mod recent;

use super::{
    daw::Daw,
    theme::{size, Theme},
    widgets::{Button, MenuHost},
};
use crate::app::Intent;
use gpui::{div, prelude::*, px, ClipboardItem, Context, Entity, MouseButton, Window};
use modal::{Accept, Dismiss, ModalFocus};
use serde_json::json;

/// The dialog on top, most urgent first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Shown {
    Error,
    Prompt,
    Monitor,
    Update,
    Export,
    Recovery,
    Help,
    /// The first-run setup.
    Onboarding,
    /// File › Open Recent….
    Recent,
}

pub struct Dialogs {
    daw: Entity<Daw>,
    focus: ModalFocus,
    menu: MenuHost,
    export: export_sheet::ExportForm,
    /// "Keep muted" was chosen; the warning returns the next time monitoring is blocked.
    monitor_dismissed: bool,
    /// The error text was copied (the Copy key says so until the next error).
    copied: bool,
}

impl Dialogs {
    pub fn new(daw: Entity<Daw>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        modal::bind(cx);
        // A dialog opens from anywhere (a menu, the CLI, a worker): redraw when the host says.
        cx.observe(&daw, |_, _, cx| cx.notify()).detach();
        Self {
            export: export_sheet::ExportForm::new(window, cx),
            focus: ModalFocus::new(cx),
            menu: MenuHost::default(),
            monitor_dismissed: false,
            copied: false,
            daw,
        }
    }

    /// Which dialog shows, if any.
    pub(crate) fn shown(&self, cx: &gpui::App) -> Option<Shown> {
        let app = &self.daw.read(cx).app;
        let monitor = matches!(
            app.monitoring,
            ryolune_engine::device::Monitoring::FeedbackRisk { .. }
        );
        if app.error.is_some() {
            Some(Shown::Error)
        } else if app.intent.is_some() {
            Some(Shown::Prompt)
        } else if monitor && !self.monitor_dismissed {
            Some(Shown::Monitor)
        } else if app.interop.show_onboarding {
            Some(Shown::Onboarding)
        } else if app.updates.show
            && (app.updates.available.is_some() || app.updates.installed.is_some())
        {
            Some(Shown::Update)
        } else if app.export.open {
            Some(Shown::Export)
        } else if app.interop.show_recent {
            Some(Shown::Recent)
        } else if app.recovery.open {
            Some(Shown::Recovery)
        } else if app.show_help {
            Some(Shown::Help)
        } else {
            None
        }
    }

    fn panel(&mut self, panel: &str, visible: bool, cx: &mut Context<Self>) {
        self.daw.update(cx, |daw, cx| {
            daw.run(
                "ui.showPanel",
                json!({ "panel": panel, "visible": visible }),
                cx,
            );
        });
    }

    /// Escape, the close key or Cancel on the dialog on top.
    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        match self.shown(cx) {
            Some(Shown::Error) => self.daw.update(cx, |daw, cx| {
                daw.fire("ui.dismissError", cx);
            }),
            Some(Shown::Prompt) => self.confirm("cancel", cx),
            Some(Shown::Monitor) => {
                self.monitor_dismissed = true;
                cx.notify();
            }
            Some(Shown::Update) => self.daw.update(cx, |daw, cx| {
                // Hiding the offer is window state: the update stays known in the title bar.
                daw.app.updates.show = false;
                cx.notify();
            }),
            Some(Shown::Export) => {
                if !self.daw.read(cx).app.export.busy() {
                    self.panel("export", false, cx);
                }
            }
            Some(Shown::Recovery) => self.panel("recovery", false, cx),
            Some(Shown::Help) => self.panel("help", false, cx),
            // Later: the setup shows again at the next start.
            Some(Shown::Onboarding) => self.daw.update(cx, |daw, cx| {
                daw.app.interop.show_onboarding = false;
                cx.notify();
            }),
            Some(Shown::Recent) => self.daw.update(cx, |daw, cx| {
                daw.app.interop.show_recent = false;
                cx.notify();
            }),
            None => {}
        }
    }

    /// Enter: the primary action of the dialog on top.
    fn accept(&mut self, _: &Accept, window: &mut Window, cx: &mut Context<Self>) {
        match self.shown(cx) {
            Some(Shown::Prompt) => {
                let dirty = self.daw.read(cx).app.store.dirty();
                self.confirm(if dirty { "save" } else { "discard" }, cx);
            }
            Some(Shown::Update) => self.update_action(cx),
            Some(Shown::Export) => self.export.start(&self.daw, cx),
            Some(Shown::Recovery) => {}
            Some(Shown::Onboarding) => self.daw.update(cx, |daw, cx| {
                onboarding::finish(daw, onboarding::Start::Demo, cx)
            }),
            _ => self.dismiss(&Dismiss, window, cx),
        }
    }

    fn confirm(&mut self, choice: &str, cx: &mut Context<Self>) {
        self.daw.update(cx, |daw, cx| {
            daw.run("app.confirm", json!({ "choice": choice }), cx);
        });
    }

    /// Install the update, or relaunch into the installed one.
    fn update_action(&mut self, cx: &mut Context<Self>) {
        self.daw.update(cx, |daw, cx| {
            let updates = &daw.app.updates;
            if updates.installed.is_some() {
                daw.fire("app.relaunch", cx);
            } else if !updates.busy() {
                daw.fire("app.installUpdate", cx);
            }
        });
    }

    fn error_sheet(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let message = self.daw.read(cx).app.error.clone().unwrap_or_default();
        let copied = self.copied;
        let dismiss = cx.listener(|this, _, window, cx| this.dismiss(&Dismiss, window, cx));
        modal::sheet("error", "ryolune", 460.0, Some(close(cx)), cx)
            .child(modal::body("error-body").child(modal::text(message.clone(), cx)))
            .child(
                modal::footer(cx)
                    .child(
                        Button::new("error-copy", if copied { "Copied" } else { "Copy" })
                            .ghost()
                            .with_icon("copy")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(message.clone()));
                                this.copied = true;
                                cx.notify();
                            })),
                    )
                    .child(Button::new("error-ok", "OK").primary().on_click(dismiss)),
            )
    }

    fn prompt_sheet(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let app = &self.daw.read(cx).app;
        let name = app
            .store
            .session()
            .name
            .trim_end_matches(".ryolune")
            .to_string();
        let dirty = app.store.dirty();
        let intent = app.intent.unwrap_or(Intent::Quit);
        let copy = prompt_copy(intent, &name, dirty, app.updates.installed.is_some());
        let mut footer = modal::footer(cx).child(
            Button::new("prompt-cancel", "Cancel")
                .on_click(cx.listener(|this, _, _, cx| this.confirm("cancel", cx))),
        );
        if dirty {
            footer = footer
                .child(
                    Button::new("prompt-discard", "Don't save")
                        .danger()
                        .on_click(cx.listener(|this, _, _, cx| this.confirm("discard", cx))),
                )
                .child(
                    Button::new("prompt-save", "Save")
                        .primary()
                        .on_click(cx.listener(|this, _, _, cx| this.confirm("save", cx))),
                );
        } else {
            footer = footer.child(
                Button::new("prompt-go", copy.verb)
                    .primary()
                    .on_click(cx.listener(|this, _, _, cx| this.confirm("discard", cx))),
            );
        }
        modal::sheet("prompt", copy.title, 440.0, Some(close(cx)), cx)
            .child(modal::body("prompt-body").child(modal::text(copy.body, cx)))
            .child(footer)
    }

    fn monitor_sheet(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        modal::sheet("monitor", "Monitoring would feed back", 440.0, Some(close(cx)), cx)
            .child(modal::body("monitor-body").child(modal::text(
                "The built-in microphone is playing through the built-in speakers. Monitoring it makes a loud howl, so it is muted. Plug in headphones to hear yourself safely.",
                cx,
            )))
            .child(
                modal::footer(cx)
                    .child(
                        Button::new("monitor-anyway", "Monitor anyway").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.daw.update(cx, |daw, cx| {
                                    daw.run(
                                        "audio.allowSpeakerMonitoring",
                                        json!({ "allow": true }),
                                        cx,
                                    );
                                })
                            },
                        )),
                    )
                    .child(Button::new("monitor-keep", "Keep muted").primary().on_click(
                        cx.listener(|this, _, _, cx| {
                            this.monitor_dismissed = true;
                            cx.notify();
                        }),
                    )),
            )
    }

    fn update_sheet(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let theme = Theme::get(cx).clone();
        let updates = &self.daw.read(cx).app.updates;
        let current = crate::update::current_version();
        let installing = updates.installing.is_some();
        let installed = updates.installed.is_some();
        let release = updates.available.clone();
        let version = release
            .as_ref()
            .map_or_else(String::new, |r| r.version.clone());
        let title = if installed {
            format!("ryolune {version} is installed")
        } else if installing {
            format!("Installing ryolune {version}…")
        } else {
            format!("ryolune {version} is available")
        };
        let mut body = modal::body("update-body");
        if installed {
            body = body.child(modal::text(
                "Relaunch to start using it. If the session has unsaved changes, ryolune asks to save them first.",
                cx,
            ));
        } else if let Some(release) = &release {
            body = body
                .child(modal::text(
                    format!(
                        "You have {current}. Every update is free and included. The download is checked against ryolune's signature before it replaces anything."
                    ),
                    cx,
                ))
                .child(modal::note(
                    format!(
                        "{:.1} MB · {}",
                        release.size as f64 / 1_048_576.0,
                        if release.signature_url.is_some() {
                            "signed release"
                        } else {
                            "checksums only"
                        }
                    ),
                    cx,
                ));
            let notes = release.notes.trim();
            if !notes.is_empty() {
                let notes: String = notes.chars().take(2400).collect();
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(size::SM))
                                .text_color(theme.text_3)
                                .child("What's new"),
                        )
                        .child(modal::well(notes, cx)),
                );
            }
        }
        let primary = if installed {
            Some("Relaunch")
        } else if installing {
            None
        } else {
            Some("Install update")
        };
        modal::sheet("update", title, 520.0, Some(close(cx)), cx)
            .child(body)
            .child(
                modal::footer(cx)
                    .child(Button::new("update-later", "Later").on_click(
                        cx.listener(|this, _, window, cx| this.dismiss(&Dismiss, window, cx)),
                    ))
                    .when(installing, |d| {
                        d.child(
                            Button::new("update-busy", "Downloading…")
                                .primary()
                                .disabled(true),
                        )
                    })
                    .when_some(primary, |d, label| {
                        d.child(
                            Button::new("update-go", label)
                                .primary()
                                .on_click(cx.listener(|this, _, _, cx| this.update_action(cx))),
                        )
                    }),
            )
    }

    fn recovery_sheet(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let theme = Theme::get(cx).clone();
        let recovery = &self.daw.read(cx).app.recovery;
        let status =
            (recovery.error.is_some() || recovery.latest.is_some()).then(|| recovery.status());
        let working = recovery.working();
        let candidates: Vec<_> = recovery
            .candidates
            .iter()
            .map(|s| (s.path.clone(), s.title.clone(), s.modified, s.bytes))
            .collect();
        let mut body = modal::body("recovery-body").child(modal::text(
            "ryolune keeps a copy of an edited session while it sits idle. Opening one makes an unsaved copy; the snapshot itself is kept.",
            cx,
        ));
        if candidates.is_empty() {
            body = body.child(modal::note(
                if working {
                    "Looking for snapshots…"
                } else {
                    "No recovery snapshots."
                },
                cx,
            ));
        }
        for (i, (path, title, modified, bytes)) in candidates.into_iter().enumerate() {
            let target = path.clone();
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
                                    .text_color(theme.text)
                                    .child(title.clone()),
                            )
                            .child(modal::note(
                                format!(
                                    "{} · {} KB",
                                    crate::recovery::age(modified),
                                    bytes.div_ceil(1024)
                                ),
                                cx,
                            ))
                            .child(
                                div()
                                    .text_size(px(size::XS))
                                    .text_color(theme.text_3)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(path.display().to_string()),
                            ),
                    )
                    .child(
                        Button::new(("recover", i), "Open copy")
                            .compact()
                            .tooltip(format!("Open a copy of “{title}”"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let path = target.display().to_string();
                                this.daw.update(cx, |daw, cx| {
                                    daw.run("session.restoreSnapshot", json!({ "path": path }), cx);
                                });
                            })),
                    ),
            );
        }
        if let Some(status) = status {
            body = body.child(modal::note(status, cx));
        }
        modal::sheet("recovery", "Recover session", 560.0, Some(close(cx)), cx)
            .child(body)
            .child(
                modal::footer(cx)
                    .child(
                        Button::new("recovery-folder", "Show folder")
                            .ghost()
                            .with_icon("folder")
                            .on_click(|_, _, _| {
                                crate::settings::reveal(&ryolune_engine::recovery::directory())
                            }),
                    )
                    .child(
                        Button::new("recovery-refresh", "Refresh")
                            .disabled(working)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.panel("recovery", true, cx)),
                            ),
                    )
                    .child(Button::new("recovery-done", "Done").primary().on_click(
                        cx.listener(|this, _, window, cx| this.dismiss(&Dismiss, window, cx)),
                    )),
            )
    }

    fn help_sheet(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        modal::sheet("help", "Working in ryolune", 760.0, Some(close(cx)), cx)
            .child(modal::body("help-body").children(help::content(cx)))
    }
}

/// The close key of a sheet: the same as Escape.
fn close(_: &Context<Dialogs>) -> modal::CloseHandler {
    Box::new(|window, cx| window.dispatch_action(Box::new(Dismiss), cx))
}

/// What the unsaved-changes question says for each reason it is asked.
pub(crate) struct PromptCopy {
    pub title: String,
    pub body: String,
    /// The primary action when nothing is unsaved (confirmation before quitting).
    pub verb: &'static str,
}

pub(crate) fn prompt_copy(intent: Intent, name: &str, dirty: bool, updated: bool) -> PromptCopy {
    let (before, verb) = match intent {
        Intent::New => ("before starting a new session", "New session"),
        Intent::Open => ("before opening another session", "Open…"),
        Intent::Recover => ("before opening the recovered copy", "Open copy"),
        Intent::Demo => ("before opening the demo", "Open demo"),
        Intent::Quit => ("before quitting", "Quit"),
        Intent::Relaunch if updated => ("before relaunching into the update", "Relaunch"),
        Intent::Relaunch => ("before relaunching", "Relaunch"),
        Intent::OpenRecent => ("before opening another song", "Open"),
        Intent::ImportFrom => ("before opening the imported song", "Import"),
    };
    if dirty {
        PromptCopy {
            title: format!("Save changes to “{name}” {before}?"),
            body: "If you don't save, the changes since the last save are lost.".into(),
            verb,
        }
    } else {
        PromptCopy {
            title: match intent {
                Intent::Quit => "Quit ryolune?".into(),
                Intent::Relaunch => "Relaunch ryolune?".into(),
                _ => format!("{verb}?"),
            },
            body:
                "Everything is saved. You asked to confirm before quitting in Settings › General."
                    .into(),
            verb,
        }
    }
}

impl Render for Dialogs {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shown = self.shown(cx);
        self.focus.sync(shown.is_some(), window, cx);
        if self.daw.read(cx).app.error.is_none() {
            self.copied = false;
        }
        if !matches!(
            self.daw.read(cx).app.monitoring,
            ryolune_engine::device::Monitoring::FeedbackRisk { .. }
        ) {
            self.monitor_dismissed = false;
        }
        self.export.follow(&self.daw, window, cx);
        let Some(shown) = shown else {
            self.menu.close();
            return div().absolute().size_0().into_any_element();
        };
        let sheet = match shown {
            Shown::Error => self.error_sheet(cx),
            Shown::Prompt => self.prompt_sheet(cx),
            Shown::Monitor => self.monitor_sheet(cx),
            Shown::Update => self.update_sheet(cx),
            Shown::Export => {
                let daw = self.daw.clone();
                self.export.sheet(&daw, window, cx)
            }
            Shown::Recovery => self.recovery_sheet(cx),
            Shown::Help => self.help_sheet(cx),
            Shown::Onboarding => self.onboarding_sheet(cx),
            Shown::Recent => self.recent_sheet(cx),
        };
        // Sheets that only inform close on a click outside; forms and questions do not.
        let outside = matches!(shown, Shown::Help | Shown::Recovery | Shown::Recent);
        modal::layer("dialogs", &self.focus.handle, cx)
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::accept))
            .when(outside, |d| {
                d.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.dismiss(&Dismiss, window, cx)),
                )
            })
            .child(sheet)
            .children(self.menu.render(window, cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_question_names_the_session_and_what_happens_next() {
        let copy = prompt_copy(Intent::Quit, "Nightfall", true, false);
        assert_eq!(copy.title, "Save changes to “Nightfall” before quitting?");
        let copy = prompt_copy(Intent::Quit, "Nightfall", false, false);
        assert_eq!(copy.title, "Quit ryolune?");
        assert_eq!(copy.verb, "Quit");
        let copy = prompt_copy(Intent::Relaunch, "Song", true, true);
        assert!(copy.title.contains("relaunching into the update"));
    }

    #[gpui::test]
    fn errors_come_first_and_nothing_shows_when_idle(cx: &mut gpui::TestAppContext) {
        let daw = cx.new(|_| {
            Daw::new(crate::app::Ryolune::from_session(
                ryolune_engine::store::empty(),
                None,
            ))
        });
        cx.update(|cx| cx.set_global(Theme::new(super::super::theme::Mode::Dark, true)));
        let window = cx.add_window(|window, cx| Dialogs::new(daw.clone(), window, cx));
        let dialogs = window.root(cx).unwrap();
        cx.update(|cx| {
            assert_eq!(dialogs.read(cx).shown(cx), None);
            daw.update(cx, |daw, _| {
                daw.app.show_help = true;
                daw.app.export.open = true;
            });
            assert_eq!(dialogs.read(cx).shown(cx), Some(Shown::Export));
            daw.update(cx, |daw, _| daw.app.error = Some("Disk full".into()));
            assert_eq!(dialogs.read(cx).shown(cx), Some(Shown::Error));
            daw.update(cx, |daw, _| {
                daw.app.error = None;
                daw.app.export.open = false;
            });
            assert_eq!(dialogs.read(cx).shown(cx), Some(Shown::Help));
        });
    }

    #[gpui::test]
    fn escape_and_enter_answer_the_dialog_not_the_window(cx: &mut gpui::TestAppContext) {
        let daw = cx.new(|_| {
            Daw::new(crate::app::Ryolune::from_session(
                ryolune_engine::store::empty(),
                None,
            ))
        });
        cx.update(|cx| {
            cx.set_global(Theme::new(super::super::theme::Mode::Dark, true));
            crate::ui::actions::bind(cx);
        });
        let (_, cx) = cx.add_window_view(|window, cx| Dialogs::new(daw.clone(), window, cx));
        cx.update(|_, cx| {
            daw.update(cx, |daw, cx| {
                daw.app.show_help = true;
                cx.notify()
            })
        });
        cx.run_until_parked();
        // Space is Play anywhere else; over a dialog it does nothing.
        cx.simulate_keystrokes("space");
        cx.update(|_, cx| assert!(!daw.read(cx).app.playing));
        cx.simulate_keystrokes("escape");
        cx.update(|_, cx| assert!(!daw.read(cx).app.show_help, "Escape closes the sheet"));
        cx.update(|_, cx| {
            daw.update(cx, |daw, cx| {
                daw.app.recovery.open = true;
                cx.notify()
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            daw.update(cx, |daw, cx| {
                daw.app.error = Some("Disk full".into());
                cx.notify()
            })
        });
        cx.run_until_parked();
        // Enter is also Go to Beginning; over the error it is OK.
        cx.simulate_keystrokes("enter");
        cx.update(|_, cx| {
            assert!(daw.read(cx).app.error.is_none());
            assert!(
                daw.read(cx).app.recovery.open,
                "the dialog under it returns"
            );
        });
    }
}
