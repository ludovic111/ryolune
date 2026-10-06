//! The Generate tab: describe a sound and the service chosen in Settings > Generation makes
//! it (`generate.audio`): a loop on the song's bars, tempo and key, a song, a one-shot, or one
//! note played across the keyboard by Sample Keys. Results are kept (`generate.list`) and can
//! be heard, placed as a clip or an instrument (`generate.place`) or deleted. Everything here
//! is `generate.*`, so the agent, the CLI and MCP clients do the same.

use super::{jobs, AgentPanel};
use crate::ui::{
    daw::Daw,
    theme::{radius, size, Theme},
    widgets::{field, select_button, Button, InputEvent, MenuItem, Switch, TextInput},
};
use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, FontWeight, SharedString, Subscription, Task,
    Window,
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    process::Child,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Loop,
    Song,
    Sound,
    Instrument,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Loop, Kind::Song, Kind::Sound, Kind::Instrument];
    pub fn key(self) -> &'static str {
        match self {
            Kind::Loop => "loop",
            Kind::Song => "song",
            Kind::Sound => "sound",
            Kind::Instrument => "instrument",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Kind::Loop => "Loop",
            Kind::Song => "Song",
            Kind::Sound => "Sound",
            Kind::Instrument => "Instrument",
        }
    }
    fn hint(self) -> &'static str {
        match self {
            Kind::Loop => "Loops on your bars, at the song's tempo and key.",
            Kind::Song => "A full idea to build on, placed at the playhead.",
            Kind::Sound => "A one-shot or effect: a riser, an impact, a foley hit.",
            Kind::Instrument => "One note, played across your keyboard by Sample Keys.",
        }
    }
    fn placeholder(self) -> &'static str {
        match self {
            Kind::Loop => "Dusty boom-bap drums with a lazy swing…",
            Kind::Song => "Dreamy synthwave with a driving bass and warm pads…",
            Kind::Sound => "Deep cinematic impact with a long metallic tail…",
            Kind::Instrument => "Felt piano, soft and intimate, close-miked…",
        }
    }
    /// The lengths offered, in seconds (bars for a loop).
    fn lengths(self) -> &'static [u32] {
        match self {
            Kind::Loop => &[1, 2, 4, 8, 16],
            Kind::Song => &[30, 60, 120, 180],
            Kind::Sound => &[1, 3, 6, 12],
            Kind::Instrument => &[2, 3, 5],
        }
    }
    fn parse(key: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.key() == key)
    }
}

/// `1:05` from a minute up, `3 s` or `2.5 s` below.
pub fn duration(seconds: f64) -> String {
    if seconds >= 60.0 {
        format!(
            "{}:{:02}",
            (seconds / 60.0).floor() as u64,
            (seconds % 60.0).round() as u64
        )
    } else {
        let tenths = (seconds * 10.0).round() / 10.0;
        if tenths.fract() == 0.0 {
            format!("{} s", tenths as u64)
        } else {
            format!("{tenths} s")
        }
    }
}

/// How long ago a Unix time was, briefly.
pub fn ago(created: u64, now: u64) -> String {
    let minutes = ((now.saturating_sub(created)) as f64 / 60.0).round() as u64;
    if minutes < 1 {
        "just now".into()
    } else if minutes < 60 {
        format!("{minutes} min ago")
    } else {
        let hours = (minutes as f64 / 60.0).round() as u64;
        if hours < 24 {
            format!("{hours} h ago")
        } else {
            format!("{} d ago", (hours as f64 / 24.0).round() as u64)
        }
    }
}

/// What `generate.audio` is asked for.
pub fn request_params(
    prompt: &str,
    kind: Kind,
    service: &str,
    length: u32,
    instrumental: bool,
    follow_song: bool,
) -> Value {
    let mut params = json!({
        "prompt": prompt.trim().chars().take(2000).collect::<String>(),
        "kind": kind.key(),
        "service": service,
    });
    if kind == Kind::Loop {
        params["bars"] = json!(length);
    } else {
        params["seconds"] = json!(length);
    }
    if kind == Kind::Song {
        params["instrumental"] = json!(instrumental);
    }
    if matches!(kind, Kind::Loop | Kind::Song) {
        params["followSong"] = json!(follow_song);
    }
    params
}

#[derive(Clone, Debug)]
struct Service {
    id: String,
    label: String,
    ready: bool,
}

#[derive(Clone, Debug)]
struct Sound {
    id: String,
    name: String,
    description: String,
    kind: Option<Kind>,
    seconds: f64,
    created: u64,
    path: PathBuf,
}

pub struct Generate {
    prompt: Entity<TextInput>,
    kind: Kind,
    /// The chosen length per kind.
    lengths: [u32; 4],
    instrumental: bool,
    follow_song: bool,
    services: Option<Vec<Service>>,
    chosen: String,
    sounds: Vec<Sound>,
    started: Option<Instant>,
    error: String,
    /// The sound being heard, and the player.
    playing: Option<(String, Child)>,
    /// The generation in flight, and the clock its button shows.
    _generation: Option<Task<()>>,
    _tick: Option<Task<()>>,
    /// A place or delete waiting for its answer.
    _action: Option<Task<()>>,
    /// Watches the preview player end.
    _player: Option<Task<()>>,
    _prompt: Subscription,
}

impl Drop for Generate {
    fn drop(&mut self) {
        if let Some((_, child)) = &mut self.playing {
            let _ = child.kill();
        }
    }
}

impl Generate {
    pub fn new(window: &mut Window, cx: &mut Context<AgentPanel>) -> Self {
        let prompt = cx.new(|cx| {
            TextInput::new(cx)
                .multiline()
                .placeholder(Kind::Loop.placeholder())
        });
        let subscription = cx.subscribe_in(&prompt, window, |this, _, event, _, cx| match event {
            InputEvent::Submit => this.generate(cx),
            _ => cx.notify(),
        });
        Self {
            prompt,
            kind: Kind::Loop,
            lengths: [4, 60, 3, 3],
            instrumental: true,
            follow_song: true,
            services: None,
            chosen: String::new(),
            sounds: vec![],
            started: None,
            error: String::new(),
            playing: None,
            _generation: None,
            _tick: None,
            _action: None,
            _player: None,
            _prompt: subscription,
        }
    }

    fn length(&self) -> u32 {
        self.lengths[self.kind as usize]
    }

    fn ready(&self) -> Vec<Service> {
        self.services
            .iter()
            .flatten()
            .filter(|s| s.ready)
            .cloned()
            .collect()
    }

    fn service(&self) -> Option<Service> {
        let ready = self.ready();
        ready
            .iter()
            .find(|s| s.id == self.chosen)
            .or(ready.first())
            .cloned()
    }

    /// Read the services and the kept sounds again.
    pub fn refresh(&mut self, daw: &Entity<Daw>, cx: &mut Context<AgentPanel>) {
        let (services, list) = daw.update(cx, |daw, cx| {
            (
                daw.request("generate.services", json!({}), cx),
                daw.request("generate.list", json!({"limit": 30}), cx),
            )
        });
        match services {
            Ok(value) => {
                self.chosen = value["chosen"].as_str().unwrap_or("").to_string();
                self.services = Some(
                    value["services"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|s| Service {
                            id: s["id"].as_str().unwrap_or("").to_string(),
                            label: s["label"].as_str().unwrap_or("").to_string(),
                            ready: s["ready"].as_bool().unwrap_or(false),
                        })
                        .collect(),
                );
            }
            Err(error) => self.error = error,
        }
        let list = list.unwrap_or(Value::Null);
        let folder = PathBuf::from(list["folder"].as_str().unwrap_or(""));
        self.sounds = list["sounds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                Some(Sound {
                    id: s["id"].as_str()?.to_string(),
                    name: s["name"].as_str().unwrap_or("").to_string(),
                    description: s["description"].as_str().unwrap_or("").to_string(),
                    kind: s["kind"].as_str().and_then(Kind::parse),
                    seconds: s["seconds"].as_f64().unwrap_or(0.0),
                    created: s["created"].as_u64().unwrap_or(0),
                    path: folder.join(s["file"].as_str().unwrap_or("")),
                })
            })
            .collect();
    }

    fn stop_preview(&mut self) {
        if let Some((_, mut child)) = self.playing.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl AgentPanel {
    fn generate(&mut self, cx: &mut Context<Self>) {
        let state = &self.generate;
        let prompt = state.prompt.read(cx).text().to_string();
        let Some(service) = state.service() else {
            return;
        };
        if prompt.trim().is_empty() || state.started.is_some() || self.busy(cx) {
            return;
        }
        let params = request_params(
            &prompt,
            state.kind,
            &service.id,
            state.length(),
            state.instrumental,
            state.follow_song,
        );
        self.generate.started = Some(Instant::now());
        self.generate.error.clear();
        let answer = jobs::request(&self.daw, "generate.audio", params, cx);
        self.generate._generation = Some(cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                this.generate.started = None;
                if let Err(error) = result {
                    this.generate.error = error;
                }
                let daw = this.daw.clone();
                this.generate.refresh(&daw, cx);
                cx.notify();
            });
        }));
        // The button counts the seconds while the service works.
        self.generate._tick = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            let going = this
                .update(cx, |this, cx| {
                    cx.notify();
                    this.generate.started.is_some()
                })
                .unwrap_or(false);
            if !going {
                break;
            }
        }));
        cx.notify();
    }

    /// Place, delete: an answer now or when the job finishes, then the list again.
    fn sound_action(&mut self, method: &'static str, params: Value, cx: &mut Context<Self>) {
        self.generate.error.clear();
        let answer = jobs::request(&self.daw, method, params, cx);
        self.generate._action = Some(cx.spawn(async move |this, cx| {
            let result = answer.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.generate.error = error;
                }
                let daw = this.daw.clone();
                this.generate.refresh(&daw, cx);
                cx.notify();
            });
        }));
    }

    /// Hear a kept sound, or stop it. macOS plays it with `afplay`; elsewhere the system's
    /// player opens it.
    fn preview(&mut self, id: String, path: PathBuf, cx: &mut Context<Self>) {
        let same = self
            .generate
            .playing
            .as_ref()
            .is_some_and(|(p, _)| *p == id);
        self.generate.stop_preview();
        if same {
            cx.notify();
            return;
        }
        if !cfg!(target_os = "macos") {
            cx.open_with_system(&path);
            return;
        }
        match std::process::Command::new("afplay").arg(&path).spawn() {
            Ok(child) => {
                self.generate.playing = Some((id, child));
                // Notice when it ends on its own.
                self.generate._player = Some(cx.spawn(async move |this, cx| loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(250))
                        .await;
                    let playing = this
                        .update(cx, |this, cx| {
                            let done = this
                                .generate
                                .playing
                                .as_mut()
                                .is_none_or(|(_, child)| !matches!(child.try_wait(), Ok(None)));
                            if done {
                                this.generate.playing = None;
                                cx.notify();
                            }
                            !done
                        })
                        .unwrap_or(false);
                    if !playing {
                        break;
                    }
                }));
            }
            Err(error) => self.generate.error = format!("Could not play the sound: {error}"),
        }
        cx.notify();
    }

    fn pick_menu(
        &mut self,
        items: Vec<MenuItem>,
        at: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu.open(items, at, window, cx);
    }

    pub(super) fn render_generate(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.generate.services.is_none() {
            let daw = self.daw.clone();
            self.generate.refresh(&daw, cx);
        }
        let theme = Theme::get(cx).clone();
        let busy = self.busy(cx);
        let entity = cx.entity();
        let state = &self.generate;
        let kind = state.kind;
        let ready = state.ready();
        let service = state.service();
        let working = state.started;
        let prompt_text = state.prompt.read(cx).text().to_string();
        let focused = state.prompt.read(cx).is_focused(window);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let playing = state.playing.as_ref().map(|(id, _)| id.clone());
        let label = |s: &'static str| {
            div()
                .text_size(px(size::SM))
                .text_color(theme.text_2)
                .child(s)
        };

        let connect = (state.services.is_some() && ready.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .p(px(12.0))
                .rounded(px(radius::MD))
                .bg(theme.accent_soft)
                .border_1()
                .border_color(theme.accent_ring)
                .child(div().font_weight(FontWeight::SEMIBOLD).child("Connect a sound service"))
                .child(
                    div()
                        .text_size(px(size::SM))
                        .line_height(px(18.0))
                        .text_color(theme.text_2)
                        .child("ElevenLabs, Stable Audio, fal.ai or your own endpoint. Add a key once in Settings; the service bills its own account directly and ryolune takes nothing."),
                )
                .child(div().flex().child(
                    Button::new("setup-generation", "Set up generation")
                        .primary()
                        .on_click(cx.listener(|this, _, _, cx| this.open_settings("generation", cx))),
                ))
        });

        let kinds = div()
            .flex()
            .p(px(2.0))
            .gap(px(2.0))
            .rounded(px(radius::SM + 1.0))
            .bg(theme.well)
            .border_1()
            .border_color(theme.hairline)
            .children(Kind::ALL.into_iter().map(|k| {
                let on = k == kind;
                div()
                    .id(k.key())
                    .flex_1()
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius::SM - 1.0))
                    .text_size(px(size::SM))
                    .text_color(if on { theme.text } else { theme.text_2 })
                    .when(on, |d| {
                        d.bg(theme.control)
                            .border_1()
                            .border_color(theme.control_edge)
                    })
                    .when(!on, |d| d.cursor_pointer().hover(|s| s.bg(theme.hover)))
                    .child(k.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.generate.kind = k;
                        this.generate
                            .prompt
                            .update(cx, |input, cx| input.set_placeholder(k.placeholder(), cx));
                        cx.notify();
                    }))
            }));

        let length = state.length();
        let length_label = if kind == Kind::Loop {
            format!("{length} {}", if length == 1 { "bar" } else { "bars" })
        } else {
            duration(length as f64)
        };
        let length_select = select_button("gen-length", length_label, cx).on_click({
            let entity = entity.clone();
            cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                let items = kind
                    .lengths()
                    .iter()
                    .map(|&n| {
                        let entity = entity.clone();
                        let text = if kind == Kind::Loop {
                            format!("{n} {}", if n == 1 { "bar" } else { "bars" })
                        } else {
                            duration(n as f64)
                        };
                        MenuItem::new(text, move |_, cx| {
                            entity.update(cx, |this, cx| {
                                this.generate.lengths[kind as usize] = n;
                                cx.notify();
                            })
                        })
                        .checked(n == this.generate.length())
                    })
                    .collect();
                this.pick_menu(items, e.position(), window, cx);
            })
        });
        let service_select = (ready.len() > 1).then(|| {
            let current = service.as_ref().map_or(String::new(), |s| s.label.clone());
            let ready = ready.clone();
            let entity = entity.clone();
            select_button("gen-service", current, cx).on_click(cx.listener(
                move |this, e: &gpui::ClickEvent, window, cx| {
                    let chosen = this.generate.service().map(|s| s.id);
                    let items = ready
                        .iter()
                        .map(|s| {
                            let (entity, id) = (entity.clone(), s.id.clone());
                            MenuItem::new(s.label.clone(), move |_, cx| {
                                let id = id.clone();
                                entity.update(cx, |this, cx| {
                                    this.generate.chosen = id;
                                    cx.notify();
                                })
                            })
                            .checked(chosen.as_deref() == Some(s.id.as_str()))
                        })
                        .collect();
                    this.pick_menu(items, e.position(), window, cx);
                },
            ))
        });
        let switch_row = |id: &'static str,
                          text: &'static str,
                          on: bool,
                          set: fn(&mut Generate, bool),
                          cx: &mut Context<Self>| {
            let entity = cx.entity();
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(Switch::new(id, on).on_toggle(move |value, _, cx| {
                    entity.update(cx, |this, cx| {
                        set(&mut this.generate, value);
                        cx.notify();
                    })
                }))
                .child(
                    div()
                        .text_size(px(size::SM))
                        .text_color(theme.text_2)
                        .child(text),
                )
        };
        let options = div()
            .flex()
            .flex_wrap()
            .items_end()
            .gap(px(12.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(label("Length"))
                    .child(length_select),
            )
            .when_some(service_select, |d, select| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(label("Service"))
                        .child(select),
                )
            })
            .when(matches!(kind, Kind::Loop | Kind::Song), |d| {
                d.child(switch_row(
                    "gen-follow",
                    "Song's tempo and key",
                    state.follow_song,
                    |g, v| g.follow_song = v,
                    cx,
                ))
            })
            .when(kind == Kind::Song, |d| {
                d.child(switch_row(
                    "gen-instrumental",
                    "Instrumental",
                    state.instrumental,
                    |g, v| g.instrumental = v,
                    cx,
                ))
            });

        let go_label = match working {
            Some(started) => format!(
                "Making it with {}… {}",
                service
                    .as_ref()
                    .map_or("your service", |s| s.label.as_str()),
                duration(started.elapsed().as_secs_f64().floor())
            ),
            None => format!("Generate {}", kind.label().to_lowercase()),
        };
        let can_go =
            service.is_some() && !prompt_text.trim().is_empty() && working.is_none() && !busy;

        let library = (!state.sounds.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .mt(px(8.0))
                .child(crate::ui::widgets::heading("Your sounds", cx))
                .children(state.sounds.iter().map(|sound| {
                    let on = playing.as_deref() == Some(sound.id.as_str());
                    let (id, path) = (sound.id.clone(), sound.path.clone());
                    let (place_id, keys_id, delete_id) =
                        (sound.id.clone(), sound.id.clone(), sound.id.clone());
                    let description: SharedString = sound.description.clone().into();
                    let kind_label = sound.kind.map_or("Sound", Kind::label);
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .py(px(5.0))
                        .border_b_1()
                        .border_color(theme.hairline)
                        .child(
                            Button::icon(
                                SharedString::from(format!("listen-{}", sound.id)),
                                if on { "stop" } else { "play" },
                            )
                            .icon_size(10.0)
                            .lit(on)
                            .tooltip(if on { "Stop" } else { "Listen" })
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.preview(id.clone(), path.clone(), cx),
                            )),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("sound-{}", sound.id)))
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(size::BASE))
                                        .whitespace_nowrap()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .child(sound.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(size::XS))
                                        .text_color(theme.text_3)
                                        .child(format!(
                                            "{kind_label} · {} · {}",
                                            duration(sound.seconds),
                                            ago(sound.created, now)
                                        )),
                                )
                                .tooltip(move |_, cx| {
                                    crate::ui::widgets::tip(description.clone(), cx)
                                }),
                        )
                        .child(
                            Button::new(SharedString::from(format!("add-{}", sound.id)), "Add")
                                .compact()
                                .tooltip("Put it in the song as an audio clip at the playhead")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.sound_action(
                                        "generate.place",
                                        json!({"id": place_id, "as": "audio"}),
                                        cx,
                                    )
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("keys-{}", sound.id)), "Keys")
                                .compact()
                                .tooltip("Play it from the keyboard on a new Sample Keys track")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.sound_action(
                                        "generate.place",
                                        json!({"id": keys_id, "as": "instrument"}),
                                        cx,
                                    )
                                })),
                        )
                        .child(
                            Button::icon(
                                SharedString::from(format!("delete-{}", sound.id)),
                                "close",
                            )
                            .ghost()
                            .compact()
                            .tooltip("Delete from this computer (clips in songs keep their audio)")
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.sound_action(
                                        "generate.delete",
                                        json!({"id": delete_id}),
                                        cx,
                                    )
                                },
                            )),
                        )
                }))
        });

        div()
            .py(px(14.0))
            .px(px(2.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(
                        div()
                            .text_size(px(size::BASE + 1.0))
                            .font_weight(FontWeight::BOLD)
                            .child("Make a sound"),
                    )
                    .child(
                        div()
                            .text_size(px(size::SM))
                            .line_height(px(18.0))
                            .text_color(theme.text_2)
                            .child("Describe it in your own words. ryolune asks your sound service and puts the result straight into the song."),
                    ),
            )
            .children(connect)
            .child(kinds)
            .child(div().text_size(px(size::SM)).text_color(theme.text_3).child(kind.hint()))
            .child(
                div()
                    .id("gen-prompt")
                    .min_h(px(64.0))
                    .max_h(px(160.0))
                    .overflow_y_scroll()
                    .child(field(&self.generate.prompt, focused, cx).min_h(px(64.0))),
            )
            .child(options)
            .child(
                div().flex().flex_col().child(
                    Button::new("gen-go", go_label)
                        .primary()
                        .disabled(!can_go)
                        .with_icon("sparkle")
                        .on_click(cx.listener(|this, _, _, cx| this.generate(cx))),
                ),
            )
            .when(!self.generate.error.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(px(size::SM))
                        .text_color(theme.danger)
                        .child(SharedString::from(self.generate.error.clone())),
                )
            })
            .children(library)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_for_bars_or_seconds_and_follows_the_song_when_asked() {
        let loop_ = request_params(" Dusty drums ", Kind::Loop, "elevenlabs", 4, true, true);
        assert_eq!(loop_["prompt"], "Dusty drums");
        assert_eq!(loop_["bars"], 4);
        assert_eq!(loop_["followSong"], true);
        assert!(loop_.get("seconds").is_none() && loop_.get("instrumental").is_none());
        let song = request_params("Synthwave", Kind::Song, "stability", 60, false, false);
        assert_eq!(song["seconds"], 60);
        assert_eq!(song["instrumental"], false);
        assert_eq!(song["followSong"], false);
        let keys = request_params("Felt piano", Kind::Instrument, "fal", 3, true, true);
        assert_eq!(keys["kind"], "instrument");
        assert!(keys.get("followSong").is_none());
        assert_eq!(
            request_params(&"x".repeat(3000), Kind::Sound, "custom", 3, true, true)["prompt"]
                .as_str()
                .unwrap()
                .len(),
            2000
        );
    }

    #[test]
    fn lengths_and_ages_read_plainly() {
        assert_eq!(duration(65.0), "1:05");
        assert_eq!(duration(3.0), "3 s");
        assert_eq!(duration(2.54), "2.5 s");
        assert_eq!(ago(1000, 1010), "just now");
        assert_eq!(ago(0, 600), "10 min ago");
        assert_eq!(ago(0, 7200), "2 h ago");
        assert_eq!(ago(0, 3 * 86_400), "3 d ago");
    }
}
