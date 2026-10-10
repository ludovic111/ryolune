//! The panel in a GPUI test window over the demo song: what the React tests checked
//! (`AgentPanel.test.tsx`, `AgentFeatures.test.tsx`, `Generation.test.tsx`), without a
//! provider. Nothing here sends a prompt.

use super::*;
use crate::{
    agent::{Entry, Role, ToolRecord},
    app::Ryolune,
    ui::theme::Mode,
};
use gpui::{TestAppContext, VisualTestContext};
use ryolune_engine::store;

fn setup(cx: &mut TestAppContext) -> (Entity<AgentPanel>, Entity<Daw>, &mut VisualTestContext) {
    cx.update(|cx| {
        cx.set_global(Theme::new(Mode::Dark, true));
        crate::ui::widgets::bind(cx);
    });
    let daw = cx.new(|_| Daw::new(Ryolune::from_session(store::demo(), None)));
    let for_panel = daw.clone();
    let (panel, vcx) = cx.add_window_view(move |window, cx| {
        let mut panel = AgentPanel::new(for_panel, window, cx);
        // No connection check in tests: the service is whatever the test says.
        panel.connection_key = Some((false, "codex"));
        panel
    });
    daw.update(vcx, |daw, _| daw.app.agents.open = true);
    vcx.run_until_parked();
    (panel, daw, vcx)
}

fn draft(panel: &Entity<AgentPanel>, cx: &mut VisualTestContext) -> String {
    panel.update(cx, |panel, cx| panel.draft(cx))
}

fn type_and(panel: &Entity<AgentPanel>, text: &str, event: InputEvent, cx: &mut VisualTestContext) {
    let input = panel.read_with(cx, |panel, _| panel.composer.clone());
    input.update(cx, |input, cx| {
        input.set_text(text, cx);
        cx.emit(InputEvent::Changed);
        cx.emit(event);
    });
    cx.run_until_parked();
}

fn sent(daw: &Entity<Daw>, cx: &mut VisualTestContext) -> bool {
    daw.read_with(cx, |daw, _| {
        daw.app.agents.runtime.running() || !daw.app.agents.runtime.transcript.is_empty()
    })
}

#[gpui::test]
fn slash_commands_fill_the_message_or_open_their_tab_without_sending(cx: &mut TestAppContext) {
    let (panel, daw, cx) = setup(cx);
    type_and(&panel, "/diag", InputEvent::Submit, cx);
    assert!(draft(&panel, cx).starts_with("Diagnose this problem"));
    assert!(!sent(&daw, cx));
    // Arrow keys move through the menu; Escape empties a slash word.
    type_and(&panel, "/r", InputEvent::Changed, cx);
    panel.update(cx, |panel, cx| assert!(panel.slash_step(true, cx)));
    assert_eq!(panel.read_with(cx, |p, _| p.slash_index), 1);
    type_and(&panel, "/r", InputEvent::Cancel, cx);
    assert_eq!(draft(&panel, cx), "");
    type_and(&panel, "/generate", InputEvent::Submit, cx);
    assert_eq!(panel.read_with(cx, |p, _| p.tab), Tab::Generate);
    assert_eq!(draft(&panel, cx), "");
    assert!(!sent(&daw, cx));
}

#[gpui::test]
fn nothing_is_sent_until_a_service_is_connected(cx: &mut TestAppContext) {
    let (panel, daw, cx) = setup(cx);
    panel.update(cx, |panel, cx| {
        panel.connection = Some(Connection {
            provider: "codex".into(),
            state: "missingCli".into(),
            message: "Install the companion.".into(),
        });
        cx.notify();
    });
    type_and(&panel, "Une mélodie", InputEvent::Submit, cx);
    assert!(!sent(&daw, cx));
    assert_eq!(
        draft(&panel, cx),
        "Une mélodie",
        "the message stays in the box"
    );
    // A starter fills the message; it is never sent by itself.
    panel.update(cx, |panel, cx| panel.set_draft(chat_starter(), cx));
    assert!(draft(&panel, cx).contains("four-bar drum groove"));
    assert!(!sent(&daw, cx));
}

fn chat_starter() -> &'static str {
    "Add a four-bar drum groove at the current tempo. Keep the rest of my project."
}

#[gpui::test]
fn asking_about_the_selection_opens_the_panel_on_the_chat(cx: &mut TestAppContext) {
    let (panel, daw, cx) = setup(cx);
    daw.update(cx, |daw, _| daw.app.agents.open = false);
    panel.update(cx, |panel, cx| {
        panel.with_context = false;
        panel.tab = Tab::Changes;
        cx.notify();
    });
    cx.run_until_parked();
    panel.update_in(cx, |panel, window, cx| {
        panel.ask_about_selection(window, cx)
    });
    cx.run_until_parked();
    assert!(daw.read_with(cx, |daw, _| daw.app.agents.open));
    panel.read_with(cx, |panel, _| {
        assert_eq!(panel.tab, Tab::Chat);
        assert!(panel.with_context);
    });
    // The draft survives collapsing and reopening.
    type_and(&panel, "Do not lose my idea", InputEvent::Changed, cx);
    daw.update(cx, |daw, _| daw.app.agents.open = false);
    cx.run_until_parked();
    daw.update(cx, |daw, _| daw.app.agents.open = true);
    cx.run_until_parked();
    assert_eq!(draft(&panel, cx), "Do not lose my idea");
}

#[gpui::test]
fn creates_a_protected_original_before_a_variation(cx: &mut TestAppContext) {
    let (panel, daw, cx) = setup(cx);
    panel.update(cx, |panel, cx| panel.select_tab(Tab::Takes, cx));
    cx.run_until_parked();
    panel.update(cx, |panel, cx| panel.create_take(cx));
    cx.run_until_parked();
    let list = daw.update(cx, |daw, cx| {
        daw.request("take.list", serde_json::json!({}), cx).unwrap()
    });
    let names: Vec<&str> = list["takes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Original", "Variation 1"]);
    assert_eq!(panel.read_with(cx, |p, _| p.tab), Tab::Chat);
    assert!(draft(&panel, cx).contains("original version is preserved"));
}

#[gpui::test]
fn conversations_are_kept_switched_and_deleted_and_the_music_stays(cx: &mut TestAppContext) {
    let (panel, daw, cx) = setup(cx);
    let tracks = daw.read_with(cx, |daw, _| daw.app.store.session().tracks.len());
    daw.update(cx, |daw, _| {
        daw.app.conversation_started("An idea");
        daw.app.agents.runtime.transcript.push(Entry {
            role: Role::User,
            text: "An idea".into(),
            tool: None,
            streaming: false,
        })
    });
    let first = daw.read_with(cx, |daw, _| daw.app.agents.conversations.thread.id.clone());
    // + keeps this one and opens an empty one.
    daw.update(cx, |daw, cx| daw.fire("agent.newConversation", cx));
    cx.run_until_parked();
    assert!(daw.read_with(cx, |daw, _| daw.app.agents.runtime.transcript.is_empty()));
    let listed = daw.update(cx, |daw, cx| {
        daw.request("agent.conversations", serde_json::json!({}), cx)
            .unwrap()
    });
    assert_eq!(listed["conversations"].as_array().unwrap().len(), 2);
    daw.update(cx, |daw, cx| {
        daw.run(
            "agent.selectConversation",
            serde_json::json!({ "id": first }),
            cx,
        )
    });
    cx.run_until_parked();
    assert_eq!(
        daw.read_with(cx, |daw, _| daw
            .app
            .agents
            .conversations
            .thread
            .title
            .clone()),
        "An idea"
    );
    // Project memory and renaming go through the editor under the header.
    panel.update_in(cx, |panel, window, cx| {
        panel.edit(Editing::Memory, window, cx);
        panel
            .memory_input
            .update(cx, |input, cx| input.set_text("Stay in D minor.", cx));
        panel.save_editing(cx);
        panel.edit(Editing::Title, window, cx);
        panel
            .title_input
            .update(cx, |input, cx| input.set_text("Ideas", cx));
        panel.save_editing(cx);
    });
    cx.run_until_parked();
    daw.read_with(cx, |daw, _| {
        assert_eq!(daw.app.agents.conversations.memory, "Stay in D minor.");
        assert_eq!(daw.app.agents.conversations.thread.title, "Ideas");
    });
    // Delete, confirmed under the header.
    panel.update(cx, |panel, cx| {
        panel.confirm_delete = true;
        panel.delete_conversation(cx);
    });
    cx.run_until_parked();
    assert_ne!(
        daw.read_with(cx, |daw, _| daw.app.agents.conversations.thread.id.clone()),
        first
    );
    assert_eq!(
        daw.read_with(cx, |daw, _| daw.app.store.session().tracks.len()),
        tracks
    );
}

#[gpui::test]
fn while_the_agent_works_enter_steers_it(cx: &mut TestAppContext) {
    let (panel, daw, cx) = setup(cx);
    let complete = daw.update(cx, |daw, _| daw.app.agents.mock_running_task());
    type_and(&panel, "Slower, please", InputEvent::Submit, cx);
    assert_eq!(draft(&panel, cx), "", "the steering left the box");
    daw.read_with(cx, |daw, _| {
        let runtime = &daw.app.agents.runtime;
        assert_eq!(runtime.pending_steering(), 1);
        assert_eq!(runtime.transcript.last().unwrap().text, "Slower, please");
    });
    complete();
    daw.update(cx, |daw, _| daw.app.run_agent_tools());
    // Never read: the person is told to send it again.
    daw.read_with(cx, |daw, _| {
        let last = daw.app.agents.runtime.transcript.last().unwrap();
        assert!(last.text.contains("before reading your steering"));
    });
}

/// Every tab draws over a conversation with messages, steps, a failure and a streaming reply,
/// and over real Changes, in both modes.
#[gpui::test]
fn every_tab_draws(cx: &mut TestAppContext) {
    let (panel, daw, cx) = setup(cx);
    daw.update(cx, |daw, _| {
        // From the CLI, so it lands in Changes.
        daw.app
            .run_control_command(
                "track.rename",
                &serde_json::json!({"trackId": "bass", "name": "Deep"}),
                false,
                "CLI",
            )
            .unwrap();
        let tool = |ok: bool| Entry {
            role: Role::Tool,
            text: String::new(),
            tool: Some(ToolRecord {
                name: "clip.create".into(),
                args: serde_json::json!({"notes": [1, 2]}),
                result: Some(if ok {
                    Ok(serde_json::json!({}))
                } else {
                    Err("Track is full".into())
                }),
                sequence: None,
            }),
            streaming: false,
        };
        let runtime = &mut daw.app.agents.runtime;
        runtime.transcript.push(Entry {
            role: Role::User,
            text: format!(
                "Add drums{}track \"Keys\" (trackId t1, instrument)]",
                context::CONTEXT_MARK
            ),
            tool: None,
            streaming: false,
        });
        runtime.transcript.push(tool(true));
        runtime.transcript.push(tool(false));
        runtime.transcript.push(Entry {
            role: Role::Assistant,
            text: "**Done.** See [the guide](https://lsuite.xyz/ryolune).\n\n- one\n- two".into(),
            tool: None,
            streaming: true,
        });
    });
    for mode in [Mode::Dark, Mode::Light] {
        cx.update(|_, cx| cx.set_global(Theme::new(mode, false)));
        for tab in [Tab::Chat, Tab::Generate, Tab::Changes, Tab::Takes] {
            panel.update(cx, |panel, cx| {
                panel.open_steps.insert((1, 1));
                panel.select_tab(tab, cx);
            });
            cx.run_until_parked();
        }
        // The rail.
        daw.update(cx, |daw, _| daw.app.agents.open = false);
        cx.run_until_parked();
        daw.update(cx, |daw, _| daw.app.agents.open = true);
    }
    panel.update(cx, |panel, cx| {
        panel.models.open = true;
        panel.select_tab(Tab::Chat, cx);
    });
    cx.run_until_parked();
}
