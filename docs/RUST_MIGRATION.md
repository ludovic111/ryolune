# Rust migration — 2026-09-12

The owner explicitly chose a fully native Rust interface. The active build is a Cargo workspace
with `ryolune` (desktop), `ryolune-engine` and `ryolune-tools` (CLI/MCP). The former source was
preserved unchanged in `legacy/` until its removal; it is in git history at `7fb6116`. Version
0.2 integrates the native app, plugin hosting, agent control and updater. See [0.2.0 release notes](releases/0.2.0.md).

## Delivered behavior

| Area | Native implementation |
| --- | --- |
| Interface | egui/wgpu window, warm graphite palette, Library/Agents tabs, arrangement, piano roll, step/score views, inspector, menus and shortcuts |
| Commands | typed serializable commands, validated atomic batches, 200 undo steps, gesture grouping, dirty/save tracking |
| Tracks/regions | add, rename, reorder, colour, mute, solo, arm, fader, pan; region create/move/trim/split/duplicate/delete |
| MIDI | note draw/move/resize/velocity/delete, preset selection, previews, nine original loop patterns, hardware MIDI with live CC64 sustain, musical typing, SMF type 0/1 note import and type 1 export |
| Audio | device-driven transport, native synthesis, decoded file playback/resampling, eight inserts per track/aux/master strip, post-mute sends, metering, metronome and cycle |
| Documents | v1 JSON projects, embedded lossless float WAV, validation before loading, atomic replacement on saving |
| Recording | selectable audio and MIDI inputs, bounded audio queue, MIDI regions; linear takes only; available from GUI, CLI and MCP |
| Plugins | stock, CLAP, VST3 and macOS Audio Units; cached isolated scanning, generic parameters, state restore; native editors on macOS |
| Export | worker-based stereo WAV or selected track stems, 44.1/48/96 kHz, PCM16/PCM24/float32, dither, arrangement/range and 0–120s tail, static latency alignment |
| Platforms | native audio and graphics backends for macOS, Linux and Windows; four-runner CI matrix including Intel and Apple Silicon Macs |
| Control | 78-command registry (`engine/src/control.rs`) shared by the window, CLI and MCP; compact session inspection, stock-only catalog and searchable plugin pages; authenticated live socket or headless file hosting |
| Agents | explicit natural-language Codex CLI tasks with Run/Stop and bounded progress, authenticated bridge status and enable/disable, copied MCP/CLI setup, searchable command schemas, validated JSON execution, bounded result/error history with revisions |
| Automation | editable read lanes for track volume/pan, master fader and instrument/insert parameters; linear/step points in quarter-note beats, undo/save and playback/export |
| Recovery | separate generated snapshots of dirty sessions every 30s when idle; one worker, generation/revision guards, manual restore with unsaved-edit prompt |
| Packaging | application plus matching CLI/MCP companions in macOS bundles and Linux/Windows archives; checksummed release assets and desktop updates |

## Original port evidence (historical baseline)

The following measurements and counts were recorded during the original native port. They are
retained as historical evidence, not as the validation report for version 0.2.0.

- macOS Apple Silicon: release build and native window launched, demo screenshot inspected.
- 31 regression tests pass locally: 23 engine tests and 8 UI/device lifecycle tests. They include
  real-time allocation/deallocation counts, sample equality between playback DSP and exported
  WAV, real pointer gestures, keyboard undo and safe deferred quit after finishing a take.
- Local optimized benchmark: 30 seconds of Nightfall at 48 kHz / 128-frame blocks rendered
  in 0.550 seconds (54.6× real time, 1.83% average render time / audio time, p99 block
  0.071 ms against a 2.667 ms deadline; no voice overflow). This excludes UI/device/OS scheduling and does not
  establish a speedup over Electron. Re-run `cargo run --release -p ryolune-engine --example benchmark`.
- Native GUI playback advanced the sample-driven position on a 48 kHz output and showed live
  DSP activity. Track rename and undo were exercised in the packaged app. Arrangement move,
  ruler selection and piano drawing also have event-driven GUI regression tests.
- Headless loading accepted the original 8-track/13-region Nightfall fixture. Its native bounce
  was decoded/inspected as 27 seconds of stereo PCM WAV, 48 kHz / 24-bit, including effect tail.
- An input-device initialization stall observed during GUI QA led to moving all stream ownership
  and initialization to dedicated workers. Finalizing a recording is also asynchronous; save,
  export and document replacement wait for the take. The UI remains separate from OS permission waits.

Local review artifacts (ignored by Git): `dist/ryolune.app`, `dist/ryolune-macos.zip`,
`artifacts/native-macos.png`, `artifacts/nightfall.ryolune` and `artifacts/nightfall-native.wav`.
Cross-platform builds, test logs and binaries are attached to the
[Native Rust workflow](https://github.com/ludovic111/ryolune/actions/workflows/native.yml).

The final delivery note and CI runs contain the latest platform verification results.

## Initial plugin hosting evidence (2026-09-12, historical baseline)

- 43 workspace tests pass (32 engine, 11 interface), including real-time allocation counts with
  four stock inserts mounted, held-note reconciliation across graph swaps, live notes surviving
  swaps, MIDI takes becoming regions, master/bus strips and the scan cache.
- `ryolune --scan-plugins` on the owner's Apple Silicon Mac found 22 CLAP, 1085 VST3 and 1102 Audio
  Unit entries in 2 m 17 s with isolated probing; one VST3 (UAD Console Recall) printed to stdout
  during its probe, which the parser now tolerates.
- `probe` instantiated FabFilter Pro-Q 4 as CLAP (600 parameters) and VST3 (736), Pro-C 2 as
  CLAP and AU, Apple AUDelay and the DLSMusicDevice instrument: parameters listed, one second
  processed with finite output, state saved and restored, instances destroyed cleanly.
- A session with Pro-Q 4 (CLAP) on the bass, DLSMusicDevice (AU) as the keys instrument and
  Pro-C 2 (AU) plus the stock Limiter on the master bounced headlessly in 1.3 s; the master
  compressor lowered the mix RMS from 0.146 to 0.091.
- The engine and desktop crates also pass `cargo check` for `x86_64-pc-windows-msvc`.

## Known limits

- This is a first native port, not a claim of exhaustive production DAW qualification.
- Native synths, EQ, reverb and dynamics differ from Web Audio. MIDI events and imported PCM
  survive migration, but old mixes and regenerated demo sound are not bit-identical.
- Stereo files are linearly resampled. No time stretching or high-quality offline resampling
  mode. Static plugin delay compensation aligns parallel track/aux
  paths using the reported latency when the graph is prepared; arbitrary dynamic latency
  changes and measured audio-interface recording compensation are not implemented.
- Automation uses absolute quarter-note beats, holds the first/last value outside the point
  range and supports linear or step interpolation. Track/master controls are sample-accurate;
  plugin values update at processing boundaries of at most 256 frames. There are no touch,
  latch or write recording modes, clip-follow lanes or MIDI CC envelopes. The model is bounded
  to 256 lanes and 65,536 total points.
- Plugin hosting (added 2026-09-12, see `PLUGINS.md`): CLAP, VST3 and Audio Units load in
  process; scanning is isolated. Native editor windows exist on macOS only. Plugins that need
  sidechain inputs, more than one audio bus or 64-bit processing get their main stereo bus only.
  VST2 and AAX are unsupported. Scan success is not a guarantee of compatibility with every
  preset, editor or processing configuration. Runtime external DSP is in process.
- Live control runs open, save, bounce, import and plugin scanning on workers. Commands which
  conflict with an active operation return a busy error; clients should wait for completion
  before continuing. Parameter/state inspection still needs the live plugin lifecycle.
- Recording requires system microphone permission, uses the chosen input and captures linear
  takes. No loop-take comping or measured hardware latency compensation. A dropped/overflowed
  take retains contiguous partial audio with an explicit warning. Completed microphone takes
  receive a separate float32 WAV in the app data recordings folder; source files are retained.
  Failed placement reports that path. Failed disk backup retains the take in memory and blocks
  document replacement/quit until Audio > Save recovered take succeeds. A crash during an active
  take cannot recover audio that has not finished recording.
- The score view is a pitch overview, not an engraving/notation editor.
- Live MIDI supports CC64 sustain and captures its duration in recorded notes. Imported SMF
  controller data, including CC64, is still ignored with warnings. Pitch bend, MPE and general
  CC control are not implemented.
- The Agents tab runs explicit tasks through an installed, authenticated Codex CLI, or connects
  another MCP client. It does not bundle a language model. Prompts and tool results go to the
  selected provider. Each Codex run ignores user tool configuration and exposes only this
  window's ryolune MCP tools, with shell/web/apps/plugins disabled and a read-only filesystem sandbox.
  CLI support and authentication are checked at runtime; existing edits survive cancellation.
- Recovery is periodic rather than continuous. File/recording operations and active editing
  gestures postpone snapshots, and only changed revisions are saved. Originals are preserved;
  recovery is user-invoked and opens a copy with a new save destination. Recovery failures and
  latest snapshot status are shown in the picker. Snapshots remain in the app data directory.
- Arrangement voices are bounded; overflow is counted. This is not a hard-real-time OS guarantee.
- Device hot-unplug reports failure; reconnect through the Audio menu. Device matrices, long
  sessions, sleep/wake and audio-driver latency need physical checks on each target system.
- Packaging is ad-hoc signed (macOS) or unsigned (Windows, Linux). GitHub releases and in-app
  updates work with that, but first launches of a browser download need a Gatekeeper override
  on macOS and a SmartScreen override on Windows. Public distribution still needs Developer ID
  signing/notarization and Windows code signing.
- Updates (added 2026-09-12): the app reads the latest GitHub release, verifies the asset
  against `SHA256SUMS` and swaps itself in place. There is no delta update, no rollback UI
  (the previous macOS bundle is removed on the next start) and no update channel other than
  the latest release. Linux/Windows updates validate the three-binary ZIP and matching versions,
  replace all three together with rollback on an install failure, and retain backups until the
  new release starts. Bare desktop assets remain for compatibility with the 0.1 updater.
