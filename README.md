# ryolune

Part of [lsuite](https://lsuite.xyz), the free, open-source creative suite your AI can drive.
Downloads are on [GitHub](https://github.com/ludovic111/ryolune/releases/latest); ryolune's page on
lsuite.xyz says "coming soon" for now.

**The open-source DAW your AI can drive.** A complete digital audio workstation for macOS,
Windows and Linux in which every action, from adding a track to mixing a plugin's parameters, is
a command that you, the built-in agent, a script or any MCP client (Claude Code, Codex, Cursor…)
can run on the same song, with the same undo.

Record while you hear yourself through your effects, write and edit MIDI with the full expression
of your keyboard, arrange with fades and song markers, mix on a real mixer with stock, CLAP, VST3
and Audio Unit plugins, automate anything, and export a mix or stems as WAV, AIFF, FLAC or Ogg
Vorbis. Ask the built-in agent for a bass line or a mix check in your own words: it works on the
same song, and every edit it makes is one undo away.

- **All Rust, window included**: the audio engine (sample-accurate automation and controllers,
  plugin delay compensation, input monitoring, count-in, crash-isolated plugin scanning) and the
  window, drawn on the GPU with GPUI. No webview, no JavaScript.
- **35 stock instruments and effects** with front panels that draw what the audio does, plus
  your own CLAP, VST3, Audio Unit (macOS) and lsuite plugins, all in one Plugins window with a
  switch on each.
- **Plugins you can ask for**: describe an effect or an instrument and your agent writes it in
  Rust on ryolune's SDK, builds it and loads it, without a restart (`plugin.guide`,
  `plugin.new`, `plugin.build`, `plugin.publishLocal`).
- **lsuite AI**: sign in once and the agent works, with no Claude Code to install and no key to
  paste; the account is shared by every lsuite app. A demo for now: no payment is taken.
  Bringing your own provider stays free.
- **The lsuite look** (design system v2, shared with kimchi): black and white, square corners,
  film grain and dithered light behind the chrome, solid work surfaces, every area titled and
  its tools boxed together, in dark and light, with text contrast tested on every surface.
- **Coming from another app**: a first-run setup asks which app you made music in, connects an
  agent and checks the sound, then starts you on the demo, an empty song or your old song.
  DAWproject import and export (Bitwig Studio, Studio One, Cubase and others) carries tracks,
  sends, MIDI and audio clips, markers, tempo, automation and plugins; MIDI and stems cover the
  apps without it. File > Open Recent lists the songs you opened lately.
- **Works with the rest of lsuite**: send a mix or stems onto a kimchi video project, score a
  cut that comes back from kimchi, use zenith's agents as ryolune's agent, and find the other
  apps through `~/.lsuite`.
- **Built for AI control**: everything a person can do in the window is a command that the
  built-in agent, `ryolune-cli` and any MCP client can call, with a one-call overview of the whole
  song and full access to external plugins' parameters and state. The built-in agent runs on
  lsuite AI, Codex, Claude Code, Anthropic, OpenAI, Gemini, OpenRouter, Mistral, Groq, DeepSeek, xAI, Ollama,
  LM Studio, zenith or any compatible server; Settings shows a ready setup for Claude Code, Codex,
  Cursor, VS Code, Claude Desktop, Gemini CLI and other MCP apps.
- **Conversations that stay with the song**: each song keeps its agent conversations and a
  project memory (the key, the style, what to leave alone) that every request reads. Type while
  the agent works to steer it without losing what it has done.
- **Sounds from a description**: loops that fit your bars and key, song ideas, one-shots and
  playable instruments, made with ElevenLabs, Stable Audio, fal.ai or your own endpoint and placed
  in one undo step. Any sound can become a Sample Keys instrument.
- **Offline and private**: no account needed (lsuite AI is optional), no telemetry. Songs are single `.ryolune`
  files with the audio inside. Logs and crash reports stay on your computer (Settings >
  Diagnostics); Help > Report a Problem opens a GitHub issue for you to read before sending.

## Install

Download the file for your computer from the
[latest release](https://github.com/ludovic111/ryolune/releases/latest):

| Computer | File |
| --- | --- |
| Mac with Apple silicon | `ryolune-macos-arm64.zip` |
| Mac with Intel | `ryolune-macos-x86_64.zip` |
| Windows | `ryolune-windows-x86_64.zip` |
| Linux | `ryolune-linux-x86_64.zip` (or `.tar.gz`) |

**macOS**: unzip and drag `ryolune.app` to Applications. The app is signed with a Developer ID
and notarized by Apple, so it opens like any other app (from 0.11.0).
**Windows and Linux**: extract all three executables (`ryolune`, `ryolune-cli`, `ryolune-mcp`) into
one folder. The demo song is `ryolune-Afterglow-demo.zip` on the same page.

ryolune checks for updates when it starts and every six hours while it stays open (Help > Check
for Updates…). An update is installed only after its Ed25519 signature, download host, checksum
and binary versions are verified; the previous copy is kept until the new one starts, and
**Restart now** starts it. The first time a new version opens, What's New lists what changed
since the one you had. `RYOLUNE_NO_UPDATE=1` turns the check off.

## Start

The first time it opens, ryolune's setup offers the demo song, an empty song or a song from your
old app (Help > Set Up ryolune… shows it again). Then:

1. File > New Session: Drums, Bass and an audio track are ready.
2. Double-click a loop in the browser, or draw a region with the Pencil tool (`2`) and click in
   the piano roll to add notes. Space plays.
3. Mix in the inspector or the mixer (`X`), save with `⌘S`, export with `⌘B`.

Or open File > Open Demo to explore a finished song, or File > Import from Another App… to bring
in a DAWproject, a MIDI file or stems. The [user guide](docs/USER_GUIDE.md) walks through
everything.

## Control ryolune from scripts and AI

The window, the built-in agent, `ryolune-cli` and `ryolune-mcp` all run the same
[command registry](docs/COMMANDS.md), with the same undo history. While the app runs, clients
connect over a local, token-protected bridge; without it they edit a song file directly.

```sh
ryolune-cli session.overview                    # the whole song in one call
ryolune-cli track.add --kind midi --name Keys --instrument "E-Piano Mk I"
ryolune-cli clip.create --trackId Keys --startBar 0 --lengthBars 2 \
    --notes '[{"start":0,"length":2,"pitch":60},{"start":2,"length":2,"pitch":64}]'
ryolune-cli plugin.list --query reverb
ryolune-cli history.undo
ryolune-cli --file song.ryolune session.exportAudio --path mix.flac
```

Add ryolune to Claude Code or any MCP client:

```sh
claude mcp add ryolune -- /Applications/ryolune.app/Contents/MacOS/ryolune-mcp --live
```

See [AI_CONTROL.md](docs/AI_CONTROL.md) for the agent, the CLI, MCP, permissions and recipes.

## Documentation

Every document is listed in [docs/README.md](docs/README.md).

| Document | What it covers |
| --- | --- |
| [User guide](docs/USER_GUIDE.md) | The window, tracks, recording, editing, mixing, automation, files, appearance, settings |
| [Keyboard shortcuts](docs/SHORTCUTS.md) | Every shortcut (generated from the app) |
| [AI control](docs/AI_CONTROL.md) | The built-in agent, `ryolune-cli`, `ryolune-mcp`, permissions, recipes |
| [Command reference](docs/COMMANDS.md) | Every command and parameter (generated from the registry) |
| [The agent panel](docs/AGENT.md) | Providers, outside agents, the conversation, Generate, Changes and Takes |
| [Plugins](docs/PLUGINS.md) | CLAP, VST3 and Audio Unit hosting |
| [Native plugins](docs/NATIVE_PLUGINS.md) | Writing plugins in Rust with the ryolune SDK |
| [Configuration](docs/CONFIGURATION.md) | Every setting, the data folder, environment variables and flags |
| [Session format](docs/SESSION_FORMAT.md) | The `.ryolune` file, field by field |
| [Architecture](docs/ARCHITECTURE.md) | How the crates, registry, audio engine, plugin hosts and window fit together |
| [Development](docs/DEVELOPMENT.md) | Code layout, building, checks, releases |
| [Release notes](docs/releases/) | What changed in each version |
| [Verification](docs/VERIFICATION.md) | What has been tested and how |

## Build from source

```sh
cargo run --release
```

Rust 1.88+ is all it needs: the window is Rust too, drawn on the GPU with GPUI. Platform prerequisites, checks and the release process are
in [DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Limits

No time stretching, comping or time signature changes inside a song yet, and buses do not feed
other buses. Recording latency is not compensated automatically. External plugin windows open on macOS; elsewhere
external plugins show their parameter list. VST2 and AAX are not supported. There is no MP3
export. Windows builds are not code-signed. DAWproject does not carry clip gain, fade curves,
plugin automation or meter changes, and Ableton Live, Logic Pro, FL Studio, REAPER, Pro Tools
and GarageBand exchange songs through MIDI and stems, not their own project files.

## Support ryolune

ryolune is open source and free forever: every feature and every update, for everyone. If it
earns a place in your music, you can [donate](https://lsuite.xyz/ryolune/support), once or monthly.
Donations are optional, unlock nothing, and are the only money ryolune takes; they keep it built
full time.

## Contributing

Issues and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first: pull requests
need the checks in [DEVELOPMENT.md](docs/DEVELOPMENT.md) and agreement to the
[Contributor License Agreement](CLA.md).

## License

MIT. Copyright Ludovic Marie. See [LICENSE](LICENSE).
