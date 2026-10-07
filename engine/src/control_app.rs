//! Registry commands that give the CLI, MCP and agents the rest of the window: view state,
//! region tools, track duplication, presets, preferences, audio devices, interface actions
//! and application/agent control. Everything that needs a window goes through
//! [`Host::live`]; the rest works on files too.

use crate::{
    control::{self, edit, opt, query, req, selection, Args, Host, Kind, Spec, CLIP_ID, TRACK_ID},
    host as plugin_host,
    model::*,
    preset::{self, PluginPreset},
    recovery, settings,
    store::Command,
    Result,
};
use serde_json::{json, Value};
use std::path::PathBuf;

const SLOT: control::Param = opt(
    "slot",
    Kind::Integer,
    "Insert slot 0-7. Omit for the instrument.",
);

pub const SPECS: &[Spec] = &[
    edit("rhythm.create", "Create a Euclidean drum groove on a new Drum Machine track, in one undo step. Each lane has its own subdivision per bar.", &[req("lanes", Kind::Array, "1-8 objects with steps (1-64), pulses (0-steps), rotation (0-steps-1), pitch (0-127), velocity (1-127)."), req("bars", Kind::Integer, "Groove length, 1-16 bars."), opt("startBar", Kind::Number, "Arrangement start, default zero."), opt("name", Kind::String, "Groove and track name.")]),
    edit("clip.humanize", "Humanize MIDI timing and velocity reproducibly without changing pitch, inside region bounds. One undo step.", &[CLIP_ID, opt("timingMs", Kind::Number, "Maximum timing offset, 0-100 ms (default 10)."), opt("velocity", Kind::Integer, "Maximum velocity offset, 0-32 (default 8)."), opt("seed", Kind::Integer, "Random seed, 0-4294967295 (default 1).")]),
    edit("clip.velocityRamp", "Shape MIDI dynamics from the first to last onset, preserving chords at equal velocity. One undo step.", &[CLIP_ID, req("from", Kind::Integer, "Starting velocity 1-127."), req("to", Kind::Integer, "Ending velocity 1-127.")]),
    edit("clip.fitScale", "Move MIDI pitches to the closest note in a scale, choosing down on ties. Timing and velocity stay intact.", &[CLIP_ID, req("root", Kind::Integer, "Root pitch class 0-11, C=0."), req("scale", Kind::String, "major, minor, dorian, mixolydian, pentatonicMajor or pentatonicMinor.")]),
    edit("clip.reverseMidi", "Reverse MIDI note timing within the region, preserving pitch, duration and velocity.", &[CLIP_ID]),
    edit("clip.legato", "Extend MIDI notes to the next distinct onset or region end. Simultaneous chord notes remain together.", &[CLIP_ID]),
    edit("clip.repeat", "Repeat a MIDI or audio region immediately after itself in one undo step, assigning unique IDs.", &[CLIP_ID, req("count", Kind::Integer, "Number of additional copies, 1-64.")]),
    query("take.list", "List creative takes saved inside this project, including the active one.", &[]),
    edit("take.create", "Save the current music as a named creative take. Create Original then Variation before experimenting; edits follow the active take. Up to eight takes travel with the saved project.", &[req("name", Kind::String, "Take name, 1-120 characters.")]),
    edit("take.select", "Switch to a creative take, preserving edits in the current take. Stops playback; one Undo restores the previous arrangement.", &[req("id", Kind::String, "Take ID from take.list.")]),
    edit("take.remove", "Remove an inactive creative take. The active arrangement is preserved; undoable.", &[req("id", Kind::String, "Inactive take ID from take.list.")]),
    query("view.get", "Read the view: zoom in pixels per bar, first visible bar, the lane width in pixels, follow mode, editor mode, the clip open in the editor, the piano roll's lowest pitch, and the browser's tab and selected row.", &[]),
    edit("view.fit", "Zoom the arrangement so the whole song, plus one bar, spans the lanes, and scroll to the first bar. Uses the lane width from view.get.", &[]),
    edit("view.set", "Change the arrangement view and the editor. Omitted fields keep their values. Not an undo step.", &[
        opt("pixelsPerBar", Kind::Number, "Arrangement zoom, 12-480 pixels per bar."),
        opt("scrollBar", Kind::Number, "First visible bar, zero-based."),
        opt("followPlayhead", Kind::Boolean, "Scroll with the playhead while playing."),
        opt("editorMode", Kind::String, "pianoRoll, score or step."),
        opt("editorClipId", Kind::String, "Clip to open in the editor; an empty string closes it."),
        opt("editorLowPitch", Kind::Integer, "Lowest MIDI pitch the piano roll shows, 0-108: its vertical scroll. -1 lets it frame the open clip again."),
        opt("browserTab", Kind::String, "instruments, loops, plugins or files."),
        opt("browserSelection", Kind::String, "Name of the browser row to select; an empty string clears it."),
        opt("laneWidth", Kind::Number, "Width of the arrangement lanes in pixels, 50-20000. The window reports it as it resizes; scripts rarely need to."),
    ]),
    edit("rhythm.preview", "Render a Euclidean groove to a WAV at the session's tempo and meter without creating anything: hear it before rhythm.create. In the app it runs as a job and answers when the file is written.", &[req("lanes", Kind::Array, "As for rhythm.create."), req("bars", Kind::Integer, "1-4 bars, at most 30 seconds."), opt("path", Kind::String, "Destination .wav; defaults to one preview file in the data folder that each preview replaces."), opt("inline", Kind::Boolean, "Also return the file as wavBase64 (default false).")]),
    edit("clip.quantize", "Snap every note start in a MIDI clip to the grid, in one undo step.", &[
        CLIP_ID,
        opt("division", Kind::Integer, "Notes per bar: 1, 2, 4, 8, 16, 32 or 64. Defaults to the transport snap."),
        opt("strength", Kind::Number, "How far to move toward the grid, 0-100 percent (default 100)."),
        opt("lengths", Kind::Boolean, "Also quantize note lengths (default false)."),
    ]),
    edit("clip.transpose", "Shift every note in a MIDI clip by semitones, clamped to 0-127.", &[
        CLIP_ID,
        req("semitones", Kind::Integer, "Signed semitones, -48 to 48."),
    ]),
    edit("track.duplicate", "Copy a track with its strip and clips right after the original.", &[
        TRACK_ID,
        opt("name", Kind::String, "Name for the copy. Defaults to the original name plus \" copy\"."),
    ]),
    edit("strip.moveInsert", "Move an insert to another slot on the same strip, shifting the others.", &[
        TRACK_ID,
        req("from", Kind::Integer, "Slot 0-7 to move."),
        req("to", Kind::Integer, "Destination slot 0-7."),
    ]),
    query("plugin.describe", "Describe an installed plugin without placing it: format, vendor, category, latency, whether it has its own window, its factory programs and ryolune presets, and its parameters with ids, ranges, units, defaults as displayed and whether they can be automated.", &[
        req("pluginId", Kind::String, "Descriptor ID from plugin.list (stock:Space, vst3:…), or the plugin's name."),
        opt("query", Kind::String, "Only parameters whose name matches these words."),
        opt("limit", Kind::Integer, "Parameters to return, 1-10000, default 200."),
    ]),
    query("preset.list", "List factory and user presets, optionally for one plugin.", &[
        opt("pluginId", Kind::String, "Only presets for this plugin ID."),
    ]),
    edit("preset.save", "Save the plugin in a slot as a named preset: its parameters and, for external plugins, its captured state.", &[
        TRACK_ID, SLOT,
        req("name", Kind::String, "Preset name, 1-120 characters."),
    ]),
    edit("preset.load", "Apply a preset to the plugin in a slot, in one undo step. The preset must belong to the same plugin.", &[
        TRACK_ID, SLOT,
        req("name", Kind::String, "Preset name from preset.list."),
    ]),
    edit("preset.delete", "Delete a user preset. Factory presets cannot be deleted.", &[
        req("pluginId", Kind::String, "Plugin ID the preset belongs to."),
        req("name", Kind::String, "Preset name."),
    ]),
    query("settings.get", "Read preferences with secrets masked, or one dotted path such as agent.model.", &[
        opt("path", Kind::String, "Dotted setting path. Omit for everything."),
    ]),
    edit("settings.set", "Change one preference and save it. The running window applies it at once.", &[
        req("path", Kind::String, "Dotted setting path, for example agent.provider or audio.outputDevice."),
        req("value", Kind::Any, "New value: string, number, boolean, list or null. Strings are converted for numbers and booleans."),
    ]),
    edit("settings.reset", "Reset one preference, a section, or everything to the defaults.", &[
        opt("path", Kind::String, "Dotted path or section name. Omit to reset everything."),
    ]),
    query("audio.devices", "List output devices, input devices and MIDI input ports, with the configured and, in live mode, the active selection.", &[]),
    query("audio.status", "The audio engine: device, sample rate, CPU load, master and selected-track peaks, MIDI port and live notes. `monitoring` reports input monitoring: state (off, on, blocked, failed), the input device and rate, the measured input and output buffer sizes, frames waiting in the ring, latencyMs computed from them, and frames dropped or underrun.", &[]),
    edit("audio.allowSpeakerMonitoring", "Answer the feedback warning: monitoring the built-in microphone through the built-in speakers howls, so it stays muted (audio.status monitoring.state = blocked) until this is called with allow=true. Lasts until the app closes.", &[
        req("allow", Kind::Boolean, "true to monitor anyway, false to mute it again."),
    ]),
    edit("audio.setOutput", "Switch the output device and reconnect. Omit name for the system default.", &[
        opt("name", Kind::String, "Output device name from audio.devices."),
    ]),
    edit("audio.setInput", "Choose the recording input. Omit name for the system default.", &[
        opt("name", Kind::String, "Input device name from audio.devices."),
    ]),
    edit("audio.setMidiInput", "Connect a MIDI input port for live playing and recording. Omit port to disconnect.", &[
        opt("port", Kind::String, "MIDI port name from audio.devices."),
    ]),
    edit("audio.reconnect", "Reopen the output device, for example after it was unplugged.", &[]),
    edit("note.preview", "Audition one note on a track's instrument, like clicking a piano-roll key.", &[
        TRACK_ID,
        req("pitch", Kind::Integer, "MIDI pitch 0-127."),
        opt("velocity", Kind::Integer, "1-127, default 100."),
    ]),
    edit("note.hold", "Hold or release a note on the selected instrument track, like a key on a MIDI keyboard. Held notes are recorded when the transport is recording. Always release what you hold.", &[
        req("pitch", Kind::Integer, "MIDI pitch 0-127."),
        req("on", Kind::Boolean, "true presses the key, false releases it."),
        opt("velocity", Kind::Integer, "1-127, default 100."),
    ]),
    edit("note.releaseAll", "Release every note held with note.hold or musical typing.", &[]),
    edit("transport.punch", "Turn record on or off. While the transport is rolling this punches in or out on the armed tracks without stopping playback; while stopped it only sets the record button.", &[
        req("enabled", Kind::Boolean, "Record on or off."),
    ]),
    edit("ui.screenshot", "Capture the window to a PNG so an agent can see the interface. Returns the file path and size.", &[
        opt("path", Kind::String, "Destination .png. Defaults to a timestamped file in the app data directory."),
    ]),
    edit("ui.showPanel", "Show or hide an interface panel: agent, automation, mixer (every channel, in place of the region editor), controllers (the controller lane under the piano roll), tempo (the tempo track under the ruler), palette (the command palette), settings, plugins (the Plugins window: stock, installed, formats, build with your agent), help, export, recovery, whatsNew (the release notes of this version), diagnostics (Settings › Diagnostics), or master / bus-a / bus-b in the inspector.", &[
        req("panel", Kind::String, "agent, automation, mixer, controllers, tempo, palette, settings, plugins, help, export, recovery, whatsNew, diagnostics, master, bus-a or bus-b."),
        opt("visible", Kind::Boolean, "Show (default) or hide."),
        opt("section", Kind::String, "Settings section: general, audio, interface, agent, generation, plugins, control, updates, diagnostics or about. Plugins window part: stock, installed, formats or build."),
    ]),
    edit("ui.openPluginWindow", "Open a plugin's parameter panel in the window, or its native editor with native=true.", &[
        TRACK_ID, SLOT,
        opt("native", Kind::Boolean, "Open the plugin's own editor window when it has one."),
    ]),
    edit("ui.closePluginWindow", "Close one plugin panel and its native editor window. ui.state lists the open ones.", &[
        req("id", Kind::String, "Window id from ui.status pluginWindows."),
    ]),
    edit("ui.dismissError", "Dismiss the error shown in the window.", &[]),
    edit("ui.closePluginWindows", "Close every plugin panel and native editor.", &[]),
    edit("ui.musicalTyping", "Turn musical typing (the computer keyboard as a piano) on or off.", &[
        req("enabled", Kind::Boolean, "On or off."),
    ]),
    edit("ui.setTool", "Choose the arrangement tool, like keys 1-3: pointer selects and drags, pencil draws clips, scissors splits.", &[
        req("tool", Kind::String, "pointer, pencil or scissors."),
    ]),
    query("ui.status", "Window state: open panels, tool, musical typing, plugin panels, status line and any error being shown.", &[]),
    query("app.info", "Version, platform, executable, data and settings paths, the control discovery file and the host mode.", &[]),
    edit("app.checkUpdates", "Check GitHub for a newer release and report it.", &[]),
    edit("app.installUpdate", "Download, verify and install the available update. Relaunching is confirmed in the window.", &[]),
    edit("app.quit", "Ask the window to quit. Unsaved changes prompt in the window unless discard is true.", &[
        opt("discard", Kind::Boolean, "Quit without saving (default false)."),
    ]),
    edit("app.confirm", "Answer the unsaved-changes prompt the window shows before New, Open, Quit or Relaunch. ui.status reports it as `prompt`.", &[
        req("choice", Kind::String, "save, discard or cancel."),
    ]),
    edit("app.openGuide", "Open one of ryolune's pages, or a sound service's key page, in the web browser.", &[
        req("guide", Kind::String, "plugins: writing native plugins with the Rust SDK. support: donate to ryolune, once or monthly (optional, unlocks nothing). elevenlabs, stability, fal: where to get that service's API key. custom: the contract a custom generation endpoint follows."),
    ]),
    edit("app.relaunch", "Relaunch the app, for example after an update was installed. Unsaved changes prompt first.", &[]),
    edit("session.saveRecoveredTake", "Write a recording that could not be placed on a track to a WAV file, which frees the window to open other sessions. ui.status reports it as `recoveredTake`.", &[
        req("path", Kind::String, "Destination .wav."),
    ]),
    query("session.snapshots", "List recovery snapshots newest first, with paths, titles, times and sizes.", &[]),
    edit("session.restoreSnapshot", "Open a recovery snapshot in the window as an unsaved copy.", &[
        req("path", Kind::String, "Snapshot path from session.snapshots."),
    ]),
    query("agent.status", "The built-in agent: provider, model, whether a task is running, turn count and last reply.", &[]),
    edit("agent.configure", "Select the agent provider, model and reasoning effort together. Only while idle.", &[
        req("provider", Kind::String, "lsuite (lsuite AI, after account.signIn), codex, claude, anthropic, openai, gemini, openrouter, mistral, groq, deepseek, xai, ollama, lmstudio, compatible or zenith."),
        req("model", Kind::String, "Model ID; empty uses the provider default."),
        req("reasoningEffort", Kind::String, "Provider effort level; empty uses its default."),
    ]),
    query("agent.providers", "Available agent providers and whether each is configured.", &[]),
    query("agent.mcp", "How to connect an outside agent to this window over MCP: the ryolune-mcp command, its environment, whether the bridge is on, and a ready configuration for Claude Code, Codex, Cursor, VS Code, Claude Desktop, Gemini CLI, Windsurf, opencode, Zed and any other MCP client.", &[]),
    edit("agent.openClient", "Open an outside agent's install link with ryolune's MCP server filled in (Cursor and VS Code install from a link; the app asks before adding it). Only a person can do this.", &[
        req("client", Kind::String, "cursor or vscode."),
    ]),
    query("agent.models", "Discover the models each connected provider offers, grouped by provider. Asks the providers over the network, so it runs as a job and answers when they have.", &[]),
    query("agent.connection", "Check that the configured agent provider can be reached and is signed in: provider, state and a message. Runs as a job, like agent.models.", &[]),
    edit("agent.send", "Send a prompt to the built-in agent panel, like typing in the window.", &[
        req("prompt", Kind::String, "The request, in plain language."),
    ]),
    edit("agent.stop", "Stop the running agent task; finished edits stay in Undo.", &[]),
    query("agent.transcript", "The agent conversation: user, assistant and tool entries.", &[
        opt("limit", Kind::Integer, "Newest entries to return, default 40."),
    ]),
    query("agent.changes", "What the agent changed, one entry per command: sequence, title, the command as typed, its output, and whether it is currently applied.", &[]),
    edit("agent.revert", "Undo back to just before one agent change, or redo up to it. Same as the buttons in the panel's Changes tab.", &[
        req("sequence", Kind::Integer, "Change sequence from agent.changes."),
        opt("redo", Kind::Boolean, "Redo up to the change instead of undoing it (default false)."),
    ]),
    edit("agent.revertTurn", "Undo the built-in agent's whole last turn in one step, back to the checkpoint taken before its first edit (or, with redo, bring the turn back). Same as Revert turn in the panel's Changes tab; agent.status lists the turn's changes.", &[
        opt("redo", Kind::Boolean, "Bring a reverted turn back instead (default false)."),
    ]),
    edit("agent.clear", "Clear the agent conversation; the edits it made stay in Undo.", &[]),
    query("app.logs", "The last lines of ryolune's log (this run's by default, or an earlier run's), with the log folder and every log file. Logs stay on this computer and never hold API keys.", &[
        opt("lines", Kind::Integer, "Lines from the end, 1-2000 (default 100)."),
        opt("file", Kind::String, "A log file name from `files`, such as ryolune.1.log for the run before."),
    ]),
    query("app.crashReports", "Crash reports newest first: panics that stopped ryolune (crash), panics a background job survived (recovered) and runs that ended without quitting (unclean). With id, one report's full text.", &[
        opt("id", Kind::String, "A report's file name from the list, to read its text."),
    ]),
    edit("app.clearCrashReports", "Delete every crash report in the crashes folder. Logs and recovery snapshots are kept.", &[]),
    query("app.diagnostics", "What a bug report needs: version and build, system, audio device, plugin scan summary, folders, the log file, counts and recent crash reports. Holds no API keys, prompts or songs.", &[]),
    edit("app.reportProblem", "Open a new GitHub issue for ryolune in the web browser, with the version, the system and the last crash's summary filled in. Nothing is sent: the person reads and submits it. Only a person can do this.", &[]),
    query("app.whatsNew", "Release notes built into this copy, newest first, in Markdown: this version's by default, one version's (version), every release after one (since), or all of them (all). ui.showPanel panel=whatsNew shows them in the window.", &[
        opt("version", Kind::String, "One release, such as 0.12.0."),
        opt("since", Kind::String, "Every release after this version, up to this copy."),
        opt("all", Kind::Boolean, "Every release this copy carries (default false)."),
    ]),
    query("agent.conversations", "The agent's saved conversations for the open song, newest first: id, title, when it last changed, how many requests and which one is open; also the size of the song's project memory and any error saving them.", &[]),
    edit("agent.newConversation", "Start a new agent conversation for this song; the open one is kept and agent.selectConversation goes back to it. Only while the agent is idle.", &[]),
    edit("agent.selectConversation", "Open one of the song's saved agent conversations in the panel, as agent.conversations lists them. Only while the agent is idle.", &[
        req("id", Kind::String, "Conversation id from agent.conversations."),
    ]),
    edit("agent.renameConversation", "Rename an agent conversation (the open one by default). New conversations are titled from their first request.", &[
        req("title", Kind::String, "The new title, 1 to 120 characters on one line."),
        opt("id", Kind::String, "Conversation id from agent.conversations; default the open one."),
    ]),
    edit("agent.deleteConversation", "Delete one of the song's agent conversations for good (its edits stay in the song). Deleting the open one opens the newest other. Only a person can do this.", &[
        req("id", Kind::String, "Conversation id from agent.conversations."),
    ]),
    query("agent.memory", "The song's project memory: notes the person keeps for the agent (style, key, what to avoid), sent ahead of every request to every provider.", &[]),
    edit("agent.setMemory", "Replace the song's project memory, at most 32 KB; empty clears it. It goes ahead of every request as user-maintained context, so only a person can change it.", &[
        req("text", Kind::String, "The whole memory text, plain language."),
    ]),
    edit("agent.steer", "Steer the agent while it works: the text joins the conversation now and reaches the agent at its next step, after the tool calls under way, instead of stopping it (Claude Code restarts its run with it).", &[
        req("text", Kind::String, "What to change or add, in plain language."),
    ]),
];

/// Commands that only a window can serve.
pub fn is_live_only(name: &str) -> bool {
    matches!(name.split('.').next().unwrap_or(""), "ui" | "agent")
        || matches!(
            name,
            "audio.status"
                | "audio.allowSpeakerMonitoring"
                | "audio.setOutput"
                | "audio.setInput"
                | "audio.setMidiInput"
                | "audio.reconnect"
                | "note.preview"
                | "note.hold"
                | "note.releaseAll"
                | "transport.punch"
                | "app.confirm"
                | "app.openGuide"
                | "app.relaunch"
                | "generate.audio"
                | "session.saveRecoveredTake"
                | "app.checkUpdates"
                | "app.installUpdate"
                | "app.quit"
                | "session.restoreSnapshot"
                | "app.reportProblem"
        )
}

/// Agent permission check from Settings > Agent. `None` when a command is allowed.
/// `denied_for_agent` for a whole request: commands that write only to ryolune's own data
/// folder by default count as file operations when they are given a `path`.
pub fn denied_for_agent_request(
    name: &str,
    params: &serde_json::Value,
    permissions: &settings::Permissions,
) -> Option<String> {
    if matches!(
        name,
        "rhythm.preview" | "ui.screenshot" | "strip.loadSample" | "harness.look"
    ) && params.get("path").is_some_and(|p| !p.is_null())
        && !permissions.file_operations
    {
        return Some(format!(
            "{name} with a path is not allowed for agents: file operations is off in Settings > Agent (fileOperations). Omit path to use the default location."
        ));
    }
    denied_for_agent(name, permissions)
}

pub fn denied_for_agent(name: &str, permissions: &settings::Permissions) -> Option<String> {
    let deny = |what: &str, setting: &str| {
        Some(format!(
            "{name} is not allowed for agents: {what} is off in Settings > Agent ({setting})."
        ))
    };
    if let Some(denied) = crate::control_interop::denied_for_agent(name, permissions) {
        return Some(denied);
    }
    if let Some(denied) = crate::control_account::denied_for_agent(name, permissions) {
        return Some(denied);
    }
    if let Some(denied) = crate::plugin_dev::denied_for_agent(name, permissions) {
        return Some(denied);
    }
    match name {
        "session.new" | "session.open" | "session.restoreSnapshot"
            if !permissions.replace_session =>
        {
            deny("replacing the session", "replaceSession")
        }
        "session.save"
        | "session.bounce"
        | "session.importAudio"
        | "session.importMidi"
        | "session.exportMidi"
        | "session.exportAudio"
        | "session.exportStems"
        | "export.toKimchi"
        | "session.scoreCut"
        | "plugin.scan"
        | "preset.save"
        | "preset.delete"
        | "session.saveRecoveredTake"
        | "plugin.scaffold"
        | "plugin.install"
            if !permissions.file_operations =>
        {
            deny("file operations", "fileOperations")
        }
        "transport.play"
        | "transport.record"
        | "transport.stop"
        | "transport.locate"
        | "transport.returnToStart"
        | "marker.goto"
        | "marker.next"
        | "marker.previous"
        | "note.preview"
        | "note.hold"
        | "transport.punch"
            if !permissions.transport =>
        {
            deny("transport control", "transport")
        }
        "settings.set"
        | "settings.reset"
        | "audio.setOutput"
        | "audio.setInput"
        | "audio.setMidiInput"
        | "audio.allowSpeakerMonitoring"
            if !permissions.settings =>
        {
            deny("changing settings", "settings")
        }
        "app.quit" | "app.installUpdate" | "app.relaunch" | "app.confirm"
            if !permissions.app_control =>
        {
            deny("application control", "appControl")
        }
        "generate.audio" if !permissions.generation => deny("sound generation", "generation"),
        "generate.delete" if !permissions.file_operations => {
            deny("file operations", "fileOperations")
        }
        "app.clearCrashReports" if !permissions.file_operations => {
            deny("file operations", "fileOperations")
        }
        _ => None,
    }
}

pub(crate) fn call(host: &mut dyn Host, name: &str, a: &Args, agent: bool) -> Result<Value> {
    if is_live_only(name) {
        return host.live(name, &args_value(a));
    }
    match name {
        "rhythm.create" => crate::rhythm::call(host, a, agent),
        "rhythm.preview" => crate::rhythm::preview(host, &args_value(a)),
        "clip.humanize" | "clip.velocityRamp" | "clip.fitScale" | "clip.reverseMidi"
        | "clip.legato" | "clip.repeat" => crate::midi_tools::call(host, name, a, agent),
        "take.list" | "take.create" | "take.select" | "take.remove" => {
            crate::takes::call(host, name, a)
        }
        "view.get" => Ok(view_json(host)),
        "view.set" => {
            let mut view = host.store().session().view.clone();
            if let Some(v) = a.opt_f64("pixelsPerBar") {
                if !(12.0..=480.0).contains(&v) {
                    return Err("pixelsPerBar must be between 12 and 480".into());
                }
                view.pixels_per_bar = v as f32;
            }
            if let Some(v) = a.opt_f64("scrollBar") {
                if !valid_time(v) {
                    return Err("scrollBar must be between 0 and 1,000,000".into());
                }
                view.scroll_bars = v;
            }
            if let Some(v) = a.opt_bool("followPlayhead") {
                view.follow_playhead = v;
            }
            if let Some(mode) = a.opt_str("editorMode") {
                if !["pianoRoll", "score", "step"].contains(&mode) {
                    return Err("editorMode must be pianoRoll, score or step".into());
                }
                view.editor_mode = mode.into();
            }
            if let Some(low) = a.opt_int("editorLowPitch") {
                if !(-1..=108).contains(&low) {
                    return Err(
                        "editorLowPitch must be between 0 and 108, or -1 for automatic".into(),
                    );
                }
                view.editor_low_pitch = u8::try_from(low).ok();
            }
            if let Some(tab) = a.opt_str("browserTab") {
                if !crate::model::BROWSER_TABS.contains(&tab) {
                    return Err("browserTab must be instruments, loops, plugins or files".into());
                }
                if view.browser_tab != tab {
                    view.browser_selection = None;
                }
                view.browser_tab = tab.into();
            }
            if let Some(row) = a.opt_str("browserSelection") {
                if row.chars().count() > 200 {
                    return Err("browserSelection is at most 200 characters".into());
                }
                view.browser_selection = (!row.is_empty()).then(|| row.to_string());
            }
            if let Some(width) = a.opt_f64("laneWidth") {
                if !(50.0..=20000.0).contains(&width) {
                    return Err("laneWidth must be between 50 and 20000 pixels".into());
                }
                host.set_lane_width(width);
            }
            if let Some(clip) = a.opt_str("editorClipId") {
                if clip.is_empty() {
                    view.editor_clip_id = None;
                } else {
                    let clip = control::find_clip(host.store().session(), clip)?;
                    view.selected_track_id = Some(clip.track_id.clone());
                    view.selected_clip_id = Some(clip.id.clone());
                    view.editor_clip_id = Some(clip.id.clone());
                    view.selected_note_id = None;
                }
            }
            host.dispatch(Command::SetView(view))?;
            host.view_changed();
            Ok(view_json(host))
        }
        "view.fit" => {
            let mut view = host.store().session().view.clone();
            let end = host
                .store()
                .session()
                .clips
                .iter()
                .map(|c| c.start_bar + c.length_bars)
                .fold(8.0, f64::max)
                + 1.0;
            view.pixels_per_bar = (host.lane_width() / end).clamp(12.0, 480.0) as f32;
            view.scroll_bars = 0.0;
            host.dispatch(Command::SetView(view))?;
            host.view_changed();
            Ok(view_json(host))
        }
        "clip.quantize" | "clip.transpose" => {
            let s = host.store().session();
            let mut clip = control::find_clip(s, a.str("clipId")?)?.clone();
            let bpb = s.beats_per_bar();
            let length = clip.length_bars * bpb;
            let snap = s.transport.snap_division;
            // Controllers stay where they are: quantize and transpose are about notes.
            let ClipData::Midi { notes, .. } = &mut clip.data else {
                return Err("Only MIDI clips hold notes".into());
            };
            let mut moved = 0;
            if name == "clip.quantize" {
                let division = a.opt_int("division").unwrap_or(snap as i64);
                if ![1, 2, 4, 8, 16, 32, 64].contains(&division) {
                    return Err("division must be 1, 2, 4, 8, 16, 32 or 64".into());
                }
                let strength = a.opt_f64("strength").unwrap_or(100.0);
                if !(0.0..=100.0).contains(&strength) {
                    return Err("strength must be between 0 and 100".into());
                }
                let step = 4.0 / division as f64;
                let lengths = a.opt_bool("lengths").unwrap_or(false);
                for n in notes.iter_mut() {
                    let mut target = (n.start / step).round() * step;
                    if target >= length {
                        // Rounding past the end lands on the last grid line inside the clip.
                        target = (target - step).max(0.0);
                    }
                    let next = (n.start + (target - n.start) * strength / 100.0)
                        .clamp(0.0, (length - 1e-6).max(0.0));
                    if lengths {
                        let target = ((n.length / step).round() * step).max(step);
                        n.length = n.length + (target - n.length) * strength / 100.0;
                    }
                    n.length = n.length.min(length - next).max(1e-3);
                    if (next - n.start).abs() > 1e-9 {
                        moved += 1;
                    }
                    n.start = next;
                }
            } else {
                let semitones = a.int("semitones")?;
                if !(-48..=48).contains(&semitones) {
                    return Err("semitones must be between -48 and 48".into());
                }
                for n in notes.iter_mut() {
                    let next = (n.pitch as i64 + semitones).clamp(0, 127) as u8;
                    if next != n.pitch {
                        moved += 1;
                    }
                    n.pitch = next;
                }
            }
            let id = clip.id.clone();
            host.dispatch(Command::PutClip(clip))?;
            let mut summary =
                control::clip_summary(control::find_clip(host.store().session(), &id)?);
            summary["changedNotes"] = json!(moved);
            Ok(summary)
        }
        "track.duplicate" => {
            let s = host.store().session();
            let source = control::find_track(s, a.str("trackId")?)?.clone();
            if s.tracks.len() >= 128 {
                return Err("The session already has 128 tracks".into());
            }
            let index = s.tracks.iter().position(|t| t.id == source.id).unwrap_or(0);
            let mut track = source.clone();
            track.id = control::new_id("track");
            track.name = a
                .opt_str("name")
                .map(str::to_string)
                .unwrap_or_else(|| format!("{} copy", source.name));
            track.armed = false;
            let mut strip = s.strips.get(&source.id).cloned().unwrap_or_default();
            for insert in strip.inserts.iter_mut().chain(strip.synth.iter_mut()) {
                if !insert.is_empty() {
                    insert.id = control::new_id("insert");
                }
            }
            let mut commands = vec![
                Command::AddTrack(track.clone()),
                Command::SetStrip {
                    track: track.id.clone(),
                    strip,
                },
            ];
            for clip in s.clips.iter().filter(|c| c.track_id == source.id) {
                let mut copy = clip.clone();
                copy.id = control::new_id("clip");
                copy.track_id = track.id.clone();
                copy.agent = agent;
                if let ClipData::Midi { notes, controllers } = &mut copy.data {
                    for n in notes {
                        n.id = control::new_id("note");
                    }
                    crate::controllers::renew_ids(controllers, agent, || control::new_id("ctl"));
                }
                commands.push(Command::PutClip(copy));
            }
            commands.push(Command::MoveTrack {
                id: track.id.clone(),
                index: index + 1,
            });
            host.dispatch(Command::Batch(commands))?;
            let s = host.store().session();
            Ok(control::track_json(s, control::find_track(s, &track.id)?))
        }
        "strip.moveInsert" => {
            let id = a.str("trackId")?;
            control::check_strip(host.store().session(), id)?;
            let (from, to) = (a.int("from")?, a.int("to")?);
            let valid = 0..MAX_INSERTS as i64;
            if !valid.contains(&from) || !valid.contains(&to) {
                return Err(format!("Slots must be 0-{}", MAX_INSERTS - 1));
            }
            let mut strip = control::full_strip(host.store().session(), id);
            let insert = strip.inserts.remove(from as usize);
            strip.inserts.insert(to as usize, insert);
            host.dispatch(Command::SetStrip {
                track: id.into(),
                strip,
            })?;
            Ok(control::strip_json(host.store().session(), id))
        }
        "plugin.describe" => {
            let wanted = a.str("pluginId")?;
            let descriptor = crate::control_plugins::choose(Some(wanted), None, None)?;
            let limit = a.opt_int("limit").unwrap_or(200);
            if !(1..=10_000).contains(&limit) {
                return Err("limit must be 1-10000".into());
            }
            let mut instance = plugin_host::instantiate(&descriptor.id, &descriptor.name, 48000)?;
            let editor = instance.editor.as_mut();
            let programs = editor.programs();
            let mut params: Vec<(u32, &crate::plugin::ParamInfo)> = editor
                .params()
                .iter()
                .filter_map(|p| match a.opt_str("query") {
                    Some(q) => crate::control_refs::score(q, &p.name).map(|s| (s, p)),
                    None => Some((0, p)),
                })
                .collect();
            if a.opt_str("query").is_some() {
                params.sort_by_key(|x| std::cmp::Reverse(x.0));
            }
            let total = params.len();
            Ok(json!({
                "descriptor": descriptor,
                "latency": editor.latency(),
                "hasGui": editor.has_gui(),
                "parameterCount": editor.params().len(),
                "total": total,
                "parameters": params.iter().take(limit as usize).map(|(_, p)| {
                    crate::control_params::parameter_json(editor, p, p.default, None)
                }).collect::<Vec<_>>(),
                "truncated": total > limit as usize,
                "programs": programs,
                "presets": preset::list(Some(&descriptor.id))?.iter().map(|p| &p.name).collect::<Vec<_>>(),
            }))
        }
        "preset.list" => Ok(json!({ "presets": preset::list(a.opt_str("pluginId"))? })),
        "preset.save" => {
            let track = a.str("trackId")?;
            let slot = control::plugin_slot(a)?;
            host.capture_states()?;
            let insert = control::selected_plugin(host.store().session(), track, slot)?;
            let preset = PluginPreset {
                name: a.str("name")?.trim().to_string(),
                plugin_id: insert.plugin_id(),
                plugin_name: insert.name.clone(),
                params: insert.params.clone(),
                blob: insert.blob.clone(),
                factory: false,
            };
            let path = preset::save(&preset)?;
            Ok(json!({ "preset": preset, "path": path }))
        }
        "preset.load" => {
            let track = a.str("trackId")?;
            let slot = control::plugin_slot(a)?;
            let mut insert = control::selected_plugin(host.store().session(), track, slot)?;
            let preset = preset::load(&insert.plugin_id(), a.str("name")?)?;
            if preset.plugin_id != insert.plugin_id() {
                return Err(format!(
                    "Preset `{}` belongs to {}, not {}",
                    preset.name,
                    preset.plugin_id,
                    insert.plugin_id()
                ));
            }
            let mut instance = plugin_host::instantiate(&insert.plugin_id(), &insert.name, 48000)?;
            if !preset.blob.is_empty() {
                instance
                    .editor
                    .load(&plugin_host::decode_blob(&preset.blob)?)?;
            }
            for (id, value) in &preset.params {
                let p = instance
                    .editor
                    .params()
                    .iter()
                    .find(|p| p.id == *id)
                    .ok_or_else(|| {
                        format!("Preset parameter {id} does not exist on this plugin")
                    })?;
                if !(p.min..=p.max).contains(value) {
                    return Err(format!("Preset value for {} is out of range", p.name));
                }
            }
            insert.blob = preset.blob.clone();
            insert.params = preset.params.clone();
            let mut strip = control::full_strip(host.store().session(), track);
            if let Some(slot) = slot {
                strip.inserts[slot] = insert;
            } else {
                strip.synth = Some(insert);
            }
            host.dispatch(Command::SetStrip {
                track: track.into(),
                strip,
            })?;
            let mut result = control::strip_json(host.store().session(), track);
            result["preset"] = json!(preset.name);
            Ok(result)
        }
        "preset.delete" => {
            preset::delete(a.str("pluginId")?, a.str("name")?)?;
            Ok(json!({ "deleted": a.str("name")? }))
        }
        "settings.get" => host.settings().get(a.opt_str("path")),
        "settings.set" => {
            let path = a.str("path")?;
            let value = a.get("value").cloned().unwrap_or(Value::Null);
            let mut settings = host.settings();
            settings.set(path, value)?;
            host.update_settings(settings.clone())?;
            settings
                .get(Some(path))
                .map(|value| json!({ "path": path, "value": value }))
        }
        "settings.reset" => {
            let mut settings = host.settings();
            settings.reset(a.opt_str("path"))?;
            host.update_settings(settings.clone())?;
            Ok(settings.redacted())
        }
        "audio.devices" => {
            let settings = host.settings();
            let mut value = json!({
                "outputs": crate::device::output_devices(),
                "inputs": crate::device::input_devices(),
                "midiInputs": crate::midi::ports(),
                "configured": {
                    "output": settings.audio.output_device,
                    "input": settings.audio.input_device,
                    "midiInput": settings.audio.midi_input,
                },
            });
            if let Ok(live) = host.live("audio.status", &json!({})) {
                value["active"] = live;
            }
            Ok(value)
        }
        "app.info" => {
            let mut value = json!({
                "version": env!("CARGO_PKG_VERSION"),
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "pid": std::process::id(),
                "executable": std::env::current_exe().ok(),
                "dataDir": plugin_host::scan::data_dir(),
                "settingsPath": settings::Settings::path(),
                "pluginCache": plugin_host::scan::cache_path(),
                "presetsDir": preset::directory(),
                "recoveryDir": recovery::directory(),
                "discoveryPath": control::wire::discovery_path(),
                "mode": host.mode(),
                "session": host.path().map(|p| p.to_path_buf()),
                "commands": control::COMMANDS.len(),
                "pluginAbi": ryolune_plugin::ABI_VERSION,
            });
            if let Ok(live) = host.live("app.status", &json!({})) {
                if let Some(map) = live.as_object() {
                    for (k, v) in map {
                        value[k] = v.clone();
                    }
                }
            }
            Ok(value)
        }
        "session.snapshots" => Ok(json!({
            "directory": recovery::directory(),
            "snapshots": recovery::list(&recovery::directory())?,
        })),
        "app.logs" => app_logs(a),
        "app.crashReports" => {
            let data = plugin_host::scan::data_dir();
            match a.get("id").and_then(Value::as_str) {
                Some(id) => {
                    Ok(json!({ "id": id, "text": crate::diagnostics::read_report(&data, id)? }))
                }
                None => Ok(json!({
                    "folder": crate::diagnostics::crashes_dir(&data),
                    "reports": crate::diagnostics::reports(&data),
                })),
            }
        }
        "app.clearCrashReports" => Ok(json!({
            "deleted": crate::diagnostics::clear_reports(&plugin_host::scan::data_dir()),
        })),
        "app.diagnostics" => Ok(app_diagnostics(host)),
        "app.whatsNew" => {
            let releases = if a.get("all").and_then(Value::as_bool).unwrap_or(false) {
                crate::release_notes::all()
            } else if let Some(version) = a.get("version").and_then(Value::as_str) {
                vec![crate::release_notes::find(version).ok_or_else(|| {
                    format!("No release notes for {version} in this copy; all=true lists the ones it has, {} has every release.", crate::release_notes::RELEASES_URL)
                })?]
            } else if let Some(since) = a.get("since").and_then(Value::as_str) {
                if crate::release_notes::version_key(since).is_none() {
                    return Err(format!("`{since}` is not a version such as 0.12.0"));
                }
                crate::release_notes::since(since)
            } else {
                crate::release_notes::find(crate::release_notes::CURRENT)
                    .into_iter()
                    .collect()
            };
            Ok(json!({ "current": crate::release_notes::CURRENT, "releases": releases }))
        }
        _ => Err(format!(
            "Command `{name}` is registered but not implemented"
        )),
    }
}

fn args_value(a: &Args) -> Value {
    let mut map = serde_json::Map::new();
    for p in a.spec().params {
        if let Some(v) = a.get(p.name) {
            map.insert(p.name.into(), v.clone());
        }
    }
    Value::Object(map)
}
fn view_json(host: &dyn Host) -> Value {
    let (zoom, scroll) = host.view_state();
    let v = &host.store().session().view;
    let mut value = selection(host);
    value["pixelsPerBar"] = json!(zoom);
    value["scrollBar"] = json!(scroll);
    value["followPlayhead"] = json!(v.follow_playhead);
    value["editorMode"] = json!(v.editor_mode);
    value["editorClipId"] = json!(v.editor_clip_id);
    value["editorLowPitch"] = json!(v.editor_low_pitch);
    value["browserTab"] = json!(v.browser_tab);
    value["browserSelection"] = json!(v.browser_selection);
    value["laneWidth"] = json!(host.lane_width());
    value
}

/// Where `ui.screenshot` writes when no path is given.
pub fn default_screenshot_path() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    plugin_host::scan::data_dir()
        .join("screenshots")
        .join(format!("ryolune-{stamp}.png"))
}

/// `app.logs`: the tail of this run's log, or of an earlier one named from the list. The
/// CLI reads the window's log the same way (the newest file) when it logs nothing itself.
fn app_logs(a: &Args) -> Result<Value> {
    use crate::diagnostics;
    let data = plugin_host::scan::data_dir();
    let lines = a.get("lines").and_then(Value::as_i64).unwrap_or(100);
    if !(1..=2000).contains(&lines) {
        return Err("lines must be 1-2000".into());
    }
    let files = diagnostics::log_files(&data);
    let path = match a.opt_str("file") {
        Some(name) => files
            .iter()
            .map(|(p, _)| p.clone())
            .find(|p| p.file_name().is_some_and(|n| n == name))
            .ok_or_else(|| {
                format!("No log file named `{name}`. app.logs lists them in `files`.")
            })?,
        None => diagnostics::current_log()
            .filter(|p| p.is_file())
            .or_else(|| files.first().map(|(p, _)| p.clone()))
            .ok_or("ryolune has not written a log yet: the window starts one when it opens.")?,
    };
    let tail = diagnostics::tail(&path, lines as usize)
        .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    Ok(json!({
        "folder": diagnostics::logs_dir(&data),
        "file": path,
        "lines": tail,
        "files": files
            .iter()
            .map(|(p, bytes)| json!({
                "name": p.file_name().map(|n| n.to_string_lossy().into_owned()),
                "path": p,
                "bytes": bytes,
            }))
            .collect::<Vec<_>>(),
    }))
}

/// `app.diagnostics`: what a bug report needs, without secrets or song content.
fn app_diagnostics(host: &mut dyn Host) -> Value {
    use crate::diagnostics;
    let data = plugin_host::scan::data_dir();
    let settings = host.settings();
    let cache = plugin_host::scan::cache();
    let reports = diagnostics::reports(&data);
    let snapshots = recovery::list(&recovery::directory()).map_or(0, |s| s.len());
    let build = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let mut value = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "build": build,
        "system": diagnostics::os_name(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "executable": std::env::current_exe().ok(),
        "pid": std::process::id(),
        "mode": host.mode(),
        "pluginAbi": ryolune_plugin::ABI_VERSION,
        "audio": {
            "output": settings.audio.output_device,
            "input": settings.audio.input_device,
            "midiInput": settings.audio.midi_input,
            "bufferFrames": settings.audio.buffer_frames,
        },
        "plugins": {
            "stock": crate::stock::descriptors().len(),
            "scanned": cache.descriptors().len(),
            "bundles": cache.entries.len(),
            "failed": cache.entries.iter().filter(|e| e.error.is_some()).count(),
            "scannedAt": cache.scanned_at,
        },
        "paths": {
            "data": data,
            "settings": settings::Settings::path(),
            "logs": diagnostics::logs_dir(&data),
            "crashes": diagnostics::crashes_dir(&data),
            "recovery": recovery::directory(),
            "presets": preset::directory(),
            "pluginCache": plugin_host::scan::cache_path(),
        },
        "logFile": diagnostics::current_log()
            .or_else(|| diagnostics::log_files(&data).first().map(|(p, _)| p.clone())),
        "counts": {
            "crashReports": reports.len(),
            "logFiles": diagnostics::log_files(&data).len(),
            "recoverySnapshots": snapshots,
            "tracks": host.store().session().tracks.len(),
            "clips": host.store().session().clips.len(),
        },
        "crashReports": reports.into_iter().take(5).collect::<Vec<_>>(),
        "agentProvider": settings.agent.provider.key(),
        "updates": {
            "checkOnStart": settings.general.check_updates_on_start,
            "installAutomatically": settings.general.install_updates_automatically,
        },
    });
    if let Ok(live) = host.live("audio.status", &json!({})) {
        for key in [
            "device",
            "sampleRate",
            "bufferFrames",
            "cpuLoad",
            "monitoring",
        ] {
            value["audio"]["active"][key] = live[key].clone();
        }
    }
    if let Ok(live) = host.live("app.status", &json!({})) {
        value["updates"]["available"] = live["updateAvailable"].clone();
        value["updates"]["installed"] = live["updateInstalled"].clone();
        value["bridgePort"] = live["bridgePort"].clone();
    }
    value
}
