//! The transport: locate keys and play, stop, record and cycle, each set boxed together; the
//! time display with position, SMPTE, tempo, signature and key; click and snap; the master
//! meter and CPU. Glass tier 1, the display a solid box. Play is inverted while it plays,
//! record red while it records.

use super::{
    actions::Do,
    daw::Daw,
    format,
    theme::{radius, size, Theme, FONT_MONO},
    widgets::{
        group, tool, Button, InputEvent, MenuHost, MenuItem, Meter, NumberDrag, Phase, TextInput,
    },
};
use gpui::{
    div, prelude::*, px, AnyElement, App, Context, Entity, MouseButton, SharedString, Subscription,
    Window,
};
use serde_json::json;

const SIGNATURES: [(u32, u32); 7] = [(4, 4), (3, 4), (6, 8), (5, 4), (7, 8), (2, 4), (12, 8)];
const KEYS: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];
const SNAPS: [u32; 7] = [1, 2, 4, 8, 16, 32, 64];

pub struct Transport {
    daw: Entity<Daw>,
    menu: MenuHost,
    tempo_input: Entity<TextInput>,
    editing_tempo: bool,
    _tempo_events: Subscription,
}

impl Transport {
    pub fn new(daw: Entity<Daw>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tempo_input = cx.new(|cx| TextInput::new(cx).mono());
        let subscription = cx.subscribe_in(&tempo_input, window, |this, input, event, _, cx| {
            match event {
                InputEvent::Submit => {
                    if let Ok(bpm) = input.read(cx).text().trim().parse::<f64>() {
                        this.set_tempo(bpm.clamp(20.0, 400.0), cx);
                    }
                    this.editing_tempo = false;
                }
                InputEvent::Cancel | InputEvent::Blur => this.editing_tempo = false,
                InputEvent::Changed => {}
            }
            cx.notify();
        });
        Self {
            daw,
            menu: MenuHost::default(),
            tempo_input,
            editing_tempo: false,
            _tempo_events: subscription,
        }
    }

    /// Set the tempo in force at the playhead: the starting tempo, or the tempo change the
    /// playhead is in.
    fn set_tempo(&mut self, bpm: f64, cx: &mut Context<Self>) {
        self.daw.update(cx, |daw, cx| {
            let session = daw.app.store.session();
            let bar = format::tempo_source_bar(session, daw.app.position);
            if bar == 0.0 {
                daw.run("transport.setTempo", json!({ "bpm": bpm }), cx);
            } else {
                daw.run("tempo.set", json!({ "bar": bar, "bpm": bpm }), cx);
            }
        });
    }

    fn pick_menu(
        &mut self,
        items: Vec<MenuItem>,
        e: &gpui::MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu.open(
            items,
            gpui::point(e.position.x - px(12.0), px(66.0)),
            window,
            cx,
        );
    }

    fn display(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let app = &self.daw.read(cx).app;
        let s = app.store.session();
        let bpb = s.beats_per_bar();
        let pos = format::bar_beat(app.position, bpb);
        let map = s.tempo_map();
        let tempo = map.bpm(app.position);
        let source = format::tempo_source_bar(s, app.position);
        let own_tempo = if source == 0.0 {
            s.transport.tempo
        } else {
            s.tempo_changes
                .iter()
                .find(|p| p.bar == source)
                .map_or(tempo, |p| p.bpm)
        };
        let smpte = format::smpte(map.seconds(app.position));
        let sig = (
            s.transport.time_signature.numerator,
            s.transport.time_signature.denominator,
        );
        let key = s.transport.key.clone();
        let has_changes = !s.tempo_changes.is_empty();

        let cell = |label: &'static str, value: AnyElement| {
            div()
                .flex()
                .flex_col()
                .justify_center()
                .px(px(12.0))
                .h_full()
                .child(
                    div()
                        .font_family(FONT_MONO)
                        .text_size(px(9.5))
                        .text_color(theme.text_3)
                        .child(label),
                )
                .child(value)
        };
        let big = |text: SharedString, dim: bool| {
            div()
                .font_family(FONT_MONO)
                .text_size(px(21.0))
                .line_height(px(24.0))
                .text_color(if dim {
                    theme.text_2
                } else {
                    theme.text_display
                })
                .child(text)
                .into_any_element()
        };
        let dot = || {
            div()
                .font_family(FONT_MONO)
                .text_size(px(16.0))
                .text_color(theme.text_3)
                .px(px(5.0))
                .child("·")
        };
        let position = div()
            .flex()
            .items_baseline()
            .font_family(FONT_MONO)
            .text_size(px(21.0))
            .line_height(px(24.0))
            .text_color(theme.text_display)
            .child(format!("{:03}", pos.bar))
            .child(dot())
            .child(pos.beat.to_string())
            .child(dot())
            .child(pos.division.to_string())
            .child(dot())
            .child(
                div()
                    .text_color(theme.text_2)
                    .child(format!("{:03}", pos.tick)),
            )
            .into_any_element();

        let tempo_value = if self.editing_tempo {
            div()
                .w(px(76.0))
                .font_family(FONT_MONO)
                .text_size(px(18.0))
                .text_color(theme.text_display)
                .child(self.tempo_input.clone())
                .into_any_element()
        } else {
            big(format!("{tempo:.2}").into(), false)
        };
        let tempo_cell = div()
            .id("tempo")
            .h_full()
            .cursor(gpui::CursorStyle::ResizeUpDown)
            .hover(|s| s.bg(theme.hover))
            .child(cell("TEMPO", tempo_value))
            .tooltip(move |_, cx| {
                super::widgets::tip(
                    if has_changes {
                        "Tempo at the playhead · drag to change · double-click to type".into()
                    } else {
                        "Drag to change tempo · double-click to type".into()
                    },
                    cx,
                )
            })
            .on_click(cx.listener(|this, e: &gpui::ClickEvent, window, cx| {
                if e.click_count() == 2 {
                    let tempo = {
                        let app = &this.daw.read(cx).app;
                        app.store.session().tempo_map().bpm(app.position)
                    };
                    this.editing_tempo = true;
                    this.tempo_input.update(cx, |input, cx| {
                        input.set_text(format!("{tempo:.2}"), cx);
                        input.select_all_text(cx);
                        input.focus(window);
                    });
                    cx.notify();
                }
            }));
        let daw = self.daw.clone();
        let tempo_cell = if self.editing_tempo {
            tempo_cell
        } else {
            NumberDrag {
                id: "tempo-drag".into(),
                value: own_tempo as f32,
                per_px: 1.0 / 3.0,
                min: 20.0,
                max: 400.0,
            }
            .attach(
                tempo_cell,
                move |bpm, phase, _, cx| {
                    let bpm = (bpm as f64 * 10.0).round() / 10.0;
                    daw.update(cx, |daw, cx| {
                        match phase {
                            Phase::Start => daw.gesture(true),
                            Phase::End => daw.gesture(false),
                            Phase::Move => {}
                        }
                        if phase == Phase::Move {
                            let session = daw.app.store.session();
                            let bar = format::tempo_source_bar(session, daw.app.position);
                            if bar == 0.0 {
                                daw.run("transport.setTempo", json!({ "bpm": bpm }), cx);
                            } else {
                                daw.run("tempo.set", json!({ "bar": bar, "bpm": bpm }), cx);
                            }
                        }
                    })
                },
                window,
                cx,
            )
        };

        let sig_cell = div()
            .id("signature")
            .h_full()
            .cursor_pointer()
            .hover(|s| s.bg(theme.hover))
            .child(cell(
                "SIG",
                big(format!("{}/{}", sig.0, sig.1).into(), false),
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e, window, cx| {
                    let daw = this.daw.clone();
                    let items = SIGNATURES
                        .iter()
                        .map(|&(n, d)| {
                            let daw = daw.clone();
                            MenuItem::new(format!("{n}/{d}"), move |_, cx| {
                                daw.update(cx, |daw, cx| {
                                    daw.run(
                                        "transport.setTimeSignature",
                                        json!({"numerator": n, "denominator": d}),
                                        cx,
                                    );
                                })
                            })
                            .checked(sig == (n, d))
                        })
                        .collect();
                    this.pick_menu(items, e, window, cx);
                }),
            );
        let key_cell = div()
            .id("key")
            .h_full()
            .cursor_pointer()
            .hover(|s| s.bg(theme.hover))
            .child(cell("KEY", big(key.clone().into(), false)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e, window, cx| {
                    let daw = this.daw.clone();
                    let current = this.daw.read(cx).app.store.session().transport.key.clone();
                    let items = ["maj", "min"]
                        .iter()
                        .flat_map(|mode| KEYS.iter().map(move |k| format!("{k} {mode}")))
                        .map(|label| {
                            let daw = daw.clone();
                            let value = label.clone();
                            MenuItem::new(label.clone(), move |_, cx| {
                                let value = value.clone();
                                daw.update(cx, |daw, cx| {
                                    daw.run("transport.setKey", json!({ "key": value }), cx);
                                })
                            })
                            .checked(current == label)
                        })
                        .collect();
                    this.pick_menu(items, e, window, cx);
                }),
            );
        let separator = || div().w(px(1.0)).h(px(30.0)).bg(theme.line);
        div()
            .flex()
            .items_center()
            .h(px(48.0))
            .rounded(px(radius::MD))
            .bg(theme.display)
            .border_1()
            .border_color(theme.line_strong)
            .child(cell("POSITION", position))
            .child(separator())
            .child(cell("SMPTE", big(smpte.into(), true)))
            .child(separator())
            .child(tempo_cell)
            .child(separator())
            .child(sig_cell)
            .child(separator())
            .child(key_cell)
            .into_any_element()
    }
}

fn act(id: &'static str) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) {
    move |_, window, cx| window.dispatch_action(Box::new(Do { id }), cx)
}

impl Render for Transport {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let display = self.display(window, cx);
        let app = &self.daw.read(cx).app;
        let s = app.store.session();
        let playing = app.playing;
        let recording = app.record_enabled;
        let cycle = s.transport.cycle;
        let metronome = s.transport.metronome;
        let snap = s.transport.snap_division;
        let counting_in = app.device.as_ref().is_some_and(|d| {
            d.telemetry
                .counting_in
                .load(std::sync::atomic::Ordering::Relaxed)
        });
        let (peaks, cpu) = app.device.as_ref().map_or(([0.0; 4], 0.0), |d| {
            (d.telemetry.peaks(), d.telemetry.load())
        });
        let master_db = format::peak_db(peaks[0].max(peaks[1]));
        let menu = self.menu.render(window, cx);

        let caps = |text: &'static str| {
            div()
                .font_family(FONT_MONO)
                .text_size(px(9.5))
                .text_color(theme.text_3)
                .child(text)
        };
        let readout = |text: String| {
            div()
                .w(px(40.0))
                .font_family(FONT_MONO)
                .text_size(px(size::SM))
                .text_color(theme.text_2)
                .child(text)
        };
        let wide = f32::from(window.viewport_size().width) >= 1560.0;
        let locate = group(
            [
                tool("return", "return", "Start", false, "Go to beginning (↩)")
                    .on_click(act("returnToStart"))
                    .into_any_element(),
                tool("rewind", "rewind", "Rewind", false, "Rewind one bar (,)")
                    .icon_size(13.0)
                    .on_click(act("rewind"))
                    .into_any_element(),
                tool(
                    "forward",
                    "forward",
                    "Forward",
                    false,
                    "Forward one bar (.)",
                )
                .icon_size(13.0)
                .on_click(act("forward"))
                .into_any_element(),
            ],
            cx,
        );
        let keys = group(
            [
                // Play is the transport's primary key: inverted while it plays.
                tool("play", "play", "Play", false, "Play (Space)")
                    .lit(playing)
                    .on_click(act("togglePlay"))
                    .into_any_element(),
                tool(
                    "stop",
                    "stop",
                    "Stop",
                    false,
                    "Stop · twice to return to start",
                )
                .icon_size(10.0)
                .on_click(act("stop"))
                .into_any_element(),
                tool(
                    "record",
                    "record",
                    "Record",
                    false,
                    if counting_in {
                        "Counting in: recording starts on the next bar"
                    } else {
                        "Record (R) · arm a track, then play"
                    },
                )
                .icon_size(11.0)
                .lit(recording)
                .lit_color(theme.record)
                .on_click(act("record"))
                .into_any_element(),
                tool(
                    "cycle",
                    "cycle",
                    "Cycle",
                    false,
                    "Cycle (C) · drag in the ruler to set the range",
                )
                .icon_size(13.0)
                .lit(cycle)
                .on_click(act("cycle"))
                .into_any_element(),
            ],
            cx,
        );
        let snap_label = if snap == 1 {
            "Snap bar".to_string()
        } else {
            format!("Snap 1/{snap}")
        };
        let modes = group(
            [
                tool("click", "metronome", "Click", true, "Metronome (K)")
                    .lit(metronome)
                    .on_click(act("metronome"))
                    .into_any_element(),
                Button::new("snap", snap_label)
                    .with_icon("grid")
                    .flush()
                    .tooltip("Snap grid")
                    .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                        let daw = this.daw.clone();
                        let items = SNAPS
                            .iter()
                            .map(|&d| {
                                let daw = daw.clone();
                                MenuItem::new(
                                    if d == 1 {
                                        "Bar".to_string()
                                    } else {
                                        format!("1/{d}")
                                    },
                                    move |_, cx| {
                                        daw.update(cx, |daw, cx| {
                                            daw.run(
                                                "transport.setSnap",
                                                json!({ "division": d }),
                                                cx,
                                            );
                                        })
                                    },
                                )
                                .checked(snap == d)
                            })
                            .collect();
                        let at = e.position();
                        this.menu
                            .open(items, gpui::point(at.x - px(30.0), px(66.0)), window, cx);
                    }))
                    .into_any_element(),
            ],
            cx,
        );
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_between()
            .px(px(14.0))
            .gap(px(12.0))
            .bg(theme.glass(1))
            .border_b_1()
            .border_color(theme.line)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(locate)
                    .child(keys)
                    .when(counting_in, |d| {
                        d.child(
                            div()
                                .px(px(8.0))
                                .py(px(2.0))
                                .bg(theme.record)
                                .text_color(theme.text_on_accent)
                                .font_family(FONT_MONO)
                                .text_size(px(size::XS))
                                .child("COUNT-IN"),
                        )
                    }),
            )
            .child(display)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.0))
                    .child(modes)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(caps("MASTER"))
                            .child(
                                div()
                                    .w(px(if wide { 150.0 } else { 110.0 }))
                                    .h(px(14.0))
                                    .child(Meter::new([peaks[0], peaks[1]]).segments(22)),
                            )
                            .child(readout(format::db(master_db, 1))),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(caps("CPU"))
                            .child(
                                div()
                                    .w(px(if wide { 80.0 } else { 56.0 }))
                                    .h(px(14.0))
                                    .child(Meter::new([cpu]).linear().segments(12)),
                            )
                            .child(readout(format!("{}%", (cpu * 100.0).round() as i32))),
                    ),
            )
            .children(menu)
    }
}
