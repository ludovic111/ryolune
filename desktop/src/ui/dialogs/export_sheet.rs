//! Export (File > Export Audio, ⌘B; Export MIDI) and MIDI import. The form edits the host's
//! draft (`Ryolune::export`): rate, file type, encoding or Ogg quality, dither, tail, a bar
//! range, stems with their tracks and folder, MIDI tracks, the import bar and tempo. Export…
//! asks the host to choose a file on a worker; the chosen path runs `session.exportAudio`,
//! `session.exportStems`, `session.exportMidi` or `session.importMidi`, and the report shows
//! here when the job ends.
//!
//! After the third export from the window ryolune asks once whether to donate
//! (`SUPPORT_AFTER_EXPORTS`): it unlocks nothing, and either answer ends it.

use super::{modal, Dialogs};
use crate::{
    export::{best_format, Mode, APP_FORMATS, CONTAINERS, FORMATS, OGG_QUALITIES, SAMPLE_RATES},
    ui::{
        daw::Daw,
        theme::{size, Theme},
        widgets::{
            field, select_button, Button, InputEvent, MenuItem, Segmented, Switch, TextInput,
        },
    },
};
use gpui::{
    div, prelude::*, px, AnyElement, App, Context, Entity, MouseButton, MouseDownEvent,
    SharedString, Subscription, Window,
};
use serde_json::json;

/// Exports from the window before ryolune asks, once, whether to donate.
pub(crate) const SUPPORT_AFTER_EXPORTS: u32 = 3;

/// Whether the export just counted earns the one request.
pub(crate) fn should_ask_support(exports_completed: u32, support_asked: bool) -> bool {
    !support_asked && exports_completed >= SUPPORT_AFTER_EXPORTS
}

/// The text fields of the form; their numbers live in the host's draft.
pub(crate) struct ExportForm {
    tail: Entity<TextInput>,
    start: Entity<TextInput>,
    end: Entity<TextInput>,
    folder: Entity<TextInput>,
    was_open: bool,
    /// The command the host was waiting for on the last frame.
    awaiting: Option<String>,
    /// Show the one-time donation request under the report.
    pub support_ask: bool,
    _subscriptions: Vec<Subscription>,
}

/// Which draft value a text field edits.
#[derive(Clone, Copy)]
enum Slot {
    Tail,
    Start,
    End,
    Folder,
}

impl ExportForm {
    pub fn new(window: &mut Window, cx: &mut Context<Dialogs>) -> Self {
        let number = |cx: &mut Context<Dialogs>| cx.new(|cx| TextInput::new(cx).mono());
        let tail = number(cx);
        let start = number(cx);
        let end = number(cx);
        let folder = cx.new(TextInput::new);
        let mut subscriptions = vec![];
        for (input, slot) in [
            (&tail, Slot::Tail),
            (&start, Slot::Start),
            (&end, Slot::End),
            (&folder, Slot::Folder),
        ] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                move |this: &mut Dialogs, input, event, window, cx| match event {
                    InputEvent::Changed => {
                        let text = input.read(cx).text().to_string();
                        this.daw.update(cx, |daw, cx| {
                            let draft = &mut daw.app.export;
                            let number = text.trim().parse::<f64>().unwrap_or(f64::NAN);
                            match slot {
                                Slot::Tail => draft.tail_seconds = number,
                                Slot::Start => draft.start_bar = number,
                                Slot::End => draft.end_bar = number,
                                Slot::Folder => draft.folder_name = text,
                            }
                            cx.notify();
                        });
                    }
                    InputEvent::Submit => {
                        let daw = this.daw.clone();
                        this.export.start(&daw, cx);
                    }
                    InputEvent::Cancel => window.dispatch_action(Box::new(modal::Dismiss), cx),
                    InputEvent::Blur => {}
                },
            ));
        }
        Self {
            tail,
            start,
            end,
            folder,
            was_open: false,
            awaiting: None,
            support_ask: false,
            _subscriptions: subscriptions,
        }
    }

    /// Keep the fields in step with the host: fill them when the dialog opens, and count an
    /// export when its job ends.
    pub fn follow(&mut self, daw: &Entity<Daw>, window: &mut Window, cx: &mut Context<Dialogs>) {
        let draft = &daw.read(cx).app.export;
        let open = draft.open;
        let now = draft.awaiting.clone();
        let finished = self
            .awaiting
            .as_deref()
            .is_some_and(|m| matches!(m, "session.exportAudio" | "session.exportStems"))
            && now.is_none()
            && draft.report.is_some();
        if open && !self.was_open {
            let values = [
                (self.tail.clone(), number_text(draft.tail_seconds)),
                (self.start.clone(), number_text(draft.start_bar)),
                (self.end.clone(), number_text(draft.end_bar)),
                (self.folder.clone(), draft.folder_name.clone()),
            ];
            for (input, text) in values {
                input.update(cx, |input, cx| input.set_text(text, cx));
            }
            self.support_ask = false;
        }
        self.was_open = open;
        self.awaiting = now;
        if finished {
            cx.defer_in(window, |this, _, cx| {
                let daw = this.daw.clone();
                this.export.count(&daw, cx);
            });
        }
    }

    /// One more export from the window, counted on this computer only.
    fn count(&mut self, daw: &Entity<Daw>, cx: &mut Context<Dialogs>) {
        let general = daw.read(cx).app.settings.general.clone();
        let done = general.exports_completed.saturating_add(1);
        let ask = should_ask_support(done, general.support_asked);
        daw.update(cx, |daw, cx| {
            let _ = daw.request(
                "settings.set",
                json!({ "path": "general.exportsCompleted", "value": done }),
                cx,
            );
            if ask {
                let _ = daw.request(
                    "settings.set",
                    json!({ "path": "general.supportAsked", "value": true }),
                    cx,
                );
            }
        });
        self.support_ask = ask;
        cx.notify();
    }

    /// Export… or Import…: choose the file, unless the form is incomplete or busy.
    pub fn start(&mut self, daw: &Entity<Daw>, cx: &mut Context<Dialogs>) {
        daw.update(cx, |daw, cx| {
            let app = &mut daw.app;
            if app.export.busy() || app.export_blocked() {
                return;
            }
            if app.export.problem(app.store.session()).is_none() {
                app.export.choose(app.store.session());
            }
            cx.notify();
        });
    }

    pub fn sheet(
        &mut self,
        daw: &Entity<Daw>,
        window: &mut Window,
        cx: &mut Context<Dialogs>,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = Theme::get(cx).clone();
        let app = &daw.read(cx).app;
        let draft = &app.export;
        let mode = draft.mode;
        let busy = draft.busy();
        let blocked = app.export_blocked();
        let problem = draft.problem(app.store.session());
        let report = draft.report.clone();
        let error = draft.error.clone();
        let choosing = draft.choosing();
        let title = match mode {
            Mode::Audio => "Export audio",
            Mode::MidiImport => "Import MIDI",
            Mode::MidiExport => "Export MIDI",
            Mode::AppImport => "Import from another app",
            Mode::AppExport => "Export for another app",
        };
        let primary = match (mode, busy, choosing) {
            (_, true, true) => "Choosing a file…",
            (Mode::MidiImport | Mode::AppImport, true, false) => "Importing…",
            (_, true, false) => "Exporting…",
            (Mode::MidiImport, false, _) => "Import…",
            (Mode::AppImport, false, _) => "Choose file…",
            _ => "Export…",
        };
        let rows = match mode {
            Mode::Audio => self.audio_rows(daw, window, cx),
            Mode::MidiImport => self.import_rows(daw, window, cx),
            Mode::MidiExport => self.track_rows(daw, true, cx),
            Mode::AppImport => self.app_rows(daw, true, cx),
            Mode::AppExport => self.app_rows(daw, false, cx),
        };
        let mut body = modal::body("export-body").children(rows);
        if blocked {
            body = body.child(modal::note(
                "Wait for the recording or the file operation to finish.",
                cx,
            ));
        } else if let Some(problem) = problem.as_ref().filter(|_| !busy) {
            body = body.child(modal::error_line(problem.clone(), cx));
        }
        if let Some(error) = error {
            body = body.child(modal::error_line(error, cx));
        }
        if let Some(report) = report {
            body = body.child(modal::well(report, cx));
        }
        if self.support_ask {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .p(px(12.0))
                    .rounded(px(crate::ui::theme::radius::MD))
                    .bg(theme.accent_soft)
                    .border_1()
                    .border_color(theme.accent_ring)
                    .child(modal::text(
                        "ryolune is free and stays free, every update included. If it is earning a place in your music, you can donate, once or monthly. It unlocks nothing, and ryolune will not ask again.",
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.0))
                            .child(Button::new("support-no", "No thanks").ghost().on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.export.support_ask = false;
                                    cx.notify();
                                }),
                            ))
                            .child(
                                Button::new("support-donate", "Donate…")
                                    .with_icon("heart")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.export.support_ask = false;
                                        this.daw.update(cx, |daw, cx| {
                                            daw.run(
                                                "app.openGuide",
                                                json!({ "guide": "support" }),
                                                cx,
                                            );
                                        });
                                    })),
                            ),
                    ),
            );
        }
        let ready = !busy && !blocked && problem.is_none();
        modal::sheet(
            "export",
            title,
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
                    Button::new("export-close", "Close")
                        .disabled(busy)
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(modal::Dismiss), cx)
                        }),
                )
                .child(
                    Button::new("export-go", primary)
                        .primary()
                        .disabled(!ready)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let daw = this.daw.clone();
                            this.export.start(&daw, cx);
                        })),
                ),
        )
    }

    fn audio_rows(
        &mut self,
        daw: &Entity<Daw>,
        window: &mut Window,
        cx: &mut Context<Dialogs>,
    ) -> Vec<AnyElement> {
        let draft = &daw.read(cx).app.export;
        let rate = SAMPLE_RATES
            .iter()
            .position(|r| *r == draft.sample_rate)
            .unwrap_or(1);
        let container = draft.container.min(CONTAINERS.len() - 1);
        let lossy = CONTAINERS[container].0 == "ogg";
        let format = draft.format.min(FORMATS.len() - 1);
        let float = FORMATS[format].0 == "float32";
        let quality = OGG_QUALITIES
            .iter()
            .min_by(|a, b| {
                (a.0 - draft.quality)
                    .abs()
                    .total_cmp(&(b.0 - draft.quality).abs())
            })
            .map_or("High", |q| q.1);
        let (dither, range, stems) = (draft.dither, draft.range, draft.stems);
        let (effects, master) = (draft.include_effects, draft.include_master);
        let mut rows = vec![];
        rows.push(row(
            "Sample rate",
            None,
            Segmented::new(
                "export-rate",
                SAMPLE_RATES
                    .iter()
                    .map(|r| format!("{} kHz", *r as f64 / 1000.0)),
                rate,
            )
            .on_select(edit(daw, |d, i| d.sample_rate = SAMPLE_RATES[i])),
            cx,
        ));
        let types = CONTAINERS
            .iter()
            .enumerate()
            .map(|(i, (_, label))| {
                MenuItem::new(
                    *label,
                    pick(daw, move |d| {
                        d.container = i;
                        // AIFF and FLAC are integer only.
                        if !matches!(CONTAINERS[i].0, "wav" | "ogg")
                            && FORMATS[d.format].0 == "float32"
                        {
                            d.format = 1;
                        }
                    }),
                )
                .checked(i == container)
            })
            .collect();
        rows.push(row(
            "File type",
            None,
            self.select("export-type", CONTAINERS[container].1, types, cx),
            cx,
        ));
        if lossy {
            let items = OGG_QUALITIES
                .iter()
                .map(|(value, label)| {
                    let value = *value;
                    MenuItem::new(*label, pick(daw, move |d| d.quality = value))
                        .checked(label == &quality)
                })
                .collect();
            rows.push(row(
                "Quality",
                Some("Ogg Vorbis is compressed: smaller files, some detail lost.".into()),
                self.select("export-quality", quality, items, cx),
                cx,
            ));
        } else {
            let items = FORMATS
                .iter()
                .enumerate()
                .map(|(i, (key, label))| {
                    let wav_only = *key == "float32" && container != 0;
                    MenuItem::new(
                        if wav_only {
                            format!("{label} (WAV only)")
                        } else {
                            label.to_string()
                        },
                        pick(daw, move |d| d.format = i),
                    )
                    .disabled(wav_only)
                    .checked(i == format)
                })
                .collect();
            rows.push(row(
                "Format",
                Some(if float {
                    "Float keeps headroom above full scale.".into()
                } else {
                    "PCM above full scale clips.".into()
                }),
                self.select("export-format", FORMATS[format].1, items, cx),
                cx,
            ));
            rows.push(row(
                "Dither",
                Some("Smooths the last bit of 16 and 24-bit PCM.".into()),
                div().when(float, |d| d.opacity(0.4)).child(
                    Switch::new("export-dither", dither && !float).on_toggle(toggle(
                        daw,
                        move |d, on| {
                            if !float {
                                d.dither = on
                            }
                        },
                    )),
                ),
                cx,
            ));
        }
        rows.push(row(
            "Release tail",
            Some("Seconds of effect tails after the end, 0 to 120.".into()),
            self.number(&self.tail.clone(), window, cx),
            cx,
        ));
        rows.push(row(
            "Bar range",
            Some("Off exports the whole arrangement.".into()),
            Switch::new("export-range", range).on_toggle(toggle(daw, |d, on| d.range = on)),
            cx,
        ));
        if range {
            rows.push(row(
                "Start bar",
                None,
                self.number(&self.start.clone(), window, cx),
                cx,
            ));
            rows.push(row(
                "End bar",
                Some("The end bar itself is not included.".into()),
                self.number(&self.end.clone(), window, cx),
                cx,
            ));
        }
        rows.push(row(
            "Export track stems",
            Some("One file per track, in a new folder.".into()),
            Switch::new("export-stems", stems).on_toggle(toggle(daw, |d, on| d.stems = on)),
            cx,
        ));
        if stems {
            rows.push(row(
                "Include track effects",
                Some("Inserts and send/bus processing.".into()),
                Switch::new("export-effects", effects)
                    .on_toggle(toggle(daw, |d, on| d.include_effects = on)),
                cx,
            ));
            rows.push(row(
                "Include master processing",
                Some("The master inserts and fader on every stem.".into()),
                Switch::new("export-master", master)
                    .on_toggle(toggle(daw, |d, on| d.include_master = on)),
                cx,
            ));
            let focused = self.folder.read(cx).is_focused(window);
            rows.push(row(
                "Folder name",
                Some("Made inside the folder you choose next.".into()),
                div().w(px(200.0)).child(field(&self.folder, focused, cx)),
                cx,
            ));
            rows.extend(self.track_rows(daw, false, cx));
        }
        rows
    }

    fn import_rows(
        &mut self,
        daw: &Entity<Daw>,
        window: &mut Window,
        cx: &mut Context<Dialogs>,
    ) -> Vec<AnyElement> {
        let tempo = daw.read(cx).app.export.import_tempo;
        vec![
            row(
                "Start bar",
                Some("Where the file's first bar lands; the playhead's bar by default.".into()),
                self.number(&self.start.clone(), window, cx),
                cx,
            ),
            row(
                "Import tempo",
                Some("Take the file's tempo and tempo changes.".into()),
                Switch::new("import-tempo", tempo)
                    .on_toggle(toggle(daw, |d, on| d.import_tempo = on)),
                cx,
            ),
        ]
    }

    /// Import from or export for another app: the app (it decides the steps shown and, for
    /// an export, the format), the format, and that app's steps.
    fn app_rows(
        &mut self,
        daw: &Entity<Daw>,
        import: bool,
        cx: &mut Context<Dialogs>,
    ) -> Vec<AnyElement> {
        let apps = ryolune_engine::interop::apps::APPS;
        let draft = &daw.read(cx).app.export;
        let chosen = draft.app.and_then(|i| apps.get(i));
        let format = draft.app_format.min(APP_FORMATS.len() - 1);
        let mut items: Vec<MenuItem> = apps
            .iter()
            .enumerate()
            .map(|(i, app)| {
                MenuItem::new(
                    app.name,
                    pick(daw, move |d| {
                        d.app = Some(i);
                        d.app_format = best_format(Some(i));
                    }),
                )
                .checked(draft.app == Some(i))
            })
            .collect();
        items.push(
            MenuItem::new(
                "Another app",
                pick(daw, |d| {
                    d.app = None;
                    d.app_format = best_format(None);
                }),
            )
            .checked(draft.app.is_none()),
        );
        let label = chosen.map_or("Another app", |a| a.name);
        let mut rows = vec![row(
            if import { "Coming from" } else { "For" },
            None,
            self.select("interop-app", label, items, cx),
            cx,
        )];
        if !import {
            let formats = APP_FORMATS
                .iter()
                .enumerate()
                .map(|(i, (_, name))| {
                    MenuItem::new(*name, pick(daw, move |d| d.app_format = i))
                        .checked(i == format)
                })
                .collect();
            let carries = ryolune_engine::interop::format(APP_FORMATS[format].0)
                .map(|f| SharedString::from(f.carries));
            rows.push(row(
                "Format",
                carries,
                self.select("interop-format", APP_FORMATS[format].1, formats, cx),
                cx,
            ));
        }
        let steps = match (chosen, import) {
            (Some(app), true) => app.bring,
            (Some(app), false) => app.take,
            (None, true) => "Export a DAWproject from the other app if it has one (Bitwig Studio, Studio One, Cubase 14 and later do); otherwise export one audio file per track (stems) and the MIDI. Then choose them here: several audio files at once each become a track.",
            (None, false) => "DAWproject keeps the most when the other app opens it; MIDI and stems work everywhere.",
        };
        rows.push(modal::text(steps, cx).into_any_element());
        if import {
            rows.push(
                modal::note(
                    "It opens as a new song in place of this one; ryolune asks to save this one first if it changed. A report says what came across.",
                    cx,
                )
                .into_any_element(),
            );
        }
        rows
    }

    /// The tracks to export: stems, or the MIDI tracks of a MIDI file.
    fn track_rows(
        &self,
        daw: &Entity<Daw>,
        midi: bool,
        cx: &mut Context<Dialogs>,
    ) -> Vec<AnyElement> {
        let theme = Theme::get(cx).clone();
        let app = &daw.read(cx).app;
        let tracks: Vec<(String, String, bool)> = app
            .store
            .session()
            .tracks
            .iter()
            .filter(|t| !midi || t.kind == "midi")
            .map(|t| {
                (
                    t.id.clone(),
                    t.name.clone(),
                    app.export.tracks.contains(&t.id),
                )
            })
            .collect();
        let all: Vec<String> = tracks.iter().map(|t| t.0.clone()).collect();
        let none_ids = all.clone();
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .pt(px(6.0))
            .child(
                div()
                    .text_size(px(size::SM))
                    .text_color(theme.text_2)
                    .child(if midi { "MIDI tracks" } else { "Tracks" }),
            )
            .child(
                div()
                    .flex()
                    .gap(px(4.0))
                    .child(
                        Button::new("tracks-all", "All")
                            .ghost()
                            .compact()
                            .on_click(click(daw, move |d| d.tracks.extend(all.iter().cloned()))),
                    )
                    .child(
                        Button::new("tracks-none", "None")
                            .ghost()
                            .compact()
                            .on_click(click(daw, move |d| {
                                for id in &none_ids {
                                    d.tracks.remove(id);
                                }
                            })),
                    ),
            );
        let mut rows = vec![header.into_any_element()];
        if tracks.is_empty() {
            rows.push(modal::note("This session has no MIDI tracks.", cx).into_any_element());
        }
        for (i, (id, name, on)) in tracks.into_iter().enumerate() {
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .min_h(px(30.0))
                    .border_b_1()
                    .border_color(theme.hairline)
                    .child(
                        div()
                            .text_size(px(size::BASE))
                            .text_color(theme.text)
                            .child(name),
                    )
                    .child(Switch::new(("export-track", i), on).on_toggle(toggle(
                        daw,
                        move |d, on| {
                            if on {
                                d.tracks.insert(id.clone());
                            } else {
                                d.tracks.remove(&id);
                            }
                        },
                    )))
                    .into_any_element(),
            );
        }
        rows
    }

    fn number(
        &self,
        input: &Entity<TextInput>,
        window: &mut Window,
        cx: &mut Context<Dialogs>,
    ) -> gpui::Div {
        let focused = input.read(cx).is_focused(window);
        div().w(px(90.0)).child(field(input, focused, cx))
    }

    fn select(
        &self,
        id: &'static str,
        label: impl Into<SharedString>,
        items: Vec<MenuItem>,
        cx: &mut Context<Dialogs>,
    ) -> gpui::Stateful<gpui::Div> {
        let items = std::cell::RefCell::new(Some(items));
        select_button(id, label, cx).min_w(px(200.0)).on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                if let Some(items) = items.borrow_mut().take() {
                    this.menu.open(items, e.position, window, cx);
                }
            }),
        )
    }
}

fn row(
    label: &'static str,
    description: Option<SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> AnyElement {
    modal::field_row(label, description, control, cx).into_any_element()
}

fn number_text(value: f64) -> String {
    if value.is_finite() {
        let text = format!("{value:.2}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        String::new()
    }
}

type Draft = crate::export::ExportDialog;

/// A segmented choice that edits the draft.
fn edit(
    daw: &Entity<Daw>,
    f: impl Fn(&mut Draft, usize) + 'static,
) -> impl Fn(usize, &mut Window, &mut App) + 'static {
    let daw = daw.clone();
    move |i, _, cx| {
        daw.update(cx, |daw, cx| {
            f(&mut daw.app.export, i);
            cx.notify();
        })
    }
}

/// A switch that edits the draft.
fn toggle(
    daw: &Entity<Daw>,
    f: impl Fn(&mut Draft, bool) + 'static,
) -> impl Fn(bool, &mut Window, &mut App) + 'static {
    let daw = daw.clone();
    move |on, _, cx| {
        daw.update(cx, |daw, cx| {
            f(&mut daw.app.export, on);
            cx.notify();
        })
    }
}

/// A menu choice that edits the draft.
fn pick(
    daw: &Entity<Daw>,
    f: impl Fn(&mut Draft) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let daw = daw.clone();
    move |_, cx| {
        daw.update(cx, |daw, cx| {
            f(&mut daw.app.export);
            cx.notify();
        })
    }
}

/// A button that edits the draft.
fn click(
    daw: &Entity<Daw>,
    f: impl Fn(&mut Draft) + 'static,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
    let daw = daw.clone();
    move |_, _, cx| {
        daw.update(cx, |daw, cx| {
            f(&mut daw.app.export);
            cx.notify();
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_is_asked_once_after_the_third_export() {
        assert!(!should_ask_support(2, false));
        assert!(should_ask_support(3, false));
        assert!(!should_ask_support(3, true), "either answer ends it");
        assert!(!should_ask_support(4, true));
    }

    #[test]
    fn numbers_show_without_trailing_zeros() {
        assert_eq!(number_text(3.0), "3");
        assert_eq!(number_text(4.5), "4.5");
        assert_eq!(number_text(f64::NAN), "");
    }
}
