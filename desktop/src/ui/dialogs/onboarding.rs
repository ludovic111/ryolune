//! The first-run setup: which app the person comes from (it decides the steps File › Import
//! from Another App shows first), whether they want AI features, the agent provider, a look
//! at the sound output, and how to start (the demo song, an empty song, or a song from
//! another app). Every answer goes through `app.finishOnboarding`; starting goes through the
//! same New / Demo / Import paths as the File menu. It shows on the first start and from
//! Help › Set Up ryolune…; Escape puts it off until the next start.
//!
//! Minimal on purpose: the existing sheet, rows and controls, nothing restyled.

use super::{modal, Dialogs};
use crate::ui::{
    daw::Daw,
    widgets::{Button, Switch},
};
use gpui::{div, prelude::*, px, Context, Entity};
use ryolune_engine::interop::apps::APPS;
use serde_json::json;

/// How the person starts once the setup is done.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Start {
    /// Keep (or reopen) the demo song.
    Demo,
    Empty,
    Import,
    /// Close the setup and go to a Settings section.
    Settings(&'static str),
    /// Skip: the answers so far are kept.
    Skip,
}

/// Record the answers and start the way the person chose.
pub(crate) fn finish(daw: &mut Daw, start: Start, cx: &mut Context<Daw>) {
    let interop = &daw.app.interop;
    let coming_from = interop
        .from
        .and_then(|i| APPS.get(i))
        .map_or("none", |a| a.id);
    let mut params = json!({ "comingFrom": coming_from, "ai": interop.ai });
    if start == Start::Skip {
        params["skipped"] = json!(true);
    }
    if daw.run("app.finishOnboarding", params, cx).is_none() {
        return;
    }
    daw.app.interop.show_onboarding = false;
    match start {
        Start::Demo => daw.app.request(crate::app::Intent::Demo),
        Start::Empty => daw.app.request(crate::app::Intent::New),
        Start::Import => daw.app.app_dialog(true),
        Start::Settings(section) => {
            daw.run(
                "ui.showPanel",
                json!({ "panel": "settings", "visible": true, "section": section }),
                cx,
            );
        }
        Start::Skip => {}
    }
    cx.notify();
}

impl Dialogs {
    pub(super) fn onboarding_sheet(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let app = &self.daw.read(cx).app;
        let interop = &app.interop;
        let from = interop.from.and_then(|i| APPS.get(i));
        let ai = interop.ai;
        let provider = app.settings.agent.provider.label();
        let device = app
            .device
            .as_ref()
            .map(|d| format!("{} · {} kHz", d.device_name, d.sample_rate as f64 / 1000.0))
            .unwrap_or_else(|| "No output device is open yet".into());
        let daw = self.daw.clone();

        let select = {
            let daw = daw.clone();
            super::apps::picker(
                "onboarding-app",
                interop.from,
                "Nothing yet, or another",
                move |index, _, cx| {
                    daw.update(cx, |d, cx| {
                        d.app.interop.from = index;
                        cx.notify();
                    })
                },
                cx,
            )
        };
        let mut body = modal::body("onboarding-body")
            .child(modal::text(
                "ryolune is a free music studio. A few questions so it starts the way you work; you can change all of it later in Settings.",
                cx,
            ))
            .child(modal::stacked(
                "Coming from",
                Some("The app you made music in until now.".into()),
                select,
                cx,
            ));
        if let Some(app) = from {
            body = body.child(modal::note(
                format!("Bringing a song from {}: {}", app.name, app.bring),
                cx,
            ));
        }
        body = body.child(modal::field_row(
            "AI features",
            Some("The agent panel and sound generation from a description. Nothing is hidden either way.".into()),
            Switch::new("onboarding-ai", ai).on_toggle({
                let daw = daw.clone();
                move |on, _, cx| {
                    daw.update(cx, |d, cx| {
                        d.app.interop.ai = on;
                        cx.notify();
                    })
                }
            }),
            cx,
        ));
        if ai {
            // lsuite AI first (lsuite's AI.md): signing in is the whole setup.
            let account = &app.account;
            let signed_in = account.signed_in();
            let waiting = account.waiting_for_browser();
            let detail = if signed_in {
                let summary = account.summary();
                format!(
                    "lsuite AI · {}",
                    if summary.is_empty() {
                        "signed in".into()
                    } else {
                        summary
                    }
                )
            } else if waiting {
                "Finish signing in in your browser.".to_string()
            } else {
                "lsuite AI: no setup. Sign in and your agent works. Or use Codex, Claude Code, an API key or a local model.".to_string()
            };
            let lsuite_chosen =
                app.settings.agent.provider == ryolune_engine::settings::Provider::Lsuite;
            let row = div()
                .flex()
                .gap(px(8.0))
                .when(!signed_in || !lsuite_chosen, |d| {
                    d.child(
                        Button::new(
                            "onboarding-lsuite",
                            if signed_in {
                                "Use lsuite AI"
                            } else {
                                "Sign in"
                            },
                        )
                        .primary()
                        .disabled(waiting)
                        .on_click({
                            let daw = daw.clone();
                            move |_, _, cx| {
                                daw.update(cx, |d, cx| {
                                    d.run(
                                        "settings.set",
                                        json!({ "path": "agent.provider", "value": "lsuite" }),
                                        cx,
                                    );
                                    if !d.app.account.signed_in() {
                                        d.run("account.signIn", json!({}), cx);
                                    }
                                    cx.notify();
                                })
                            }
                        }),
                    )
                })
                .child(
                    Button::new("onboarding-provider", "Other services…")
                        .ghost()
                        .on_click(click(&daw, Start::Settings("agent"))),
                );
            let _ = provider;
            body = body.child(modal::field_row("Agent", Some(detail.into()), row, cx));
        }
        body = body.child(modal::field_row(
            "Sound",
            Some(device.into()),
            Button::new("onboarding-audio", "Audio settings…")
                .on_click(click(&daw, Start::Settings("audio"))),
            cx,
        ));
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .pt(px(4.0))
                .child(modal::note("Start with", cx))
                .child(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .child(
                            Button::new("onboarding-demo", "The demo song")
                                .primary()
                                .on_click(click(&daw, Start::Demo)),
                        )
                        .child(
                            Button::new("onboarding-empty", "An empty song")
                                .on_click(click(&daw, Start::Empty)),
                        )
                        .child(
                            Button::new("onboarding-import", "A song from another app…")
                                .on_click(click(&daw, Start::Import)),
                        ),
                ),
        );
        modal::sheet("onboarding", "Welcome to ryolune", 560.0, None, cx)
            .child(body)
            .child(
                modal::footer(cx).child(
                    Button::new("onboarding-skip", "Skip")
                        .ghost()
                        .tooltip("Keep the answers so far and start")
                        .on_click(click(&daw, Start::Skip)),
                ),
            )
    }
}

fn click(
    daw: &Entity<Daw>,
    start: Start,
) -> impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static {
    let daw = daw.clone();
    move |_, _, cx| daw.update(cx, |d, cx| finish(d, start, cx))
}
