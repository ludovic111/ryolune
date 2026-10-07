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
`ui.screenshot` to see it. `harness.context` is the same song in a dozen lines, and
`harness.look` a picture of it (see [The agent harness](#the-agent-harness)).

## The agent harness

Whatever the agent (the built-in one, Claude Code or Codex through `ryolune-mcp`, the lsuite
app's suite agent), it works from the same harness (lsuite's HARNESS.md), all in
`engine/src/harness/` and readable in full in [HARNESS.md](HARNESS.md):

- **A brief**: the system prompt of a music producer and mix engineer in ryolune (the song's
  model, the commands for common jobs, the quality bar for parts, levels and loudness, the usual
  mistakes and the finish routine). `harness.brief` returns it; it is the built-in agent's system
  prompt and `ryolune-mcp`'s `instructions`, from one file (`brief.md`).
- **Skills**: 13 playbooks (compose from a brief, drums, bass and chords, melody, arrangement,
  sound design, automation, mixing, mastering to a loudness target, export and stems, scoring a
  kimchi cut, writing a plugin, review and fix), each with steps, exact commands and the checks
  that prove the job worked. `harness.skills` lists them, `harness.skill name=mixing` loads one;
  over MCP each is also a prompt (`mixing`, with an optional `request`) and a resource
  (`ryolune://skills/mixing`; `ryolune://brief` is the brief).
- **Live context**: `harness.context` is the song in brief (tempo, key, meter, length,
  sections), every track in one line, the selection, the playhead and the newest checkpoint, and
  what changed since this agent's last command (`key`: `agent` for the built-in agent, `mcp`
  for outside ones). The built-in agent gets it before every model step after the first; through
  `ryolune-mcp` in live mode each tool result ends with what the person changed since the
  agent's last call, and each edit's result with a line of the song's state (`--no-context`
  turns both off).
- **Eyes and ears**: `harness.look fromBar toBar` renders the range offline (like an export,
  plugins and buses included) and draws a picture: the waveform with the bar grid and sections
  (clipping in red), the short-term loudness, the average spectrum against a pink slope and a
  piano roll of the notes in track colours, with the numbers. The picture reaches the model as an
  image: an image block for the built-in agent's vision providers (lsuite AI, Anthropic, OpenAI,
  Gemini, OpenRouter, xAI), MCP image content outside (and the PNG is at `image.path`; the CLI
  prints the path, not the bytes). `view=notes` draws only the piano roll (no render),
  `view=mix` leaves it out, `trackId` solos a track, `targetLufs` draws a target.
  `harness.measure` gives the numbers alone: integrated, short-term and momentary loudness
  (ITU-R BS.1770-4 / EBU R128 gating), loudness range (EBU Tech 3342), true peak (4x
  oversampled), sample peak, clipped samples, energy in five bands and `findings` that name the
  fix; `tracks=true` measures every track on its own. Ranges are limited to 600 seconds (120
  with `tracks`).
- **The finish routine** (in the brief and every skill): look and measure, compare with the
  request, fix what is off (up to three passes), then report in a few lines with the numbers.
- **One undo per turn**: the built-in agent takes a checkpoint before each turn; the chat and
  the Changes tab then show what the turn changed with **Revert turn** (`agent.revertTurn`,
  `redo=true` brings it back), one undo step either way. Any agent can do the same with
  `harness.checkpoint label=…`, `harness.changes` (what changed since, in plain words),
  `harness.checkpoints` and `harness.revert`; `ryolune-mcp` takes one before a connection's
  first edit. Checkpoints last until another song is opened.

```sh
ryolune-cli --file song.ryolune harness.measure --targetLufs -14
ryolune-cli --file song.ryolune harness.look --fromBar 8 --toBar 16 --path /tmp/chorus.png
ryolune-cli harness.skill --name mastering
```

**Evals** (`evals/`, see its README): 13 scripted music jobs run headless through Claude Code
and `ryolune-mcp`, scored by checks on the song (tempo, key, notes in key, registers, sections,
loudness and true peak, clipping, files, the person's work kept, the finish routine). Results are
in `evals/RESULTS.md`; run them before a release.

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
| Build and install plugins | `plugin.new`, `plugin.writeSource`, `plugin.build`, `plugin.publishLocal`, `plugin.install`, `plugin.remove`, `plugin.enable`, `plugin.disable`, `plugin.scaffold` (off by default; Build in the Plugins window turns it on) |

Connecting an AI service or a generation service, signing in and changing these permissions stay
with the person: agents may not set `agent.*`, `control.*` or `generation.*`. The song's project
memory and its saved conversations are the person's too: an agent may read the memory
(`agent.memory`), list, open, start and rename conversations, and steer the built-in agent, but
`agent.setMemory` and `agent.deleteConversation` are refused to agents (the memory goes ahead of
every later request, like the standing instructions in Settings; a deleted conversation is gone).

## lsuite AI

lsuite AI is the subscription that makes the agent work without setup (lsuite's AI.md). The
account is shared by every lsuite app on the computer (`~/.lsuite/account.json`):

```sh
ryolune-cli account.status                 # plan, allowance used, models; never the token
ryolune-cli account.plans                  # the plans the server offers (a demo: no payment)
ryolune-cli account.signIn key=lsk_…       # the key from the account page (headless use)
ryolune-cli account.signIn                 # in the window: opens the browser and waits
ryolune-cli account.signOut                # signs out every lsuite app here
```

Agents may read `account.status` and `account.plans`; `account.signIn`, `account.signOut` and
`account.manage` are the person's. The provider `lsuite` is Anthropic's Messages API at
`<server>/api/ai` with the account's token, so Claude Code can run on it too
(`agent.claudeThroughLsuite`, which sets `ANTHROPIC_BASE_URL` and `ANTHROPIC_AUTH_TOKEN`). When
the plan's allowance is used up, the turn ends with one line saying so; ryolune never switches
provider by itself. `LSUITE_ACCOUNT_SERVER` points a new sign-in at another server (a local
demo).

## Plugins an agent builds

Ask for a plugin and the agent writes it (lsuite's PLUGINS.md). The recipe, which
`plugin.guide` also returns with the SDK, the rules of the audio thread and an example:

```sh
ryolune-cli plugin.guide                              # Markdown for the agent
ryolune-cli plugin.toolchain                          # {cargo, rustc, version, ok, installHint}
ryolune-cli plugin.new name="Night Crush" kind=effect # ~/.lsuite/plugins-src/ryolune/night-crush
ryolune-cli plugin.writeSource name="Night Crush" path=src/lib.rs contents="$(cat lib.rs)"
ryolune-cli plugin.build name="Night Crush"           # ok, errors: [{file, line, column, message}]
ryolune-cli plugin.publishLocal name="Night Crush"    # installed, loaded, no restart
ryolune-cli strip.insertPlugin trackId=Drums pluginId=native:com.you.nightcrush firstFreeSlot=true
```

The crate takes `ryolune-plugin` from GitHub by tag, and builds against the SDK built into this
ryolune (`.cargo/config.toml` replaces that source), so a plugin always matches the app that asked
for it. `plugin.publishLocal` installs a bundle (`plugin.toml` and the library) in
`~/.lsuite/plugins/ryolune/<id>/` under a new library name each time, rescans, and the window
reloads every insert that uses it. `plugin.list` rows say `enabled`; `plugin.enable` /
`plugin.disable` are the switches; `plugin.remove` deletes a plugin the person installed;
`plugin.rescan` is `plugin.scan`. The Plugins window is `ui.showPanel panel=plugins
section=stock|installed|formats|build`.

## The built-in agent's conversations

The panel's conversations are kept per song (by the song's stable `id`, saved in the file) in
`<data dir>/agent-conversations.json`, and every client sees the same ones:

| Command | Does |
| --- | --- |
| `agent.conversations` | the song's conversations, newest first: `id`, `title`, `updatedAt`, `requests`, `current`; `memoryBytes`, `storageError` |
| `agent.newConversation` | opens an empty conversation; the open one is kept |
| `agent.selectConversation id=…` | opens a saved one (only while the agent is idle) |
| `agent.renameConversation title=… [id=…]` | renames one (new ones take their first request's first 60 characters) |
| `agent.deleteConversation id=…` | deletes one for good (refused to agents) |
| `agent.clear` | empties the open conversation; the edits stay in Undo |
| `agent.memory` / `agent.setMemory text=…` | the song's project memory, at most 32 KB (setting it is refused to agents) |
| `agent.steer text=…` | steers the running request: it reaches the agent at its next step |

Project memory goes ahead of every request for every provider as `Project memory
(user-maintained context):\n…\n\nCurrent request:\n…`, and is taken out again of the history
the next request carries. Steering reaches API providers after the tool results of the step in
progress (or as one more round when the answer was being written); Codex through `turn/steer`;
zenith through `thread.steer` (or, for an older zenith or a turn waiting on an approval,
`thread.send` once the turn ends); Claude Code, which reads its whole request at start,
by stopping the run and starting it again with the request, what it had answered and the
steering. `agent.status` reports the open conversation and `steeringPending`.

```sh
ryolune-cli agent.send --prompt "Add a bass line"
ryolune-cli agent.steer --text "Keep it under the kick, and simpler"
ryolune-cli agent.setMemory --text "D minor, 92 BPM, no hi-hats"
ryolune-cli agent.conversations
```

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
- **zenith as the agent.** With **Zenith · lsuite** chosen in Settings > Agent (`agent.configure
  provider=zenith`), the panel's requests go to a zenith thread through `zenith-cli`
  (`$RYOLUNE_ZENITH_CLI`, the path in Settings, zenith's lsuite entry, then PATH). Each song has a
  folder, `<data dir>/agent-workspaces/<song id>`, registered as a zenith project and holding
  ryolune's MCP recipe; zenith hands its agents ryolune's MCP server from ryolune's lsuite entry,
  and `ryolune-mcp --live` started that way finds the window's control file there. The edits
  come back as MCP edits: undo steps, in Changes and in the chat. Models are zenith's
  `provider/model` pairs (`provider.list`); an approval or a question zenith waits on shows in
  the status line, to answer in zenith; Stop runs `thread.interrupt` and waits for the thread.
- **Shared names.** Commands that every lsuite app has keep one name across the suite:
  `app.version`, `project.overview`, `export.audio`, `export.stems`, `export.midi` and
  `app.restart` work here too and run `app.info`, `session.overview`, `session.export*` and
  `app.relaunch`.

## Other music apps

`session.formats` lists what ryolune opens and writes for other apps (DAWproject, MIDI, audio
files, stems, and a package of MIDI plus stems), each with what survives the trip, and the apps
people come from (Ableton Live, Logic Pro, FL Studio, Bitwig Studio, REAPER, Cubase, Studio
One, Pro Tools, GarageBand) with their formats, the steps to bring a song over and take it back,
and whether each is installed here (`app` narrows it to one).

- `session.importFrom path=…` opens a `.dawproject`, a `.mid`, or audio files (`paths=[…]`, one
  track each) as a new, unsaved song in place of the open one. It answers with a `report`:
  `kept`, `approximated`, `dropped` and `missingMedia`, the same shape kimchi uses. Plugins are
  matched by CLAP id, VST3 class id or Audio Unit name against the scanned plugins.
- `session.exportTo path=… format=dawproject|midi|audio|stems|package` (or `app=bitwig`, which
  picks the app's best format) writes the song for another app, with the same report.
- `project.formats`, `project.importFrom` and `project.exportTo` are the lsuite names for them.
- The first-run setup is `app.onboarding` (its state, steps, the apps found, the providers ready)
  and `app.finishOnboarding comingFrom=… ai=…`, which only a person can answer: agents are
  refused. Recent songs are `app.recent` and `app.openRecent index=…` (or `path`).

Importing replaces the song, so agents need both `replaceSession` and `fileOperations`;
exporting needs `fileOperations`; `app.openRecent` needs `replaceSession`.

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
2. `harness.measure --tracks true`: the mix's loudness, true peak and clipping, and every track's
   own level. `harness.look` shows the waveform, loudness and spectrum.
3. Adjust with `track.setVolume`, `strip.setParameter` or a limiter on `master`, then measure
   again. The skills `mixing` and `mastering` (`harness.skill`) walk through it.

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

A few things deliberately have no command for agents: signing in to an AI service or to lsuite AI and changing the agent's
connection or permissions or the generation service, opening a GitHub issue
(`app.reportProblem` is refused to agents), answering the first-run setup, the menu bar itself, the agent panel's own composer, and pure layout
(vertical track scroll, folding a browser folder). The reasons are listed in
[AGENT_PARITY.md](AGENT_PARITY.md).

## Limits

CLAP preset discovery and presets a plugin shows only inside its own window are not reachable;
`strip.programs` reports what the format exposes. External plugin windows open on macOS only.
Playback needs the running app; rendering and exports do not.
