//! The region editor under the arrangement: a toolbar (Piano Roll, Score and Step, the open
//! clip, quantize, velocity and scale), the keyboard column and the roll, and the controller
//! lane under it when it is shown.
//!
//! The roll and the lane are canvases painted from one geometry (`geometry::Roll`, the
//! clip fitted to the width) and hit-tested from the same. Every edit is a registry
//! command: `note.add`, `note.update`, `note.remove`, `clip.select` with a note,
//! `note.preview`, `view.set` (`editorMode`, `editorLowPitch`) and the `controller.*`
//! family. A drag is previewed here and lands as one command on release, inside one
//! gesture, so it is one undo step.

mod controllers;
mod geometry;
mod lane;
mod paint;
mod piano_roll;
mod score;

use super::{
    actions,
    daw::Daw,
    theme::{editor::CONTROLLER_LANE, editor::KEY_COLUMN, layout, size, Theme, FONT_MONO},
    widgets::{
        group, panel_info, panel_title, InputEvent, MenuHost, MenuItem, Segmented, TextInput,
    },
};
use geometry::{notes, snap, step_beats, Roll};
use gpui::{
    canvas, div, prelude::*, px, App, Context, CursorStyle, DispatchPhase, Entity, Hitbox,
    HitboxBehavior, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollWheelEvent, Subscription, Window,
};
use lane::Lane;
use paint::Pen;
use piano_roll::{Ghost, Overlay};
use ryolune_engine::model::{Clip, ClipData, Note};
use serde_json::json;

/// Velocities offered for new notes and in a note's menu.
const VELOCITIES: [u8; 6] = [40, 64, 80, 96, 112, 127];
/// How far the pointer must travel before a press on a note becomes a move.
const DRAG_THRESHOLD: f32 = 3.0;

/// The editor's three views of a MIDI clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    PianoRoll,
    Score,
    Step,
}

impl Mode {
    const ALL: [Mode; 3] = [Mode::PianoRoll, Mode::Score, Mode::Step];
    fn parse(text: &str) -> Self {
        match text {
            "score" => Mode::Score,
            "step" => Mode::Step,
            _ => Mode::PianoRoll,
        }
    }
    fn id(self) -> &'static str {
        match self {
            Mode::PianoRoll => "pianoRoll",
            Mode::Score => "score",
            Mode::Step => "step",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Mode::PianoRoll => "Piano Roll",
            Mode::Score => "Score",
            Mode::Step => "Step",
        }
    }
}

/// What the canvases paint, read from the session once per frame.
#[derive(Clone)]
pub struct Shown {
    /// The clip open in the editor (MIDI or audio).
    pub clip: Option<Clip>,
    pub track_color: Hsla,
    pub mode: Mode,
    /// The view's lowest pitch, when it sets one.
    pub low: Option<u8>,
    pub division: u32,
    pub bpb: f64,
    /// The playhead, in beats.
    pub position: f64,
    pub selected_note: Option<String>,
    pub signature: (u32, u32),
    pub key: String,
}

impl Shown {
    pub fn notes(&self) -> &[Note] {
        notes(self.clip.as_ref())
    }
    pub fn start_bar(&self) -> f64 {
        self.clip.as_ref().map_or(0.0, |c| c.start_bar)
    }
    pub fn is_midi(&self) -> bool {
        matches!(
            self.clip.as_ref().map(|c| &c.data),
            Some(ClipData::Midi { .. })
        )
    }
    /// The roll's geometry for a canvas of this size.
    pub fn roll(&self, size: gpui::Size<Pixels>) -> Roll {
        Roll::new(
            f32::from(size.width),
            f32::from(size.height),
            self.clip.as_ref().map(|c| c.length_bars),
            self.bpb,
            self.low,
            self.notes(),
        )
    }
}

/// A drag in the roll, previewed until release.
#[derive(Clone, Debug)]
enum RollDrag {
    Move {
        note: String,
        grab: f64,
        origin: (f64, i32),
        start: f64,
        pitch: i32,
        length: f64,
        pressed: Point<f32>,
        moved: bool,
    },
    Resize {
        note: String,
        start: f64,
        pitch: i32,
        length: f64,
        original: f64,
    },
    Pencil {
        anchor: f64,
        start: f64,
        length: f64,
        pitch: i32,
    },
}

pub struct Editor {
    daw: Entity<Daw>,
    menu: MenuHost,
    /// Velocity of new notes.
    velocity: u8,
    drag: Option<RollDrag>,
    /// The key being auditioned from the keyboard column.
    pressed_key: Option<i32>,
    roll_cursor: CursorStyle,
    /// Wheel travel not yet turned into whole rows.
    wheel: f32,
    /// The controller lane: which lane, for which clip it was chosen, a drag and the point
    /// under the pointer.
    lane: Lane,
    lane_clip: Option<String>,
    lane_drag: Option<controllers::LaneDrag>,
    lane_hover: Option<String>,
    lane_cursor: CursorStyle,
    /// "Other CC…": the controller number being typed.
    cc_input: Entity<TextInput>,
    typing_cc: bool,
    _cc_events: Subscription,
}

impl Editor {
    pub fn new(daw: Entity<Daw>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let cc_input = cx.new(|cx| TextInput::new(cx).mono());
        let cc_events = cx.subscribe_in(&cc_input, window, |this, input, event, _, cx| {
            match event {
                InputEvent::Submit => {
                    if let Some(number) = lane::parse_cc_number(input.read(cx).text()) {
                        this.lane = Lane::cc(number);
                    }
                    this.typing_cc = false;
                }
                InputEvent::Cancel | InputEvent::Blur => this.typing_cc = false,
                InputEvent::Changed => {}
            }
            cx.notify();
        });
        Self {
            daw,
            menu: MenuHost::default(),
            velocity: 100,
            drag: None,
            pressed_key: None,
            roll_cursor: CursorStyle::Crosshair,
            wheel: 0.0,
            lane: lane::LANE_CHOICES[0],
            lane_clip: None,
            lane_drag: None,
            lane_hover: None,
            lane_cursor: CursorStyle::Crosshair,
            cc_input,
            typing_cc: false,
            _cc_events: cc_events,
        }
    }

    /// The session as the canvases paint it.
    fn shown(&self, theme: &Theme, cx: &App) -> Shown {
        let app = &self.daw.read(cx).app;
        let s = app.store.session();
        let clip = s
            .view
            .editor_clip_id
            .as_ref()
            .and_then(|id| s.clips.iter().find(|c| &c.id == id))
            .cloned();
        let track_color = clip
            .as_ref()
            .and_then(|c| s.tracks.iter().position(|t| t.id == c.track_id))
            .map_or(theme.note, |i| theme.track(&s.tracks[i].color, i));
        Shown {
            clip,
            track_color,
            mode: Mode::parse(&s.view.editor_mode),
            low: s.view.editor_low_pitch,
            division: s.transport.snap_division,
            bpb: s.beats_per_bar(),
            position: app.position,
            selected_note: s.view.selected_note_id.clone(),
            signature: (
                s.transport.time_signature.numerator,
                s.transport.time_signature.denominator,
            ),
            key: s.transport.key.clone(),
        }
    }

    /// The open MIDI clip, as the session holds it now.
    fn midi_clip(&self, cx: &App) -> Option<Clip> {
        let s = self.daw.read(cx).app.store.session();
        let id = s.view.editor_clip_id.as_ref()?;
        s.clips
            .iter()
            .find(|c| &c.id == id && matches!(c.data, ClipData::Midi { .. }))
            .cloned()
    }
    fn mode(&self, cx: &App) -> Mode {
        Mode::parse(&self.daw.read(cx).app.store.session().view.editor_mode)
    }
    fn division(&self, cx: &App) -> u32 {
        self.daw
            .read(cx)
            .app
            .store
            .session()
            .transport
            .snap_division
    }

    /// Play a pitch on the clip's instrument, as a key under the finger would. Best effort:
    /// a track without an instrument or an absent device stays silent, without a dialog.
    fn audition(&mut self, clip: &Clip, pitch: i32, velocity: u8, cx: &mut Context<Self>) {
        if !(0..=127).contains(&pitch) {
            return;
        }
        let track = clip.track_id.clone();
        self.daw.update(cx, |daw, cx| {
            let _ = daw.request(
                "note.preview",
                json!({ "trackId": track, "pitch": pitch, "velocity": velocity }),
                cx,
            );
        });
    }

    fn select_note(&mut self, clip: &Clip, note: Option<&str>, cx: &mut Context<Self>) {
        let mut params = json!({ "clipId": clip.id });
        if let Some(note) = note {
            params["noteId"] = json!(note);
        }
        self.daw.update(cx, |daw, cx| {
            daw.run("clip.select", params, cx);
        });
    }

    fn run(&mut self, method: &str, params: serde_json::Value, cx: &mut Context<Self>) {
        self.daw.update(cx, |daw, cx| {
            daw.run(method, params, cx);
        });
    }

    fn gesture(&mut self, active: bool, cx: &mut Context<Self>) {
        self.daw.update(cx, |daw, _| daw.gesture(active));
    }

    // Roll interactions, in the canvas's local pixels.

    fn roll_down(&mut self, at: Point<f32>, roll: Roll, alt: bool, cx: &mut Context<Self>) {
        let mode = self.mode(cx);
        let Some(clip) = self.midi_clip(cx) else {
            return;
        };
        if mode == Mode::Score || at.y < roll.grid_top() {
            return;
        }
        let division = self.division(cx);
        let length = clip.length_bars * roll.bpb;
        let clip_notes = notes(Some(&clip));
        if let Some((note, edge)) = roll.hit_note(clip_notes, at.x, at.y) {
            let note = note.clone();
            self.select_note(&clip, Some(&note.id), cx);
            self.audition(&clip, note.pitch as i32, note.velocity, cx);
            if mode == Mode::Step {
                self.run(
                    "note.remove",
                    json!({ "clipId": clip.id, "noteId": note.id }),
                    cx,
                );
                return;
            }
            self.drag = Some(if edge {
                RollDrag::Resize {
                    note: note.id,
                    start: note.start,
                    pitch: note.pitch as i32,
                    length: note.length,
                    original: note.length,
                }
            } else {
                RollDrag::Move {
                    grab: roll.beat_at_x(at.x) - note.start,
                    origin: (note.start, note.pitch as i32),
                    start: note.start,
                    pitch: note.pitch as i32,
                    length: note.length,
                    note: note.id,
                    pressed: at,
                    moved: false,
                }
            });
            self.gesture(true, cx);
            cx.notify();
            return;
        }
        self.select_note(&clip, None, cx);
        let pitch = roll.pitch_at_y(at.y);
        if !(0..=127).contains(&pitch) {
            return;
        }
        let beat = snap(roll.beat_at_x(at.x), division, alt).max(0.0);
        if beat >= length {
            return;
        }
        if mode == Mode::Step {
            let len = step_beats(division).min(length - beat);
            let velocity = self.velocity;
            self.run(
                "note.add",
                json!({ "clipId": clip.id, "start": beat, "length": len, "pitch": pitch, "velocity": velocity }),
                cx,
            );
            self.audition(&clip, pitch, velocity, cx);
            return;
        }
        // A click on empty space adds a step-long note; a drag draws a longer one.
        self.drag = Some(RollDrag::Pencil {
            anchor: beat,
            start: beat,
            length: 0.0,
            pitch,
        });
        self.gesture(true, cx);
        cx.notify();
    }

    fn roll_move(
        &mut self,
        at: Point<f32>,
        roll: Roll,
        alt: bool,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(mut drag) = self.drag.take() else {
            if hovered {
                let cursor = self.roll_hover_cursor(at, roll, cx);
                if cursor != self.roll_cursor {
                    self.roll_cursor = cursor;
                    cx.notify();
                }
            }
            return;
        };
        let Some(clip) = self.midi_clip(cx) else {
            return;
        };
        let division = self.division(cx);
        let length = clip.length_bars * roll.bpb;
        let beat = roll.beat_at_x(at.x);
        let min_len = step_beats(division);
        let mut audition = None;
        match &mut drag {
            RollDrag::Move {
                grab,
                start,
                pitch,
                length: note_length,
                pressed,
                moved,
                ..
            } => {
                let travel = ((at.x - pressed.x).powi(2) + (at.y - pressed.y).powi(2)).sqrt();
                if *moved || travel >= DRAG_THRESHOLD {
                    *moved = true;
                    *start = snap(beat - *grab, division, alt)
                        .min(length - *note_length)
                        .max(0.0);
                    let next = roll.pitch_at_y(at.y).clamp(0, 127);
                    if next != *pitch {
                        *pitch = next;
                        audition = Some(next);
                    }
                }
            }
            RollDrag::Resize {
                start,
                length: note_length,
                ..
            } => {
                *note_length = (snap(beat, division, alt) - *start)
                    .min(length - *start)
                    .max(min_len);
            }
            RollDrag::Pencil {
                anchor,
                start,
                length: note_length,
                ..
            } => {
                let b = snap(beat, division, alt).clamp(0.0, length);
                *start = anchor.min(b);
                *note_length = (b - *anchor).abs().max(min_len);
            }
        }
        self.drag = Some(drag);
        if let Some(pitch) = audition {
            let velocity = self.velocity;
            self.audition(&clip, pitch, velocity, cx);
        }
        cx.notify();
    }

    fn roll_up(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        cx.notify();
        let clip = self.midi_clip(cx);
        if let Some(clip) = clip {
            let length = clip.length_bars * self.daw.read(cx).app.store.session().beats_per_bar();
            match drag {
                RollDrag::Move {
                    note,
                    origin,
                    start,
                    pitch,
                    moved,
                    ..
                } => {
                    if moved && (start != origin.0 || pitch != origin.1) {
                        self.run(
                            "note.update",
                            json!({ "clipId": clip.id, "noteId": note, "start": start, "pitch": pitch }),
                            cx,
                        );
                    }
                }
                RollDrag::Resize {
                    note,
                    length: new_length,
                    original,
                    ..
                } => {
                    if new_length != original {
                        self.run(
                            "note.update",
                            json!({ "clipId": clip.id, "noteId": note, "length": new_length }),
                            cx,
                        );
                    }
                }
                RollDrag::Pencil {
                    start,
                    length: drawn,
                    pitch,
                    ..
                } => {
                    let division = self.division(cx);
                    let len = if drawn > 0.0 {
                        drawn
                    } else {
                        step_beats(division)
                    }
                    .min(length - start);
                    if len > 0.0 {
                        let velocity = self.velocity;
                        self.run(
                            "note.add",
                            json!({ "clipId": clip.id, "start": start, "length": len, "pitch": pitch, "velocity": velocity }),
                            cx,
                        );
                        self.audition(&clip, pitch, velocity, cx);
                    }
                }
            }
        }
        self.gesture(false, cx);
    }

    fn roll_hover_cursor(&self, at: Point<f32>, roll: Roll, cx: &App) -> CursorStyle {
        let mode = self.mode(cx);
        let Some(clip) = self.midi_clip(cx) else {
            return CursorStyle::Arrow;
        };
        if mode == Mode::Score || at.y < roll.grid_top() {
            return CursorStyle::Arrow;
        }
        match roll.hit_note(notes(Some(&clip)), at.x, at.y) {
            Some((_, true)) => CursorStyle::ResizeLeftRight,
            Some((_, false)) => CursorStyle::Arrow,
            None => CursorStyle::Crosshair,
        }
    }

    /// Right click on a note: its velocity and Delete.
    fn roll_context(
        &mut self,
        at: Point<f32>,
        roll: Roll,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode(cx) == Mode::Score {
            return;
        }
        let Some(clip) = self.midi_clip(cx) else {
            return;
        };
        let Some((note, _)) = roll.hit_note(notes(Some(&clip)), at.x, at.y) else {
            return;
        };
        let note = note.clone();
        self.select_note(&clip, Some(&note.id), cx);
        let mut items: Vec<MenuItem> = VELOCITIES
            .iter()
            .map(|&v| {
                let (daw, clip_id, note_id) = (self.daw.clone(), clip.id.clone(), note.id.clone());
                MenuItem::new(format!("Velocity {v}"), move |_, cx| {
                    daw.update(cx, |daw, cx| {
                        daw.run(
                            "note.update",
                            json!({ "clipId": clip_id, "noteId": note_id, "velocity": v }),
                            cx,
                        );
                    })
                })
                .checked(note.velocity == v)
            })
            .collect();
        items.push(MenuItem::Separator);
        let (daw, clip_id, note_id) = (self.daw.clone(), clip.id.clone(), note.id.clone());
        items.push(
            MenuItem::new("Delete Note", move |_, cx| {
                daw.update(cx, |daw, cx| {
                    daw.run(
                        "note.remove",
                        json!({ "clipId": clip_id, "noteId": note_id }),
                        cx,
                    );
                })
            })
            .detail("⌫"),
        );
        self.menu.open(items, position, window, cx);
    }

    /// The wheel scrolls the keyboard: whole rows, up shows higher pitches.
    fn roll_wheel(&mut self, delta_y: f32, roll: Roll, cx: &mut Context<Self>) {
        use crate::ui::theme::editor::KEY_ROW;
        self.wheel += delta_y;
        let rows = (self.wheel / KEY_ROW).trunc() as i32;
        if rows == 0 {
            return;
        }
        self.wheel -= rows as f32 * KEY_ROW;
        let low = geometry::scrolled_low(roll.low, rows, roll.rows);
        if low != roll.low {
            self.run("view.set", json!({ "editorLowPitch": low }), cx);
        }
    }

    fn key_down(&mut self, pitch: i32, cx: &mut Context<Self>) {
        let Some(clip) = self.midi_clip(cx) else {
            return;
        };
        if !(0..=127).contains(&pitch) {
            return;
        }
        self.pressed_key = Some(pitch);
        let velocity = self.velocity;
        self.audition(&clip, pitch, velocity, cx);
        cx.notify();
    }

    fn open_velocity_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        let selected = self.selected_note(cx);
        let current = selected.as_ref().map_or(self.velocity, |(_, n)| n.velocity);
        let items = VELOCITIES
            .iter()
            .map(|&v| {
                let this = this.clone();
                let selected = selected.clone();
                MenuItem::new(format!("Velocity {v}"), move |_, cx| {
                    let _ = this.update(cx, |editor, cx| {
                        editor.velocity = v;
                        if let Some((clip, note)) = &selected {
                            editor.run(
                                "note.update",
                                json!({ "clipId": clip, "noteId": note.id, "velocity": v }),
                                cx,
                            );
                        }
                        cx.notify();
                    });
                })
                .checked(current == v)
            })
            .collect();
        self.menu.open(items, position, window, cx);
    }

    /// The selected note of the open clip, with the clip's id.
    fn selected_note(&self, cx: &App) -> Option<(String, Note)> {
        let s = self.daw.read(cx).app.store.session();
        let note_id = s.view.selected_note_id.as_ref()?;
        let clip = self.midi_clip(cx)?;
        notes(Some(&clip))
            .iter()
            .find(|n| &n.id == note_id)
            .cloned()
            .map(|n| (clip.id.clone(), n))
    }

    /// What a drag in the roll shows.
    fn overlay(&self) -> Overlay {
        match &self.drag {
            Some(RollDrag::Move {
                start,
                pitch,
                length,
                moved: true,
                ..
            }) => Overlay {
                ghost: Some(Ghost {
                    start: *start,
                    length: *length,
                    pitch: *pitch,
                }),
                pencil: None,
            },
            Some(RollDrag::Resize {
                start,
                pitch,
                length,
                ..
            }) => Overlay {
                ghost: Some(Ghost {
                    start: *start,
                    length: *length,
                    pitch: *pitch,
                }),
                pencil: None,
            },
            Some(RollDrag::Pencil {
                start,
                length,
                pitch,
                ..
            }) if *length > 0.0 => Overlay {
                ghost: None,
                pencil: Some(Ghost {
                    start: *start,
                    length: *length,
                    pitch: *pitch,
                }),
            },
            _ => Overlay::default(),
        }
    }

    fn toolbar(&mut self, shown: &Shown, width: f32, cx: &mut Context<Self>) -> gpui::AnyElement {
        // Narrow: the clip's details and the settings' names give way (tooltips keep them).
        let roomy = width >= 1000.0;
        let theme = Theme::get(cx).clone();
        let daw = self.daw.clone();
        let mode = shown.mode;
        let tabs = Segmented::new(
            "editor-mode",
            Mode::ALL.iter().map(|m| m.label()),
            Mode::ALL.iter().position(|m| *m == mode).unwrap_or(0),
        )
        .on_select(move |i, _, cx| {
            let id = Mode::ALL[i].id();
            daw.update(cx, |daw, cx| {
                daw.run("view.set", json!({ "editorMode": id }), cx);
            });
        });
        let dim = |text: &str| {
            div()
                .whitespace_nowrap()
                .text_color(theme.text_3)
                .child(text.to_string())
        };
        // The area's title is the view it shows; what it shows (the clip) follows in mono.
        let heading = match mode {
            Mode::PianoRoll => "Piano roll",
            Mode::Score => "Score",
            Mode::Step => "Step sequencer",
        };
        let title = match &shown.clip {
            Some(clip) => div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(px(8.0))
                .child(panel_title(heading, cx))
                .child(div().flex_none().size(px(9.0)).bg(shown.track_color))
                .when(roomy, |d| {
                    d.child(panel_info(
                        format!(
                            "{} · bars {} – {}",
                            clip.name,
                            piano_roll::bar_number(clip.start_bar + 1.0),
                            piano_roll::bar_number(clip.start_bar + clip.length_bars)
                        ),
                        cx,
                    ))
                })
                .into_any_element(),
            None => div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(px(10.0))
                .child(panel_title(heading, cx))
                .when(roomy, |d| {
                    d.child(panel_info(
                        "Select a MIDI clip to edit it, or draw one with the pencil tool",
                        cx,
                    ))
                })
                .into_any_element(),
        };
        let velocity = self
            .selected_note(cx)
            .map_or(self.velocity, |(_, n)| n.velocity);
        let scale = shown.key.replace("min", "minor").replace("maj", "major");
        let quantize = if shown.division == 1 {
            "Bar".to_string()
        } else {
            format!("1/{}", shown.division)
        };
        let cell = |label: &str, value: String| {
            div()
                .flex()
                .items_center()
                .gap(px(5.0))
                .h_full()
                .px(px(8.0))
                .whitespace_nowrap()
                .when(roomy, |d| d.child(dim(label)))
                .child(
                    div()
                        .font_family(FONT_MONO)
                        .text_color(theme.text)
                        .child(value),
                )
        };
        let settings = group(
            [
                cell("Quantize", quantize).into_any_element(),
                cell("Velocity", velocity.to_string())
                    .id("editor-velocity")
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.hover))
                    .tooltip(|_, cx| super::widgets::tip("Velocity for new notes".into(), cx))
                    .on_click(cx.listener(|this, e: &gpui::ClickEvent, window, cx| {
                        let at = e.position();
                        this.open_velocity_menu(
                            gpui::point(at.x - px(40.0), at.y + px(14.0)),
                            window,
                            cx,
                        );
                    }))
                    .into_any_element(),
                cell("Scale", scale).into_any_element(),
            ],
            cx,
        );
        let lanes = {
            let daw = self.daw.read(cx);
            group(
                [
                    actions::tool("toggleControllerLane", "sliders", "Controllers", true, daw)
                        .into_any_element(),
                ],
                cx,
            )
        };
        div()
            .h(px(layout::TOOLBAR))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(12.0))
            .bg(theme.glass(1))
            .border_b_1()
            .border_color(theme.line)
            .text_size(px(size::SM))
            .overflow_hidden()
            .child(title)
            .when(mode == Mode::Score, |d| {
                d.child(
                    dim("Preview · edit in Piano Roll or Step")
                        .min_w_0()
                        .overflow_hidden(),
                )
            })
            .child(div().flex_1())
            .child(tabs)
            .child(settings)
            .child(lanes)
            .into_any_element()
    }
}

/// Mouse handlers of a canvas, in its local pixels: press, move (also outside while a drag
/// lasts), release, right click and the wheel. Registered while painting, like GPUI's own
/// elements do, so the hit testing reads the geometry that was just painted.
pub(super) struct CanvasEvents {
    pub down: OnPress,
    pub moved: OnMove,
    pub up: OnRelease,
    pub right: Option<OnPress>,
    pub wheel: Option<OnWheel>,
}

/// A press at a point in the canvas.
pub(super) type OnPress = Box<dyn Fn(Point<f32>, &MouseDownEvent, &mut Window, &mut App)>;
/// A move, and whether the pointer is over the canvas (moves arrive from outside while a
/// drag lasts).
pub(super) type OnMove = Box<dyn Fn(Point<f32>, &MouseMoveEvent, bool, &mut Window, &mut App)>;
pub(super) type OnRelease = Box<dyn Fn(Point<f32>, &MouseUpEvent, &mut Window, &mut App)>;
/// Vertical wheel travel in pixels; positive scrolls up.
pub(super) type OnWheel = Box<dyn Fn(f32, &mut Window, &mut App)>;

impl CanvasEvents {
    pub fn register(self, bounds: gpui::Bounds<Pixels>, hitbox: &Hitbox, window: &mut Window) {
        let local = move |p: Point<Pixels>| {
            let d = p - bounds.origin;
            Point {
                x: f32::from(d.x),
                y: f32::from(d.y),
            }
        };
        let (down, right) = (self.down, self.right);
        let h = hitbox.clone();
        window.on_mouse_event(move |e: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !h.is_hovered(window) {
                return;
            }
            match e.button {
                MouseButton::Left => down(local(e.position), e, window, cx),
                MouseButton::Right => {
                    if let Some(right) = &right {
                        right(local(e.position), e, window, cx)
                    }
                }
                _ => {}
            }
        });
        let (moved, h) = (self.moved, hitbox.clone());
        window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                moved(local(e.position), e, h.is_hovered(window), window, cx);
            }
        });
        let up = self.up;
        window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                up(local(e.position), e, window, cx);
            }
        });
        if let Some(wheel) = self.wheel {
            let h = hitbox.clone();
            window.on_mouse_event(move |e: &ScrollWheelEvent, phase, window, cx| {
                // Command and Control wheels belong to zooming, not to the keyboard.
                let zoom = e.modifiers.platform || e.modifiers.control;
                if phase == DispatchPhase::Bubble && !zoom && h.should_handle_scroll(window) {
                    let delta = e.delta.pixel_delta(px(crate::ui::theme::editor::KEY_ROW));
                    wheel(f32::from(delta.y), window, cx);
                }
            });
        }
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let shown = self.shown(&theme, cx);
        let show_lane =
            self.daw.read(cx).app.show_controllers && shown.mode != Mode::Score && shown.is_midi();
        self.follow_clip(&shown);
        let width = super::centre_width(window, self.daw.read(cx).app.agents.open);
        let toolbar = self.toolbar(&shown, width, cx);
        let this = cx.entity();

        // The keyboard column.
        let keys = {
            let (shown, theme, this) = (shown.clone(), theme.clone(), this.clone());
            let pressed = self.pressed_key;
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                move |bounds, hitbox, window, cx| {
                    let roll = shown.roll(bounds.size);
                    piano_roll::paint_keys(Pen::new(bounds), &roll, pressed, &theme, window, cx);
                    if shown.is_midi() {
                        window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
                    }
                    let (a, b, c) = (this.clone(), this.clone(), this.clone());
                    CanvasEvents {
                        down: Box::new(move |at, _, _, cx| {
                            if at.y >= roll.grid_top() {
                                a.update(cx, |ed, cx| ed.key_down(roll.pitch_at_y(at.y), cx));
                            }
                        }),
                        moved: Box::new(|_, _, _, _, _| {}),
                        up: Box::new(move |_, _, _, cx| {
                            b.update(cx, |ed, cx| {
                                if ed.pressed_key.take().is_some() {
                                    cx.notify();
                                }
                            })
                        }),
                        right: None,
                        wheel: Some(Box::new(move |dy, _, cx| {
                            c.update(cx, |ed, cx| ed.roll_wheel(dy, roll, cx))
                        })),
                    }
                    .register(bounds, &hitbox, window);
                },
            )
            .size_full()
        };

        // The roll, the step view or the score.
        let grid = {
            let (shown, theme, this) = (shown.clone(), theme.clone(), this.clone());
            let overlay = self.overlay();
            let cursor = if shown.mode == Mode::Score || !shown.is_midi() {
                CursorStyle::Arrow
            } else if self.drag.is_some() {
                match self.drag {
                    Some(RollDrag::Resize { .. }) => CursorStyle::ResizeLeftRight,
                    Some(RollDrag::Move { .. }) => CursorStyle::ClosedHand,
                    _ => CursorStyle::Crosshair,
                }
            } else {
                self.roll_cursor
            };
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                move |bounds, hitbox, window, cx| {
                    let roll = shown.roll(bounds.size);
                    let pen = Pen::new(bounds);
                    if shown.mode == Mode::Score {
                        score::paint_score(pen, &roll, &shown, &theme, window, cx);
                    } else {
                        piano_roll::paint_roll(pen, &roll, &shown, &overlay, &theme, window, cx);
                    }
                    window.set_cursor_style(cursor, &hitbox);
                    let (a, b, c, d, e) = (
                        this.clone(),
                        this.clone(),
                        this.clone(),
                        this.clone(),
                        this.clone(),
                    );
                    CanvasEvents {
                        down: Box::new(move |at, ev, _, cx| {
                            a.update(cx, |ed, cx| ed.roll_down(at, roll, ev.modifiers.alt, cx))
                        }),
                        moved: Box::new(move |at, ev, hovered, _, cx| {
                            b.update(cx, |ed, cx| {
                                ed.roll_move(at, roll, ev.modifiers.alt, hovered, cx)
                            })
                        }),
                        up: Box::new(move |_, _, _, cx| c.update(cx, |ed, cx| ed.roll_up(cx))),
                        right: Some(Box::new(move |at, ev, window, cx| {
                            d.update(cx, |ed, cx| {
                                ed.roll_context(at, roll, ev.position, window, cx)
                            })
                        })),
                        wheel: Some(Box::new(move |dy, _, cx| {
                            e.update(cx, |ed, cx| ed.roll_wheel(dy, roll, cx))
                        })),
                    }
                    .register(bounds, &hitbox, window);
                },
            )
            .size_full()
        };

        let empty = match &shown.clip {
            None => Some("No MIDI region open".to_string()),
            Some(clip) if !shown.is_midi() => Some(format!(
                "{} is an audio region · the editor shows MIDI regions",
                clip.name
            )),
            _ => None,
        };
        let body = div()
            .flex()
            .flex_1()
            .min_h_0()
            .child(div().w(px(KEY_COLUMN)).flex_none().h_full().child(keys))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(grid)
                    .when_some(empty, |d, text| {
                        d.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(size::BASE))
                                .text_color(theme.text_3)
                                .child(text),
                        )
                    }),
            );

        let lane = show_lane.then(|| self.lane_row(&shown, window, cx));
        let menu = self.menu.render(window, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.editor)
            .child(toolbar)
            .child(body)
            .when_some(lane, |d, lane| {
                d.child(
                    div()
                        .h(px(CONTROLLER_LANE))
                        .flex_none()
                        .border_t_1()
                        .border_color(theme.line)
                        .child(lane),
                )
            })
            .children(menu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Ryolune;
    use crate::ui::theme::{editor::KEY_ROW, Mode as ThemeMode};
    use gpui::{point, Modifiers, ScrollDelta, TestAppContext, VisualTestContext};

    fn open(cx: &mut TestAppContext) -> (Entity<Editor>, Entity<Daw>, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(Theme::new(ThemeMode::Dark, true)));
        let daw = cx.new(|_| Daw::new(Ryolune::from_session(ryolune_engine::store::demo(), None)));
        let d = daw.clone();
        let (editor, cx) = cx.add_window_view(|window, cx| Editor::new(d, window, cx));
        cx.run_until_parked();
        (editor, daw, cx)
    }

    /// The roll as the window lays it out, and where its canvas starts.
    fn roll(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> (Roll, Point<Pixels>) {
        cx.update(|window, cx| {
            let size = window.viewport_size();
            let origin = point(px(KEY_COLUMN), px(layout::TOOLBAR));
            let grid = gpui::size(size.width - origin.x, size.height - origin.y);
            let theme = Theme::get(cx).clone();
            (editor.read(cx).shown(&theme, cx).roll(grid), origin)
        })
    }

    fn clip_notes(daw: &Entity<Daw>, cx: &mut VisualTestContext) -> Vec<Note> {
        cx.update(|_, cx| {
            let s = daw.read(cx).app.store.session();
            let clip = s.clips.iter().find(|c| c.id == "bass-2").unwrap();
            notes(Some(clip)).to_vec()
        })
    }
    fn undo_depth(daw: &Entity<Daw>, cx: &mut VisualTestContext) -> usize {
        cx.update(|_, cx| daw.read(cx).app.store.undo_depth())
    }

    #[gpui::test]
    fn drawing_moving_and_resizing_notes_are_one_undo_step_each(cx: &mut TestAppContext) {
        let (editor, daw, cx) = open(cx);
        let (roll, origin) = roll(&editor, cx);
        let at = |beat: f64, pitch: i32| {
            origin
                + point(
                    px(roll.x_of_beat(beat)),
                    px(roll.y_of_pitch(pitch) + KEY_ROW / 2.0),
                )
        };
        let before = clip_notes(&daw, cx);
        let depth = undo_depth(&daw, cx);
        // A pitch the bass line does not use.
        let pitch = roll.high() - 1;
        assert!(before.iter().all(|n| n.pitch as i32 != pitch));

        // A click on empty space adds a sixteenth on the grid.
        cx.simulate_mouse_down(at(1.07, pitch), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(at(1.07, pitch), MouseButton::Left, Modifiers::none());
        let after = clip_notes(&daw, cx);
        assert_eq!(after.len(), before.len() + 1);
        let added = after
            .iter()
            .find(|n| n.pitch as i32 == pitch)
            .unwrap()
            .clone();
        assert_eq!(
            (added.start, added.length, added.velocity),
            (1.0, 0.25, 100)
        );
        assert_eq!(undo_depth(&daw, cx), depth + 1);

        // Dragging it one beat later and two keys down moves it.
        cx.simulate_mouse_down(at(1.05, pitch), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(at(1.5, pitch - 1), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(at(2.05, pitch - 2), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(at(2.05, pitch - 2), MouseButton::Left, Modifiers::none());
        let moved = clip_notes(&daw, cx)
            .into_iter()
            .find(|n| n.id == added.id)
            .unwrap();
        assert_eq!(
            (moved.start, moved.pitch as i32, moved.length),
            (2.0, pitch - 2, 0.25)
        );
        assert_eq!(undo_depth(&daw, cx), depth + 2);

        // Its right edge stretches it to the grid.
        let edge = at(2.25, pitch - 2) - point(px(2.0), px(0.0));
        cx.simulate_mouse_down(edge, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(at(3.0, pitch - 2), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(at(3.0, pitch - 2), MouseButton::Left, Modifiers::none());
        let resized = clip_notes(&daw, cx)
            .into_iter()
            .find(|n| n.id == added.id)
            .unwrap();
        assert_eq!((resized.start, resized.length), (2.0, 1.0));
        assert_eq!(undo_depth(&daw, cx), depth + 3);

        // Dragging on empty space draws a longer note.
        cx.simulate_mouse_down(at(4.0, pitch), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(at(5.6, pitch), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(at(5.6, pitch), MouseButton::Left, Modifiers::none());
        let drawn = clip_notes(&daw, cx)
            .into_iter()
            .find(|n| n.pitch as i32 == pitch && n.start == 4.0)
            .unwrap();
        assert_eq!(drawn.length, 1.5);
        assert_eq!(undo_depth(&daw, cx), depth + 4);
    }

    #[gpui::test]
    fn the_wheel_scrolls_the_keyboard_and_the_step_view_toggles_notes(cx: &mut TestAppContext) {
        let (editor, daw, cx) = open(cx);
        let (roll, origin) = roll(&editor, cx);
        let low = roll.low;
        cx.simulate_event(ScrollWheelEvent {
            position: origin + point(px(100.0), px(100.0)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(KEY_ROW * 3.0))),
            ..Default::default()
        });
        let view_low = cx.update(|_, cx| daw.read(cx).app.store.session().view.editor_low_pitch);
        assert_eq!(view_low, Some((low + 3) as u8));

        daw.update(cx, |daw, cx| {
            daw.run("view.set", json!({ "editorMode": "step" }), cx);
        });
        cx.run_until_parked();
        let (roll, origin) = self::roll(&editor, cx);
        let pitch = roll.high() - 1;
        let at = origin + point(px(roll.x_of_beat(3.1)), px(roll.y_of_pitch(pitch) + 4.0));
        let count = clip_notes(&daw, cx).len();
        cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
        assert_eq!(clip_notes(&daw, cx).len(), count + 1);
        // A second click on the step clears it.
        cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
        assert_eq!(clip_notes(&daw, cx).len(), count);
    }

    fn lane_points(
        daw: &Entity<Daw>,
        cx: &mut VisualTestContext,
    ) -> Vec<ryolune_engine::model::Controller> {
        cx.update(|_, cx| {
            let s = daw.read(cx).app.store.session();
            let clip = s.clips.iter().find(|c| c.id == "bass-2").unwrap();
            lane::lane_points(Some(clip), &Lane::cc(1))
        })
    }

    #[gpui::test]
    fn the_controller_lane_adds_draws_and_deletes_points(cx: &mut TestAppContext) {
        let (editor, daw, cx) = open(cx);
        daw.update(cx, |daw, cx| {
            daw.run(
                "ui.showPanel",
                json!({ "panel": "controllers", "visible": true }),
                cx,
            );
        });
        cx.run_until_parked();
        let (origin, size) = cx.update(|window, _| {
            let v = window.viewport_size();
            // The lane row sits under a 1 px rule at the bottom of the pane.
            let top = v.height - px(CONTROLLER_LANE) + px(1.0);
            (
                point(px(KEY_COLUMN), top),
                gpui::size(v.width - px(KEY_COLUMN), px(CONTROLLER_LANE - 1.0)),
            )
        });
        let roll = cx.update(|_, cx| {
            let theme = Theme::get(cx).clone();
            editor.read(cx).shown(&theme, cx).roll(size)
        });
        let h = f32::from(size.height);
        let lane = Lane::cc(1);
        let at = |beat: f64, value: i16| {
            origin
                + point(
                    px(roll.x_of_beat(beat)),
                    px(lane::y_of_value(&lane, value, h)),
                )
        };
        assert!(lane_points(&daw, cx).is_empty());
        let depth = undo_depth(&daw, cx);

        // A click adds one point on the grid.
        cx.simulate_mouse_down(at(1.1, 64), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(at(1.1, 64), MouseButton::Left, Modifiers::none());
        let points = lane_points(&daw, cx);
        assert_eq!(points.len(), 1);
        assert_eq!((points[0].time, points[0].value), (1.0, 64));
        assert_eq!(undo_depth(&daw, cx), depth + 1);

        // A stroke lands one point per sixteenth, in one step.
        cx.simulate_mouse_down(at(2.0, 0), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(at(2.6, 60), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(at(3.1, 120), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(at(3.1, 120), MouseButton::Left, Modifiers::none());
        let points = lane_points(&daw, cx);
        let drawn: Vec<f64> = points
            .iter()
            .map(|p| p.time)
            .filter(|t| *t >= 2.0)
            .collect();
        assert_eq!(drawn, [2.0, 2.25, 2.5, 2.75, 3.0]);
        assert_eq!(undo_depth(&daw, cx), depth + 2);

        // Option-click on a point deletes it.
        let alt = Modifiers {
            alt: true,
            ..Modifiers::none()
        };
        cx.simulate_mouse_down(at(1.0, 64), MouseButton::Left, alt);
        cx.simulate_mouse_up(at(1.0, 64), MouseButton::Left, alt);
        assert!(lane_points(&daw, cx).iter().all(|p| p.time != 1.0));
    }

    #[gpui::test]
    fn right_click_on_a_note_selects_it_and_offers_its_menu(cx: &mut TestAppContext) {
        let (editor, daw, cx) = open(cx);
        let (roll, origin) = roll(&editor, cx);
        let note = clip_notes(&daw, cx)
            .into_iter()
            .find(|n| (n.pitch as i32) >= roll.low && (n.pitch as i32) <= roll.high())
            .unwrap();
        let at = origin
            + point(
                px(roll.x_of_beat(note.start) + 2.0),
                px(roll.y_of_pitch(note.pitch as i32) + KEY_ROW / 2.0),
            );
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
        let selected = cx.update(|_, cx| {
            daw.read(cx)
                .app
                .store
                .session()
                .view
                .selected_note_id
                .clone()
        });
        assert_eq!(selected.as_deref(), Some(note.id.as_str()));
        assert!(cx.update(|_, cx| editor.read(cx).menu.is_open()));
    }
}
