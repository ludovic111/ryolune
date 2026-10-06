//! Settings › Diagnostics: crash reports (`app.crashReports`, `app.clearCrashReports`), the
//! last lines of this run's log (`app.logs`), the folders they live in, and what a bug report
//! needs (`app.diagnostics`, `app.reportProblem`). Everything stays on this computer; the
//! person copies or opens what they choose to share.

use super::SettingsWindow;
use crate::ui::{
    dialogs::modal,
    theme::{radius, size, Theme, FONT_MONO},
    widgets::Button,
};
use gpui::{div, prelude::*, px, AnyElement, ClipboardItem, Context, SharedString};
use ryolune_engine::diagnostics::Report;
use serde_json::{json, Value};

/// Lines of the log the section shows.
const LOG_LINES: usize = 40;

#[derive(Default)]
pub(crate) struct DiagState {
    reports: Vec<Report>,
    log: Vec<String>,
    log_file: Option<String>,
    /// The report shown in full: (id, text).
    open: Option<(String, String)>,
    loaded: bool,
    /// The copy key last pressed, so it says Copied.
    copied: Option<&'static str>,
}

impl SettingsWindow {
    /// Read the log's last lines and the crash reports: when the section shows, and on Refresh.
    pub(super) fn load_diagnostics(&mut self, cx: &mut Context<Self>) {
        let logs = self.daw.update(cx, |daw, cx| {
            daw.request("app.logs", json!({ "lines": LOG_LINES }), cx)
        });
        // No log yet is not an error worth a line in red: the section says so.
        if let Ok(v) = logs {
            self.diag.log = v["lines"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|l| l.as_str().map(str::to_string))
                .collect();
            self.diag.log_file = v["file"].as_str().map(str::to_string);
        }
        match self
            .daw
            .update(cx, |daw, cx| daw.request("app.crashReports", json!({}), cx))
        {
            Ok(v) => {
                self.diag.reports = serde_json::from_value(v["reports"].clone()).unwrap_or_default();
            }
            Err(error) => self.show_error(error, cx),
        }
        self.diag.loaded = true;
        self.diag.copied = None;
        cx.notify();
    }

    fn toggle_report(&mut self, id: String, cx: &mut Context<Self>) {
        if self.diag.open.as_ref().is_some_and(|(open, _)| *open == id) {
            self.diag.open = None;
            cx.notify();
            return;
        }
        let result = self.daw.update(cx, |daw, cx| {
            daw.request("app.crashReports", json!({ "id": id }), cx)
        });
        match result {
            Ok(v) => {
                if let (Some(id), Some(text)) = (v["id"].as_str(), v["text"].as_str()) {
                    self.diag.open = Some((id.to_string(), text.to_string()));
                }
            }
            Err(error) => self.show_error(error, cx),
        }
        cx.notify();
    }

    fn copy(&mut self, key: &'static str, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.diag.copied = Some(key);
        cx.notify();
    }

    fn copy_diagnostics(&mut self, cx: &mut Context<Self>) {
        let result = self
            .daw
            .update(cx, |daw, cx| daw.request("app.diagnostics", json!({}), cx));
        match result {
            Ok(v) => self.copy("copy-diagnostics", summary(&v), cx),
            Err(error) => self.show_error(error, cx),
        }
    }

    fn clear_reports(&mut self, cx: &mut Context<Self>) {
        let result = self
            .daw
            .update(cx, |daw, cx| daw.request("app.clearCrashReports", json!({}), cx));
        match result {
            Ok(_) => {
                self.diag.open = None;
                self.load_diagnostics(cx);
            }
            Err(error) => self.show_error(error, cx),
        }
    }

    /// The section's content, under its heading.
    pub(super) fn diagnostics(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = Theme::get(cx).clone();
        let data = ryolune_engine::host::scan::data_dir();
        let logs = ryolune_engine::diagnostics::logs_dir(&data);
        let crashes = ryolune_engine::diagnostics::crashes_dir(&data);
        let copied = self.diag.copied;

        let mut out = vec![modal::text(
            "Logs and crash reports stay on this computer. Nothing is sent anywhere: copy or attach what you choose to share.",
            cx,
        )
        .into_any_element()];

        // Crash reports.
        let has_reports = !self.diag.reports.is_empty();
        let crashes_folder = crashes.clone();
        out.push(
            modal::field_row(
                "Crash reports",
                Some(SharedString::from(
                    "Written when ryolune runs into a bug, or finds at start that it did not quit properly last time.",
                )),
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(
                        Button::new("crashes-folder", "Show folder")
                            .compact()
                            .ghost()
                            .with_icon("folder")
                            .on_click(move |_, _, _| {
                                let _ = std::fs::create_dir_all(&crashes_folder);
                                crate::settings::reveal(&crashes_folder)
                            }),
                    )
                    .when(has_reports, |d| {
                        d.child(
                            Button::new("crashes-clear", "Delete all")
                                .compact()
                                .danger()
                                .with_icon("trash")
                                .on_click(cx.listener(|this, _, _, cx| this.clear_reports(cx))),
                        )
                    }),
                cx,
            )
            .into_any_element(),
        );
        if !self.diag.loaded {
            out.push(modal::note("Loading…", cx).into_any_element());
        } else if !has_reports {
            out.push(
                modal::note("No crash reports. ryolune has not crashed on this computer.", cx)
                    .into_any_element(),
            );
        }
        for (i, report) in self.diag.reports.clone().into_iter().enumerate() {
            let open = self
                .diag
                .open
                .as_ref()
                .filter(|(id, _)| *id == report.id)
                .map(|(_, text)| text.clone());
            let what = match report.kind.as_str() {
                "unclean" => "Did not quit properly",
                "recovered" => "Recovered from a bug",
                _ => "Crash",
            };
            let color = if report.kind == "crash" {
                theme.danger
            } else {
                theme.warning
            };
            let id = report.id.clone();
            let mut row = div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .py(px(6.0))
                .border_b_1()
                .border_color(theme.hairline)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .child(crate::ui::widgets::icon("warning", 11.0, color))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(1.0))
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(8.0))
                                        .text_size(px(size::BASE))
                                        .text_color(theme.text)
                                        .child(what)
                                        .child(
                                            div()
                                                .font_family(FONT_MONO)
                                                .text_size(px(size::SM))
                                                .text_color(theme.text_3)
                                                .child(report.at.replace('T', " ").replace('Z', " UTC")),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(size::SM))
                                        .text_color(theme.text_2)
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .child(report.summary.clone()),
                                ),
                        )
                        .child(
                            Button::new(("report-view", i), if open.is_some() { "Hide" } else { "View" })
                                .compact()
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_report(id.clone(), cx)
                                })),
                        ),
                );
            if let Some(text) = open {
                let copy_text = text.clone();
                let key = if copied == Some("copy-report") { "Copied" } else { "Copy report" };
                row = row
                    .child(
                        div()
                            .id(("report-text", i))
                            .max_h(px(220.0))
                            .overflow_y_scroll()
                            .child(modal::well(text, cx)),
                    )
                    .child(
                        div().flex().child(
                            Button::new(("report-copy", i), key)
                                .compact()
                                .with_icon("copy")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.copy("copy-report", copy_text.clone(), cx)
                                })),
                        ),
                    );
            }
            out.push(row.into_any_element());
        }

        // This run's log.
        let log_text = if self.diag.log.is_empty() {
            "Nothing in the log yet.".to_string()
        } else {
            self.diag.log.join("\n")
        };
        let copy_log = self.diag.log.join("\n");
        let logs_folder = logs.clone();
        out.push(
            modal::field_row(
                "This run's log",
                Some(SharedString::from(
                    self.diag
                        .log_file
                        .clone()
                        .unwrap_or_else(|| logs.display().to_string()),
                )),
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(
                        Button::new("logs-folder", "Show folder")
                            .compact()
                            .ghost()
                            .with_icon("folder")
                            .on_click(move |_, _, _| crate::settings::reveal(&logs_folder)),
                    )
                    .child(
                        Button::new(
                            "logs-copy",
                            if copied == Some("copy-log") { "Copied" } else { "Copy" },
                        )
                        .compact()
                        .ghost()
                        .with_icon("copy")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.copy("copy-log", copy_log.clone(), cx)
                        })),
                    )
                    .child(
                        Button::new("logs-refresh", "Refresh")
                            .compact()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| this.load_diagnostics(cx))),
                    ),
                cx,
            )
            .into_any_element(),
        );
        out.push(
            div()
                .id("log-tail")
                .max_h(px(180.0))
                .overflow_y_scroll()
                .rounded(px(radius::SM))
                .child(modal::well(log_text, cx))
                .into_any_element(),
        );

        // The data folder.
        let data_folder = data.clone();
        out.push(
            modal::field_row(
                "Data folder",
                Some(SharedString::from(data.display().to_string())),
                Button::new("data-folder", "Show folder")
                    .compact()
                    .ghost()
                    .with_icon("folder")
                    .on_click(move |_, _, _| crate::settings::reveal(&data_folder)),
                cx,
            )
            .into_any_element(),
        );

        // Reporting.
        out.push(
            modal::field_row(
                "Report a problem",
                Some(SharedString::from(
                    "Opens a GitHub issue with the version and system filled in, for you to read and submit. The diagnostics hold no keys, prompts or songs.",
                )),
                div()
                    .flex()
                    .gap(px(6.0))
                    .child(
                        Button::new(
                            "copy-diagnostics",
                            if copied == Some("copy-diagnostics") {
                                "Copied"
                            } else {
                                "Copy diagnostics"
                            },
                        )
                        .compact()
                        .with_icon("copy")
                        .on_click(cx.listener(|this, _, _, cx| this.copy_diagnostics(cx))),
                    )
                    .child(
                        Button::new("report-problem", "Report a Problem…")
                            .compact()
                            .with_icon("external")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.daw.update(cx, |daw, cx| {
                                    daw.fire("app.reportProblem", cx);
                                })
                            })),
                    ),
                cx,
            )
            .into_any_element(),
        );
        out
    }
}

/// `app.diagnostics` as lines a person can paste into an issue.
pub(crate) fn summary(v: &Value) -> String {
    let s = |value: &Value| match value {
        Value::Null => "-".to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let audio = &v["audio"];
    let device = if audio["active"]["device"].is_null() {
        format!("{} (configured)", s(&audio["output"]))
    } else {
        format!(
            "{} at {} Hz, {} frames",
            s(&audio["active"]["device"]),
            s(&audio["active"]["sampleRate"]),
            s(&audio["active"]["bufferFrames"])
        )
    };
    let plugins = &v["plugins"];
    let mut out = format!(
        "ryolune {} ({})\nSystem: {} ({})\nAudio: {}\nPlugins: {} scanned in {} bundles, {} failed, {} stock\nAgent: {}\nLog: {}\n",
        s(&v["version"]),
        s(&v["build"]),
        s(&v["system"]),
        s(&v["arch"]),
        device,
        s(&plugins["scanned"]),
        s(&plugins["bundles"]),
        s(&plugins["failed"]),
        s(&plugins["stock"]),
        s(&v["agentProvider"]),
        s(&v["logFile"]),
    );
    if let Some(reports) = v["crashReports"].as_array().filter(|r| !r.is_empty()) {
        out.push_str("Recent crash reports:\n");
        for r in reports {
            out.push_str(&format!(
                "- {} {}: {}\n",
                s(&r["at"]),
                s(&r["kind"]),
                s(&r["summary"])
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copied_summary_reads_like_a_bug_report() {
        let text = summary(&json!({
            "version": "0.13.0",
            "build": "release",
            "system": "Ubuntu 24.04",
            "arch": "x86_64",
            "audio": {"output": null, "active": {"device": "Speakers", "sampleRate": 48000, "bufferFrames": 512}},
            "plugins": {"scanned": 12, "bundles": 10, "failed": 1, "stock": 30},
            "agentProvider": "codex",
            "logFile": "/tmp/logs/ryolune.log",
            "crashReports": [{"at": "2026-10-06T10:00:00Z", "kind": "unclean", "summary": "Last in its log: x"}],
        }));
        assert!(text.starts_with("ryolune 0.13.0 (release)\nSystem: Ubuntu 24.04 (x86_64)\n"));
        assert!(text.contains("Audio: Speakers at 48000 Hz, 512 frames"));
        assert!(text.contains("- 2026-10-06T10:00:00Z unclean: Last in its log: x"));
    }
}
