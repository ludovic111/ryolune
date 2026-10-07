//! The window's root: the design frame's layout, the action dispatcher, musical typing,
//! file drops and window captures.

use super::{
    actions::{self, Do},
    agent_panel::AgentPanel,
    arrangement::Arrangement,
    automation::Automation,
    browser::Browser,
    daw::Daw,
    dialogs::Dialogs,
    editor::Editor,
    inspector::Inspector,
    mixer::Mixer,
    palette::Palette,
    plugin_panel::PluginPanels,
    settings_window::SettingsWindow,
    theme::{layout, Theme},
    titlebar::TitleBar,
    transport::Transport,
};
use gpui::{
    div, prelude::*, px, Context, Entity, ExternalPaths, FocusHandle, Focusable, KeyDownEvent,
    KeyUpEvent, Window,
};

pub struct Workspace {
    pub daw: Entity<Daw>,
    focus: FocusHandle,
    title_bar: Entity<TitleBar>,
    transport: Entity<Transport>,
    browser: Entity<Browser>,
    arrangement: Entity<Arrangement>,
    editor: Entity<Editor>,
    mixer: Entity<Mixer>,
    inspector: Entity<Inspector>,
    agent: Entity<AgentPanel>,
    dialogs: Entity<Dialogs>,
    settings: Entity<SettingsWindow>,
    palette: Entity<Palette>,
    plugins: Entity<PluginPanels>,
    automation: Entity<Automation>,
    title: String,
    mode: String,
}

impl Workspace {
    pub fn new(daw: Entity<Daw>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        // Closing the window is Quit: it waits for a take to finish and offers to save.
        let close_daw = daw.clone();
        window.on_window_should_close(cx, move |_, cx| {
            close_daw.update(cx, |daw, cx| {
                if !daw.app.closing {
                    daw.app.request(crate::app::Intent::Quit);
                    cx.notify();
                }
                daw.app.closing
            })
        });
        cx.observe(&daw, |this: &mut Self, _, cx| {
            this.follow_settings(cx);
            cx.notify();
        })
        .detach();
        // Losing focus releases musical-typing notes, which no key-up will end.
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() {
                this.daw.update(cx, |daw, _| daw.app.release_typing());
            }
        })
        .detach();
        let mode = daw.read(cx).app.settings.interface.mode.clone();
        Self {
            title_bar: cx.new(|cx| TitleBar::new(daw.clone(), window, cx)),
            transport: cx.new(|cx| Transport::new(daw.clone(), window, cx)),
            browser: cx.new(|cx| Browser::new(daw.clone(), window, cx)),
            arrangement: cx.new(|cx| Arrangement::new(daw.clone(), window, cx)),
            editor: cx.new(|cx| Editor::new(daw.clone(), window, cx)),
            mixer: cx.new(|cx| Mixer::new(daw.clone(), window, cx)),
            inspector: cx.new(|cx| Inspector::new(daw.clone(), window, cx)),
            agent: cx.new(|cx| AgentPanel::new(daw.clone(), window, cx)),
            dialogs: cx.new(|cx| Dialogs::new(daw.clone(), window, cx)),
            settings: cx.new(|cx| SettingsWindow::new(daw.clone(), window, cx)),
            palette: cx.new(|cx| Palette::new(daw.clone(), window, cx)),
            plugins: cx.new(|cx| PluginPanels::new(daw.clone(), window, cx)),
            automation: cx.new(|cx| Automation::new(daw.clone(), window, cx)),
            daw,
            focus,
            title: String::new(),
            mode,
        }
    }

    /// The light/dark setting changed (Settings, `settings.set`, an agent): swap the theme.
    fn follow_settings(&mut self, cx: &mut Context<Self>) {
        let mode = self.daw.read(cx).app.settings.interface.mode.clone();
        if mode != self.mode {
            self.mode = mode;
            let theme = Theme::new(super::resolve_mode(&self.mode, cx), Theme::get(cx).opaque);
            cx.set_global(theme);
            cx.refresh_windows();
        }
    }

    fn perform(&mut self, action: &Do, window: &mut Window, cx: &mut Context<Self>) {
        let handled = self
            .daw
            .update(cx, |daw, cx| actions::perform(action.id, daw, cx));
        if !handled && action.id == "askAgent" {
            self.agent
                .update(cx, |agent, cx| agent.ask_about_selection(window, cx));
        }
    }

    /// Musical typing: while it is on, the letter keys play the selected instrument.
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if k.modifiers.platform
            || k.modifiers.control
            || k.modifiers.alt
            || event.is_held
            || typing(window)
        {
            return;
        }
        let handled = self.daw.update(cx, |daw, _| {
            daw.app.musical_typing && daw.app.typing_key(&k.key, true)
        });
        if handled {
            cx.stop_propagation();
        }
    }
    fn key_up(&mut self, event: &KeyUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        if typing(window) {
            return;
        }
        let handled = self.daw.update(cx, |daw, _| {
            daw.app.musical_typing && daw.app.typing_key(&k.key, false)
        });
        if handled {
            cx.stop_propagation();
        }
    }

    fn drop_paths(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        let paths = paths.paths().to_vec();
        self.daw.update(cx, |daw, cx| {
            if let Err(error) = daw.app.drop_files(paths) {
                daw.app.error = Some(error);
            }
            cx.notify();
        });
    }

    /// Keep the window title, the lane width and pending captures in step with the host.
    fn sync_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (title, capture) = self.daw.update(cx, |daw, _| {
            let app = &mut daw.app;
            // `ui.status` reports it as frontendReady: the window has drawn once.
            app.frontend_ready = true;
            let title = format!(
                "{}{} — ryolune",
                app.store.session().name,
                if app.store.dirty() { " *" } else { "" }
            );
            let startup = app.screenshot.is_some() && app.frames >= 90 && app.job.is_none();
            (title, app.take_capture_request() || startup)
        });
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
        if capture {
            // Bring the window forward first (Stage Manager draws others as thumbnails, and
            // a capture would see the thumbnail), then capture once it has settled.
            window.activate_window();
            let daw = self.daw.clone();
            cx.spawn_in(window, async move |_, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let _ = cx.update(|window, cx| super::capture::capture(window, daw, cx));
            })
            .detach();
        }
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_window(window, cx);
        let theme = Theme::get(cx).clone();
        let app = &self.daw.read(cx).app;
        let agent_open = app.agents.open;
        let mixer = app.show_mixer;
        let palette = app.show_palette;
        let automation = app.show_automation;
        let settings = app.settings_ui.open;
        let scale = app.settings.interface.scale.clamp(0.75, 1.5);
        let plugin_windows = !app.plugins.windows.is_empty();
        window.set_rem_size(px(16.0 * scale));
        // The region editor (or the mixer) takes a share of the height, so a smaller window
        // keeps room for the arrangement.
        let body = f32::from(window.viewport_size().height) - layout::TITLE_BAR - layout::TRANSPORT;
        let editor_height = (body * 0.46).clamp(280.0, layout::EDITOR);

        let body = div()
            .flex()
            .flex_row()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .w(px(layout::BROWSER))
                    .min_w(px(layout::BROWSER_MIN))
                    .flex_shrink()
                    .h_full()
                    .child(self.browser.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(layout::ARRANGEMENT_MIN))
                    .h_full()
                    .child(div().flex_1().min_h_0().child(self.arrangement.clone()))
                    .child(
                        div()
                            .h(px(editor_height))
                            .flex_shrink_0()
                            .border_t_1()
                            .border_color(theme.line)
                            .child(if mixer {
                                self.mixer.clone().into_any_element()
                            } else {
                                self.editor.clone().into_any_element()
                            }),
                    ),
            )
            .child(
                div()
                    .w(px(layout::INSPECTOR))
                    .min_w(px(layout::INSPECTOR_MIN))
                    .flex_shrink()
                    .h_full()
                    .child(self.inspector.clone()),
            )
            .child(
                div()
                    .w(px(if agent_open {
                        layout::AGENT
                    } else {
                        layout::AGENT_RAIL
                    }))
                    .min_w(px(if agent_open {
                        layout::AGENT_MIN
                    } else {
                        layout::AGENT_RAIL
                    }))
                    .flex_shrink()
                    .h_full()
                    .child(self.agent.clone()),
            );

        div()
            .id("workspace")
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::perform))
            .capture_key_down(cx.listener(Self::key_down))
            .capture_key_up(cx.listener(Self::key_up))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| this.drop_paths(paths, cx)))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .font_family(super::theme::FONT_UI)
            .text_size(px(super::theme::size::BASE))
            .text_color(theme.text)
            // The page (`.ls-backdrop`): film grain and two corners of dithered light,
            // drawn at device pixels, under the chrome.
            .child(super::grain::backdrop(window, cx))
            .child(
                div()
                    .h(px(layout::TITLE_BAR))
                    .flex_shrink_0()
                    .child(self.title_bar.clone()),
            )
            .child(
                div()
                    .h(px(layout::TRANSPORT))
                    .flex_shrink_0()
                    .child(self.transport.clone()),
            )
            .child(body)
            .when(plugin_windows, |d| d.child(self.plugins.clone()))
            .when(automation, |d| d.child(self.automation.clone()))
            .when(settings, |d| d.child(self.settings.clone()))
            .when(palette, |d| d.child(self.palette.clone()))
            .child(self.dialogs.clone())
            .drag_over::<ExternalPaths>(|style, _, _, cx| {
                let theme = Theme::get(cx);
                style.bg(theme.accent_soft)
            })
    }
}

/// A text field has the keyboard: letters are text, not notes.
fn typing(window: &Window) -> bool {
    window
        .context_stack()
        .iter()
        .any(|context| context.contains("TextInput"))
}
