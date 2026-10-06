# Controlling ryolune from AI and scripts

Everything a person can do in the ryolune window, an AI or a script can do too, through one
command registry. This page explains the four ways in, what an agent should call first, and how
to drive tracks, notes, mixing and external plugins. The full list of commands and parameters
is the generated [command reference](COMMANDS.md); the audit that maps every window interaction
to its command is [AGENT_PARITY.md](AGENT_PARITY.md).

## Four ways in, one registry

| Way in | For | How it connects |
| --- | --- | --- |
| The window | people | menus, shortcuts and drags call the registry by name |
| The built-in agent | talking to ryolune in your own words | Agent panel, see [AGENT.md](AGENT.md) |
| `ryolune-cli` | scripts, terminals, other programs | the running app, or a song file |
| `ryolune-mcp` | Claude Code, Claude Desktop, Codex, any MCP client | the running app, or its own session |

All four share the same undo history. Every edit is an ordinary undo step (a `session.batch` is
one step for many edits), so anything an AI does can be undone from the Edit menu or with
`history.undo`. Clips and notes an agent creates are drawn with the agent accent colour, and the
agent panel's **Changes** tab lists edits that came from the agent, the CLI or MCP.

## Start with the overview

`session.overview` returns the whole song in one bounded answer: tempo and tempo changes, meter,
key, length and sections; every track with its instrument (name, format, vendor), inserts with
the parameters changed from their defaults as the plugin displays them, sends, fader in dB, pan,
mute, solo, arm and monitoring, the bus it is routed to (or, for a bus, what it sums); **problems** that keep a track silent (muted, excluded by a solo, zero fader,
bypassed instrument, a plugin that is not installed or failed to load); clips with bars, note
counts, pitch ranges, audio sources and fades; automation and controller lanes; the buses,
selection, takes and undo history; and, in the running app, what the window shows. When a big
song does not fit, `truncated` says what was left out and `next` names the command for each
detail.

```sh
ryolune-cli session.overview
ryolune-cli session.overview --trackId Bass       # one track in full
```

Then drill down only where needed: `clip.get` or `note.list` for notes, `strip.parameters` for a
plugin's parameters, `automation.list`, `controller.list`, `ui.state` for the window and
`ui.screenshot` to see it.

## Conventions

- Bars and beats are **zero-based**: bar 0 is the first bar. Note `start` and `length` are in
  beats relative to their clip; automation points are in absolute beats.
- Pitch 60 is C4; velocity is 1–127; a fader value of 0.75 is unity gain (0 dB).
- **Names work wherever ids do**: `trackId`, `clipId` and `markerId` accept a unique name
  (`--trackId Bass`). A wrong or ambiguous name is refused with the list of what exists and the
  closest match ("Unknown track `Bas`. Did you mean Bass?").
- Strip commands take a track (a bus track too), `master`, `bus-a` (reverb) or `bus-b` (delay);
  insert slots are 0–7; omit `slot` for a MIDI track's instrument.
- Tempo: `transport.setTempo` is the starting tempo; `tempo.set bar=…` adds a change later on
  (bars stay bars, so the music keeps its place and only its speed changes).
- Errors are written for the reader: they say what was expected and how to fix the call.

## The CLI

`ryolune-cli` talks to the running app over a local bridge: the app writes a port and a random
token to `control.json` in its data folder (readable only by you; `RYOLUNE_CONTROL` overrides the
path) and accepts only clients that present the token. Without a running app, `--file` edits a
song directly and saves atomically after every change.

```sh
ryolune-cli commands                     # every command (add --json for the full schema)
ryolune-cli help strip.setParameter      # one command's parameters
ryolune-cli doctor                       # bridge, versions, settings and plugin cache
ryolune-cli --file song.ryolune session.new
ryolune-cli --file song.ryolune session.exportAudio --path mix.flac
```

Parameters are `--name value` or `name=value`; arrays and objects are JSON. `--compact` prints
one line of JSON. `ryolune-cli batch` reads one `{"command": …, "params": {…}}` per line from
standard input and stops at the first error (`--continue` keeps going).

A song open in the window belongs to the window: `--file` on the same file is refused. Use the
live commands instead.

## MCP

`ryolune-mcp` is a stdio Model Context Protocol server. Each command is a tool, with the dot
replaced by an underscore (`track.add` is `track_add`), its description and JSON schema taken
from the registry.

```sh
# Claude Code
claude mcp add ryolune -- /Applications/ryolune.app/Contents/MacOS/ryolune-mcp --live
# Codex CLI
codex mcp add ryolune -- /Applications/ryolune.app/Contents/MacOS/ryolune-mcp --live
```

```json
{"mcpServers": {"ryolune": {"command": "/path/to/ryolune-mcp", "args": ["--live"]}}}
```

The block above suits Cursor (`~/.cursor/mcp.json`), Claude Desktop, Gemini CLI
(`~/.gemini/settings.json`), Windsurf and most other clients; VS Code uses `"servers"` with
`"type": "stdio"`, opencode `"mcp"` with a `"command"` array, Zed `"context_servers"`.
**Settings > Agent > Use another agent** shows each one filled in with this computer's paths
and the `RYOLUNE_CONTROL` environment the server needs, with a Copy button, and installs Cursor
and VS Code from their links; `ryolune-cli agent.mcp` prints the same.

- `--live` requires the running app and controls it; `--headless` hosts an independent session;
  `--file <song.ryolune>` edits a file. Without a flag it uses the app when it runs.
- **Resources**: `ryolune://session/overview` (read it first), `ryolune://session`,
  `ryolune://session/info`, `ryolune://session/inspect`, `ryolune://catalog`, `ryolune://plugins`,
  `ryolune://presets`, `ryolune://settings` and the app state.
- **Prompts**: `compose`, `mix-review` and `see-the-window` start common tasks.

## Permissions

Settings > Agent > Permissions decide what an agent may do: the built-in agent, MCP clients and
`ryolune-cli --agent`. Editing the song is always allowed (and always undoable); these are
switches for the rest:

| Permission | Covers |
| --- | --- |
| File operations | open, save, import, export, bounce, plugin scan, writing a screenshot to a path, deleting crash reports |
| Transport | play, record, stop, locate, marker navigation |
| Replace the session | new session, open another song |
| Settings | `settings.set`, `settings.reset` |
| Application control | quit, install an update, restart (`app.relaunch`, also `app.restart`) |
| Generate sounds | `generate.audio`, which spends the generation service's credits (on by default) |

Connecting an AI service or a generation service, signing in and changing these permissions stay
with the person: agents may not set `agent.*`, `control.*` or `generation.*`.

## Generation

`generate.audio` makes a sound from a description with the service chosen in Settings >
Generation, keeps it in `<data folder>/generated` with a JSON note beside it, and places it in the
song in one undo step:

| kind | Default length | Lands as |
| --- | --- | --- |
| `loop` | `bars` (4) at the song's tempo, trimmed or padded to fit exactly | an audio clip at the playhead |
| `song` | 60 s (5-300), `instrumental` by default | an audio clip at the playhead |
| `sound` | 3 s (0.5-30): one-shots, effects, risers | an audio clip at the playhead |
| `instrument` | 3 s (0.5-10): one note, middle C | Sample Keys on a new MIDI track |

Loops and songs tell the service the song's tempo and key unless `followSong:false`. `place:false`
only keeps the result. `generate.list`, `generate.preview` (base64 audio), `generate.place` (again,
as `audio` or `instrument`) and `generate.delete` manage what was made; all but `generate.audio`
also work headless. `strip.loadSample` turns any audio file or song source into Sample Keys.

```sh
ryolune-cli generate.audio --prompt "dusty boom-bap drums, lazy swing" --kind loop --bars 2
ryolune-cli generate.audio --prompt "felt piano, soft and close" --kind instrument
ryolune-cli strip.loadSample --path ~/Samples/choir-ah.wav --rootNote 64
```

Services: **ElevenLabs** (Eleven Music for songs and loops, the sound-effects model for sounds,
instruments and loops under 3 s; `ELEVENLABS_API_KEY`), **Stable Audio** by Stability AI
(`stable-audio-2.5`, up to 190 s; `STABILITY_API_KEY`), **fal.ai** (any audio model, such as
`fal-ai/stable-audio`, set in Settings; `FAL_KEY`) and a **custom endpoint**. A custom endpoint
receives a POST with JSON `{prompt, description, kind, seconds, instrumental, seed, name}` and a
bearer token when a key is set, and answers with the audio itself (`audio/*`), or with JSON holding
`audio` (base64, with an optional `format` such as `"wav"`) or `url` (a file to fetch). Any format
ryolune imports works; a result may be up to 200 MB.

## The lsuite: other apps

ryolune is part of [lsuite](https://lsuite.xyz) with kimchi (video) and zenith (code). Each
app writes `~/.lsuite/apps/<app>.json` when it starts: its version, where its CLI and MCP
server are, its data folder, the hand-offs it takes and, while it runs, its pid and bridge
port (never a token). `app.suite` lists them, so an agent working in ryolune knows which other
apps it can drive and how.

- **To kimchi.** `export.toKimchi` renders the mix (or `stems: true`, one file per track) for
  kimchi at `startSeconds`. kimchi open: kimchi places each file on its open project itself,
  through its own `handoff.fromRyolune` on its bridge, so the change is in kimchi's undo
  history. kimchi closed: the files are added to a project's file on new audio tracks
  (`project`: id or name, default the latest), its previous version kept beside it.
- **From kimchi.** kimchi's `handoff.toRyolune` renders a cut as a WAV and writes
  `<name>.kimchi-cut.json` (length and markers) in `~/.lsuite/handoff/ryolune`; while ryolune
  runs it also imports the audio and adds the markers through ryolune's bridge itself.
  Otherwise `handoff.inbox` lists the waiting cuts and `session.scoreCut manifest=…` puts the
  audio on a new track at bar 1, each marker on the ruler at the bar where it falls, and the
  cycle over the cut, in one undo step. Without a manifest, give `path`, `markers` (`time` in
  seconds, `label`) and `durationSeconds` yourself.
- **Shared names.** Commands that every lsuite app has keep one name across the suite:
  `app.version`, `project.overview`, `export.audio`, `export.stems`, `export.midi` and
  `app.restart` work here too and run `app.info`, `session.overview`, `session.export*` and
  `app.relaunch`.

## Recipes

### Write a part

```sh
ryolune-cli track.add --kind midi --name Keys --instrument "E-Piano Mk I"
ryolune-cli clip.create --trackId Keys --startBar 0 --lengthBars 2 \
  --notes '[{"start":0,"length":2,"pitch":60,"velocity":90},{"start":0,"length":2,"pitch":64},{"start":0,"length":2,"pitch":67}]'
ryolune-cli clip.addLoop --trackId Drums --name "Four Floor 124" --startBar 0
ryolune-cli marker.add --bar 0 --name Intro
```

`clip.setNotes` replaces a clip's notes in one step; `clip.quantize`, `clip.transpose`,
`clip.humanize`, `clip.fitScale`, `clip.legato` and `clip.repeat` edit a whole clip.
`controller.setPoints` writes mod wheel, pitch bend, sustain or any CC into a clip.

### Many edits, one undo step

```sh
ryolune-cli session.batch --commands '[
  {"command":"track.setVolume","params":{"trackId":"Drums","volume":0.6}},
  {"command":"track.setPan","params":{"trackId":"Keys","pan":-20}},
  {"command":"strip.setPlugin","params":{"trackId":"master","plugin":"Limiter","firstFreeSlot":true}}
]'
```

With `atomic` (the default) a failing command rolls back the ones before it.

### External plugins

Any installed CLAP, VST3, Audio Unit or ryolune native plugin can be found, loaded and set.

```sh
ryolune-cli plugin.list --query reverb                  # by name, vendor or what it does
ryolune-cli strip.setPlugin --trackId Vocals --plugin "pro q 3" --firstFreeSlot true
ryolune-cli strip.parameters --trackId Vocals --slot 0 --query "band 1"
ryolune-cli strip.setParameter --trackId Vocals --slot 0 --parameter "Band 1 Gain" --text "-4.5 dB"
ryolune-cli strip.setParameters --trackId Vocals --slot 0 \
  --values '{"Band 1 Frequency":"250 Hz","Band 1 Q":{"normalized":0.3}}'
ryolune-cli strip.programs --trackId Bass --slot 0         # the plugin's own factory programs
ryolune-cli strip.setProgram --trackId Bass --slot 0 --name Cathedral
ryolune-cli automation.create --target pluginParameter --trackId Vocals --slot 0 \
  --parameter "Band 1 Gain" --points '[{"beat":0,"value":0},{"beat":16,"value":-6}]'
```

- `plugin.list` searches names, vendors and what a plugin is for ("reverb", "compressor",
  "saturation"). A plugin installed in several formats loads as CLAP, then VST3, then AU.
- `strip.parameters` lists every parameter with its value, display text, range, 0–1 position,
  whether it can be automated and its automation lane; `query` filters by name and
  `changed=true` shows only what differs from the defaults.
- A value is set as a plain number (`value`), a 0–1 position (`normalized`) or what the plugin
  displays (`text`: "-6 dB", "2.5k", "50%", "On", "Hall").
- `strip.programs` / `strip.setProgram` reach VST3 program lists and Audio Unit factory presets,
  with the current one when the plugin reports it; ryolune's own presets are `preset.list`,
  `preset.save` and `preset.load`.
- `strip.getState` / `strip.setState` read and restore a plugin's full saved state;
  `strip.setBypass`, `strip.moveInsert` and `strip.removeInsert` manage the chain;
  `ui.openPluginWindow` opens the plugin's own window (macOS).
- Scanning (`plugin.scan`) runs in a separate process, so a faulty plugin cannot crash the app.

### Tempo changes

```sh
ryolune-cli tempo.set --bar 16 --bpm 96 --ramp true    # slow down into bar 17
ryolune-cli tempo.set --bar 24 --bpm 124               # a jump back up at bar 25
ryolune-cli tempo.list                                 # every change, in seconds too
ryolune-cli tempo.move --bar 24 --toBar 32
ryolune-cli tempo.clear --startBar 16
```

`transport.locate` answers with `tempoAtPosition` and `positionSeconds` once a song has changes.
MIDI export writes them; `session.importMidi importTempo=true` follows a file's.

### Buses: groups and aux returns

```sh
ryolune-cli track.group --params '{"trackIds":["Kick","Snare","Hats"],"name":"Drums"}'
ryolune-cli strip.setPlugin --trackId Drums --plugin "ryolune Comp" --firstFreeSlot true
ryolune-cli track.add --kind bus --name "Plate"
ryolune-cli strip.setSend --trackId Vocals --send 2 --bus Plate --levelDb -12
ryolune-cli track.setOutput --trackId Snare --output "Stereo Out"
```

A bus track holds no clips; it sums what tracks route (`track.setOutput`) or send
(`strip.setSend`) to it through its inserts, fader and pan. Sends 0 and 1 feed A · Reverb and
B · Delay until pointed elsewhere; a strip has up to four. Buses feed the Stereo Out, A and B,
never another bus. `session.overview` shows each track's `output` and each bus's `inputs`, and
flags a track whose bus is muted.

### Check a mix

1. `session.overview`: read `problems` for silent tracks and the fader and insert summary.
2. `session.exportAudio --path /tmp/check.wav`: the report gives the peak and the number of
   clipped samples. `session.exportStems` does the same per track.
3. Adjust with `track.setVolume`, `strip.setParameter` or a limiter on `master`, then export again.

### See and steer the window

`ui.state` reports what the window shows: open panels and dialogs, a pending prompt, open plugin
windows, the editor's clip and mode, zoom and visible bars, the tool, the browser, the selection
and the theme. `ui.showPanel` opens the mixer, automation, controller lane, tempo track,
settings, help, the command palette and more; `view.set` scrolls and zooms; `ui.setTool` picks a tool;
`ui.screenshot` saves a PNG of the window, captured once running animations have settled.

### When something goes wrong

`app.diagnostics` returns what a bug report needs: version and build, system, audio device,
plugin scan summary, folders, the log file and recent crash reports, with no keys, prompts or
songs. `app.logs` (`lines`, `file`) reads the end of this run's log or an earlier one;
`app.crashReports` lists crash reports (`crash`, `recovered`, `unclean`) and reads one with
`id`. These work without the window too. `app.whatsNew` returns the release notes built into
this copy (`version`, `since`, `all`). `app.reportProblem` is for people only: it opens a
prefilled GitHub issue in their browser, and nothing is ever sent by itself.

## What only a person does

A few things deliberately have no command: signing in to an AI service and changing the agent's
connection or permissions or the generation service, opening a GitHub issue
(`app.reportProblem` is refused to agents), the menu bar itself, the agent panel's own composer, and pure layout
(vertical track scroll, folding a browser folder). The reasons are listed in
[AGENT_PARITY.md](AGENT_PARITY.md).

## Limits

CLAP preset discovery and presets a plugin shows only inside its own window are not reachable;
`strip.programs` reports what the format exposes. External plugin windows open on macOS only.
Playback needs the running app; rendering and exports do not.
