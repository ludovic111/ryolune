//! The arrangement's title bar: its name and what it shows, then the tools boxed by kind
//! (edit tools · modes · lanes) and the zoom box at the right. Each tool shows its icon and
//! label when there is room, the icon alone when there isn't. Glass tier 1, like the chrome.

use super::{geometry, Arrangement};
use crate::ui::{
    actions,
    theme::{arrange, layout, Theme},
    widgets::{group, panel_info, panel_title, Phase, Slider},
};
use gpui::{div, prelude::*, px, AnyElement, Context, Window};
use serde_json::json;

/// A bar number as the ruler shows it: 1-based, whole when it is.
fn bar_label(bar: f64) -> String {
    let n = bar + 1.0;
    if (n - n.round()).abs() < 1e-6 {
        format!("{}", n.round() as i64)
    } else {
        format!("{n:.2}")
    }
}

impl Arrangement {
    pub(super) fn toolbar(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::get(cx).clone();
        let daw = self.daw.read(cx);
        let app = &daw.app;
        let s = app.store.session();
        let snap = s.transport.snap_division;
        let (start, end) = (s.transport.cycle_start_bar, s.transport.cycle_end_bar);
        let zoom = app.zoom as f64;
        let tracks = s.tracks.len();
        // The width the arrangement had at the last frame: tools lose their labels first.
        let width = f32::from(self.areas.lanes.get().size.width) + layout::TRACK_HEADER;
        let (roomy, wide) = (width >= 980.0, width >= 1180.0);

        let info = format!(
            "{tracks} track{} · grid {}",
            if tracks == 1 { "" } else { "s" },
            if snap == 1 {
                "bar".to_string()
            } else {
                format!("1/{snap}")
            }
        );
        let title = div()
            .flex()
            .flex_none()
            .items_baseline()
            .gap(px(10.0))
            .child(panel_title("Arrangement", cx))
            .when(wide, |d| d.child(panel_info(info, cx)));
        let edit = group(
            [
                actions::tool("toolPointer", "pointer", "Pointer", roomy, daw).into_any_element(),
                actions::tool("toolPencil", "pencil", "Pencil", roomy, daw).into_any_element(),
                actions::tool("toolScissors", "scissors", "Scissors", roomy, daw)
                    .into_any_element(),
            ],
            cx,
        );
        let cycle_label: &'static str = if roomy { "Cycle" } else { "" };
        let modes = group(
            [
                actions::tool("followPlayhead", "move-horizontal", "Follow", roomy, daw)
                    .into_any_element(),
                actions::tool("cycle", "cycle", cycle_label, roomy, daw).into_any_element(),
                div()
                    .px(px(8.0))
                    .font_family(crate::ui::theme::FONT_MONO)
                    .text_size(px(crate::ui::theme::size::XS))
                    .text_color(theme.text_2)
                    .whitespace_nowrap()
                    .child(format!("{} – {}", bar_label(start), bar_label(end)))
                    .into_any_element(),
            ],
            cx,
        );
        let lanes = group(
            [
                actions::tool("toggleTempoTrack", "activity", "Tempo", wide, daw)
                    .into_any_element(),
                actions::tool("addMarker", "marker", "Marker", wide, daw).into_any_element(),
            ],
            cx,
        );
        let daw_entity = self.daw.clone();
        let zoom_slider = Slider::new("zoom", geometry::zoom_to_t(zoom) as f32)
            .default_value(geometry::zoom_to_t(geometry::ZOOM_DEFAULT) as f32)
            .on_change(move |t, phase, _, cx| {
                if phase == Phase::Start && (t as f64 - geometry::zoom_to_t(zoom)).abs() < 1e-6 {
                    return;
                }
                let ppb = geometry::t_to_zoom(t as f64);
                daw_entity.update(cx, |daw, cx| {
                    daw.run("view.set", json!({ "pixelsPerBar": ppb }), cx);
                });
            });
        let zoom_box = group(
            [
                actions::tool("zoomOut", "zoom-out", "Zoom out", false, daw).into_any_element(),
                div()
                    .w(px(arrange::ZOOM_RAIL))
                    .px(px(8.0))
                    .flex()
                    .child(zoom_slider)
                    .into_any_element(),
                actions::tool("zoomIn", "zoom-in", "Zoom in", false, daw).into_any_element(),
                actions::tool("zoomToFit", "maximize-2", "Fit", roomy, daw).into_any_element(),
            ],
            cx,
        );

        div()
            .h(px(layout::TOOLBAR))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(12.0))
            .bg(theme.glass(1))
            .border_b_1()
            .border_color(theme.line)
            .overflow_hidden()
            .child(title)
            .child(div().flex_1().min_w(px(8.0)))
            .child(edit)
            .child(modes)
            .child(lanes)
            .child(div().flex_1().min_w(px(8.0)))
            .child(zoom_box)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bar_labels_are_one_based() {
        assert_eq!(super::bar_label(4.0), "5");
        assert_eq!(super::bar_label(0.5), "1.50");
    }
}
