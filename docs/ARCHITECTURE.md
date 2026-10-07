# How ryolune is put together

This page is for contributors. It explains the parts of ryolune, which thread runs what, and
how an edit travels from a click (or a script) to the speakers. File paths are relative to
the repository root. Build and release steps are in [DEVELOPMENT.md](DEVELOPMENT.md); the file
format is in [SESSION_FORMAT.md](SESSION_FORMAT.md); preferences and environment variables are
in [CONFIGURATION.md](CONFIGURATION.md).

## The workspace

ryolune is one Cargo workspace (`Cargo.toml`). All crates share one version.

| Crate | Folder | What it is |
|---|---|---|
| `ryolune-engine` | `engine/` | Everything without a window: the session model, the store and undo, the command registry, DSP and the renderer, audio and MIDI devices, plugin hosting (VST3, CLAP, AU, native), documents, export, settings, lsuite discovery. |
| `ryolune` | `desktop/` | The app. The host (`desktop/src/app.rs`, struct `Ryolune`) and the GPUI window (`desktop/src/ui/`), plus the live half of the registry (`desktop/src/control.rs`), the built-in agent (`desktop/src/agent/`), generation, updates, recovery and discovery. The default member. |
| `ryolune-plugin` | `sdk/` | The native plugin SDK: the `Plugin` trait, DSP helpers, the frozen C ABI (`sdk/src/ffi.rs`) and a test bench (`sdk/src/testing.rs`). |
| `ryolune-tools` | `tools/` | `ryolune-cli` and `ryolune-mcp`, thin clients of the registry (`tools/src/lib.rs`, `tools/src/bin/`). |
| `ryolune-plugin-gain` | `plugins/gain/` | An example native plugin bundle (Trim and Tilt EQ), used by tests. |
| `ondera-abi1-fixture` | `plugins/abi1-fixture/` | A plugin frozen at ABI 1 with no SDK dependency, so tests prove old libraries still load. Its name is kept on purpose. |

The former Electron and TypeScript app (`legacy/`) was removed; it is in git history at
`7fb6116`, for reference only.

## Threads

```
                 ryolune-cli / ryolune-mcp / Claude Code CLI          vendor CLIs, HTTP APIs
                        |  JSON-RPC over 127.0.0.1                            ^
                        v                                                     |
  +---------------------------------+      +----------------------------------+---------+
  | control server                  |      | workers (std::thread::spawn)               |
  |  "ryolune-control" accept loop  |      |  graph preparation (Renderer::new)         |
  |  "ryolune-control-client" x16   |      |  file jobs: open, save, export, bounce     |
  |  bounded queue (16) + wake()    |      |  live jobs (start_worker), generation      |
  +----------------+----------------+      |  recovery writer, update check/install     |
                   |                       |  device open, take collector, scan driver  |
                   v                       |  "ryolune-agent" (one turn of the agent)   |
  +-----------------------------------------+-----------------------------------------+
  | interface thread (GPUI main thread)                                               |
  |  Daw entity -> Ryolune host: Store, Library, plugin Editors, settings, agent state  |
  |  tick: 16 ms while busy, 100 ms idle, at once on wake()                            |
  |  serves control requests and agent tool calls between frames                     |
  +-------------+-----------------------------------------------^---------------------+
                | rtrb ring (256 Messages):                     | rtrb ring: Retired
                | Replace(Renderer), Mount, Unmount,            | (old renderers,
                | SetParam, Start, Stop, Locate, Monitor ...    |  processors, taps)
                v                                               |
  +-----------------------------------------+     +-------------+-------------------+
  | audio output callback (cpal)            |<----| audio input callback (cpal)     |
  |  Renderer + Rack of Processors          | ring|  meter, monitor ring, take ring |
  |  no allocation, locks, I/O or logging   |     +---------------------------------+
  |  Telemetry atomics -> interface         |<---- MIDI input (midir callback):
  +-----------------------------------------+      RoutedNote / RoutedControl
```

- The **interface thread** owns everything that changes the document: the `Store`, the audio
  `Library`, plugin editors and settings. Other threads never touch them; they send results
  back through channels and call `wake` so the window ticks at once.
- The **audio output callback** owns the current `Renderer` and the plugin `Rack`. It receives
  `device::Message`s through a bounded lock-free ring (`rtrb`, 256 slots) and hands back what it
  no longer needs through a second ring (`device::Retired`), so nothing is freed on the audio
  thread.
- Each **platform stream** lives on the worker that opened it (`DeviceEngine::open`,
  `LiveInput::open`) and closes when its handle is dropped. No stream is forced `Send`.
- **Workers** do anything slow: building a renderer, reading and writing files, network calls,
  offline renders. They report through `mpsc` channels the interface thread polls.

## The command registry

Every user-facing action is a named command in one registry. The window, `ryolune-cli`,
`ryolune-mcp` and the built-in agent call the same names, with the same validation and the
same undo history. No client is privileged.

- `engine/src/control.rs` defines `Spec` (name, description, parameters, whether it
  `mutates`), the `BASE_COMMANDS` and `COMMANDS`, which chains the families:
  `control_media.rs`, `control_edit.rs` (edits and `session.batch`), `control_arrange.rs`,
  `control_tempo.rs`, `control_routing.rs`, `control_plugins.rs` (the plugin library and
  folders), `control_automation.rs`, `control_controllers.rs`, `control_app.rs` (view, presets,
  settings, audio, ui, app, agent, takes), `control_params.rs` (plugin parameters and
  programs), `control_overview.rs` (`session.overview`), `control_generate.rs` and
  `control_suite.rs`.
- `control::call` resolves shared lsuite aliases (`control::ALIASES`: `app.version`,
  `project.overview`, `export.audio`, `export.stems`, `export.midi`), resolves names given in
  place of ids (`control_refs::resolve`, so `trackId: "Bass"` works and a wrong name lists what
  exists), validates parameters against the `Spec`, then runs the command.
- A command runs against a `control::Host`: the store, the library, transport, open/save,
  bounce, settings, the clipboard and the lane width. `control::Headless` implements it for
  files and tests. The desktop `Ryolune` implements it in `desktop/src/control.rs`.
- Actions only a running window can do (`ui.*`, `agent.*`, audio devices, note preview,
  updates, quitting, `generate.audio`, snapshot restore: `control_app::is_live_only`) go to
  `Host::live`. In a headless host they return an error that says to start the app.
- The CLI help, the MCP tool list (`track.add` becomes the tool `track_add`), the agent's
  tools and `docs/COMMANDS.md` are all generated from `COMMANDS`.

### The live bridge

`engine/src/control/wire.rs` serves the registry to other processes while the window runs.

1. The window binds `127.0.0.1` on a free port and writes `{version, port, token, pid}` to the
   control file (`~/.ryolune/control.json`, mode 0600; `RYOLUNE_CONTROL` moves it).
2. A client connects, sends `auth` with the token, then newline-delimited JSON-RPC 2.0
   requests. A request may carry `"agent": true`.
3. A connection thread queues the request (at most 16 pending, 16 clients) and calls `wake`.
   The interface thread drains the queue between frames (`Ryolune::serve_control`) and runs
   each request through `run_control_command`, so commands apply in order on the same thread
   as the window, never concurrently with it.

`ryolune-cli` and `ryolune-mcp` (`tools/src/lib.rs`, `Backend`) either talk to the window
(`Live`) or, with `--file` or when no window runs, host a `Headless` session in their own
process and hold the file's lock (`engine/src/session_file.rs`).

### `run_control_command`

`Ryolune::run_control_command` (`desktop/src/control.rs`) is the one entry for the window, the
bridge and the agent:

- For agent-flagged requests it enforces `settings.agent.permissions`
  (`control_app::denied_for_agent_request`) and refuses changes to the `agent`, `control` and
  `generation` settings.
- File operations (`session.open`, `save`, `bounce`, imports, exports, `export.toKimchi`,
  `plugin.scan`) stop the transport, capture plugin state when the output needs it (save,
  bounce, exports), then run on a worker against a `Headless` copy of the session. The command answers `{"status":"running"}` and the real
  reply is sent when the job finishes.
- Work that waits on the network or renders offline becomes a live job
  (`Ryolune::start_worker`, `LiveWait`), answered the same way.
- `session.batch` runs a list of commands inside one gesture, so they are one undo step; an
  atomic batch that fails is rolled back with `cancel_gesture` (`run_batch`).

### Parity

A window interaction is a registry command first, and the view calls that name; there are no
private window-only handlers for things a script could want. `ui/actions.rs` is the one table
of window actions, and `docs/agent-parity.json` maps each action id to its commands.
`engine/tests/agent_parity.rs` fails when an action has no entry, when the window sends an
unknown command name, or when a command lacks a real description. `docs/AGENT_PARITY.md` is
the human audit.

## The store and undo

`engine/src/store.rs`:

- `Session` is immutable behind an `Arc`. `Store::dispatch(Command)` clones it, applies the
  command, validates the result (`Session::validate`) and only then swaps it in. A command
  that fails validation changes nothing.
- `store::Command` is the closed set of document edits: tracks, clips, sources, markers, tempo
  changes, strips, transport, master volume, automation, selection, view, `Undo`, `Redo` and
  `Batch`. Every persistent edit in the app is one of these; registry commands build them.
- History keeps up to 200 past states. `revision` increases on every change; `dirty()`
  compares against the saved document id.
- `Select` and `SetView` are transient: no undo step, not dirty. They cannot be batched,
  nor can `Undo`/`Redo`. A batch holds at most 10,000 commands, nested at most 8 deep.
- `Store::set_gesture(true)` coalesces every edit until `set_gesture(false)` into one undo
  step: a fader drag, a knob, a clip drag. `cancel_gesture` drops the gesture's edits and
  restores the redo history it had set aside. The window calls this through `Daw::gesture`.
- `Store::amend` updates derived data (captured plugin state) without an undo step or the
  dirty flag.

Audio buffers live outside history, in the `Library` (`engine/src/audio.rs`), keyed by source
id, so an undo never copies audio.

## The document model

- `engine/src/model.rs`: `Session`, `Track` (kinds `audio`, `midi`, `bus`), `Clip` with
  `ClipData::Midi` (notes and controllers) or `ClipData::Audio` (source, offset, fades, gain),
  `Source`, `Strip`, `Insert`, `Send`, `Transport`, `View`, `Marker`. It also holds validation,
  normalization of older files, routing rules (`validate_routing`, `prune_routing`) and
  `Session::needs`, the list of plugin instances a session requires.
- `engine/src/tempo.rs`: `TempoPoint` and `TempoMap`. The map is the only place beats become
  seconds (steps and linear ramps, closed form both ways). Use `Session::bars_seconds`,
  `seconds_bars` and `tempo_map()`, never `60 / tempo`.
- `engine/src/automation.rs`: automation lanes and targets (track volume and pan, master
  volume, plugin parameters).
- `engine/src/controllers.rs`: helpers for clip controllers (windows, reverse, playback,
  recorded data).
- `engine/src/document.rs`: load and save, `atomic_write`, the `.ondera` and
  `ondera-session` compatibility.

## Audio

### From an edit to the speakers

1. A command changes the store; `Ryolune::try_dispatch` sets `sync_needed`.
2. On the next tick, if no job runs, `Ryolune::poll` snapshots the session and starts a worker
   that prepares sources (`audio::prepare_sources`) and builds a `render::Renderer` for the
   device's sample rate, with plugin latencies (`desktop/src/app.rs`).
3. When the worker returns and the store has not moved on, the renderer is sent as
   `Message::Replace`. If the revision changed meanwhile, the result is dropped and a new one
   is prepared.
4. The output callback (`device::Callback`) calls `new.adopt(&old)`: the new renderer takes the
   transport, held notes (released or chased), controller state and count-in from the old
   one, and glides a sounding clip's envelope over 5 ms (`Renderer::glide_from`). The old
   renderer goes back through the garbage ring.

The callback processes at most 64 queued messages per buffer (256 after an input overflow)
and stops early when the garbage ring is nearly full, so it never blocks and never frees.

### The renderer

`engine/src/render.rs` turns a session into audio block by block: it sequences notes and
controllers as `plugin::Event`s for instruments, samples audio clips directly with their fades
and gain (`clip_envelope`), applies automation (plugin parameters at frame 0, on breakpoints
and every `AUTOMATION_GRAIN` = 32 frames while they move), runs inserts, sends, bus tracks,
the A and B returns and the master chain, and compensates plugin latency per stage. Tracks
run before buses; outputs that skip a bus wait in delay lines so every path meets. The count
in (`Renderer::count_in`) and the click live here too.

`render::offline` and `render::bounce` build the same graph with fresh plugin instances for
exports (`engine/src/export.rs`: WAV, AIFF, FLAC written by `engine/src/flac.rs`, Ogg Vorbis
through `vorbis_rs`; MIDI files in `engine/src/midi_file.rs`).

### Plugins in the audio graph

Every insert and instrument, stock or external, is a `plugin::Instance` (`engine/src/plugin.rs`):

- an `Editor` that stays on the interface thread (parameters, state save and load, programs,
  the native GUI), and
- a `Processor` that is mounted into the callback's `Rack` by slot number.

The rack outlives renderer rebuilds, so reverb tails, synth voices and plugin state survive
every edit. `Session::needs` lists the instances a session wants; the desktop plugin bank
(`desktop/src/plugins.rs`, `reconcile_plugins`) creates missing ones on the interface thread
with `host::instantiate`, sends `Message::Mount`, and sends `Message::Unmount` for ones no
longer needed. An editor is dropped only after the audio thread has returned its processor
(`collect_retired`). Parameter values are document state (`Insert.params`), sent to the
callback as `Message::SetParam`; external plugin state is captured into `Insert.blob` on save,
bounce and recovery (`capture_plugin_states`).

`Rack::set_param_at` keeps timed parameter changes sorted. A processor that reports
`timed_params()` (CLAP, VST3, native ABI 2) receives the whole block with frame offsets; any
other is split at each change.

### Input, monitoring and recording

`device::LiveInput` is the only input stream (`engine/src/device.rs`). Its callback
(`InputCallback`) feeds three things:

- the input meter (`Telemetry::input_peak`);
- the monitor ring (`device::monitor_ring`): a bounded ring to the output callback's
  `MonitorTap`, which resamples between rates, waits for one input buffer before it starts,
  skips a backlog and counts drops in `Telemetry`. `Renderer::render_monitored` mixes it into
  monitoring tracks ahead of their inserts. A built-in microphone into built-in speakers is
  detected by device names (`device::feedback_risk`) and stays muted until
  `audio.allowSpeakerMonitoring`;
- while a take runs, a take ring (`LiveInput::record` returns a `Recorder`) drained by a
  collector worker. Frames are dropped while `Telemetry::counting_in` is set, so a take starts
  on the beat.

Monitoring and takes attach through a small control ring and are handed back through a
garbage ring that the input's worker frees. Starting a take or toggling monitoring never
reopens the device. Every finished take is also written to `<data dir>/recordings/`.

MIDI input (`engine/src/midi.rs`, `midir`) sends `Message::RoutedNote` and `RoutedControl`
straight to the output callback for live playing and queues timestamped events for recording.

## Plugin hosting

`engine/src/host/`:

| Format | File | Notes |
|---|---|---|
| Stock | `engine/src/stock.rs` | ryolune's own instruments and effects, written on the SDK's `Plugin` trait and served through the same vtables as native plugins. |
| Native | `engine/src/host/native.rs` | Libraries built with `ryolune-plugin`. The host asks for `ryolune_plugin_entry_v2` first and falls back to `ryolune_plugin_entry`; the `ondera_plugin_entry*` names are accepted too. Libraries stay loaded for the process lifetime. |
| CLAP | `engine/src/host/clap.rs` | Through `clap-sys`. |
| VST3 | `engine/src/host/vst3.rs` | Through the `vst3` COM bindings; controllers reach the plugin through `IMidiMapping`. |
| Audio Unit | `engine/src/host/au.rs` | macOS only. |

The native ABI (`sdk/src/ffi.rs`) has two versions. ABI 1 (`Entry`, `PluginVTable`,
`RawContext`, `NoteEvent`) is frozen: notes only, parameters at block starts. ABI 2
(`PluginVTable2 { size, base, .. }`) embeds the ABI 1 table and adds timed events
(controllers, bend, pressure, parameter changes), an opaque state blob and a tail length. The
SDK's export macro writes both symbols; compile-time assertions in `ffi.rs` pin every frozen
offset. Every call into plugin code is panic-guarded (`Guarded` in `ffi.rs`). Never change a
`repr(C)` layout without bumping the ABI. See [NATIVE_PLUGINS.md](NATIVE_PLUGINS.md).

Scanning (`engine/src/host/scan.rs`): native, CLAP and VST3 bundles are probed one by one in
a child process (the same executable with `--scan-plugin <format> <bundle>`, 30 second
timeout), so a crashing plugin cannot take the app down. Audio Units come from the system
registry. Results go to `<data dir>/plugins.json`, reused while a bundle's modification time
is unchanged. Folders come from `CLAP_PATH`, `VST3_PATH`, `RYOLUNE_PLUGIN_PATH`, Settings and
the platform's standard locations. `control_plugins.rs` files every plugin into a sound folder
for the browser.

## The window

The window is drawn with GPUI 0.2 (Zed's GPU interface framework, `gpui` crate with
`runtime_shaders`) in `desktop/src/ui/`. `desktop/src/ui/README.md` is the contract; in short:

- `ui::run` (`desktop/src/ui/mod.rs`) creates the `Daw` entity and one window holding a
  `Workspace` (title bar, transport, browser, arrangement over the editor or mixer,
  inspector, agent panel, dialogs).
- `Daw` (`desktop/src/ui/daw.rs`) owns the host, `crate::app::Ryolune`. It ticks the host every
  16 ms while something moves (`Ryolune::busy`), every 100 ms when idle, and at once when
  another thread calls the host's `wake` closure (the bridge, a worker, the agent). After a
  tick it notifies views only when a fingerprint of what they show changed.
- Views read with `daw.read(cx).app` and change things only through registry commands:
  `daw.run(name, params, cx)` shows a failure in the error dialog; `daw.request` returns it to
  forms that show their own. Both call `run_control_command` with the source "Interface".
- Continuous controls call `daw.gesture(true)` on press and `daw.gesture(false)` on release,
  so a drag is one undo step. Drag previews stay in the view until release.
- `ui/actions.rs` is the one table behind the title-bar menus, the macOS menu bar, keyboard
  shortcuts, the command palette (`ui/palette.rs`) and the shortcut sheet
  (`ui/dialogs/help.rs`, which also generates `docs/SHORTCUTS.md`).
- `ui/theme.rs` is the only place colours, sizes and radii live (lsuite design system).
- `ui/capture.rs` captures the window for `ui.screenshot` and `--screenshot` through
  CoreGraphics; it works on macOS only.

## The built-in agent

`desktop/src/agent/` runs the agent panel's conversation. One turn runs on a worker thread
named `ryolune-agent` (`agent::Runtime::start`), by provider:

- `anthropic.rs`: the Anthropic Messages API with streaming and tool use.
- `openai.rs`: OpenAI Chat Completions, used by every provider after Anthropic in
  `settings::Provider` (OpenAI, Gemini, OpenRouter, Mistral, Groq, DeepSeek, xAI, Ollama,
  LM Studio, any compatible endpoint), with the address from `Settings::base_url`.
- `codex.rs`: the Codex CLI's app-server, with the registry as dynamic tools.
- `cli.rs`: Claude Code (`claude -p --output-format stream-json`) with an MCP config that
  points at `ryolune-mcp`, which reaches this window through the bridge.

The registry is the tool list (`agent::tool_specs`: every command except `agent.*` and
`session.commands`, named `family_action`). The worker never touches the session: each tool
call goes back to the interface thread (`Ryolune::run_agent_tools` in `desktop/src/agents.rs`),
runs through `run_control_command` with the agent flag, and the worker waits for the answer.
So permissions apply, and agent edits are ordinary undo steps. `desktop/src/agents.rs` holds
the panel state, including the Changes log with Revert and Redo; the panel is
`ui/agent_panel/` (tabs Chat, Generate, Changes, Takes). `agent/clients.rs` builds the MCP
recipes for outside agents (`agent.mcp`).

## Generation

`generate.audio` asks a service (ElevenLabs, Stable Audio, fal.ai or a custom endpoint) for a
song, a loop, a one-shot or an instrument note. The work is split:

- `engine/src/control_generate.rs` shapes the request from the song (a loop gets the tempo and
  key and is fitted to its bars), keeps each result in `<data dir>/generated` with a JSON note,
  and places it as an audio clip or as a Sample Keys instrument (`engine/src/sample_keys.rs`).
  Everything but the network call works headless.
- `desktop/src/generate.rs` is the network call, run on a worker by `start_generation` in
  `desktop/src/control.rs`. The result is placed on the interface thread when it arrives
  (`LiveWait::Generation`), in one undo step.

## lsuite: discovery and hand-offs

ryolune is part of lsuite with kimchi (video) and zenith (hub).

- **Discovery.** `desktop/src/discovery.rs` writes `~/.lsuite/apps/ryolune.json` (format 1,
  `engine/src/lsuite.rs`): version, paths to the app, CLI and MCP server, the data folder,
  document types and, while the window runs, its pid, port and control file path. It is
  refreshed every 120 ticks and its `running` part cleared on quit. No secret goes in it.
  `app.suite` lists every app's entry.
- **Hand-offs** (`engine/src/control_suite.rs`). `export.toKimchi` renders the mix or stems for
  kimchi: through kimchi's bridge when it is open, or into its project file when it is closed.
  `session.scoreCut` scores a cut that kimchi wrote (`<name>.kimchi-cut.json` beside its WAV).
  `handoff.inbox` lists files waiting in `~/.lsuite/handoff/ryolune/`.

## Updates

`desktop/src/update.rs` checks GitHub Releases of `ludovic111/ryolune` on a worker. A release
(built by `.github/workflows/release.yml` from a `vX.Y.Z` tag) carries one zip per platform
(`update::asset_name`: `ryolune-macos-arm64.zip`, `ryolune-macos-x86_64.zip`,
`ryolune-linux-x86_64.zip`, `ryolune-windows-x86_64.zip`), a `SHA256SUMS` file and its
Ed25519 signature `SHA256SUMS.sig`. The app checks that every URL belongs to the repository's
releases, verifies the signature against the public key compiled in from
`desktop/assets/update-signing.pub`, verifies the asset's checksum, checks that the new
binaries report the expected version, swaps the installed copy in place (keeping a backup to
roll back) and relaunches. The secret key is never in the repository.

## Recovery snapshots

`desktop/src/recovery.rs` writes a copy of an edited song every
`general.recoveryIntervalSeconds` (10-600, default 30) while the window is idle and the
document is dirty. One worker owns the file I/O; generation and revision checks keep an old job
from replacing a newer document. Snapshots are ordinary `.ryolune` files in
`<data dir>/recovery`, named `recovery-<run>-<pid>-<generation>-<title>.ryolune`
(`engine/src/recovery.rs`), and only such files in that folder can be restored. A restored copy
opens as unsaved, so Save asks where to put it. Background captures skip over a pending redo
or an open gesture.

## Settings

`engine/src/settings.rs` defines the preferences (`settings.json`, mode 0600, secrets masked by
`Settings::redacted`). The window keeps a live copy and changes it only through
`apply_settings` (`desktop/src/settings.rs`) or `settings.set`. See
[CONFIGURATION.md](CONFIGURATION.md).

## Rules that keep this working

- Every persistent edit goes through `store::Command`. Keep drag previews local; group
  continuous edits in one gesture.
- No allocation, deallocation, blocking, I/O or logging in an audio callback. Build graphs on
  workers, send them through bounded queues, and free old ones off the audio thread.
- Create, activate, save and destroy plugins on the interface thread. Unmount through the
  queue and wait for retirement before dropping an editor.
- A new user-facing action is a registry command first, with a row in
  `docs/agent-parity.json` if the window exposes it.
- File writes preserve the old file on failure (`document::atomic_write`).

## Where the tests are

| Area | Tests |
|---|---|
| Engine, documents, rendering | `engine/tests/engine.rs`, `regressions.rs`, `export_memory.rs`, `media.rs` |
| Registry and bridge | `engine/tests/control.rs`, `agent_control.rs`, `parity.rs`, `agent_parity.rs` |
| Routing, automation, controllers | `engine/tests/buses.rs`, `automation.rs`, `controllers.rs`, `midi_routing.rs` |
| Plugins | `engine/tests/native_plugin.rs`, `abi1_plugin.rs`, `plugin_runtime.rs`, `au_presets.rs` |
| lsuite | `engine/tests/lsuite_handoffs.rs` |
| CLI and MCP | `tools/tests/cli.rs`, `mcp.rs`, `locking.rs`, `command_docs.rs` (keeps `docs/COMMANDS.md` current) |
| Window and host | unit tests in `desktop/src` (for example `ui/arrangement/tests.rs`, `ui/agent_panel/tests.rs`, `ui/dialogs/help.rs` for `docs/SHORTCUTS.md`) |
