<p align="center">
  <img src="desktop/icons/icon.png" width="112" alt="ryolune" />
</p>

<h1 align="center">ryolune</h1>

<p align="center"><strong>The open-source DAW your AI can drive.</strong></p>

<p align="center"><strong>Status:</strong> Beta · v0.17.0 · Linux and macOS</p>

<p align="center">
  <a href="https://lsuite.xyz/ryolune">Website</a> ·
  <a href="https://lsuite.xyz/ryolune/download">Download</a> ·
  <a href="docs/README.md">Documentation</a> ·
  <a href="docs/releases/0.17.0.md">Release notes</a>
</p>

<p align="center">
  <img src="https://lsuite.xyz/assets/img/ryolune/arrangement.webp" alt="The ryolune window: the arrangement, the mixer and the agent panel" />
</p>

ryolune is a digital audio workstation for musicians, producers and anyone who wants an AI agent
to work on a song with them. Every action, from adding a track to setting a plugin parameter, is a
command that you, the built-in agent, a script or any MCP client (Claude Code, Codex, Cursor…) can
run on the same song, with the same undo. It is a native Rust app, window included, free and open
source (MIT), and part of [lsuite](https://lsuite.xyz), a suite of four free, open-source apps
(kimchi, ryolune, nori and folio).

## What it does

Record while you hear yourself through your effects, write and edit MIDI with the full expression
of your keyboard, arrange with fades and song markers, mix on a real mixer with stock, CLAP, VST3
and Audio Unit plugins, automate anything, and export a mix or stems as WAV, AIFF, FLAC or Ogg
Vorbis.

- **All Rust, window included.** The audio engine (sample-accurate automation and controllers,
  plugin delay compensation, input monitoring, count-in, crash-isolated plugin scanning) and the
  window, drawn on the GPU with GPUI. No webview, no JavaScript.
- **35 stock instruments and effects**, with front panels that draw what the audio does, plus
  your own CLAP, VST3, Audio Unit (macOS) and native plugins in one Plugins window.
- **An agent that works on the song.** Each song keeps its agent conversations and a project
  memory (the key, the style, what to leave alone). The agent plans, follows a playbook for the
  job, looks at and measures what it made (loudness, true peak, spectrum), and every turn can be
  reverted in one step. You can type while it works to steer it.
- **Sounds from a description.** Loops that fit your bars and key, one-shots and playable
  instruments, made with ElevenLabs, Stable Audio, fal.ai or your own endpoint.
- **Plugins you can ask for.** Describe an effect or an instrument and your agent writes it in
  Rust on ryolune's SDK, builds it and loads it without a restart.
- **Coming from another app.** DAWproject import and export (Bitwig Studio, Studio One, Cubase
  and others) carries tracks, sends, MIDI and audio clips, markers, tempo, automation and plugins;
  MIDI and stems cover the rest.
- **Offline and private.** No account, no telemetry. A song is one `.ryolune` file with its audio
  inside. Logs and crash reports stay on your computer.

## Getting it

ryolune runs on **Linux** (x86_64) and **macOS** (Apple Silicon and Intel; signed and notarized).

**Download:** [lsuite.xyz/ryolune/download](https://lsuite.xyz/ryolune/download). ryolune comes
through the lsuite app, which installs and updates every lsuite app from its signed builds, with no
account.

Once installed, ryolune checks for updates when it starts and every six hours (Help > Check for
Updates…). An update is installed only after its Ed25519 signature, download host, checksum and
binary versions are verified, and the previous copy is kept until the new one starts.
`RYOLUNE_NO_UPDATE=1` turns the check off.

### Building from source

Rust 1.88 or later. On Ubuntu or Debian, install the system libraries first:

```sh
sudo apt-get install build-essential pkg-config libasound2-dev libudev-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-dev libx11-xcb-dev \
  libxcb1-dev libfontconfig1-dev libfreetype6-dev libvulkan-dev libdbus-1-dev \
  libssl-dev libzstd-dev
cargo run --release
```

The build makes three executables, `ryolune` (the app), `ryolune-cli` and `ryolune-mcp`; keep them
in one folder. Platform prerequisites, packaging and the release process are in
[DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Usage

The first time it opens, ryolune's setup asks which app you made music in, connects an agent,
checks the sound and offers the demo song, an empty song or a song from your old app (Help > Set
Up ryolune… shows it again). Then:

1. File > New Session: Drums, Bass and an audio track are ready.
2. Double-click a loop in the browser, or draw a region with the Pencil tool (`2`) and click in
   the piano roll to add notes. Space plays.
3. Mix in the inspector or the mixer (`X`), save with `Ctrl+S`, export with `Ctrl+B`.

The [user guide](docs/USER_GUIDE.md) walks through everything, and
[SHORTCUTS.md](docs/SHORTCUTS.md) lists every shortcut.

### From AI and scripts

The window, the built-in agent, `ryolune-cli` and `ryolune-mcp` all run the same
[command registry](docs/COMMANDS.md) (245 commands), with the same undo history. While the app
runs, clients connect over a local, token-protected bridge; without it they edit a song file
directly.

```sh
claude mcp add ryolune -- /path/to/ryolune/ryolune-mcp --live   # Claude Code or any MCP client

ryolune-cli session.overview                    # the whole song in one call
ryolune-cli track.add --kind midi --name Keys --instrument "E-Piano Mk I"
ryolune-cli clip.create --trackId Keys --startBar 0 --lengthBars 2 \
    --notes '[{"start":0,"length":2,"pitch":60},{"start":2,"length":2,"pitch":64}]'
ryolune-cli history.undo
ryolune-cli --file song.ryolune session.exportAudio --path mix.flac
```

Settings shows a ready setup for Claude Code, Codex, Cursor, VS Code, Claude Desktop, Gemini CLI
and other MCP apps. The built-in agent runs on the agent you bring: Codex or Claude Code with their
own sign-in, an API key (Anthropic, OpenAI, Gemini, OpenRouter, Mistral, Groq, DeepSeek, xAI), or a
model on your computer (Ollama, LM Studio or any compatible server). See
[AI_CONTROL.md](docs/AI_CONTROL.md) for permissions and recipes.

### With the rest of lsuite

Send a mix or stems onto a kimchi video project, score a cut that comes back from kimchi, and
find the other apps through `~/.lsuite`.

## Documentation

Every document is listed in [docs/README.md](docs/README.md). The main ones:

| Document | What it covers |
| --- | --- |
| [User guide](docs/USER_GUIDE.md) | The window, tracks, recording, editing, mixing, automation, files, settings |
| [AI control](docs/AI_CONTROL.md) | The built-in agent, `ryolune-cli`, `ryolune-mcp`, permissions, recipes |
| [Command reference](docs/COMMANDS.md) | Every command and parameter (generated from the registry) |
| [The agent panel](docs/AGENT.md) | Providers, outside agents, the conversation, Generate, Changes and Takes |
| [Plugins](docs/PLUGINS.md) and [native plugins](docs/NATIVE_PLUGINS.md) | CLAP, VST3 and Audio Unit hosting; writing plugins in Rust |
| [Configuration](docs/CONFIGURATION.md) | Every setting, the data folder, environment variables and flags |
| [Session format](docs/SESSION_FORMAT.md) | The `.ryolune` file, field by field |
| [Architecture](docs/ARCHITECTURE.md) | How the crates, registry, audio engine, plugin hosts and window fit together |
| [Release notes](docs/releases/) | What changed in each version |

## Project status and roadmap

ryolune is in **beta**: expect rough edges, and keep backups of songs you care about. The current version is 0.17.0, the first with signed and notarized Mac
builds. Ready-made builds are published for Linux and macOS; **Windows is coming soon** (the code
builds from source, but no Windows builds are published).

Known limitations:

- No time stretching, comping or time signature changes inside a song yet, and buses do not feed
  other buses.
- Recording latency is not measured or compensated automatically.
- On Linux, external plugins show their parameter list; their own windows open on macOS only.
- VST2 and AAX are not supported. There is no MP3 export.
- DAWproject does not carry clip gain, fade curves, plugin automation or meter changes. Ableton
  Live, Logic Pro, FL Studio, REAPER, Pro Tools and GarageBand exchange songs through MIDI and
  stems, not their own project files.
- A picture of the window (`ui.screenshot`) is available on macOS only.

Next on the roadmap: Windows builds, time signature changes, time stretching and comping (both
built on the tempo map), and buses that feed other buses.

## Contributing

Issues and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first: pull requests
need the checks in [DEVELOPMENT.md](docs/DEVELOPMENT.md) (`cargo fmt`, `cargo clippy` with warnings
denied, `cargo test --workspace`) and agreement to the [Contributor License Agreement](CLA.md).
Security issues: see [SECURITY.md](SECURITY.md).

## License

MIT. See [LICENSE](LICENSE).

The bundled fonts, Chakra Petch and IBM Plex Mono, are under the SIL Open Font License 1.1
(`desktop/assets/fonts`). A few interface icons come from Lucide (ISC,
`desktop/assets/icons/LICENSE.lucide.txt`) and the AI provider marks from Lobe Icons (MIT,
`desktop/assets/providers`). Logos of plugin formats and music apps are trademarks of their
owners, used only to name them; sources and terms are in `desktop/assets/logos/NOTICE.md`.

ryolune is free, with every feature and every update, for everyone. If it earns a place in your
music, you can [support it](https://lsuite.xyz/ryolune/support) with an optional donation; it
unlocks nothing.

---

Made by [Ludovic Marie](https://lu4ovic.com) · [github.com/ludovic111](https://github.com/ludovic111)
