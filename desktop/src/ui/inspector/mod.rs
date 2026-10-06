//! The inspector: the selected channel at the right of the arrangement. For a track: its
//! instrument, input and output (with the routing menus), the Channel EQ curve, the eight
//! insert slots, the sends, pan and the fader with its meter and scale, the channel keys and
//! the selected region. For the Stereo Out and the A and B returns (`ui.showPanel
//! panel=master|bus-a|bus-b`): their inserts, and the Stereo Out's fader. Glass tier 1.
//!
//! Every change is a registry command (`track.*`, `strip.*`, `master.setVolume`, `clip.*`,
//! `ui.openPluginWindow`), so the window does what a script or the agent can do.

pub mod eq;
pub mod region;

use super::{
    daw::Daw,
    strip::{self, SendRow},
    theme::{radius, size, Theme, FONT_MONO},
    widgets::{caps, tip, Button, Knob, MenuHost, MenuItem},
};
use eq::EqState;
use gpui::{
    div, prelude::*, px, AnyElement, App, ClickEvent, Context, Entity, Hsla, MouseButton,
    MouseDownEvent, Pixels, Point, SharedString, Window,
};
use region::Region;
use ryolune_engine::{
    device::Monitoring,
    dsp::{EFFECTS, INSTRUMENTS},
    model::{self, Insert, Track, MASTER, MAX_SENDS},
};
use serde_json::json;

/// Height of the inspector's fader box, as the design frame draws it.
const FADER_H: f32 = 150.0;

pub struct Inspector {
    daw: Entity<Daw>,
    menu: MenuHost,
    region: Entity<Region>,
}

/// What a clickable row does when pressed: open its menu where the pointer is.
type Press = Box<dyn Fn(&mut Inspector, Point<Pixels>, &mut Window, &mut Context<Inspector>)>;

/// The channel the inspector shows.
enum Shown {
    Nothing,
    /// A track (audio, MIDI or bus) and its place in the arrangement.
    Track(Track, usize),
    /// The Stereo Out or an aux return.
    Fixed(&'static str),
}

/// The header's caps label for a track.
pub fn track_meta(track: &Track, index: usize, inputs: usize) -> String {
    match track.kind.as_str() {
        "bus" => format!("BUS · {inputs} in"),
        "midi" => format!("MIDI · Ch {}", index + 1),
        _ => "AUDIO · In 1".into(),
    }
}

impl Inspector {
    pub fn new(daw: Entity<Daw>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let region = cx.new(|cx| Region::new(daw.clone(), window, cx));
        Self {
            daw,
            menu: MenuHost::default(),
            region,
        }
    }

    fn shown(&self, cx: &App) -> Shown {
        let s = self.daw.read(cx).app.store.session();
        let Some(id) = s.view.selected_track_id.as_deref() else {
            return Shown::Nothing;
        };
        if let Some(index) = s.tracks.iter().position(|t| t.id == id) {
            return Shown::Track(s.tracks[index].clone(), index);
        }
        match id {
            model::MASTER => Shown::Fixed(model::MASTER),
            model::BUS_A => Shown::Fixed(model::BUS_A),
            model::BUS_B => Shown::Fixed(model::BUS_B),
            _ => Shown::Nothing,
        }
    }

    fn open(
        &mut self,
        items: Vec<MenuItem>,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !items.is_empty() {
            self.menu.open(items, at, window, cx);
        }
    }

    /// A menu row that runs one registry command.
    fn item(
        &self,
        label: impl Into<SharedString>,
        method: &'static str,
        params: serde_json::Value,
    ) -> MenuItem {
        let daw = self.daw.clone();
        MenuItem::new(label, move |_, cx| {
            strip::fire(&daw, method, params.clone(), cx)
        })
    }

    /// The stock instruments, then any installed instrument plugin.
    fn instrument_menu(
        &mut self,
        track_id: String,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = &self.daw.read(cx).app;
        let current = app.store.session().strips.get(&track_id).map_or_else(
            || model::Strip::default().instrument_name(),
            |s| s.instrument_name(),
        );
        let mut items: Vec<MenuItem> = INSTRUMENTS
            .iter()
            .map(|name| {
                self.item(
                    *name,
                    "strip.setInstrument",
                    json!({ "trackId": track_id, "instrument": name }),
                )
                .checked(current == *name)
            })
            .collect();
        let installed: Vec<_> = app
            .catalog
            .iter()
            .filter(|d| d.instrument && !d.id.starts_with("stock:"))
            .map(|d| (d.id.clone(), d.name.clone(), d.format.label()))
            .collect();
        if !installed.is_empty() {
            items.push(MenuItem::Separator);
            items.push(MenuItem::Header("Installed".into()));
            for (id, name, format) in installed {
                let checked = current == name;
                items.push(
                    self.item(
                        name,
                        "strip.setPlugin",
                        json!({ "trackId": track_id, "pluginId": id }),
                    )
                    .detail(format)
                    .checked(checked),
                );
            }
        }
        self.open(items, at, window, cx);
    }

    /// Where the fader goes: the Stereo Out, a bus track, or a new bus.
    fn output_menu(
        &mut self,
        track: Track,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let s = self.daw.read(cx).app.store.session();
        let route = |label: String, output: &str, checked: bool| {
            self.item(
                label,
                "track.setOutput",
                json!({ "trackId": track.id, "output": output }),
            )
            .checked(checked)
        };
        let mut items = vec![route(
            "Stereo Out".into(),
            "Stereo Out",
            track.output.is_none(),
        )];
        for bus in strip::bus_tracks(s) {
            items.push(route(
                bus.name.clone(),
                &bus.id,
                track.output.as_deref() == Some(bus.id.as_str()),
            ));
        }
        items.push(MenuItem::Separator);
        items.push(self.item("New Bus", "track.group", json!({ "trackIds": [track.id] })));
        self.open(items, at, window, cx);
    }

    /// Load an effect into a slot, bypass, open, move or clear it.
    fn insert_menu(
        &mut self,
        strip_id: String,
        slot: usize,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = &self.daw.read(cx).app;
        let slots = strip::inserts(app.store.session(), &strip_id);
        let insert = slots[slot].clone();
        let empty = insert.is_empty();
        let last = strip::last_used(&slots).unwrap_or(0);
        let mut items: Vec<MenuItem> = EFFECTS
            .iter()
            .map(|name| {
                self.item(
                    *name,
                    "strip.setInsert",
                    json!({ "trackId": strip_id, "slot": slot, "effect": name }),
                )
                .checked(!empty && insert.plugin_id() == format!("stock:{name}"))
            })
            .collect();
        let installed: Vec<_> = app
            .catalog
            .iter()
            .filter(|d| d.effect && !d.instrument && !d.id.starts_with("stock:"))
            .map(|d| (d.id.clone(), d.name.clone(), d.format.label()))
            .collect();
        if !installed.is_empty() {
            items.push(MenuItem::Separator);
            items.push(MenuItem::Header("Installed".into()));
            for (id, name, format) in installed {
                let checked = !empty && insert.plugin == id;
                items.push(
                    self.item(
                        name,
                        "strip.setPlugin",
                        json!({ "trackId": strip_id, "slot": slot, "pluginId": id }),
                    )
                    .detail(format)
                    .checked(checked),
                );
            }
        }
        let bypassed = insert.state == "bypassed";
        items.extend([
            MenuItem::Separator,
            self.item(
                if bypassed { "Enable" } else { "Bypass" },
                "strip.setBypass",
                json!({ "trackId": strip_id, "slot": slot, "bypassed": !bypassed }),
            )
            .disabled(empty),
            self.item(
                "Parameters",
                "ui.openPluginWindow",
                json!({ "trackId": strip_id, "slot": slot }),
            )
            .disabled(empty),
            self.item(
                "Open plugin window",
                "ui.openPluginWindow",
                json!({ "trackId": strip_id, "slot": slot, "native": true }),
            )
            .disabled(empty),
            self.item(
                "Move up",
                "strip.moveInsert",
                json!({ "trackId": strip_id, "from": slot, "to": slot.saturating_sub(1) }),
            )
            .disabled(empty || slot == 0),
            self.item(
                "Move down",
                "strip.moveInsert",
                json!({ "trackId": strip_id, "from": slot, "to": slot + 1 }),
            )
            .disabled(empty || slot >= last),
            self.item(
                "Remove",
                "strip.setInsert",
                json!({ "trackId": strip_id, "slot": slot }),
            )
            .disabled(empty),
        ]);
        self.open(items, at, window, cx);
    }

    /// What a send feeds: A, B or (from a track) a bus track; sends 2 and 3 can be removed.
    fn send_menu(
        &mut self,
        track: Track,
        send: SendRow,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let s = self.daw.read(cx).app.store.session();
        let current = send.bus.as_deref();
        let point = |label: String, bus: &str, checked: bool| {
            self.item(
                label,
                "strip.setSend",
                json!({ "trackId": track.id, "send": send.index, "bus": bus }),
            )
            .checked(checked)
        };
        let mut items = vec![
            point("A · Reverb".into(), "A", current == Some(model::BUS_A)),
            point("B · Delay".into(), "B", current == Some(model::BUS_B)),
        ];
        if !track.is_bus() {
            for bus in strip::bus_tracks(s) {
                items.push(point(
                    bus.name.clone(),
                    &bus.id,
                    current == Some(bus.id.as_str()),
                ));
            }
        }
        items.push(MenuItem::Separator);
        items.push(point(
            if send.index >= 2 {
                "Remove Send".into()
            } else {
                "Reset to Default, Off".into()
            },
            "none",
            false,
        ));
        self.open(items, at, window, cx);
    }

    /// A further send, to a bus track, at −6 dB.
    fn add_send_menu(
        &mut self,
        track_id: String,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let s = self.daw.read(cx).app.store.session();
        let next = strip::sends(s, &track_id).len();
        let items = strip::bus_tracks(s)
            .into_iter()
            .map(|bus| {
                self.item(
                    bus.name.clone(),
                    "strip.setSend",
                    json!({ "trackId": track_id, "send": next, "bus": bus.id, "levelDb": -6.0 }),
                )
            })
            .collect();
        self.open(items, at, window, cx);
    }

    fn header(&self, color: Hsla, name: String, meta: String, cx: &App) -> gpui::Div {
        let theme = Theme::get(cx);
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(9.0))
            .px(px(14.0))
            .pt(px(12.0))
            .pb(px(10.0))
            .border_b_1()
            .border_color(theme.line)
            .child(
                div()
                    .flex_none()
                    .size(px(12.0))
                    .rounded(px(radius::XS))
                    .bg(color),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(size::LG))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.text)
                    .child(name),
            )
            .child(
                div()
                    .flex_none()
                    .font_family(FONT_MONO)
                    .text_size(px(10.5))
                    .text_color(theme.text_3)
                    .child(meta),
            )
    }

    /// A label and its value; clickable rows open a menu and light their value on hover.
    fn info_row(
        &self,
        id: &'static str,
        label: &'static str,
        value: String,
        on_press: Option<Press>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let clickable = on_press.is_some();
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .h(px(22.0))
            .text_size(px(size::BASE))
            .child(div().flex_none().text_color(theme.text_3).child(label))
            .child(
                div()
                    .id(SharedString::from(format!("{id}-value")))
                    .min_w_0()
                    .truncate()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme.text)
                    .when(clickable, |d| d.hover(|s| s.text_color(theme.accent_text)))
                    .child(value),
            )
            .when_some(on_press, |d, f| {
                d.cursor_pointer()
                    .tooltip(|_, cx| tip("Click to change".into(), cx))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                            f(this, e.position, window, cx)
                        }),
                    )
            })
            .into_any_element()
    }

    fn insert_row(
        &self,
        strip_id: &str,
        slot: usize,
        insert: &Insert,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let empty = insert.is_empty();
        let bypassed = insert.state == "bypassed";
        let (bg, edge, fg) = if empty {
            (theme.well.opacity(0.6), theme.hairline, theme.text_3)
        } else if bypassed {
            (theme.control, theme.control_edge, theme.text_3)
        } else {
            (theme.control, theme.control_edge, theme.text)
        };
        let led = div()
            .id(SharedString::from(format!("insert-led-{slot}")))
            .flex_none()
            .size(px(7.0))
            .map(|d| {
                if empty {
                    d.border_1().border_color(theme.line_strong)
                } else if bypassed {
                    d.bg(theme.meter_off)
                        .border_1()
                        .border_color(theme.control_edge)
                } else {
                    d.bg(theme.accent).shadow(vec![gpui::BoxShadow {
                        color: theme.accent_glow,
                        offset: gpui::point(px(0.0), px(0.0)),
                        blur_radius: px(5.0),
                        spread_radius: px(0.0),
                    }])
                }
            })
            .when(!empty, |d| {
                let daw = self.daw.clone();
                let strip_id = strip_id.to_string();
                d.cursor_pointer()
                    .tooltip(move |_, cx| {
                        tip(if bypassed { "Enable" } else { "Bypass" }.into(), cx)
                    })
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.stop_propagation();
                        strip::fire(
                            &daw,
                            "strip.setBypass",
                            json!({ "trackId": strip_id, "slot": slot, "bypassed": !bypassed }),
                            cx,
                        );
                    })
            });
        let meta = if bypassed {
            "bypassed".to_string()
        } else {
            insert.meta.clone()
        };
        let open_id = strip_id.to_string();
        let menu_id = strip_id.to_string();
        div()
            .id(SharedString::from(format!("insert-{slot}")))
            .flex()
            .items_center()
            .gap(px(8.0))
            .h(px(26.0))
            .px(px(8.0))
            .rounded(px(radius::SM))
            .bg(bg)
            .border_1()
            .border_color(edge)
            .text_size(px(size::BASE))
            .font_weight(if empty {
                gpui::FontWeight::NORMAL
            } else {
                gpui::FontWeight::SEMIBOLD
            })
            .text_color(fg)
            .cursor_pointer()
            .hover(|s| s.bg(theme.control_hover))
            .tooltip(move |_, cx| {
                tip(
                    if empty {
                        "Click to add an effect".into()
                    } else {
                        "Click to open · LED toggles bypass · right-click for more".into()
                    },
                    cx,
                )
            })
            .child(led)
            .child(div().flex_1().min_w_0().truncate().child(if empty {
                "Add effect…".to_string()
            } else {
                insert.name.clone()
            }))
            .when(!meta.is_empty(), |d| {
                d.child(
                    div()
                        .flex_none()
                        .font_family(FONT_MONO)
                        .text_size(px(10.5))
                        .font_weight(gpui::FontWeight::NORMAL)
                        .text_color(theme.text_3)
                        .child(meta),
                )
            })
            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                if empty {
                    this.insert_menu(open_id.clone(), slot, e.position(), window, cx);
                } else {
                    strip::fire(
                        &this.daw,
                        "ui.openPluginWindow",
                        json!({ "trackId": open_id, "slot": slot }),
                        cx,
                    );
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.insert_menu(menu_id.clone(), slot, e.position, window, cx);
                }),
            )
            .into_any_element()
    }

    fn inserts_section(&self, strip_id: &str, cx: &Context<Self>) -> AnyElement {
        let s = self.daw.read(cx).app.store.session();
        let slots = strip::inserts(s, strip_id);
        let used = slots.iter().filter(|i| !i.is_empty()).count();
        section(
            "Inserts",
            meta_text(format!("{used} / {}", model::MAX_INSERTS), cx).into_any_element(),
            cx,
        )
        .gap(px(4.0))
        .children(
            slots
                .iter()
                .take(strip::visible_slots(&slots))
                .enumerate()
                .map(|(slot, insert)| self.insert_row(strip_id, slot, insert, cx)),
        )
        .into_any_element()
    }

    fn eq_section(&self, track_id: &str, cx: &Context<Self>) -> AnyElement {
        let s = self.daw.read(cx).app.store.session();
        let (state, slot) = EqState::of(&strip::inserts(s, track_id));
        let track_id = track_id.to_string();
        section(
            "Channel EQ",
            meta_text(state.label().into(), cx).into_any_element(),
            cx,
        )
        .child(
            div()
                .id("channel-eq")
                .cursor_pointer()
                .tooltip(move |_, cx| {
                    tip(
                        if slot.is_some() {
                            "Open the Channel EQ".into()
                        } else {
                            "Click to insert a Channel EQ".into()
                        },
                        cx,
                    )
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| match slot {
                    Some(slot) => strip::fire(
                        &this.daw,
                        "ui.openPluginWindow",
                        json!({ "trackId": track_id, "slot": slot }),
                        cx,
                    ),
                    None => strip::fire(
                        &this.daw,
                        "strip.setPlugin",
                        json!({
                            "trackId": track_id,
                            "pluginId": format!("stock:{}", eq::EQ_NAME),
                            "firstFreeSlot": true,
                        }),
                        cx,
                    ),
                }))
                .child(eq::display(state, cx)),
        )
        .into_any_element()
    }

    fn send_card(&self, track: &Track, send: SendRow, cx: &Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let index = send.index;
        let daw = self.daw.clone();
        let track_id = track.id.clone();
        let knob = Knob::new(
            SharedString::from(format!("send-{index}")),
            strip::send_to_knob(send.level_db),
        )
        .size(30.0)
        .default_value(0.0)
        .tooltip("Drag to set the send level")
        .on_change(move |value, phase, _, cx| {
            let level = strip::knob_to_send(value);
            let track_id = track_id.clone();
            strip::continuous(&daw, phase, cx, move |daw| {
                let s = daw.app.store.session();
                let current = strip::sends(s, &track_id).get(index)?.level_db;
                (current != level).then(|| {
                    let mut params = json!({ "trackId": track_id, "send": index });
                    if let Some(db) = level {
                        params["levelDb"] = json!(db);
                    }
                    ("strip.setSendLevel", params)
                })
            });
        });
        let pick_track = track.clone();
        let pick_send = send.clone();
        div()
            .flex()
            .flex_1()
            .min_w(px(116.0))
            .items_center()
            .gap(px(9.0))
            .px(px(8.0))
            .py(px(6.0))
            .rounded(px(radius::SM))
            .bg(theme.well)
            .border_1()
            .border_color(theme.hairline)
            .child(knob)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .gap(px(1.0))
                    .child(
                        div()
                            .id(SharedString::from(format!("send-name-{index}")))
                            .truncate()
                            .text_size(px(size::BASE))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .cursor_pointer()
                            .hover(|s| s.text_color(theme.accent_text))
                            .tooltip(|_, cx| tip("Choose where this send goes".into(), cx))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                                    this.send_menu(
                                        pick_track.clone(),
                                        pick_send.clone(),
                                        e.position,
                                        window,
                                        cx,
                                    )
                                }),
                            )
                            .child(send.name.clone()),
                    )
                    .child(meta_text(strip::send_label(send.level_db), cx)),
            )
            .into_any_element()
    }

    fn sends_section(&self, track: &Track, cx: &Context<Self>) -> AnyElement {
        let s = self.daw.read(cx).app.store.session();
        let rows = strip::sends(s, &track.id);
        let can_add = !track.is_bus() && !strip::bus_tracks(s).is_empty() && rows.len() < MAX_SENDS;
        let track_id = track.id.clone();
        let add = if can_add {
            Button::new("add-send", "+ Send")
                .compact()
                .ghost()
                .tooltip("Send to a bus")
                .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                    this.add_send_menu(track_id.clone(), e.position(), window, cx)
                }))
                .into_any_element()
        } else {
            div().into_any_element()
        };
        section("Sends", add, cx)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.0))
                    .children(rows.into_iter().map(|send| self.send_card(track, send, cx))),
            )
            .into_any_element()
    }

    /// Pan and volume at the left, the fader, meter and scale at the right. The Stereo Out
    /// has no pan.
    fn fader_section(
        &self,
        id: &str,
        track: Option<&Track>,
        volume: f32,
        peaks: [f32; 2],
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let db = super::format::fader_to_db(volume);
        let mut left = div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.0))
            .w(px(58.0))
            .h(px(FADER_H));
        if let Some(track) = track {
            let daw = self.daw.clone();
            let pan_id = track.id.clone();
            left = left
                .child(caps("Pan", cx))
                .child(
                    Knob::new("inspector-pan", strip::pan_to_knob(track.pan))
                        .size(40.0)
                        .bipolar()
                        .tooltip("Pan · double-click to centre")
                        .on_change(move |v, phase, _, cx| {
                            strip::set_pan(&daw, &pan_id, v, phase, cx)
                        }),
                )
                .child(meta_text(strip::pan_label(track.pan), cx));
        }
        let reset_daw = self.daw.clone();
        let reset_id = id.to_string();
        left = left.child(div().flex_1()).child(caps("Vol", cx)).child(
            div()
                .id("inspector-volume")
                .px(px(6.0))
                .py(px(2.0))
                .rounded(px(radius::XS))
                .bg(theme.display)
                .border_1()
                .border_color(theme.hairline)
                .font_family(FONT_MONO)
                .text_size(px(size::SM))
                .text_color(theme.text_display)
                .tooltip(|_, cx| tip("Double-click for 0 dB".into(), cx))
                .on_click(move |e, _, cx| {
                    if e.click_count() == 2 {
                        let unity = super::format::FADER_UNITY;
                        strip::set_volume(
                            &reset_daw,
                            &reset_id,
                            unity,
                            super::widgets::Phase::Start,
                            cx,
                        );
                        strip::set_volume(
                            &reset_daw,
                            &reset_id,
                            unity,
                            super::widgets::Phase::End,
                            cx,
                        );
                    }
                })
                .child(super::format::db(db, 1)),
        );
        let daw = self.daw.clone();
        let fader_id = id.to_string();
        div()
            .flex()
            .flex_none()
            .gap(px(14.0))
            .px(px(14.0))
            .pt(px(12.0))
            .pb(px(12.0))
            .child(left)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .justify_center()
                    .child(strip::fader_block(
                        "inspector-fader",
                        volume,
                        &peaks,
                        FADER_H,
                        true,
                        move |v, phase, _, cx| strip::set_volume(&daw, &fader_id, v, phase, cx),
                        cx,
                    )),
            )
            .into_any_element()
    }

    fn track_view(
        &self,
        track: &Track,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = Theme::get(cx).clone();
        let app = &self.daw.read(cx).app;
        let s = app.store.session();
        let inputs: Vec<String> = s
            .tracks
            .iter()
            .filter(|t| t.output.as_deref() == Some(track.id.as_str()))
            .map(|t| t.name.clone())
            .collect();
        let instrument = s.strips.get(&track.id).map_or_else(
            || model::Strip::default().instrument_name(),
            |st| st.instrument_name(),
        );
        let output = strip::output_name(s, track);
        let peaks = app
            .device
            .as_ref()
            .map_or([0.0; 4], |d| d.telemetry.peaks());
        let blocked = matches!(app.monitoring, Monitoring::FeedbackRisk { .. });
        let has_region = s
            .view
            .selected_clip_id
            .as_ref()
            .is_some_and(|id| s.clips.iter().any(|c| &c.id == id));
        let _ = window;

        let mut out = vec![self
            .header(
                theme.track(&track.color, index),
                track.name.clone(),
                track_meta(track, index, inputs.len()),
                cx,
            )
            .into_any_element()];

        let mut rows = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .px(px(14.0))
            .py(px(10.0))
            .border_b_1()
            .border_color(theme.line);
        if !track.is_bus() {
            let midi = track.kind == "midi";
            let id = track.id.clone();
            rows = rows.child(self.info_row(
                "instrument",
                "Instrument",
                if midi { instrument } else { "—".into() },
                midi.then(|| -> Press {
                    Box::new(move |this, at, window, cx| {
                        this.instrument_menu(id.clone(), at, window, cx)
                    })
                }),
                cx,
            ));
            if midi {
                let daw = self.daw.clone();
                let id = track.id.clone();
                rows = rows.child(
                    div().py(px(4.0)).child(
                        Button::new("instrument-parameters", "Instrument parameters")
                            .full_width()
                            .on_click(move |_, _, cx| {
                                strip::fire(
                                    &daw,
                                    "ui.openPluginWindow",
                                    json!({ "trackId": id }),
                                    cx,
                                )
                            }),
                    ),
                );
            }
            rows = rows.child(self.info_row(
                "input",
                "Input",
                if midi {
                    "Musical typing".into()
                } else {
                    "Input".into()
                },
                None,
                cx,
            ));
            let routed = track.clone();
            rows = rows.child(self.info_row(
                "output",
                "Output",
                output.clone(),
                Some(Box::new(move |this, at, window, cx| {
                    this.output_menu(routed.clone(), at, window, cx)
                })),
                cx,
            ));
        } else {
            rows = rows
                .child(self.info_row(
                    "inputs",
                    "Inputs",
                    if inputs.is_empty() {
                        "—".into()
                    } else {
                        inputs.join(", ")
                    },
                    None,
                    cx,
                ))
                .child(self.info_row("output", "Output", output.clone(), None, cx));
        }
        out.push(rows.into_any_element());
        if !track.is_bus() {
            out.push(self.eq_section(&track.id, cx));
        }
        out.push(self.inserts_section(&track.id, cx));
        out.push(self.sends_section(track, cx));
        out.push(self.fader_section(
            &track.id,
            Some(track),
            track.volume,
            [peaks[2], peaks[3]],
            cx,
        ));
        out.push(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(5.0))
                .px(px(14.0))
                .pb(px(14.0))
                .children(strip::track_keys(&self.daw, track, blocked, 22.0, cx))
                .child(
                    div()
                        .ml(px(6.0))
                        .min_w_0()
                        .truncate()
                        .text_size(px(size::SM))
                        .text_color(theme.text_3)
                        .child(output),
                )
                .into_any_element(),
        );
        if has_region {
            out.push(
                div()
                    .border_t_1()
                    .border_color(theme.line)
                    .child(self.region.clone())
                    .into_any_element(),
            );
        }
        out
    }

    fn fixed_view(&self, id: &'static str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = Theme::get(cx).clone();
        let app = &self.daw.read(cx).app;
        let volume = app.store.session().master_volume;
        let peaks = app
            .device
            .as_ref()
            .map_or([0.0; 4], |d| d.telemetry.peaks());
        let mut out = vec![
            self.header(
                theme.accent,
                model::bus_name(id).into(),
                if id == MASTER { "MASTER" } else { "AUX BUS" }.into(),
                cx,
            )
            .into_any_element(),
            self.inserts_section(id, cx),
        ];
        if id == MASTER {
            out.push(self.fader_section(id, None, volume, [peaks[0], peaks[1]], cx));
        }
        out
    }
}

/// A panel section: a caps title running into a hairline, a detail at the right, then its
/// body.
fn section(title: &'static str, detail: AnyElement, cx: &App) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(6.0))
        .px(px(14.0))
        .pt(px(10.0))
        .pb(px(10.0))
        .border_b_1()
        .border_color(theme.line)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .h(px(20.0))
                .child(caps(title, cx).text_color(theme.text_2).flex_none())
                .child(div().flex_1().h(px(1.0)).bg(theme.line))
                .child(detail),
        )
}

fn meta_text(text: String, cx: &App) -> gpui::Div {
    let theme = Theme::get(cx);
    div()
        .font_family(FONT_MONO)
        .text_size(px(10.5))
        .text_color(theme.text_3)
        .whitespace_nowrap()
        .child(text)
}

impl Render for Inspector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let menu = self.menu.render(window, cx);
        let shown = self.shown(cx);
        let what = match &shown {
            Shown::Nothing => "nothing selected".to_string(),
            Shown::Track(track, index) => format!(
                "track {} · {}",
                index + 1,
                super::strip::kind_caps(&track.kind).to_lowercase()
            ),
            Shown::Fixed(_) => "output".to_string(),
        };
        let body = match shown {
            Shown::Nothing => vec![self
                .header(theme.text_3, "Select a track".into(), String::new(), cx)
                .into_any_element()],
            Shown::Track(track, index) => self.track_view(&track, index, window, cx),
            Shown::Fixed(id) => self.fixed_view(id, cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.glass(1))
            .border_l_1()
            .border_color(theme.line)
            // Titled like every area: its name, what it shows in mono.
            .child(
                div()
                    .h(px(super::theme::layout::TOOLBAR))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(14.0))
                    .border_b_1()
                    .border_color(theme.line)
                    .child(super::widgets::panel_title("Inspector", cx))
                    .child(super::widgets::panel_info(what, cx)),
            )
            .child(
                div()
                    .id("inspector")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .children(body),
            )
            .children(menu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ui::{
        mixer::Mixer,
        theme::{Mode, Theme},
    };

    /// The mixer and the inspector side by side, as the workspace lays them out.
    struct Both {
        inspector: Entity<Inspector>,
        mixer: Entity<Mixer>,
    }
    impl Render for Both {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .child(div().flex_1().child(self.mixer.clone()))
                .child(div().w(px(300.0)).child(self.inspector.clone()))
        }
    }

    /// Every channel the inspector can show renders, beside the mixer, in both modes: each
    /// track (MIDI, audio, a bus), with a MIDI and an audio region, the Stereo Out and both
    /// returns.
    #[gpui::test]
    fn every_channel_renders(cx: &mut gpui::TestAppContext) {
        let daw = cx.new(|_| {
            Daw::new(crate::app::Ryolune::from_session(
                ryolune_engine::store::demo(),
                None,
            ))
        });
        for mode in [Mode::Dark, Mode::Light] {
            cx.update(|cx| cx.set_global(Theme::new(mode, false)));
            let (_, vcx) = cx.add_window_view(|window, cx| Both {
                inspector: cx.new(|cx| Inspector::new(daw.clone(), window, cx)),
                mixer: cx.new(|cx| Mixer::new(daw.clone(), window, cx)),
            });
            let (tracks, clips) = daw.read_with(vcx, |d, _| {
                let s = d.app.store.session();
                (
                    s.tracks.iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
                    s.clips.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
                )
            });
            let first = tracks[0].clone();
            let mut steps: Vec<(&str, serde_json::Value)> = vec![
                ("track.group", json!({ "trackIds": [first] })),
                (
                    "strip.setInsert",
                    json!({ "trackId": first, "slot": 0, "effect": "Channel EQ" }),
                ),
            ];
            steps.extend(
                tracks
                    .iter()
                    .map(|t| ("track.select", json!({ "trackId": t }))),
            );
            steps.extend(
                clips
                    .iter()
                    .map(|c| ("clip.select", json!({ "clipId": c }))),
            );
            for panel in [model::MASTER, model::BUS_A, model::BUS_B] {
                steps.push(("ui.showPanel", json!({ "panel": panel, "visible": true })));
            }
            for (method, params) in steps {
                daw.update(vcx, |daw, cx| {
                    daw.request(method, params, cx).expect(method);
                });
                vcx.run_until_parked();
            }
            daw.update(vcx, |daw, cx| {
                daw.run("history.undo", json!({ "steps": 2 }), cx);
            });
        }
    }

    #[test]
    fn the_header_names_the_kind_and_channel() {
        let session = ryolune_engine::store::demo();
        let mut track = session.tracks[0].clone();
        track.kind = "midi".into();
        assert_eq!(track_meta(&track, 2, 0), "MIDI · Ch 3");
        track.kind = "audio".into();
        assert_eq!(track_meta(&track, 0, 0), "AUDIO · In 1");
        track.kind = "bus".into();
        assert_eq!(track_meta(&track, 0, 3), "BUS · 3 in");
    }
}
