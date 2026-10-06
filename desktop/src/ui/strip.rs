//! What the inspector and the mixer share about a channel strip: how a strip reads (padded
//! inserts and sends, where the fader goes, what a send feeds), how its dials map onto the
//! registry's values, and the pieces both draw (channel keys, the fader with its meter and
//! scale). Strips exist in the document only once edited, so every reader here pads what is
//! missing the way the engine's `strip.get` does.

use super::{
    daw::Daw,
    format,
    theme::{radius, Theme, FONT_MONO},
    widgets::{icon, tip, Fader, Meter, Phase},
};
use gpui::{div, prelude::*, px, App, ElementId, Entity, Hsla, SharedString, Stateful};
use ryolune_engine::model::{
    self, Insert, Monitor, Session, Track, BUS_A, BUS_B, MASTER, MAX_INSERTS,
};
use serde_json::{json, Value};

/// The lowest level a send knob shows; below it the send is off.
pub const SEND_MIN_DB: f32 = -48.0;
/// Segments of a channel meter.
pub const METER_SEGMENTS: usize = 20;
/// The labels beside a fader, in dB (−∞ at the bottom).
pub const SCALE_DB: [f32; 6] = [6.0, 0.0, -6.0, -12.0, -24.0, f32::NEG_INFINITY];
/// Height of the fader widget's cap: the travel is the box less the cap.
const FADER_CAP: f32 = 14.0;

/// One send as a strip shows it: where it goes and how loud.
#[derive(Clone, Debug, PartialEq)]
pub struct SendRow {
    pub index: usize,
    /// The strip it feeds: `bus-a`, `bus-b` or a bus track's id.
    pub bus: Option<String>,
    pub name: String,
    /// `None` is off.
    pub level_db: Option<f32>,
}

/// The eight insert slots of a strip, empty ones included.
pub fn inserts(session: &Session, id: &str) -> Vec<Insert> {
    let mut slots = session
        .strips
        .get(id)
        .map(|s| s.inserts.clone())
        .unwrap_or_default();
    slots.truncate(MAX_INSERTS);
    while slots.len() < MAX_INSERTS {
        slots.push(Insert::empty_slot());
    }
    slots
}

/// The last slot that holds an effect.
pub fn last_used(slots: &[Insert]) -> Option<usize> {
    slots.iter().rposition(|i| !i.is_empty())
}

/// How many slots the inspector lists: every used one and one free slot after them.
pub fn visible_slots(slots: &[Insert]) -> usize {
    last_used(slots).map_or(1, |i| i + 2).clamp(1, MAX_INSERTS)
}

/// Effects loaded on a strip, bypassed ones included.
pub fn fx_count(session: &Session, id: &str) -> usize {
    session
        .strips
        .get(id)
        .map_or(0, |s| s.inserts.iter().filter(|i| !i.is_empty()).count())
}

/// The name of what a send feeds: A · Reverb, B · Delay or a bus track.
pub fn send_target_name(session: &Session, bus: Option<&str>) -> String {
    match bus {
        Some(BUS_A) => model::bus_name(BUS_A).into(),
        Some(BUS_B) => model::bus_name(BUS_B).into(),
        Some(id) => session
            .tracks
            .iter()
            .find(|t| t.id == id)
            .map_or_else(|| id.to_string(), |t| t.name.clone()),
        None => "—".into(),
    }
}

/// A strip's sends, at least the two that feed A and B by default.
pub fn sends(session: &Session, id: &str) -> Vec<SendRow> {
    let stored = session
        .strips
        .get(id)
        .map(|s| s.sends.clone())
        .unwrap_or_default();
    (0..stored.len().max(2))
        .map(|index| {
            let send = stored.get(index);
            let bus = send
                .and_then(|s| s.target(index))
                .or(match index {
                    0 => Some(BUS_A),
                    1 => Some(BUS_B),
                    _ => None,
                })
                .map(str::to_string);
            SendRow {
                index,
                name: send_target_name(session, bus.as_deref()),
                bus,
                level_db: send.and_then(|s| s.level_db),
            }
        })
        .collect()
}

/// Where a track's fader goes, by name.
pub fn output_name(session: &Session, track: &Track) -> String {
    track
        .output
        .as_deref()
        .and_then(|id| session.tracks.iter().find(|b| b.id == id))
        .map_or_else(|| "Stereo Out".to_string(), |b| b.name.clone())
}

/// The bus tracks of the song, in arrangement order.
pub fn bus_tracks(session: &Session) -> Vec<&Track> {
    session.tracks.iter().filter(|t| t.is_bus()).collect()
}

/// A track's kind as the mixer's caps label writes it.
pub fn kind_caps(kind: &str) -> &'static str {
    match kind {
        "midi" => "MIDI",
        "bus" => "BUS",
        _ => "AUD",
    }
}

/// Input monitoring cycles off → auto → on → off, like its key.
pub fn next_monitor(monitor: Monitor) -> Monitor {
    match monitor {
        Monitor::Off => Monitor::Auto,
        Monitor::Auto => Monitor::On,
        Monitor::On => Monitor::Off,
    }
}

/// What the monitoring key says about itself.
pub fn monitor_tooltip(monitor: Monitor, blocked: bool) -> String {
    let base = match monitor {
        Monitor::Off => "Input monitoring off · click for Auto",
        Monitor::Auto => {
            "Input monitoring Auto: heard while armed, until the track plays its own clip · click for On"
        }
        Monitor::On => "Input monitoring On: always heard · click for Off",
    };
    if blocked && monitor != Monitor::Off {
        format!("{base} · muted: built-in speakers would feed back")
    } else {
        base.to_string()
    }
}

/// A send level as a knob position: −48 dB (or off) at the bottom, 0 dB at the top.
pub fn send_to_knob(level_db: Option<f32>) -> f32 {
    level_db.map_or(0.0, |db| {
        ((db.max(SEND_MIN_DB) - SEND_MIN_DB) / -SEND_MIN_DB).clamp(0.0, 1.0)
    })
}
/// A knob position as a send level: the bottom half-decibel is off, the rest in 0.5 dB steps.
pub fn knob_to_send(value: f32) -> Option<f32> {
    let db = SEND_MIN_DB + value.clamp(0.0, 1.0) * -SEND_MIN_DB;
    if db <= SEND_MIN_DB + 0.5 {
        None
    } else {
        Some((db * 2.0).round() / 2.0)
    }
}
/// "−12.0 dB", "−∞ dB".
pub fn send_label(level_db: Option<f32>) -> String {
    format!(
        "{} dB",
        format::db(level_db.unwrap_or(f32::NEG_INFINITY), 1)
    )
}

/// Pan (−100..100, as the document keeps it) as a knob position.
pub fn pan_to_knob(pan: f32) -> f32 {
    ((pan + 100.0) / 200.0).clamp(0.0, 1.0)
}
/// A knob position as a whole pan step.
pub fn knob_to_pan(value: f32) -> f32 {
    (value.clamp(0.0, 1.0) * 200.0 - 100.0).round()
}
/// "C", "L 15", "R 40".
pub fn pan_label(pan: f32) -> String {
    format::pan(pan / 100.0)
}

/// A level as a meter position on the fader's scale, so the meter and the fader read the
/// same dB at the same height and the scale beside them serves both.
pub fn meter_level(peak: f32) -> f32 {
    format::db_to_fader(format::peak_db(peak))
}

/// Run one step of a continuous edit (a fader, a knob): the press opens one undo step, the
/// release closes it, and a value goes out only when it differs from the document's.
pub fn continuous(
    daw: &Entity<Daw>,
    phase: Phase,
    cx: &mut App,
    command: impl FnOnce(&Daw) -> Option<(&'static str, Value)>,
) {
    daw.update(cx, |daw, cx| {
        if phase == Phase::Start {
            daw.gesture(true);
        }
        if let Some((method, params)) = command(daw) {
            daw.run(method, params, cx);
        }
        if phase == Phase::End {
            daw.gesture(false);
        }
    });
}

/// Move a fader: a track's (`track.setVolume`) or the Stereo Out's (`master.setVolume`).
pub fn set_volume(daw: &Entity<Daw>, id: &str, value: f32, phase: Phase, cx: &mut App) {
    let id = id.to_string();
    continuous(daw, phase, cx, move |daw| {
        let session = daw.app.store.session();
        if id == MASTER {
            (session.master_volume != value)
                .then(|| ("master.setVolume", json!({ "volume": value })))
        } else {
            let track = session.tracks.iter().find(|t| t.id == id)?;
            (track.volume != value)
                .then(|| ("track.setVolume", json!({ "trackId": id, "volume": value })))
        }
    });
}

/// Turn a pan knob.
pub fn set_pan(daw: &Entity<Daw>, id: &str, value: f32, phase: Phase, cx: &mut App) {
    let id = id.to_string();
    let pan = knob_to_pan(value);
    continuous(daw, phase, cx, move |daw| {
        let track = daw.app.store.session().tracks.iter().find(|t| t.id == id)?;
        (track.pan != pan).then(|| ("track.setPan", json!({ "trackId": id, "pan": pan })))
    });
}

/// Run a one-shot command from a click.
pub fn fire(daw: &Entity<Daw>, method: &'static str, params: Value, cx: &mut App) {
    daw.update(cx, |daw, cx| {
        daw.run(method, params, cx);
    });
}

/// What a channel key shows: a letter or an icon.
pub enum Face {
    Text(&'static str),
    Icon(&'static str, f32),
}

/// A channel key (mute, solo, arm, monitor): small and square, lit in its own colour. The
/// caller adds the click and the tooltip.
pub fn channel_key(
    id: impl Into<ElementId>,
    face: Face,
    on: bool,
    color: Hsla,
    size: f32,
    cx: &App,
) -> Stateful<gpui::Div> {
    let theme = Theme::get(cx);
    let fg = if on {
        theme.text_on_accent
    } else {
        theme.text_2
    };
    let hover = if on { color } else { theme.control_hover };
    div()
        .id(id.into())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px(radius::XS))
        .bg(if on { color } else { theme.control })
        .border_1()
        .border_color(if on {
            color.opacity(0.9)
        } else {
            theme.control_edge
        })
        .font_family(FONT_MONO)
        .text_size(px(10.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(fg)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .map(|d| match face {
            Face::Text(text) => d.child(text),
            Face::Icon(name, size) => d.child(icon(name, size, fg)),
        })
}

/// The M, S, arm and monitoring keys of a track, as the inspector and the mixer show them:
/// buses have no arm, only audio tracks monitor.
pub fn track_keys(
    daw: &Entity<Daw>,
    track: &Track,
    blocked: bool,
    size: f32,
    cx: &App,
) -> Vec<gpui::AnyElement> {
    let theme = Theme::get(cx).clone();
    let id = track.id.clone();
    let key_id = |name: &str| ElementId::Name(format!("{id}-{name}").into());
    let mut keys = Vec::new();
    let (muted, solo, armed) = (track.mute, track.solo, track.armed);
    {
        let (daw, id) = (daw.clone(), id.clone());
        keys.push(
            channel_key(key_id("mute"), Face::Text("M"), muted, theme.mute, size, cx)
                .tooltip(|_, cx| tip("Mute".into(), cx))
                .on_click(move |_, _, cx| {
                    fire(
                        &daw,
                        "track.setMute",
                        json!({ "trackId": id, "muted": !muted }),
                        cx,
                    )
                })
                .into_any_element(),
        );
    }
    {
        let (daw, id) = (daw.clone(), id.clone());
        keys.push(
            channel_key(key_id("solo"), Face::Text("S"), solo, theme.solo, size, cx)
                .tooltip(|_, cx| tip("Solo".into(), cx))
                .on_click(move |_, _, cx| {
                    fire(
                        &daw,
                        "track.setSolo",
                        json!({ "trackId": id, "solo": !solo }),
                        cx,
                    )
                })
                .into_any_element(),
        );
    }
    if !track.is_bus() {
        let (daw, id) = (daw.clone(), id.clone());
        keys.push(
            channel_key(
                key_id("arm"),
                Face::Icon("record", 7.0),
                armed,
                theme.record,
                size,
                cx,
            )
            .tooltip(|_, cx| tip("Record arm".into(), cx))
            .on_click(move |_, _, cx| {
                fire(
                    &daw,
                    "track.setArmed",
                    json!({ "trackId": id, "armed": !armed }),
                    cx,
                )
            })
            .into_any_element(),
        );
    }
    if track.kind == "audio" {
        let monitor = track.monitor;
        let text: SharedString = monitor_tooltip(monitor, blocked).into();
        let (daw, id) = (daw.clone(), id.clone());
        let face = if monitor == Monitor::Auto {
            Face::Text("A")
        } else {
            Face::Icon("monitor", 9.0)
        };
        keys.push(
            channel_key(
                key_id("monitor"),
                face,
                monitor != Monitor::Off,
                theme.accent_fill,
                size,
                cx,
            )
            .tooltip(move |_, cx| tip(text.clone(), cx))
            .on_click(move |_, _, cx| {
                let next = next_monitor(monitor).as_str();
                fire(
                    &daw,
                    "track.setMonitor",
                    json!({ "trackId": id, "monitor": next }),
                    cx,
                )
            })
            .into_any_element(),
        );
    }
    keys
}

/// A fader with its meter (one bar per channel, on the fader's scale) and, optionally, the
/// dB scale at the right. `height` is the fader's box.
pub fn fader_block(
    id: impl Into<ElementId>,
    value: f32,
    peaks: &[f32],
    height: f32,
    scale: bool,
    on_change: impl Fn(f32, Phase, &mut gpui::Window, &mut App) + 'static,
    cx: &App,
) -> gpui::Div {
    let theme = Theme::get(cx).clone();
    let levels: Vec<f32> = peaks.iter().map(|p| meter_level(*p)).collect();
    let bars = levels.len().max(1) as f32;
    let meter_w = 5.0 * bars + (bars - 1.0) + 8.0;
    div()
        .flex()
        .flex_none()
        .items_start()
        .gap(px(8.0))
        .child(
            div()
                .h(px(height))
                .child(Fader::new(id, value).on_change(on_change)),
        )
        .child(
            div()
                .h(px(height))
                .w(px(meter_w))
                .p(px(3.0))
                .rounded(px(radius::XS))
                .bg(theme.display)
                .border_1()
                .border_color(theme.hairline)
                .child(
                    Meter::new(levels)
                        .linear()
                        .vertical()
                        .segments(METER_SEGMENTS),
                ),
        )
        .when(scale, |d| d.child(scale_labels(height, cx)))
}

/// The dB marks beside a fader, each at its height on the fader's taper, on the cap's centre
/// line.
fn scale_labels(height: f32, cx: &App) -> gpui::Div {
    let theme = Theme::get(cx);
    let travel = height - FADER_CAP;
    div()
        .relative()
        .h(px(height))
        .w(px(22.0))
        .font_family(FONT_MONO)
        .text_size(px(9.0))
        .text_color(theme.text_3)
        .children(SCALE_DB.iter().map(|db| {
            let y = FADER_CAP / 2.0 + travel * (1.0 - format::db_to_fader(*db)) - 6.0;
            div()
                .absolute()
                .top(px(y))
                .left_0()
                .line_height(px(12.0))
                .child(format::db(*db, 0))
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryolune_engine::model::Send;

    fn session() -> Session {
        ryolune_engine::store::demo()
    }

    #[test]
    fn strips_are_padded_like_strip_get() {
        let s = session();
        let slots = inserts(&s, "no-such-strip");
        assert_eq!(slots.len(), MAX_INSERTS);
        assert!(slots.iter().all(Insert::is_empty));
        assert_eq!(visible_slots(&slots), 1);
        let rows = sends(&s, "no-such-strip");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "A · Reverb");
        assert_eq!(rows[1].bus.as_deref(), Some(BUS_B));
        assert_eq!(rows[0].level_db, None);
    }

    #[test]
    fn the_inspector_lists_used_slots_and_one_free() {
        let mut slots = vec![Insert::empty_slot(); MAX_INSERTS];
        slots[2] = Insert::new("x".into(), "stock:Echo", "Echo");
        assert_eq!(last_used(&slots), Some(2));
        assert_eq!(visible_slots(&slots), 4);
        slots[7] = Insert::new("y".into(), "stock:Space", "Space");
        assert_eq!(visible_slots(&slots), MAX_INSERTS);
    }

    #[test]
    fn sends_name_what_they_feed() {
        let mut s = session();
        let bus = Track {
            id: "drums-bus".into(),
            name: "Drum Bus".into(),
            kind: "bus".into(),
            output: None,
            ..s.tracks[0].clone()
        };
        s.tracks.push(bus);
        let id = s.tracks[0].id.clone();
        let send = |level_db: Option<f32>, bus: Option<&str>| Send {
            level_db,
            name: String::new(),
            bus: bus.map(Into::into),
        };
        s.strips.entry(id.clone()).or_default().sends = vec![
            send(Some(-12.0), None),
            send(None, None),
            send(Some(-6.0), Some("drums-bus")),
        ];
        let rows = sends(&s, &id);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].name, "A · Reverb");
        assert_eq!(rows[0].level_db, Some(-12.0));
        assert_eq!(rows[2].name, "Drum Bus");
        assert_eq!(send_target_name(&s, Some("gone")), "gone");
        let mut track = s.tracks[0].clone();
        assert_eq!(output_name(&s, &track), "Stereo Out");
        track.output = Some("drums-bus".into());
        assert_eq!(output_name(&s, &track), "Drum Bus");
        assert_eq!(bus_tracks(&s).len(), 1);
    }

    #[test]
    fn dials_map_onto_registry_values() {
        assert_eq!(send_to_knob(None), 0.0);
        assert_eq!(send_to_knob(Some(0.0)), 1.0);
        assert_eq!(send_to_knob(Some(-100.0)), 0.0);
        assert_eq!(knob_to_send(0.0), None);
        assert_eq!(knob_to_send(0.005), None);
        assert_eq!(knob_to_send(1.0), Some(0.0));
        assert_eq!(knob_to_send(send_to_knob(Some(-12.0))), Some(-12.0));
        assert_eq!(knob_to_send(0.51), Some(-23.5));
        assert_eq!(send_label(None), "−∞ dB");
        assert_eq!(send_label(Some(-12.0)), "−12.0 dB");
        assert_eq!(pan_to_knob(0.0), 0.5);
        assert_eq!(knob_to_pan(pan_to_knob(-15.0)), -15.0);
        assert_eq!(knob_to_pan(0.0), -100.0);
        assert_eq!(pan_label(15.0), "R 15");
        assert_eq!(pan_label(-40.0), "L 40");
        assert_eq!(pan_label(0.0), "C");
    }

    #[test]
    fn monitoring_cycles_and_says_when_it_is_muted() {
        assert_eq!(next_monitor(Monitor::Off), Monitor::Auto);
        assert_eq!(next_monitor(Monitor::Auto), Monitor::On);
        assert_eq!(next_monitor(Monitor::On), Monitor::Off);
        assert!(monitor_tooltip(Monitor::On, true).contains("feed back"));
        assert!(!monitor_tooltip(Monitor::Off, true).contains("feed back"));
    }

    /// The window's host on the demo song, without audio.
    fn daw(cx: &mut gpui::TestAppContext) -> Entity<Daw> {
        cx.new(|_| Daw::new(crate::app::Ryolune::from_session(session(), None)))
    }

    #[gpui::test]
    fn a_drag_is_one_undo_step_and_a_press_alone_is_none(cx: &mut gpui::TestAppContext) {
        let daw = daw(cx);
        let depth =
            |cx: &mut gpui::TestAppContext| daw.read_with(cx, |d, _| d.app.store.undo_depth());
        let start = depth(cx);
        let id = daw.read_with(cx, |d, _| d.app.store.session().tracks[0].id.clone());
        let volume = daw.read_with(cx, |d, _| d.app.store.session().tracks[0].volume);
        cx.update(|cx| {
            set_volume(&daw, &id, volume, Phase::Start, cx);
            set_volume(&daw, &id, 0.5, Phase::Move, cx);
            set_volume(&daw, &id, 0.4, Phase::Move, cx);
            set_volume(&daw, &id, 0.4, Phase::End, cx);
        });
        assert_eq!(depth(cx), start + 1);
        assert_eq!(
            daw.read_with(cx, |d, _| d.app.store.session().tracks[0].volume),
            0.4
        );
        // A press and release without moving changes nothing.
        cx.update(|cx| {
            set_pan(&daw, &id, pan_to_knob(0.0), Phase::Start, cx);
            set_pan(&daw, &id, pan_to_knob(0.0), Phase::End, cx);
        });
        assert_eq!(depth(cx), start + 1);
        // The Stereo Out's fader, then a reset by double click (press and release at unity).
        cx.update(|cx| {
            set_volume(&daw, MASTER, 0.6, Phase::Start, cx);
            set_volume(&daw, MASTER, 0.6, Phase::End, cx);
        });
        assert_eq!(
            daw.read_with(cx, |d, _| d.app.store.session().master_volume),
            0.6
        );
        assert_eq!(depth(cx), start + 2);
        assert!(!daw.read_with(cx, |d, _| d.app.interacting));
    }

    #[test]
    fn meters_share_the_fader_scale() {
        assert_eq!(meter_level(0.0), 0.0);
        assert!((meter_level(1.0) - format::FADER_UNITY).abs() < 1e-4);
        assert!(meter_level(2.0) > format::FADER_UNITY);
        assert_eq!(kind_caps("audio"), "AUD");
        assert_eq!(kind_caps("bus"), "BUS");
    }
}
