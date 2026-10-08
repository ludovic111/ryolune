# Developing ryolune

How the code is laid out, how to build and check it, and how a release is published. The rules
contributors (human or AI) follow are in [`CLAUDE.md`](../CLAUDE.md) at the repository root.

## Layout

```
desktop/   the app: the host that owns the document and audio, settings, the agent runtime
    │      (desktop/src/agent), native plugin windows, live-only commands, updates, and the
    │      window drawn with GPUI (desktop/src/ui: theme on the lsuite tokens, the action
    │      table behind menus and shortcuts, controls, every panel; see its README.md)
tools/     ryolune-cli and ryolune-mcp: thin clients of the command registry, live or on a file
engine/    command registry (control*.rs), session model and validation, undo/redo, documents,
    │      DSP and stock plugins, plugin hosts (native ABI, CLAP, VST3, Audio Units), scanning,
    │      settings, presets, MIDI, devices, rendering and export
    └──    CPAL output callback → renderer + plugin rack → system audio
           CPAL input callback → meter, monitor ring and takes → workers
sdk/       ryolune-plugin: the Plugin trait, DSP primitives and the frozen C ABI
plugins/   example native plugin bundle (plugins/gain) and the ABI 1 fixture
```

**The command registry is the contract.** Every user-facing action is a command in
`engine/src/control.rs` and its `control_*.rs` families (window-only ones are served by
`desktop/src/control.rs`). The window, `ryolune-cli`, `ryolune-mcp` and the built-in agent all
call it; the CLI help, the MCP tool list, the agent's tools and [COMMANDS.md](COMMANDS.md) are
generated from it. A new action lands as a command first, then the interface calls it.

**Audio thread rules.** The callback performs no allocation, deallocation, locking, file access
or logging. Graphs are compiled on workers and handed over through bounded queues; old graphs and
unmounted plugins are reclaimed off the audio thread. Plugin processors live in a rack the
callback owns; a rebuilt renderer adopts the old one (transport, held notes, controller state,
the envelope of sounding clips) so edits during playback are seamless.

**Time and routing.** Positions are bars and beats everywhere; `engine/src/tempo.rs` turns them
into seconds through the song's tempo map (a starting tempo plus steps and linear ramps), and
the renderer advances by it frame by frame while audio clips keep a clock in seconds. Channels
are processed in stages: tracks, then bus tracks (what tracks route or send to them), then the
A and B returns, then the master, with plugin delay compensation per stage.

**Capacity** is explicit: 128 tracks (up to 32 of them buses), 1000 tempo changes, four sends
per strip, 256 simultaneous arrangement events, 32 voices per stock
instrument, 8 inserts per strip, 1024 rack slots, 512 MiB per decoded source, 1 GiB of session
audio and 4-hour exports.

## Build

Install Rust 1.88 or newer; nothing else builds the interface (GPUI compiles its Metal
shaders when the window opens, so no Metal toolchain is needed either):

```sh
cargo run --release
```

Use release builds for real-time audio. Platform prerequisites:

- **macOS**: the Xcode command-line tools.
- **Windows**: Visual Studio C++ build tools and the Windows SDK, and the MSVC Rust toolchain.
- **Ubuntu/Debian**:

  ```sh
  sudo apt-get install build-essential pkg-config libasound2-dev libudev-dev \
    libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-dev libx11-xcb-dev \
    libxcb1-dev libfontconfig1-dev libfreetype6-dev libvulkan-dev libdbus-1-dev \
    libssl-dev libzstd-dev
  ```

  The window draws with Vulkan (`libvulkan1` and a driver, such as `mesa-vulkan-drivers`).

  File dialogs use the desktop portal (`xdg-desktop-portal` and a backend).

The executables are `target/release/ryolune`, `ryolune-cli` and `ryolune-mcp`; keep them together.
`bash scripts/package-macos.sh` builds `dist/ryolune.app` and a zip: ad-hoc signed, or signed with
a Developer ID and notarized when `APPLE_SIGNING_IDENTITY` (and `APPLE_API_KEY_PATH`,
`APPLE_API_KEY_ID`, `APPLE_API_ISSUER`) are set, as in the release workflow below.

## Checks

Run all of these before a pull request:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Point the Rust tests at scratch settings so they never touch your own:
`RYOLUNE_SETTINGS=/tmp/ryolune-test/settings.json RYOLUNE_DATA_DIR=/tmp/ryolune-test/data`.

Generated documentation is checked by tests. After changing a command or a shortcut:

```sh
RYOLUNE_BLESS=1 cargo test -p ryolune-tools --test command_docs     # docs/COMMANDS.md
RYOLUNE_BLESS=1 cargo test -p ryolune shortcuts                     # docs/SHORTCUTS.md
```

## Checking the real window

`target/debug/ryolune --screenshot window.png` opens the demo song, captures the window and
quits. To look at other states, run the app with a scratch profile and drive it with the CLI
(`LSUITE_HOME` keeps its lsuite discovery entry out of `~/.lsuite` too):

```sh
export RYOLUNE_DATA_DIR=/tmp/ryolune-check RYOLUNE_CONTROL=/tmp/ryolune-check/control.json \
  RYOLUNE_SETTINGS=/tmp/ryolune-check/settings.json LSUITE_HOME=/tmp/ryolune-check/lsuite
RYOLUNE_NO_UPDATE=1 target/debug/ryolune &
target/debug/ryolune-cli app.info                       # repeat until it answers
target/debug/ryolune-cli session.overview
target/debug/ryolune-cli ui.showPanel --panel mixer --visible true
target/debug/ryolune-cli ui.screenshot --path /tmp/ryolune-check/window.png
target/debug/ryolune-cli app.quit --discard true
```

The app binary also validates and renders without a window:

```sh
cargo run --release -- --validate song.ryolune
cargo run --release -- --bounce song.ryolune mix.wav
cargo run --release -- --scan-plugins
cargo run --release -p ryolune-engine --example probe -- "clap:com.example.plugin"
```

## Logs and crash reports

The window process logs through the `log` facade to `<data dir>/logs/ryolune.log` and echoes
every line to stderr (`engine/src/diagnostics.rs`, installed in `desktop/src/main.rs`). The
data dir is `RYOLUNE_DATA_DIR` when set, else the platform folder (`~/Library/Application
Support/ryolune`, `%APPDATA%\ryolune`, `~/.local/share/ryolune`). Each start rotates the log to
`ryolune.1.log` … `ryolune.3.log`; a file past 8 MB rolls over too. `RYOLUNE_LOG=debug` (or
`trace`, `warn`…) sets the level for ryolune's crates, `info` by default; other crates log
warnings and errors. The settings' API keys are masked in every line. Never log from the audio
callback.

- A panic nothing catches writes `<data dir>/crashes/crash-<time>-<pid>.txt` (message, thread,
  backtrace, system, the last log lines) before the default hook runs. Background work wraps its
  closure in `diagnostics::catch("what", ...)` instead of `catch_unwind`, so its panic becomes
  a `recovered-…` report and the job fails without taking the app down.
- `logs/running-<pid>.json` exists while a window runs; a clean quit removes it. One left by a
  process that is gone becomes an `unclean-…` report at the next start.
- `app.logs`, `app.crashReports`, `app.clearCrashReports` and `app.diagnostics` read them
  (Settings › Diagnostics shows the same). Debug builds take `RYOLUNE_CRASH_TEST=main` (a panic
  on the interface thread) or `=worker` (a recovered one) to check the reports.

## Releases

1. Bump `version` in the workspace `Cargo.toml` and add `docs/releases/X.Y.Z.md`. The notes
   are built into the app (`engine/build.rs`) for What's New and `app.whatsNew`; a test fails
   when the version has no notes file.
2. Run the agent evals (`python3 evals/run.py --record`, see `evals/README.md`): a harness change
   that lowers the pass rate does not ship.
3. Update the public page, lsuite.xyz/ryolune: `ryolune/index.html` in the lsuite repo
   (ludovic111/lsuite).
4. Merge to `main`, then push a matching tag: `git tag vX.Y.Z && git push origin vX.Y.Z`.
5. When the workflow is done, publish the build to lsuite: `scripts/publish-build.sh X.Y.Z`.

The `Release` workflow builds macOS (arm64 and x86_64), Linux and Windows, writes and signs
`SHA256SUMS` (Ed25519, repository secret `RYOLUNE_SIGNING_KEY`, public key in
`desktop/assets/update-signing.pub`) and leaves a complete **draft** release: since 0.16 the
builds are not public (lsuite's DISTRIBUTION.md). `scripts/publish-build.sh X.Y.Z` (needs `gh`
with access to both repositories) checks the draft's files against `SHA256SUMS`, copies them and
the notes to the private `ludovic111/lsuite-builds` as `ryolune-vX.Y.Z`, and deletes the draft.
lsuite.xyz serves that release to signed-in lsuite accounts: the updater reads
`<server>/api/apps/ryolune/releases/latest` with the account's token and downloads through the
server's file route. Installed apps verify the signature, the download location and the new
binaries' versions before replacing themselves. `RYOLUNE_UPDATE_URL` points the updater at
another release document (tests). A new key pair comes from `ryolune --release-keygen <file>`.
Windows binaries are unsigned.

### Signing and notarizing for macOS

Without Apple secrets the macOS app is ad-hoc signed and Gatekeeper blocks its first launch. With
all six repository secrets set, `scripts/prepare-apple-signing.sh` imports the certificate into a
throwaway keychain and `scripts/package-macos.sh` signs every executable with the hardened runtime
and `desktop/ryolune.entitlements` (microphone; library validation, JIT and writable code for
third-party plugins), notarizes with `notarytool`, staples the ticket and checks it with `spctl`.
Setting only some of the secrets stops the release.

| Secret | Where it comes from |
| --- | --- |
| `APPLE_CERTIFICATE_P12_BASE64` | A **Developer ID Application** certificate (developer.apple.com › Certificates, created by the account holder), exported from Keychain Access with its private key as `.p12`, then `base64 -i cert.p12 \| pbcopy` |
| `APPLE_CERTIFICATE_PASSWORD` | The password chosen when exporting the `.p12` |
| `APPLE_SIGNING_IDENTITY` | The certificate's full name, for example `Developer ID Application: Jane Doe (TEAMID1234)` (`security find-identity -v -p codesigning`) |
| `APPLE_API_KEY_P8_BASE64` | App Store Connect › Users and Access › Integrations › Team Keys: a key with the Developer role, downloaded once as `AuthKey_XXXX.p8`, then `base64 -i AuthKey_XXXX.p8 \| pbcopy` |
| `APPLE_API_KEY_ID` | That key's ID |
| `APPLE_API_ISSUER` | The Issuer ID shown above the keys list |

To check a signature locally with a Developer ID in your keychain:
`APPLE_SIGNING_IDENTITY="Developer ID Application: …" bash scripts/package-macos.sh` (add the three
`APPLE_API_*` variables, with a path to the `.p8`, to notarize too). The updater's own checks
(`codesign --verify --deep --strict`, Ed25519 checksums) accept both kinds of build. The
repository has had all six secrets since 0.11.0 (team `YYJU63HSD4`, App Store Connect API key
`X89FN53K29`); the owner keeps the `.p8` in `~/.ryolune/keys`.

## Website

The public page is lsuite.xyz/ryolune, in the lsuite repo (ludovic111/lsuite, file
`ryolune/index.html`); ryolune.com redirects there. The former standalone site (`site/`) was
removed; it is in git history at `7fb6116`.
