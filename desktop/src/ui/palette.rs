//! The command palette (⌘P): every action of the table in one searchable list. It is built
//! from the menus and then the actions no menu lists (transport, tools, editor modes), so
//! what it runs, enables and ticks is exactly what the menu bar does. Glass tier 2 over the
//! scrim, near the top of the window; a click outside or Escape closes it
//! (`ui.showPanel panel=palette visible=false`, which a script can send too).

use super::{
    actions::{self, Do, ACTIONS, MENUS},
    daw::Daw,
    theme::{radius, size, Theme, FONT_MONO},
    widgets::{self, icon, text_input, InputEvent, TextInput},
};
use gpui::{
    div, prelude::*, px, uniform_list, AnyElement, Context, Entity, FocusHandle, MouseButton,
    ScrollStrategy, Subscription, UniformListScrollHandle, Window,
};
use serde_json::json;
use std::collections::HashSet;

/// One line of the palette: an action and where it lives (a menu title, or "Action").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
}

/// Menu rows first, in menu order, then every action no menu lists. The palette's own
/// entry is left out: running it from the palette would only reopen it.
pub fn entries() -> Vec<Entry> {
    let mut seen = HashSet::new();
    let mut out = vec![];
    let menus = MENUS
        .iter()
        .flat_map(|(title, ids)| ids.iter().flatten().map(move |id| (*title, *id)));
    let rest = ACTIONS.iter().map(|d| ("Action", d.id));
    for (group, id) in menus.chain(rest) {
        let Some(def) = actions::def(id) else {
            continue;
        };
        if def.id == "commandPalette" || !seen.insert(def.label) {
            continue;
        }
        out.push(Entry {
            id: def.id,
            label: def.label,
            group,
        });
    }
    out
}

/// Score a query against a label: a prefix beats a word start beats a substring beats
/// scattered letters in order; 0 is no match. An empty query matches everything.
pub fn match_score(label: &str, query: &str) -> u8 {
    let l = label.to_lowercase();
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return 1;
    }
    if l.starts_with(&q) {
        return 4;
    }
    if l.contains(&format!(" {q}")) {
        return 3;
    }
    if l.contains(&q) {
        return 2;
    }
    let wanted: Vec<char> = q.chars().collect();
    let mut i = 0;
    for ch in l.chars() {
        if i < wanted.len() && ch == wanted[i] {
            i += 1;
        }
    }
    u8::from(i == wanted.len())
}

/// The entries a query keeps, best first; at equal score the ones that can run now come
/// before the disabled ones, and the table's order decides the rest.
pub fn results(entries: &[Entry], query: &str, enabled: impl Fn(&str) -> bool) -> Vec<usize> {
    let mut scored: Vec<(usize, u8, bool)> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            (
                i,
                match_score(&format!("{} {}", e.label, e.group), query),
                enabled(e.id),
            )
        })
        .filter(|(_, score, _)| *score > 0)
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));
    scored.into_iter().map(|(i, _, _)| i).collect()
}

/// Every line of the list is this tall, so the list is virtual.
const ROW_H: f32 = 30.0;
const WIDTH: f32 = 560.0;

pub struct Palette {
    daw: Entity<Daw>,
    input: Entity<TextInput>,
    entries: Vec<Entry>,
    query: String,
    /// The highlighted line, an index into `results`.
    active: usize,
    results: Vec<usize>,
    /// Whether the palette was showing at the last look, to catch it opening and closing.
    open: bool,
    /// Where the keyboard was before the palette took it, to give it back.
    previous: Option<FocusHandle>,
    scroll: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl Palette {
    pub fn new(daw: Entity<Daw>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new(cx).placeholder("Type a command…"));
        let typing = cx.subscribe_in(
            &input,
            window,
            |this, input, event, window, cx| match event {
                InputEvent::Changed => {
                    this.query = input.read(cx).text().to_string();
                    this.active = 0;
                    this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                    cx.notify();
                }
                InputEvent::Submit => this.run(this.active, window, cx),
                InputEvent::Cancel => this.close(cx),
                InputEvent::Blur => {}
            },
        );
        // Opening (⌘P, the View menu, `ui.showPanel`) starts a fresh search with the
        // keyboard in the field; closing gives the keyboard back.
        let showing = cx.observe_in(&daw, window, |this, daw, window, cx| {
            let open = daw.read(cx).app.show_palette;
            if open && !this.open {
                this.previous = window.focused(cx);
                this.query.clear();
                this.active = 0;
                this.input.update(cx, |input, cx| input.set_text("", cx));
                this.input.read(cx).focus(window);
            } else if !open && this.open {
                if let Some(previous) = this.previous.take() {
                    window.focus(&previous);
                }
            }
            this.open = open;
        });
        let open = daw.read(cx).app.show_palette;
        if open {
            input.read(cx).focus(window);
        }
        Self {
            daw,
            input,
            entries: entries(),
            query: String::new(),
            active: 0,
            results: vec![],
            open,
            previous: None,
            scroll: UniformListScrollHandle::new(),
            _subscriptions: vec![typing, showing],
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.daw.update(cx, |daw, cx| {
            daw.run(
                "ui.showPanel",
                json!({"panel": "palette", "visible": false}),
                cx,
            );
        });
    }

    /// Run the line at `index` of the results, after the palette has closed and the
    /// keyboard is back where it was. A disabled line does nothing.
    fn run(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.results.get(index).map(|&i| self.entries[i]) else {
            return;
        };
        if !actions::enabled(entry.id, self.daw.read(cx)) {
            return;
        }
        self.close(cx);
        if let Some(previous) = self.previous.take() {
            window.focus(&previous);
        }
        window.dispatch_action(Box::new(Do { id: entry.id }), cx);
    }

    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let n = self.results.len().max(1);
        self.active = if forward {
            (self.active + 1) % n
        } else {
            (self.active + n - 1) % n
        };
        self.scroll.scroll_to_item(self.active, ScrollStrategy::Top);
        cx.notify();
    }

    fn render_rows(
        &mut self,
        range: std::ops::Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = Theme::get(cx).clone();
        let daw = self.daw.read(cx);
        let lines: Vec<(usize, Entry, bool, Option<bool>)> = range
            .filter_map(|i| {
                let entry = self.entries[*self.results.get(i)?];
                Some((
                    i,
                    entry,
                    actions::enabled(entry.id, daw),
                    actions::checked(entry.id, daw),
                ))
            })
            .collect();
        lines
            .into_iter()
            .map(|(i, entry, enabled, checked)| {
                let active = i == self.active;
                div()
                    .id(("palette-row", i))
                    .w_full()
                    .h(px(ROW_H))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(8.0))
                    .rounded(px(radius::XS))
                    .text_size(px(size::BASE))
                    .text_color(if active {
                        theme.text_on_accent
                    } else {
                        theme.text
                    })
                    // The row under the pointer or the arrows is inverted, paper on ink.
                    .when(active, |d| d.bg(theme.accent_fill))
                    .child(div().w(px(16.0)).flex_none().flex().justify_center().when(
                        checked == Some(true),
                        |d| {
                            d.child(icon(
                                "check",
                                10.0,
                                if active {
                                    theme.text_on_accent
                                } else {
                                    theme.accent_text
                                },
                            ))
                        },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .when(!enabled, |d| d.opacity(0.45))
                            .child(entry.label),
                    )
                    .child(
                        div()
                            .flex_none()
                            .font_family(FONT_MONO)
                            .text_size(px(10.0))
                            .text_color(if active {
                                theme.text_on_accent
                            } else {
                                theme.text_3
                            })
                            .child(entry.group.to_uppercase()),
                    )
                    .child(
                        div()
                            .w(px(64.0))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .font_family(FONT_MONO)
                            .text_size(px(size::XS))
                            .text_color(if active {
                                theme.text_on_accent
                            } else {
                                theme.text_2
                            })
                            .children(actions::shortcut_label(entry.id)),
                    )
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.active != i {
                            this.active = i;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| this.run(i, window, cx)))
                    .into_any_element()
            })
            .collect()
    }
}

impl Render for Palette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx).clone();
        let daw = self.daw.read(cx);
        self.results = results(&self.entries, &self.query, |id| actions::enabled(id, daw));
        if self.active >= self.results.len() {
            self.active = 0;
        }
        let viewport = window.viewport_size();
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
        let width = WIDTH.min(vw * 0.86);
        // The field and its margins take 56 px; the list gets the rest of 60 % of the window.
        let list_h = (self.results.len() as f32 * ROW_H).min((vh * 0.6 - 64.0).max(ROW_H));
        let focused = self.input.read(cx).is_focused(window);
        let none = self.results.is_empty();
        let query = self.query.clone();

        div()
            .id("palette-scrim")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(vh * 0.14))
            .bg(theme.scrim)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close(cx)),
            )
            .child(
                widgets::surface(2, cx)
                    .id("palette")
                    .w(px(width))
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .font_family(super::theme::FONT_UI)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    // Arrow keys move through the list while the field keeps the keyboard.
                    .capture_action(cx.listener(|this, _: &text_input::Down, _, cx| {
                        cx.stop_propagation();
                        this.step(true, cx);
                    }))
                    .capture_action(cx.listener(|this, _: &text_input::Up, _, cx| {
                        cx.stop_propagation();
                        this.step(false, cx);
                    }))
                    .child(
                        div()
                            .flex_none()
                            .m(px(10.0))
                            .px(px(11.0))
                            .py(px(9.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .rounded(px(radius::MD))
                            .bg(theme.well)
                            .border_1()
                            .border_color(if focused {
                                theme.accent_ring
                            } else {
                                theme.line
                            })
                            .text_size(px(size::BASE))
                            .text_color(theme.text)
                            .child(icon("search", 12.0, theme.text_3))
                            .child(div().flex_1().min_w_0().child(self.input.clone())),
                    )
                    .child(if none {
                        div()
                            .px(px(16.0))
                            .pt(px(4.0))
                            .pb(px(16.0))
                            .text_size(px(size::BASE))
                            .text_color(theme.text_3)
                            .child(format!("No command matches “{}”.", query.trim()))
                            .into_any_element()
                    } else {
                        div()
                            .px(px(6.0))
                            .pb(px(8.0))
                            .child(
                                uniform_list(
                                    "palette-rows",
                                    self.results.len(),
                                    cx.processor(|this, range, window, cx| {
                                        this.render_rows(range, window, cx)
                                    }),
                                )
                                .track_scroll(self.scroll.clone())
                                .h(px(list_h)),
                            )
                            .into_any_element()
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_prefixes_over_word_starts_over_substrings_and_rejects_non_matches() {
        assert!(match_score("Mixer View", "mix") > match_score("Show Mixer", "mix"));
        assert_eq!(match_score("Show Mixer", "mix"), 3);
        assert_eq!(match_score("Remixer", "mix"), 2);
        assert_eq!(match_score("Duplicate Track", "dptk"), 1);
        assert_eq!(match_score("Undo", "zzz"), 0);
        assert_eq!(match_score("Undo", "  "), 1);
    }

    #[test]
    fn reaches_every_action_once_menus_first() {
        let list = entries();
        let labels: HashSet<&str> = list.iter().map(|e| e.label).collect();
        assert_eq!(labels.len(), list.len(), "labels are unique");
        for def in ACTIONS {
            if def.id != "commandPalette" {
                assert!(list.iter().any(|e| e.id == def.id), "{} is missing", def.id);
            }
        }
        // Transport actions no menu lists are still there, under "Action".
        let play = list.iter().find(|e| e.id == "togglePlay").unwrap();
        assert_eq!(play.group, "Action");
        assert_eq!(list[0].group, "File");
        assert!(list.iter().all(|e| e.id != "commandPalette"));
    }

    #[test]
    fn results_keep_matches_best_first_and_disabled_last() {
        let list = entries();
        let ids = |q: &str, enabled: &dyn Fn(&str) -> bool| -> Vec<&str> {
            results(&list, q, enabled)
                .iter()
                .map(|&i| list[i].id)
                .collect()
        };
        let all = ids("", &|_| true);
        assert_eq!(all.len(), list.len());
        let undo = ids("undo", &|_| true);
        assert_eq!(undo[0], "undo");
        // At equal score, what cannot run now sorts after what can.
        let save = ids("save", &|_| true);
        assert_eq!(save[0], "save");
        let save = ids("save", &|id| id != "save");
        assert_eq!(save[0], "saveAs");
        assert!(save.contains(&"save"));
        assert!(ids("qqqq", &|_| true).is_empty());
        // The group is searched too: every File menu entry matches "file".
        assert!(ids("file", &|_| true).contains(&"save"));
    }

    fn open_palette(
        cx: &mut gpui::TestAppContext,
    ) -> (Entity<Daw>, Entity<Palette>, &mut gpui::VisualTestContext) {
        let daw = crate::ui::browser::tests::test_daw(cx);
        let (palette, cx) = cx.add_window_view(|window, cx| Palette::new(daw.clone(), window, cx));
        cx.update(|_, cx| {
            daw.update(cx, |daw, cx| {
                daw.run(
                    "ui.showPanel",
                    json!({"panel": "palette", "visible": true}),
                    cx,
                );
            })
        });
        cx.run_until_parked();
        (daw, palette, cx)
    }

    #[gpui::test]
    fn typing_filters_arrows_move_and_enter_runs_after_closing(cx: &mut gpui::TestAppContext) {
        let (daw, palette, cx) = open_palette(cx);
        cx.simulate_input("mixer");
        cx.run_until_parked();
        let first = palette.read_with(cx, |p, _| p.entries[p.results[0]].id);
        assert_eq!(first, "toggleMixer");
        let count = palette.read_with(cx, |p, _| p.results.len());
        cx.simulate_keystrokes("down");
        assert_eq!(palette.read_with(cx, |p, _| p.active), 1 % count);
        cx.simulate_keystrokes("up");
        assert_eq!(palette.read_with(cx, |p, _| p.active), 0);
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert!(!daw.read_with(cx, |d, _| d.app.show_palette), "closed");
    }

    #[gpui::test]
    fn escape_closes_and_reopening_starts_a_fresh_search(cx: &mut gpui::TestAppContext) {
        let (daw, palette, cx) = open_palette(cx);
        cx.simulate_input("zzzz");
        cx.run_until_parked();
        assert!(palette.read_with(cx, |p, _| p.results.is_empty()));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(!daw.read_with(cx, |d, _| d.app.show_palette));
        cx.update(|_, cx| {
            daw.update(cx, |daw, cx| {
                daw.run(
                    "ui.showPanel",
                    json!({"panel": "palette", "visible": true}),
                    cx,
                );
            })
        });
        cx.run_until_parked();
        let (query, text) = palette.read_with(cx, |p, cx| {
            (p.query.clone(), p.input.read(cx).text().to_string())
        });
        assert!(query.is_empty() && text.is_empty());
    }
}
