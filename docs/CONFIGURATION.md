# Configuration

Where ryolune keeps its preferences and files, every preference it has, and every
environment variable it reads. The preferences are defined in `engine/src/settings.rs`; the
window edits them in Settings, and `settings.get`, `settings.set` and `settings.reset` reach
them from the CLI, MCP and the built-in agent.

## The data folder

Settings, presets, recovery copies and the plugin cache live in one data folder
(`host::scan::data_dir`):

| System | Folder |
|---|---|
| macOS | `~/Library/Application Support/ryolune` |
| Windows | `%APPDATA%\ryolune` |
| Linux and others | `$XDG_CONFIG_HOME/ryolune`, else `~/.config/ryolune` |

`RYOLUNE_DATA_DIR` replaces it. The first time the folder is needed, an install from before
the rename is adopted: the old `Ondera` folder (`ondera` on Linux) is renamed to the new one.
If the rename fails, the old folder is used as it is.

What is inside:

| Path | What |
|---|---|
| `settings.json` | Preferences (below). Mode 0600 on Unix. |
| `settings.json.invalid` | A copy of a settings file that could not be read, kept before defaults replace it (mode 0600). |
| `presets/` | User presets (`engine/src/preset.rs`). |
| `recovery/` | Recovery snapshots: `recovery-<run>-<pid>-<generation>-<title>.ryolune`, ordinary session files (`engine/src/recovery.rs`). |
| `plugins.json` | The plugin scan cache: one entry per bundle path with its modification time, descriptors or error (`host::scan::cache_path`). |
| `plugins/` | Native plugins installed by `plugin.install`; also scanned. |
| `generated/` | Sounds made by `generate.audio`, each with a `<id>.json` note beside it (`control_generate::folder`). |
| `recordings/` | A WAV backup of every audio take, written when the take ends (`take-*.wav`). |
| `screenshots/` | `ui.screenshot` captures when no path is given. |
| `previews/groove.wav` | The one file `rhythm.preview` writes when no path is given. |
| `agent-workspace/` | Working folder and MCP config (`mcp-<pid>.json`) for the Claude Code provider. |

Outside the data folder:

| Path | What |
|---|---|
| `~/.ryolune/control.json` | The live control file: `{version, port, token, pid}` for the loopback bridge, mode 0600, removed when the window closes (`control::wire::discovery_path`). `RYOLUNE_CONTROL` replaces the path. |
| `~/.lsuite/apps/ryolune.json` | ryolune's lsuite discovery entry (no secrets). `LSUITE_HOME` replaces `~/.lsuite`. |
| `~/.lsuite/handoff/<app>/` | Files handed between lsuite apps. |
| `.<song>.ryolune-lock` | Beside an open song: the advisory lock that keeps two processes from editing it (mode 0600). |

Inside a `cargo test` binary (one that runs from a `deps` folder), settings, data, the control
file, `~/.lsuite` and the kimchi library all resolve to `ryolune-test-<pid>` in the system
temporary folder unless an override is set (`host::scan::test_sandbox`). Set the overrides
anyway when you run tests or the app from source.

## The settings file

`settings.json` is pretty-printed JSON with camelCase keys. Every field has a default, so a
partial or older file loads. Writes validate the whole document first and go through
`document::atomic_write`, so the file is never half-written; the file is set to 0600 because
it may hold API keys.

The path is `$RYOLUNE_SETTINGS`, else `settings.json` in the data folder.

A file that cannot be parsed or fails validation is copied to `settings.json.invalid` and the
defaults are used; the next change writes the defaults over the original. A file larger than
4 MiB is refused the same way.

### Reading and changing settings

- `settings.get` returns the whole document or one dotted path (`agent.model`), with secrets
  masked.
- `settings.set path=<dotted path> value=<value>` changes one field. Strings are converted
  for convenience: `true`, `on`, `yes`, `1` and their opposites for booleans, numbers from
  text, an empty string for `null`, and a comma-separated string for a list. Sections cannot
  be set whole, and `version` cannot be set. Setting a secret to its masked display value
  leaves it unchanged. Changing `agent.provider` clears `agent.model` and
  `agent.reasoningEffort`.
- `settings.reset` resets one path, a section, or everything.

In the window, changes go through `apply_settings` (`desktop/src/settings.rs`), which saves and
then applies them: a new output device or buffer size reopens the audio output, a new MIDI
input reconnects, and `control.enableBridge` starts or stops the bridge.

### Secrets

These paths hold API keys (`settings::SECRET_PATHS`). `settings.get`, the registry and any
display show them masked by `Settings::redacted`: `••••` followed by the last four characters,
or `••••` alone for a key of six characters or fewer.

`agent.anthropicApiKey`, `agent.openaiApiKey`, `agent.geminiApiKey`,
`agent.openrouterApiKey`, `agent.mistralApiKey`, `agent.groqApiKey`, `agent.deepseekApiKey`,
`agent.xaiApiKey`, `agent.compatibleApiKey`, `generation.elevenlabsApiKey`,
`generation.stabilityApiKey`, `generation.falApiKey`, `generation.customApiKey`.

Keys must be printable and shorter than 4,096 characters.

### What agents may change

A request flagged as coming from an agent (the built-in agent, or an MCP or CLI client that
sets the `agent` flag) is checked in `run_control_command` (`desktop/src/control.rs`):

- `settings.set` and `settings.reset` need `agent.permissions.settings` (off by default).
- Even then, agents cannot change anything under `agent`, `control` or `generation`, and
  cannot reset all settings at once. These sections hold provider keys, permissions and the
  bridge; only the person changes them, in Settings.
- `agent.configure` and `agent.openClient` are refused to agents.

The permissions themselves are listed under [`agent.permissions`](#agentpermissions).

### `version`

| Path | Type | Default | What it does |
|---|---|---|---|
| `version` | integer | `1` | Format of the file. Always written as 1; cannot be set. |

### `general`

| Path | Type | Default | What it does |
|---|---|---|---|
| `general.checkUpdatesOnStart` | bool | `true` | Look for a new release when the window opens. |
| `general.installUpdatesAutomatically` | bool | `false` | Install a release found by the check without asking. |
| `general.recoveryIntervalSeconds` | integer 10-600 | `30` | Seconds between recovery snapshots of an edited song. |
| `general.confirmBeforeQuit` | bool | `true` | Ask before quitting. |
| `general.reopenLastSession` | bool | `false` | Open `lastSession` at start. |
| `general.lastSession` | string or null | `null` | The last song opened or saved. |
| `general.recentSessions` | list of strings | `[]` | Recent songs, newest first. The window keeps 10; the file may hold up to 20. |
| `general.exportsCompleted` | integer | `0` | Mixes and stem sets exported from the window, counted on this computer only. |
| `general.supportAsked` | bool | `false` | The one-time donation request after the third export was shown. |

### `audio`

| Path | Type | Default | What it does |
|---|---|---|---|
| `audio.outputDevice` | string or null | `null` | Output device by name; `null` is the system default. |
| `audio.inputDevice` | string or null | `null` | Input device by name; `null` is the system default. |
| `audio.midiInput` | string or null | `null` | MIDI input port; `null` picks the first one. |
| `audio.connectMidiOnStart` | bool | `true` | Connect the MIDI input when the window opens. |
| `audio.countInBars` | integer 0-4 | `1` | Bars of click before a recording that starts from a stopped transport. |
| `audio.meterInputWhenArmed` | bool | `true` | Open the input while an audio track is armed, so its level shows before the take. |
| `audio.bufferFrames` | integer or null | `null` | Frames per device buffer, for both output and input: a power of two from 32 to 4096. `null` leaves the system default. Smaller means lower latency and more CPU. |

### `interface`

| Path | Type | Default | What it does |
|---|---|---|---|
| `interface.appearance` | string | `"ryolune"` | The theme. There is one; any other value is refused. Older values (`graphite`, `aero`, `modern`, `skeuo`, `console`, `ink`, `neon`) are migrated on load. |
| `interface.mode` | `"dark"`, `"light"`, `"auto"` | `"dark"` | Dark or light, or follow the system. |
| `interface.scale` | number 0.75-1.75 | `1.0` | Interface zoom. The window currently clamps it to 0.75-1.5 when drawing (`ui/workspace.rs`). |
| `interface.agentPanelOpenOnStart` | bool | `false` | Open the agent panel when the window opens. |
| `interface.showTooltips` | bool | `true` | Shown in Settings; not read by the GPUI window at present. |
| `interface.followPlayhead` | bool | `true` | Shown in Settings; the arrangement follows the song's own `view.followPlayhead` instead. |

### `agent`

| Path | Type | Default | What it does |
|---|---|---|---|
| `agent.provider` | string | `"codex"` | Which provider the built-in agent uses (table below). |
| `agent.model` | string | `""` | Model name, at most 200 printable characters. Empty uses the provider's default. |
| `agent.reasoningEffort` | string | `""` | Provider effort level (letters, digits, `_`, `-`; at most 40). Empty leaves it to the provider. |
| `agent.anthropicApiKey` | string, secret | `""` | |
| `agent.openaiApiKey` | string, secret | `""` | |
| `agent.geminiApiKey` | string, secret | `""` | |
| `agent.openrouterApiKey` | string, secret | `""` | |
| `agent.mistralApiKey` | string, secret | `""` | |
| `agent.groqApiKey` | string, secret | `""` | |
| `agent.deepseekApiKey` | string, secret | `""` | |
| `agent.xaiApiKey` | string, secret | `""` | |
| `agent.ollamaBaseUrl` | string | `""` | Ollama's address when not `http://127.0.0.1:11434/v1`. http(s) URL. |
| `agent.lmstudioBaseUrl` | string | `""` | LM Studio's address when not `http://127.0.0.1:1234/v1`. http(s) URL. |
| `agent.compatibleBaseUrl` | string | `""` | The address of any other OpenAI-compatible server. http(s) URL. |
| `agent.compatibleApiKey` | string, secret | `""` | Key for that server. It never falls back to an environment variable. |
| `agent.codexExecutable` | string | `""` | Path to the Codex CLI. Empty searches `PATH`, then the usual install locations. |
| `agent.claudeExecutable` | string | `""` | Path to the Claude Code CLI. Same search when empty. |
| `agent.maxOutputTokens` | integer 256-128000 | `4096` | Largest reply per turn. |
| `agent.maxToolRounds` | integer 1-500 | `48` | Tool calls allowed in one task before the agent must answer. |
| `agent.instructions` | string | `""` | Standing instructions appended to the system prompt, at most 20,000 characters. |
| `agent.permissions` | object | see below | |

Providers (`settings::Provider`):

| Value | Service | Default model | Key from settings, then environment |
|---|---|---|---|
| `codex` | Codex CLI, its own sign-in | (CLI's own) | none |
| `claude` | Claude Code CLI, its own sign-in | (CLI's own) | none |
| `anthropic` | Anthropic Messages API | `claude-sonnet-5` | `agent.anthropicApiKey`, `ANTHROPIC_API_KEY` |
| `openai` | OpenAI, `https://api.openai.com/v1` | `gpt-5` | `agent.openaiApiKey`, `OPENAI_API_KEY` |
| `gemini` | `https://generativelanguage.googleapis.com/v1beta/openai` | `gemini-flash-latest` | `agent.geminiApiKey`, `GEMINI_API_KEY`, `GOOGLE_API_KEY` |
| `openrouter` | `https://openrouter.ai/api/v1` | `openrouter/auto` | `agent.openrouterApiKey`, `OPENROUTER_API_KEY` |
| `mistral` | `https://api.mistral.ai/v1` | `mistral-large-latest` | `agent.mistralApiKey`, `MISTRAL_API_KEY` |
| `groq` | `https://api.groq.com/openai/v1` | (choose one) | `agent.groqApiKey`, `GROQ_API_KEY` |
| `deepseek` | `https://api.deepseek.com/v1` | `deepseek-chat` | `agent.deepseekApiKey`, `DEEPSEEK_API_KEY` |
| `xai` | `https://api.x.ai/v1` | (choose one) | `agent.xaiApiKey`, `XAI_API_KEY` |
| `ollama` | Ollama on this computer | (choose one) | none |
| `lmstudio` | LM Studio on this computer | (choose one) | none |
| `compatible` | `agent.compatibleBaseUrl` | (choose one) | `agent.compatibleApiKey` only |

A key stored in settings wins; an empty one falls back to the environment variables in the
order listed (`Settings::api_key`).

#### `agent.permissions`

What the built-in agent and agent-flagged MCP or CLI requests may do
(`control_app::denied_for_agent`). Document edits are always allowed.

| Path | Type | Default | Gates |
|---|---|---|---|
| `agent.permissions.fileOperations` | bool | `true` | `session.save`, `session.bounce`, imports and exports, `export.toKimchi`, `session.scoreCut`, `plugin.scan`, `plugin.scaffold`, `plugin.install`, `preset.save`, `preset.delete`, `session.saveRecoveredTake`, `generate.delete`, and a `path` on `rhythm.preview`, `ui.screenshot` or `strip.loadSample`. |
| `agent.permissions.transport` | bool | `true` | `transport.play`, `record`, `stop`, `locate`, `returnToStart`, `punch`, `marker.goto`, `marker.next`, `marker.previous`, `note.preview`, `note.hold`. |
| `agent.permissions.replaceSession` | bool | `false` | `session.new`, `session.open`, `session.restoreSnapshot`. |
| `agent.permissions.settings` | bool | `false` | `settings.set`, `settings.reset`, `audio.setOutput`, `audio.setInput`, `audio.setMidiInput`, `audio.allowSpeakerMonitoring`. |
| `agent.permissions.appControl` | bool | `false` | `app.quit`, `app.installUpdate`, `app.relaunch`, `app.confirm`. |
| `agent.permissions.generation` | bool | `true` | `generate.audio` (it spends the generation service's credits). |

### `generation`

The service `generate.audio` uses.

| Path | Type | Default | What it does |
|---|---|---|---|
| `generation.service` | `"elevenlabs"`, `"stability"`, `"fal"`, `"custom"` | `"elevenlabs"` | Service used when a request names none. |
| `generation.elevenlabsApiKey` | string, secret | `""` | Else `ELEVENLABS_API_KEY`. |
| `generation.stabilityApiKey` | string, secret | `""` | Else `STABILITY_API_KEY`. |
| `generation.falApiKey` | string, secret | `""` | Else `FAL_KEY`, then `FAL_API_KEY`. |
| `generation.falModel` | string | `"fal-ai/stable-audio"` | The fal.ai model path (letters, digits, `-_./`; at most 200). |
| `generation.customUrl` | string | `""` | An endpoint that follows ryolune's generation contract (see `AI_CONTROL.md`). http(s) URL. |
| `generation.customApiKey` | string, secret | `""` | Key for that endpoint. No environment fallback. |

### `plugins`

| Path | Type | Default | What it does |
|---|---|---|---|
| `plugins.scanOnStart` | bool | `false` | Scan plugin folders when the window opens. |
| `plugins.extraClapPaths` | list of strings | `[]` | More CLAP folders to scan (at most 64). |
| `plugins.extraVst3Paths` | list of strings | `[]` | More VST3 folders to scan (at most 64). |
| `plugins.extraNativePaths` | list of strings | `[]` | More native plugin folders to scan (at most 64). |
| `plugins.favorites` | list of strings | `[]` | Starred plugin ids (at most 4,096). |
| `plugins.folders` | object | `{}` | Plugin id to a sound folder name (1-40 characters) that overrides the automatic one. |
| `plugins.recent` | list of strings | `[]` | Recently loaded plugin ids, newest first (at most 64). |

### `control`

| Path | Type | Default | What it does |
|---|---|---|---|
| `control.enableBridge` | bool | `true` | Serve the loopback bridge that `ryolune-cli`, `ryolune-mcp` and the CLI agent providers use. `ryolune --no-control` turns it off for one run. |

## Environment variables

### ryolune's own

| Variable | Read in | What it does |
|---|---|---|
| `RYOLUNE_SETTINGS` | `engine/src/settings.rs` | Path of the settings file. Ignored when empty. |
| `RYOLUNE_DATA_DIR` | `engine/src/host/scan.rs` | The data folder (presets, recovery, plugin cache, generated sounds, and the settings file unless `RYOLUNE_SETTINGS` is set). |
| `RYOLUNE_CONTROL` | `engine/src/control/wire.rs` | Path of the live control file, for the window that writes it and for the clients that read it. Ignored when empty. |
| `RYOLUNE_PLUGIN_PATH` | `engine/src/host/scan.rs` | Extra native plugin folders, in the system's path-list form, searched first. |
| `RYOLUNE_NO_UPDATE` | `desktop/src/main.rs` | Any non-empty value skips the update check at start (like `--no-update-check`). |
| `RYOLUNE_PRETEND_VERSION` | `desktop/src/update.rs` | Report this version instead of the real one, to exercise the update flow against a real release. |
| `RYOLUNE_BLESS` | `desktop/src/ui/dialogs/help.rs`, `tools/tests/command_docs.rs` | In tests: regenerate `docs/SHORTCUTS.md` and `docs/COMMANDS.md` instead of comparing them. |
| `RYOLUNE_AGENT_FIXTURE`, `RYOLUNE_AGENT_TAB`, `RYOLUNE_AGENT_TOP`, `RYOLUNE_AGENT_DRAFT`, `RYOLUNE_AGENT_MODELS` | `desktop/src/ui/agent_panel/fixture.rs` | Debug builds only: put the agent panel in sample states for screenshots (a sample conversation; open tab `generate`, `changes` or `takes`; stay at the top; a draft message; the model menu open). |
| `RYOLUNE_TEST_LOCK_TARGET`, `RYOLUNE_TEST_LOCK_DENIED` | `desktop/src/control.rs` (tests) | Used by a session-lock test that runs in a child process. |

### lsuite and kimchi

| Variable | What it does |
|---|---|
| `LSUITE_HOME` | Replaces `~/.lsuite` (discovery entries and hand-off folders). Ignored when empty. |
| `KIMCHI_LIBRARY`, then `KIMCHI_DATA_DIR` | kimchi's project library, where `export.toKimchi` writes when kimchi is closed. Without them, kimchi's discovery entry (`dataDir`) or its platform default is used. |

### Provider and service keys

Read only when the matching setting is empty (`Settings::api_key`,
`Settings::generation_key`): `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`,
`GOOGLE_API_KEY`, `OPENROUTER_API_KEY`, `MISTRAL_API_KEY`, `GROQ_API_KEY`,
`DEEPSEEK_API_KEY`, `XAI_API_KEY`, `ELEVENLABS_API_KEY`, `STABILITY_API_KEY`, `FAL_KEY`,
`FAL_API_KEY`. The compatible endpoint and the custom generation endpoint never read the
environment.

### Plugin folders

| Variable | What it does |
|---|---|
| `CLAP_PATH` | Extra CLAP folders, searched before the standard ones. |
| `VST3_PATH` | Extra VST3 folders, searched before the standard ones. |
| `COMMONPROGRAMFILES`, `LOCALAPPDATA` | Windows: where the standard CLAP, VST3 and native plugin folders are. |

### System

| Variable | What it does |
|---|---|
| `HOME`, else `USERPROFILE` | Home folder for `~/.ryolune`, `~/.lsuite`, the data folder and the CLI search. |
| `APPDATA` | Windows data folder (ryolune and kimchi). |
| `XDG_CONFIG_HOME` | Linux: parent of the ryolune data folder. |
| `XDG_DATA_HOME` | Linux: parent of kimchi's default library. |
| `PATH` | Where the Codex and Claude Code CLIs are looked for. |
| `CODEX_HOME` | Where the Codex CLI keeps its sign-in (default `~/.codex`); ryolune links its `auth.json` into a private config folder for each run. |

## Command-line flags of `ryolune`

From `desktop/src/main.rs`:

| Flag | What it does |
|---|---|
| `ryolune [song.ryolune]` | Open the window, optionally on a song. |
| `--validate <song>` | Load a song and report whether it is valid. |
| `--bounce <song> <out.wav>` | Render a song to WAV at 48 kHz without a window. |
| `--scan-plugins` | Scan plugin folders and print what was found. |
| `--plugins` | List the stock plugins and those in the scan cache. |
| `--scan-plugin <format> <bundle>` | Internal: the child process that probes one bundle. |
| `--screenshot <image.png>` | Open, capture the window, quit (macOS). |
| `--agents` | Open with the agent panel open. |
| `--no-control` | Do not serve the CLI and MCP bridge. |
| `--update` | Install the latest release and exit. |
| `--no-update-check` | Skip the update check at start. |
| `--release-keygen <file>`, `--sign-release <key> <file>`, `--verify-release <file>` | Release signing tools (see `DEVELOPMENT.md`). |
| `--version`, `-V`; `--help`, `-h` | |
