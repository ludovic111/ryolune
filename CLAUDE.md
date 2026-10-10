# ryolune native Rust branch

Renamed from Ondera on 2026-09-28 (0.11, owner's decision; the brand is written in lowercase).
Compatibility kept on purpose, do not "clean it up": `document::LEGACY_EXTENSION` (`.ondera`)
and the `ondera-session` format still load; `host::scan::data_dir` adopts the old `Ondera`
data folder once and native plugin folders under the old name are still scanned; the SDK
exports `ondera_plugin_entry*` beside `ryolune_plugin_entry*` (`LEGACY_ENTRY_SYMBOL*`) and the
host accepts both; `plugins/abi1-fixture` keeps its old crate name, symbol and id; the GitHub
secret is still named `ONDERA_SIGNING_KEY`; release notes before 0.11 and git history keep the old
name.

Since 0.13 (2026-10-02, owner's request: "enlève tauri, passe au GPUI") the window is drawn with
GPUI 0.2 (gpui.rs, Zed's framework, direct upstream crate, `runtime_shaders` so no Metal
toolchain is needed) in `desktop/src/ui`; Tauri, the React `frontend/` and the egui painting code
are gone. `desktop/src/ui/README.md` is the contract: the `Daw` entity (`ui/daw.rs`) owns the
host (`crate::app::Ryolune`) and ticks it (per frame while busy, 10/s idle, at once when the
bridge or a worker calls the host's `wake` closure); views read `daw.read(cx).app` and change things only
through registry commands (`daw.run`, `daw.request`), with `daw.gesture(true/false)` around
drags. `ui/actions.rs` is the one table behind the title-bar menus, the macOS menu bar,
shortcuts and the command palette (ids mapped in `docs/agent-parity.json`); the shortcut sheet
(`ui/dialogs/help.rs`) generates `docs/SHORTCUTS.md`. Panels: `titlebar`, `transport`,
`browser/`, `arrangement/`, `editor/`, `inspector/`, `mixer`, `agent_panel/`, `plugin_panel/`,
`automation`, `dialogs/`, `settings_window/`, `palette`; shared controls in `ui/widgets/`
(Button, Key, Segmented, Switch, Knob, Fader, Slider, NumberDrag, Meter, TextInput, menus).
Window captures (`ui.screenshot`, `--screenshot`) use CoreGraphics (`ui/capture.rs`, macOS only).
Check with `cargo test -p ryolune` (scratch profile env vars), then look at the real window
(`--screenshot`, or the app driven by `ryolune-cli` with `RYOLUNE_CONTROL` in a scratch folder).

Theme (design system v2, owner's decision 2026-10-06: "the whole suite looks like kimchi now";
PR #32, released in 0.14.0): `desktop/src/ui/theme.rs` is the only place visual values
live, on lsuite's v2 tokens (`desktop/assets/lsuite/tokens.json` + `tokens.css`, copies of
`../lsuite/design/`; a test checks the palette against the JSON, copy the file again when the
suite changes). Black and white: the accent is the ink of the mode (white dark, black light), a
chosen thing is inverted (`accent_fill` + `text_on_accent`), red (`record`/`danger`) only for
record, arm and errors, warnings/success are greys; mute/solo keys light in ink, meters are greys
with red for clipping, `Theme::family` is grey (plugin panels draw in ink). The work keeps its
colours (track palette on clips and notes) and makers' logos keep theirs. Radii are zero; only
knobs and score note heads are round. Floating surfaces: hard offset shadow
(`Theme::float_shadow`, `chip_shadow` for the primary button). The page is `ui/grain.rs`
(ported from kimchi: grain tiles and Bayer-dither corners as RenderImages at device pixels, the
strength baked into the alpha because GPUI 0.2.2 images ignore element opacity), under glass
tier 1 chrome; work surfaces stay solid, lanes past the song end are hatched; dialogs sit in
`grain::brackets` (`dialogs::modal::sheet`), so does the agent composer. Organization (owner liked
it in kimchi): every area has a title bar (`widgets::panel_title`/`panel_info`), tools are boxed
by kind in `widgets::group` with `Button::flush` / `actions::tool` (lit when on, shortcut in the
tooltip, label only when `ui::centre_width` leaves room), track headers show a number chip, the
whole name on two lines and always-visible boxed keys. The title bar holds history · views
(mixer, automation, commands) · agent · app groups, Sponsor and Export (primary); the transport
keeps locate/keys/click+snap groups and the meters. The contrast test covers every surface and
tier over the page at its densest grain + dither and over white and black desktops: fix the
palette, not the threshold. `engine/src/settings.rs` `THEMES` is still `["ryolune"]` with
`interface.mode` dark/light/auto (no coloured themes left to remap). Fonts: Chakra Petch
(UI, 400-700) and IBM Plex Mono TTFs in `desktop/assets/fonts` (Manrope is gone). Icons are SVGs in
`desktop/assets/icons` (tinted by GPUI; a few Lucide ones, ISC). The mark (the ring and the dot cut
square, the ring's shadow side dissolving into dither) and the app icon are written by
`scripts/gen-mark.py` (`desktop/icons/mark.svg`, `desktop/icons/ryolune.svg`,
`desktop/assets/icons/mark.svg` in currentColor); `scripts/make-icon.sh` renders the .icns, png and
.ico with resvg. The window is still `WindowBackgroundAppearance::Blurred` (opaque with macOS
Reduce transparency). The public page is lsuite.xyz/ryolune, in
the lsuite repo (ludovic111/lsuite); ryolune.com redirects there with the same path, so
`/support` and `/download/<platform>` links keep working. The former standalone site (`site/`,
no longer deployed) and the 0.12 launch-film pipeline (`marketing/`) were removed; they are in git
history at 7fb6116.
When v2 is released, update lsuite's DESIGN.md (it still says ryolune wears v1) and the
captures on lsuite.xyz/ryolune.

The owner requested a complete Rust rewrite on 2026-09-12, including the interface.
This supersedes the former Electron / TypeScript app (`legacy/`, removed; it is in git history at
7fb6116).

- `desktop/`: the app. The host (`app.rs` `Ryolune`: store, audio, plugins, workers; no UI code)
  and the GPUI window in `desktop/src/ui` (above). `agents.rs` holds the agent panel's state (the
  Changes log with Revert/Redo, the prompt); the panel itself is `ui/agent_panel/` (380 px open,
  32 px rail closed). `agent/` is the runtime (providers `anthropic`, `openai`, `cli` for Codex and
  Claude Code; tool calls execute on the interface thread through `run_control_command`).
  `settings.rs` holds the Settings window's state over `engine::settings::Settings`; apply changes
  through `apply_settings` / `settings.set`, never by writing fields. The window draws its own
  title bar (transparent native title bar, traffic lights at the left, `ui/platform.rs` moves the
  window). Keep every panel to what the design frame shows; anything extra goes into a menu or
  Settings, not the panel.
- `engine/`: pure Rust command store, session model, DSP, audio devices and documents.
  `engine/src/control.rs` is the public command registry (`control_app.rs` holds the view, preset,
  settings, audio, ui, app and agent families); `control/wire.rs` the loopback protocol. `tools/`
  builds `ryolune-cli` and `ryolune-mcp` as thin clients of that registry, and `desktop/src/control.rs`
  serves it from the window between frames, implementing `Host::live` for window-only actions
  (screenshots, panels, devices, updates, the agent). A new user-facing action goes into the
  registry so the window, the CLI, MCP and the built-in agent get it together; the CLI help, MCP
  tool list and agent tools are generated from it. Agent permissions (`settings.agent.permissions`)
  are enforced in `run_control_command` for every agent-flagged request.
- `sdk/` is `ryolune-plugin`: the `Plugin` trait, DSP primitives and the frozen C ABI (`ffi.rs`,
  ABI version 1, never change a `repr(C)` layout without bumping it). `engine/src/host/native.rs`
  loads libraries and adapts vtables to `Editor`/`Processor`; `engine/src/stock.rs` is written on
  the trait and served through the same vtables. `plugins/gain` is the example bundle used by tests.
- Preferences live in `engine/src/settings.rs` (`settings.json`, 0600, secrets masked by
  `redacted()`); presets in `engine/src/preset.rs`; recovery snapshot naming in `engine/src/recovery.rs`.
- 0.7 work (decided 2026-09-19, owner delegated the calls): the registry is the contract for GUI
  parity, so a new window interaction lands as a command first (`control_edit.rs` for edits and
  `session.batch`, `control_plugins.rs` for the plugin library, live-only ones in
  `desktop/src/control.rs`) and the window calls that name; no private window-only handlers
  for things a script could want. Plugins are browsed by sound folder: `control_plugins.rs` files
  every descriptor (`automatic_folder`, ordered `EFFECT_RULES`), favourites/recents/overrides live in
  `settings.plugins`, and the window colours a folder with `Theme::family`. Drags are never eased. The count-in
  lives in the renderer (`Renderer::count_in`), the capture callback drops frames while
  `Telemetry::counting_in` is set, and `device::LiveInput` holds the input open only while an audio
  track is armed (`audio.meterInputWhenArmed`). Native plugin calls are panic-guarded in `sdk/src/ffi.rs` (`Guarded`); test plugins with
  `ryolune_plugin::testing::Bench`. Continuous controls dispatch on every move inside one
  `Daw::gesture`, so a drag is one undo step.
- 0.8 (released 2026-09-23; the owner delegated lossy export and MIDI CC scope): input monitoring
  is a bounded ring (`device::monitor_ring`) from the one input stream to the output callback's `MonitorTap`, which resamples, waits for one input buffer before it
  starts, skips a backlog and counts drops and underruns in `Telemetry`; `Renderer::render_monitored`
  mixes it into monitoring tracks ahead of their inserts with a 5 ms ramp. `Track.monitor` is
  absent from the file when off. Built-in microphone into built-in speakers is decided by device
  names (`device::feedback_risk`) and stays muted until `audio.allowSpeakerMonitoring`.
  `settings.audio.bufferFrames` sets both device buffers. FLAC is `engine/src/flac.rs`, written by
  hand (fixed predictors, Rice, mid/side, MD5) so no dependency was added; a mix takes its
  container from the path, stems from `ExportOptions::container`. MP3 was not added: no clean
  encoder fits the licence and the build; Ogg Vorbis is the lossy format. Plugin ABI 2 never touches an ABI 1
  layout: `ryolune_plugin_entry_v2` returns `PluginVTable2 { size, base, .. }`, the macro exports
  both symbols, the host tries v2 then v1, `sdk/src/ffi.rs` asserts every frozen offset at compile
  time, and `plugins/abi1-fixture` (no SDK dependency, never "update" it) is loaded by
  `engine/tests/abi1_plugin.rs`. Native state is saved from a main-thread model instance and
  restored by swapping a freshly loaded instance in on the audio thread. The window's private
  handlers are gone: views call the registry through `Daw::run` / `Daw::request` and
  `engine/tests/agent_parity.rs` checks them against `docs/agent-parity.json`; anything else is a registry command, and work that waits on the network or renders offline is
  a live job through `Ryolune::start_worker`. The clipboard and the lane width belong to the host
  (`Host::clipboard`, `Host::lane_width`). Browser rows fold channel layouts
  (`control_plugins::layout_of`); when extending `EFFECT_RULES`, diff every plugin's folder before
  and after on a real library, because a new word in an early group steals from later ones.
- 0.8 additions. Input: `device::LiveInput` is the only input stream (meter + monitor ring +
  takes), opened by `Ryolune::poll_input`; monitoring (`LiveInput::monitor`) and takes
  (`LiveInput::record` returns a `Recorder`) attach through a control ring, and the callback
  (`InputCallback::process`) hands them back through a garbage ring its worker frees. Test the
  callback with `live_input(..)`, no device needed; it reopens only for a new device or buffer size,
  never during a take. Parameters: `plugin::ParamChange` carries `frame`, `Rack::set_param_at` keeps
  changes sorted; a processor with `Processor::timed_params()` (CLAP, VST3, native ABI 2) gets the
  whole block, any other is split at each change by the Rack; the renderer sends a lane's value on
  frame 0, on breakpoint frames and every `render::AUTOMATION_GRAIN` (32) frames while it moves.
  Events: `Processor::process` takes `&[plugin::Event]` (notes, controllers, bend, pressure); ABI 1
  gets notes only; stock instruments come from `TABLES_V2` (ABI 2), stock effects stay ABI 1. Clip
  controllers are `model::Controller` in `ClipData::Midi.controllers` (absent when empty), helpers in
  `controllers.rs` (`window`, `reverse`, `playback`, `recorded`), commands in
  `control_controllers.rs`; the renderer remembers what each instrument was last sent (`applied`),
  chases only differences on locate and rests bend/pedal/pressure at stop. Live MIDI controllers
  are `Message::RoutedControl`. CLAP gets controllers as MIDI only when its note port speaks MIDI;
  VST3 through `IMidiMapping` (`Shared.midi_map`), one queue point per value; AU through
  `Event::to_midi`. Lane UI: `desktop/src/ui/editor/controllers.rs` and `lane.rs`,
  `ui.showPanel panel=controllers`. Inserts hear a MIDI track's controllers (never its notes) when
  `Processor::accepts_events` says so (native ABI 2, CLAP note port, VST3 event bus, AU music
  effect); they ride the same per-track list, so chase and rest reach them. Channels: `Note` and
  `Controller` carry `channel` 0-15, absent when 0; a lane is (kind, number, channel); the
  renderer keeps voices and `applied` per channel and `Message::RoutedNote` carries it. Poly
  pressure is `ControllerKind::PolyPressure` (`number` = key) inside `controllers`, but the file
  writes it to its own `polyPressure` list (`ClipDataFile`/`ClipDataOut` in `model.rs`) so older
  versions and the lane UI never see the kind; it is played, chased onto notes a locate restarts
  (`Renderer::chase_poly`, keys without a note go to 0), rested at stop and ranked after note-ons. VST3 mapped
  controllers also reach the edit controller through `Shared.mapped` (atomics, read in `idle`).
  Audio clips carry `fade_in`/`fade_out` (seconds), `fade_curve`
  and `gain_db`, absent when default; build them with `ClipData::audio(src, offset)`;
  `Command::PutClip` clamps fades (`model::clamp_fades`) and `render.rs` `clip_envelope` applies
  fades, gain and the 3 ms edge ramp per sample; the arrangement's waveform drawing mirrors the curves.
  `Session.markers` (bar order, absent when empty) change only through `Command::PutMarker` /
  `RemoveMarker` from `control_arrange.rs`; marker navigation is gated by the agent's transport
  permission. Ogg is `Container::Ogg` through `vorbis_rs` (C built by `cc`, no system packages),
  block by block at `ExportOptions::quality` (0 to 1, default 0.6); `Container::for_path` refuses
  extensions ryolune does not write. Plugin folders: `control_plugins::AutoFolders` decides per
  product (vendor, kind, name without layout): name first, then `PRODUCTS`/`PRIORITY_RULES`, then
  `EFFECT_RULES`, the category last. `Store` restores redo on `cancel_gesture`; background captures
  skip over redo or an open gesture. `Daw::request` (`ui/daw.rs`) is `run` without the error dialog, for
  forms that show their own errors. `atomic_write` keeps the target's mode (0644 when new) and writes
  through symlinks. Engine regression tests live in `engine/tests/regressions.rs`. The agent's
  Changes list records only document edits that did not come from the window.
- Agent control (decided 2026-09-25): an agent starts from `session.overview`
  (`control_overview.rs`, bounded; the built-in agent gets a compact one each turn) and
  `ui.state` (live). `control::call` runs `control_refs::resolve` first, so every `trackId`,
  `clipId` and `markerId` also takes a unique name, and a wrong one lists what exists.
  Plugin parameters and programs live in `control_params.rs`: read through
  `Host::loaded_editor` (the window's instance) or a fresh one, set by name, plain value,
  0-1 or display text (`Editor::parse_text`), programs through `Editor::programs` (VST3
  program-change parameter, AU factory presets loaded into a fresh instance and saved as
  state). `docs/AGENT_PARITY.md` is the audit of window interactions against the registry;
  `engine/tests/agent_parity.rs` fails when an action of `desktop/src/ui/actions.rs` has no
  entry in `docs/agent-parity.json`, when the window sends an unknown name, or when a command has
  no real description. A new window interaction adds its row there.
- 0.9 (2026-09-25, owner asked to "improve the app" and delegated): stock voices are scaled by
  `dsp::HEADROOM` (0.5, -6 dB) because loops and chords clipped at unity; old songs play 6 dB
  quieter on stock instruments, accepted. `store::empty()` starts Drums (Drum Machine), Bass
  (Analog Bass) and Vocals (audio); strips exist only once edited, so set them with
  `entry().or_default()`. `Host::loaded_editor(insert)` serves the window's instance only when
  its plugin id and blob match the document (a new song reuses insert keys; reconcile runs a
  frame later), otherwise callers read a fresh instance. A rebuilt `Renderer` glides a
  sounding clip's envelope from the old graph (`glide_from`, 5 ms), never touching playback
  without a rebuild. `plugin.list` rows fold formats and layouts (vendor + name; CLAP, VST3, AU
  order, others under `formats`); search also matches `folder_words` and stock descriptions.
  Side panels shrink to the `theme::layout` floors (`BROWSER_MIN`, `INSPECTOR_MIN`, `AGENT_MIN`)
  so the arrangement keeps `ARRANGEMENT_MIN` at the 1120 px minimum (`WINDOW_MIN_W`).
  `ui.screenshot` grabs the window through CoreGraphics (`ui/capture.rs`, macOS only).
  Docs: `docs/COMMANDS.md` and `docs/SHORTCUTS.md` are generated and checked by tests (`RYOLUNE_BLESS=1` regenerates); `USER_GUIDE.md`, `AI_CONTROL.md` and `DEVELOPMENT.md`
  are written by hand, keep them true when behaviour changes.
- 0.10 (2026-09-27, owner asked for the next update and delegated): tempo changes live in
  `Session.tempo_changes` (`TempoPoint { bar, bpm, ramp }`, bar order, after bar 0, absent when
  empty); `transport.tempo` is the starting tempo. `engine/src/tempo.rs` `TempoMap` (steps and
  ramps linear in beats, closed-form seconds both ways) is the only place beats become seconds:
  use `Session::bars_seconds` / `seconds_bars` / `tempo_map()`, never `60 / tempo`. The renderer
  advances `position` by the segment's tempo each frame, keeps `seconds` for audio clips
  (`Scheduled.start_seconds`), resnaps it at segment ends and fills `frame_beats` for
  automation and the click. the tempo track is drawn by the arrangement
  (`desktop/src/ui/arrangement`, `ui.showPanel panel=tempo`). Commands are
  `control_tempo.rs`; MIDI export writes ramps as sixteenth steps of equal duration. Meter
  changes inside a song are not supported. Buses: a track of kind `bus` (no clips, never armed)
  sums tracks routed to it (`Track.output`) or sending to it (`Send.bus`; sends 0/1 default to
  bus-a/bus-b, up to `MAX_SENDS` 4). Tracks feed bus tracks, bus tracks feed A, B and the Stereo
  Out only (`Session::validate_routing`); `prune_routing` on track removal. The renderer's
  `order` runs tracks then buses; track outputs to the Stereo Out/A/B wait in `early_*` buffers
  delayed by the slowest bus so every path meets (PDC stages: tracks, buses, A/B, master).
  Commands in `control_routing.rs`. Audio Unit CF objects from `AudioUnitGetProperty` are the
  caller's to release (factory preset arrays, PresentPreset names); `Editor::current_program`.
- 0.12 agent and generation (owner asked 2026-10-01 for better agent integration, more agents, no
  Rhythm Lab, and generating music and instruments through provider APIs; delegated). Providers:
  `settings::Provider` has 13 variants; every one after Anthropic runs through `agent/openai.rs`
  (Chat Completions) with `Settings::base_url` / `api_key`; `Provider::hosted()` holds the fixed
  address, env names, key page and `strict` (Mistral and DeepSeek get `max_tokens`, no stream
  usage). The Settings window's agent section (`desktop/src/ui/settings_window/agent.rs`) lists them
  (a test keeps it in step); keep it, `SECRET_PATHS` and `validate`'s key loop in sync. Outside agents: `agent/clients.rs` builds the
  per-client MCP recipes (`agent.mcp`) and install links (`agent.openClient`, refused to agents).
  Generation: `control_generate.rs` shapes requests from the song (loops get tempo/key and are
  fitted to their bars), keeps results in `<data dir>/generated` with a JSON note, and places them
  (`place_audio` in control.rs, or Sample Keys via `load_sample`); the network call is
  `desktop/src/generate.rs` (ElevenLabs, Stability, fal, custom contract in docs/AI_CONTROL.md),
  run by `start_generation` in desktop control.rs and placed on the interface thread from
  `LiveWait::Generation`. `settings.generation` is agent-protected like `agent.*` and `control.*`;
  `permissions.generation` gates `generate.audio`. Sample Keys (`sample_keys.rs`, stock index 34)
  keeps its sound in its state: the insert blob is the native host's `{values, state}` document
  (`sample_keys::insert_blob`). The agent panel tabs are Chat, Generate, Changes, Takes; Rhythm Lab's
  UI is gone, its commands stay.
- 0.14 (2026-10-06, owner asked to bring ryolune to kimchi's level; delegated): design v2 (#32).
  Agent conversations are saved per song in `<data dir>/agent-conversations.json`
  (`desktop/src/conversations.rs`; `Session.id` keys them, added on load when absent), with project
  memory (32 KB, sent ahead of every request; `agent.setMemory` and `agent.deleteConversation` are
  refused to agents) and `agent.steer` (queued and joined at the next model call; Claude Code
  restarts its run). Diagnostics (`engine/src/diagnostics.rs`, `desktop/src/diagnostics.rs`):
  `<data dir>/logs` (four runs, 8 MB each), `crashes/` (panic hook, recovered worker panics,
  unclean-exit marker), Settings › Diagnostics, `app.reportProblem` (refused to agents). What's New
  reads `docs/releases/*.md` built in by `engine/build.rs` (`release_notes.rs`; a test fails when
  the workspace version has no notes) and opens once after an update (`general.lastRunVersion`).
  Updates re-check every 6 h; `app.relaunch` (alias `app.restart`). Other apps:
  `engine/src/interop/` (DAWproject import/export with a report, `apps.rs` per-DAW steps and
  install paths), commands in `control_interop.rs` (`session.formats/importFrom/exportTo`, aliases
  `project.*`), first-run setup `settings.onboarding` (`app.onboarding`, `app.finishOnboarding`,
  pre-0.14 settings count as done), `app.recent`/`app.openRecent`. Docs index `docs/README.md`,
  `ARCHITECTURE.md`, `CONFIGURATION.md`, `SESSION_FORMAT.md`. kimchi pins `ryolune-engine` at a rev:
  keep the engine's public API additive.
- 0.15 (2026-10-07, overnight lsuite work, owner asleep; decisions mine): lsuite AI (the shared
  lsuite account, `account.*` commands, the `lsuite` provider, Claude Code on the plan) came in
  here and went out when lsuite went fully free (2026-10-10, see the lsuite section below).
  **Plugins** (lsuite's PLUGINS.md): `engine/src/plugin_dev.rs` holds the bundle manifest
  (`plugin.toml`, toml crate), `plugin.info/enable/disable/remove/guide/toolchain/new/
  writeSource/build/publishLocal`, `plugin.rescan` is an alias of `plugin.scan`, and
  `plugin.install` takes bundles. Crates live in `~/.lsuite/plugins-src/ryolune/<name>/`: the
  Cargo.toml takes `ryolune-plugin` by git URL and tag `v<version>`; `.cargo/config.toml`
  replaces that source with a directory source holding the SDK built into the app
  (`SDK_FILES`, include_str! of `sdk/src`, written to `.sdk/<version>/`) plus a seeded Cargo.lock
  (cargo refuses a replaced git source without one; a [patch] still fetches the tag, which does
  not exist before a release). Builds share `plugins-src/ryolune/.target`
  (`RYOLUNE_PLUGIN_TARGET_DIR`). Bundles go to `~/.lsuite/plugins/ryolune/<id>/` (scanned as a
  native folder) under a new library name per build, the previous one retired, because dlopen
  hands back the library already loaded for a path; the window's `adopt_catalog` retires the
  inserts whose library moved so `reconcile_plugins` reloads them (hot reload). Disabled ids are
  `settings.plugins.disabled` (out of `plugin.list` unless `includeDisabled`, refused by
  `choose`; songs still play them). `permissions.plugins` (off) gates the build commands; the
  Plugins window's Build turns it on (pressing it is the person asking). Test binaries probe
  plugins in-process (`probe_isolated`), they cannot be the `--scan-plugin` child.
  `engine/tests/plugin_dev.rs` runs the real recipe with cargo (skipped without cargo).
  The Plugins window is `ui/dialogs/plugins.rs` (sheet, four parts, `ui.showPanel
  panel=plugins section=…`, Mix › Plugins… ⌘⇧P, Agent › Build a Plugin…). Examples on the SDK:
  `plugins/bitcrusher`, `plugins/chorus` (with `plugin.toml`). Logos of formats and DAWs are in
  `desktop/assets/logos` with `NOTICE.md` (REAPER's is Wikipedia fair use: replace it with one
  from Cockos when possible; Logic Pro, GarageBand and Audio Units show Apple's logo and Studio
  One PreSonus's, no SVG of their own exists); `ui/dialogs/apps.rs` is the logo grid of setup
  and Import/Export for Another App. Idle: `reconcile_plugins` returns at once while settled
  (`Bank::settled`), and waiting only on workers no longer ticks per frame (`Ryolune::busy`).
  The mark generator follows kimchi's constants (ring of even weight, kimchi's dither ramp and
  scale); `ryolune.icns` needs `scripts/make-icon.sh` on a Mac (iconutil).
- 0.16 (2026-10-07, lsuite's HARNESS.md and DISTRIBUTION.md; delegated): **the agent harness**
  lives in `engine/src/harness/`: `brief.md` (one source: the built-in agent's system prompt is
  `harness::brief()` plus a paragraph in `desktop/src/agent/mod.rs` `BUILT_IN`; `ryolune-mcp`'s
  `instructions` are a mode line plus the brief), `skills/*.md` (13, front matter `name`/`title`/
  `when`, each with `## Steps` and `## Checks`; a test checks every backticked `family.action` in
  them and in the brief is a real command; add a skill to `SKILL_FILES`), `changes.rs` (a song
  diff in plain words), `loudness.rs` (BS.1770-4/R128 K-weighting, gating, LRA, 4x true peak,
  tested on Tech 3341/3342 cases), `analysis.rs` (FFT, Welch spectrum, waveform columns),
  `look.rs` (offline render of a bar range like `export::mix`, then SVG → PNG with resvg 0.45
  `text` and the built-in IBM Plex Mono). Commands `harness.*` (brief, skills, skill, context,
  look, measure, checkpoint, checkpoints, changes, revert). `Store` holds `marks` (per agent key,
  the song as it last saw it; the window moves `agent`/`mcp` in `record_agent_activity`) and
  `checkpoints` (24, cleared by `load`); a revert is `Command::RestoreTake` of the snapshot, so one
  undo step. The built-in agent: a checkpoint per turn (`agents.rs` `TurnRecord`, Revert turn card
  in the chat and Changes tab, `agent.revertTurn`), the live context before every model step
  after the first (`Event::Context`, answered as a `harness_context` call that is not shown),
  pictures as image blocks (`Part::ToolResult.images`, `#[serde(skip)]`, dropped from the history
  when a turn ends; `sees_images` lists the providers that get them; Codex gets text only).
  `harness.look`/`measure` run on a worker in the window. `ryolune-mcp`: skills as prompts and
  resources, image content, a checkpoint before a connection's first edit, the person's changes
  and a state line on results, plus a finish-routine reminder after edits until it looks or
  measures (`--no-context` drops both), also under `harnessNotes` in `structuredContent`
  because Claude Code shows that instead of the text. The CLI prints a look's path, not its base64.
  `docs/HARNESS.md` is generated (command_docs test). Evals: `evals/` (Python runner, 13 jobs,
  Claude Code + `ryolune-mcp --file`, `RESULTS.md`). **Updates through lsuite**:
  `desktop/src/update.rs` reads `<server>/api/apps/ryolune/releases/latest` (public since
  2026-10-10: no account, no token, no Authorization header; `release_url()`, server =
  `LSUITE_SERVER`, else lsuite.xyz), downloads through the server's file route, keeps the
  signature checks, `RYOLUNE_UPDATE_URL` for tests. Release workflow makes a draft;
  `scripts/publish-build.sh <version>` copies it to `ludovic111/lsuite-builds` as
  `ryolune-v<version>` and deletes the draft (tests in `scripts/tests/release_workflow.py`).
- Tests: a cargo test binary resolves settings, data, the control file, `~/.lsuite` and the kimchi
  library to a per-process scratch folder when no override is set (`host::scan::test_sandbox`);
  still run tests and the app with `RYOLUNE_SETTINGS`, `RYOLUNE_DATA_DIR`, `RYOLUNE_CONTROL` and
  `LSUITE_HOME` in a scratch folder (a test run without them overwrote the owner's settings once).
- Parallel worktrees must not share `CARGO_TARGET_DIR`: cargo can link another worktree's
  `ryolune-engine` into yours. The public page is lsuite.xyz/ryolune (`ryolune/index.html` in
  ludovic111/lsuite); each release updates its "New in" section, changelog and captures (the
  version is filled in from the latest GitHub release).
- Money (owner's decision, 2026-09-29): ryolune is MIT and free forever, every update included; the
  only income is optional donations, once or monthly, through GitHub Sponsors behind
  lsuite.xyz/ryolune/support (`SUPPORT_URL` in desktop control.rs, since 2026-10-01; the retired
  standalone site defaulted ryolune.com/support to `SPONSORS_URL`). Nothing
  is sold or locked, so copy says donate or sponsor, never pay, price or checkout. The app asks once,
  after the third export (`SUPPORT_AFTER_EXPORTS`); a quiet Sponsor key sits at the right of the
  title bar (`app.openGuide guide=support`), and `.github/FUNDING.yml` shows GitHub's Sponsor button.
  Since 2026-10-10 (owner's decision) the whole suite is free: no lsuite account, no lsuite AI,
  no plan; agents are the person's own (Codex, Claude Code, API keys, Ollama).
- Every persistent UI edit dispatches `store::Command`. Keep drag previews local and group
  continuous edits with `Store::set_gesture`. Preserve undo and source/clip alignment.
- No allocations, deallocations, blocking, I/O or logging in the audio callback. Compile graphs
  on workers, transfer through bounded queues, and reclaim old graphs outside the callback.
- Plugins (`engine/src/plugin.rs`, `engine/src/host/`, `engine/src/stock.rs`): every insert and
  instrument is an `Instance` (main-thread `Editor` + audio-thread `Processor`). Processors live
  in the callback's `Rack`, keyed by a numeric slot (`desktop/src/plugins.rs` maps insert keys to slots), and survive renderer rebuilds; a new `Renderer`
  must `adopt` the old one so held notes are released or chased. Create, activate, save state
  and destroy plugins on the UI thread only; unmount through the queue and wait for retirement
  before dropping an editor. Parameter values are document state (`Insert.params`) so they undo;
  external plugin state is captured into `Insert.blob` on save and bounce. Scan bundles only in
  the `--scan-plugin` child process. New stock DSP goes in `stock.rs` behind the same traits.
- Platform streams belong to their owning workers; never force Send with an unsafe impl.
- File operations must preserve the old file on failure. Keep v1 `.ryolune` loading covered by tests.
- Run fmt, clippy with warnings denied, and workspace tests. Check a real native window after
  UI changes. Distinguish tests, builds, actual device checks and public signing/notarization.
- Work on a branch. Do not merge or publish a release without the owner's request.
- Releases: bump the workspace `version` in `Cargo.toml`, add `docs/releases/X.Y.Z.md`, run the
  evals (`python3 evals/run.py --record`), then push a matching `vX.Y.Z` tag.
  `.github/workflows/release.yml` builds Linux (the only platform during the lsuite beta), writes and signs `SHA256SUMS` (Ed25519,
  secret `RYOLUNE_SIGNING_KEY`, public key in `desktop/assets/update-signing.pub`) and leaves a
  draft release; `scripts/publish-build.sh X.Y.Z` publishes it to lsuite-builds, which lsuite.xyz
  serves to `desktop/src/update.rs`, which installs after verifying the signature, the download
  location and the new binaries' versions.
  Keep the asset names in `update::asset_name` and the workflow in sync. The secret key stays in
  `~/.ryolune/keys/update-signing.key` on the owner's machine; never commit it. Builds are ad-hoc
  signed, not notarized.
- The former Electron / TypeScript app and the retired site are in git history at 7fb6116,
  the parent of the commit that removed them; they are reference only.

## lsuite: bring ryolune up to the suite standard (next session; notes updated 2026-10-01)

ryolune is part of **lsuite** (lowercase), the free open-source creative suite with kimchi
(video). Two documents in ludovic111/lsuite (locally `../lsuite/`) are the
contract: `STANDARD.md` (every action a command, CLI + MCP + built-in agent on one registry,
signed auto-update, apps that work together) and `design/DESIGN.md` (the shared design system,
live at lsuite.xyz/design). ryolune is the reference implementation of the standard.

Done: 0.12.0 released and notarized (2026-10-01; the Apple developer agreement had to be
accepted). The notarization key is now "ryolune notarization" (`APPLE_API_KEY_*` secrets); the old
"Ondera notarization" key is revoked. The public page is lsuite.xyz/ryolune; ryolune.com is a
Porkbun 301 there with the path kept.

Still to do:

- [x] **Design system** (0.13, 2026-10-02: done in the GPUI window). v2 (black and white, grain,
      square, kimchi's organization; new mark and icon) on branch `design-v2` (2026-10-06), to be
      released separately; see Theme above.
- [x] **Discovery** (0.13: `~/.lsuite/apps/ryolune.json`, format 1 in `engine/src/lsuite.rs`,
      written by `desktop/src/discovery.rs`; `app.suite` reads every app's. Documented in lsuite's
      STANDARD.md on branch `claude/ryolune-discovery-handoffs` of the lsuite repo, not merged.)
- [x] **Hand-offs** (0.13: `export.toKimchi`, `session.scoreCut`, `handoff.inbox` in
      `engine/src/control_suite.rs`; kimchi still has to take its inbox and send cuts.)
- [x] **Shared command names** (0.13: `control::ALIASES`: `app.version`, `project.overview`,
      `export.audio`, `export.stems`, `export.midi`.)
- [x] **Site** (0.12 page done 2026-10-01): keep updating lsuite.xyz/ryolune
      (`../lsuite/ryolune/index.html`) with every release: what's new, features, captures
      (`../lsuite/assets/img/ryolune/`). The version shown comes from the latest GitHub release.
- [x] Point `SUPPORT_URL` (desktop/src/control.rs) at `https://lsuite.xyz/ryolune/support` in the
      next release.

- [x] **Agent harness** (0.16, lsuite's HARNESS.md parts 1-7): brief, 13 skills, `harness.*`,
      live context per model step, `harness.look`/`measure` (picture + LUFS/true peak), the
      finish routine, one undo per turn, `evals/`. Part 8 (the suite agent) belongs to the lsuite
      app.
- [x] **Distribution** (0.16, lsuite's DISTRIBUTION.md): updater on `<server>/api/apps/ryolune/
      releases/latest` (public, no token since the suite went free); draft releases +
      `scripts/publish-build.sh`.
      `lsuite-builds` exists; 0.16.0 is published there by `scripts/publish-build.sh`. The
      coordinator turns the old public releases into drafts once all five apps are out.
- [x] **Linux only for the beta** (owner's decision, 2026-10-08, via the lsuite coordinator):
      while lsuite is in beta, ryolune is built and shipped for Linux only; macOS and Windows are
      "coming soon". The platform code stays and builds from source; `release.yml`'s matrix,
      the Checksums step, `scripts/publish-release.sh`, `resume-release.yml` and `native.yml`
      list Linux alone (their comments say what to add back). The macOS and Windows files were
      deleted from every existing ryolune release (public GitHub releases and `ryolune-v*` in
      lsuite-builds); their signed `SHA256SUMS` stay as they were. README, USER_GUIDE,
      DEVELOPMENT and the 0.16.0 notes say "beta for Linux, macOS and Windows coming soon".
- [x] **lsuite is fully free** (owner's decision, 2026-10-10, via the lsuite coordinator): no
      lsuite Pass, lsuite AI, lsuite account, Cloud or Marketplace. Removed here: `account.rs`,
      `control_account.rs` and the `account.*` commands, `Provider::Lsuite` (old settings with
      `"lsuite"` load with the default, Codex, through `without_retired_provider`), Claude Code's
      `claudeThroughLsuite` (an old field is ignored), the window's account state, the lsuite AI
      card and the allowance line. The updater fetches `<server>/api/apps/ryolune/releases/latest`
      and `SHA256SUMS(.sig)` with no Authorization header (`LSUITE_SERVER`, else lsuite.xyz;
      `LSUITE_ACCOUNT_SERVER` is gone). An old `~/.lsuite/account.json` is ignored, never
      deleted. Plugins (PLUGINS.md) stay. Agents are the person's own: Codex, Claude Code, API
      keys, Ollama, LM Studio, compatible servers.
- [ ] Harness gaps: a picture of the window is only on macOS (`ui.screenshot`); the Codex
      provider gets the numbers of a look, not the picture; evals run through MCP (Claude Code),
      not through the built-in agent's own loop.

When done, tick these, and update the status table at the end of `../lsuite/STANDARD.md`.
