//! The window, drawn with GPUI (gpui.rs, Zed's GPU interface framework). One `Workspace`
//! lays out the design frame: title bar, transport, browser, arrangement over the region
//! editor or the mixer, inspector and the agent panel, with dialogs above. Views read the
//! host through the shared [`daw::Daw`] entity and change it through registry commands, so
//! the window, `ryolune-cli`, `ryolune-mcp` and the built-in agent act on one session the
//! same way.

pub mod actions;
pub mod agent_panel;
pub mod arrangement;
pub mod assets;
pub mod automation;
pub mod browser;
pub mod capture;
pub mod daw;
pub mod dialogs;
pub mod editor;
pub mod format;
pub mod inspector;
pub mod mixer;
pub mod palette;
pub mod platform;
pub mod plugin_panel;
pub mod settings_window;
pub mod strip;
pub mod theme;
pub mod titlebar;
pub mod transport;
pub mod widgets;
pub mod workspace;

use crate::app::Ryolune;
use daw::Daw;
use gpui::{
    point, px, size, App, AppContext, Application, Bounds, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowOptions,
};
use std::path::PathBuf;
use theme::{layout, Mode, Theme};

/// Which mode the settings ask for: dark, light, or the system's ("auto").
pub fn resolve_mode(setting: &str, cx: &App) -> Mode {
    match setting {
        "light" => Mode::Light,
        "dark" => Mode::Dark,
        _ => match cx.window_appearance() {
            gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight => Mode::Light,
            _ => Mode::Dark,
        },
    }
}

/// The system asks apps to avoid translucency (macOS Accessibility > Display).
pub fn reduce_transparency() -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWorkspace;
        NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency()
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Only macOS blurs the window behind the glass; elsewhere the tiers are opaque.
        true
    }
}

pub fn run(
    path: Option<PathBuf>,
    screenshot: Option<PathBuf>,
    control: bool,
    updates: bool,
    agents: bool,
    unclean: Vec<PathBuf>,
) {
    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            let _ = cx.text_system().add_fonts(assets::fonts());
            let (wake_tx, wake_rx) = futures::channel::mpsc::unbounded::<()>();
            let wake: crate::Wake = std::sync::Arc::new(move || {
                let _ = wake_tx.unbounded_send(());
            });
            let mut app = Ryolune::new(wake, path.clone(), screenshot.clone(), control, updates);
            app.agents.open |= agents;
            app.previous_run_ended(&unclean);
            let opaque = reduce_transparency();
            let mode = resolve_mode(&app.settings.interface.mode, cx);
            cx.set_global(Theme::new(mode, opaque));
            actions::bind(cx);

            let bounds = Bounds::centered(None, size(px(1600.0), px(1000.0)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("ryolune".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(14.0), px(12.0))),
                }),
                window_background: if opaque {
                    WindowBackgroundAppearance::Opaque
                } else {
                    WindowBackgroundAppearance::Blurred
                },
                window_min_size: Some(size(px(layout::WINDOW_MIN_W), px(layout::WINDOW_MIN_H))),
                app_id: Some("org.ryolune.desktop".into()),
                ..Default::default()
            };
            let daw = cx.new(|cx| {
                let mut daw = Daw::new(app);
                daw.start(wake_rx, cx);
                daw
            });
            let window = cx.open_window(options, |window, cx| {
                cx.new(|cx| workspace::Workspace::new(daw.clone(), window, cx))
            });
            if let Err(error) = window {
                log::error!("ryolune could not open its window: {error}");
                cx.quit();
                return;
            }
            actions::set_menus(cx);
            cx.activate(true);
        });
}
