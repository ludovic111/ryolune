# Command reference

<!-- Generated from the command registry by tools/tests/command_docs.rs. Do not edit by hand: run `RYOLUNE_BLESS=1 cargo test -p ryolune-tools --test command_docs`. -->

ryolune has 250 commands. The window, `ryolune-cli`, `ryolune-mcp` and the built-in agent all run these same commands, with the same undo history. On the CLI a command is `ryolune-cli <name> --param value`; in MCP it is the tool `<name>` with the dot replaced by an underscore (`track.add` is `track_add`); the agent sees the same tools.

Conventions: bars and beats are zero-based; note `start` and `length` are beats relative to their clip; pitch 60 is C4; velocity is 1–127; a fader value of 0.75 is unity gain. Strip commands accept a track id or `master`, `bus-a`, `bus-b`; insert slots are 0–7.

**Edits** marks a command that can change the song, the transport, settings or files (it is one undo step when it changes the song). **Needs the app** marks a command only the running window can serve; the others also work on a file (`ryolune-cli --file song.ryolune …`).

Names shared across the lsuite apps are accepted too, and run the ryolune command beside them: `app.version` → `app.info`, `project.overview` → `session.overview`, `export.audio` → `session.exportAudio`, `export.stems` → `session.exportStems`, `export.midi` → `session.exportMidi`, `app.restart` → `app.relaunch`, `project.formats` → `session.formats`, `project.importFrom` → `session.importFrom`, `project.exportTo` → `session.exportTo`, `plugin.rescan` → `plugin.scan`.

## Families

- [session](#session) — `session.info`, `session.get`, `session.inspect`, `session.catalog`, `session.commands`, `session.new`, `session.open`, `session.save`, `session.rename`, `session.bounce`, `session.importAudio`, `session.importMidi`, `session.exportMidi`, `session.exportAudio`, `session.exportStems`, `session.batch`, `session.saveRecoveredTake`, `session.snapshots`, `session.restoreSnapshot`, `session.overview`, `session.scoreCut`, `session.formats`, `session.importFrom`, `session.exportTo`
- [plugin](#plugin) — `plugin.list`, `plugin.scan`, `plugin.folders`, `plugin.setFavorite`, `plugin.setFolder`, `plugin.scaffold`, `plugin.install`, `plugin.describe`, `plugin.info`, `plugin.enable`, `plugin.disable`, `plugin.remove`, `plugin.guide`, `plugin.toolchain`, `plugin.new`, `plugin.writeSource`, `plugin.build`, `plugin.publishLocal`
- [transport](#transport) — `transport.play`, `transport.record`, `transport.stop`, `transport.locate`, `transport.returnToStart`, `transport.setTempo`, `transport.setTimeSignature`, `transport.setKey`, `transport.setCycle`, `transport.setMetronome`, `transport.setSnap`, `transport.punch`
- [track](#track) — `track.list`, `track.add`, `track.remove`, `track.rename`, `track.setMute`, `track.setSolo`, `track.setArmed`, `track.setMonitor`, `track.setVolume`, `track.setPan`, `track.setColor`, `track.move`, `track.select`, `track.setOutput`, `track.group`, `track.duplicate`
- [clip](#clip) — `clip.list`, `clip.get`, `clip.create`, `clip.move`, `clip.resize`, `clip.rename`, `clip.split`, `clip.duplicate`, `clip.copy`, `clip.cut`, `clip.paste`, `clip.remove`, `clip.setNotes`, `clip.addLoop`, `clip.select`, `clip.trim`, `clip.deselect`, `clip.setFades`, `clip.setGain`, `clip.humanize`, `clip.velocityRamp`, `clip.fitScale`, `clip.reverseMidi`, `clip.legato`, `clip.repeat`, `clip.quantize`, `clip.transpose`
- [note](#note) — `note.list`, `note.add`, `note.update`, `note.remove`, `note.preview`, `note.hold`, `note.releaseAll`
- [strip](#strip) — `strip.get`, `strip.setInstrument`, `strip.setInsert`, `strip.setSendLevel`, `strip.setPlugin`, `strip.setBypass`, `strip.getState`, `strip.setState`, `strip.setSend`, `strip.moveInsert`, `strip.parameters`, `strip.setParameter`, `strip.setParameters`, `strip.programs`, `strip.setProgram`, `strip.removeInsert`, `strip.loadSample`
- [master](#master) — `master.setVolume`
- [history](#history) — `history.undo`, `history.redo`, `history.info`
- [source](#source) — `source.peaks`
- [marker](#marker) — `marker.list`, `marker.add`, `marker.rename`, `marker.move`, `marker.setColor`, `marker.remove`, `marker.goto`, `marker.next`, `marker.previous`, `marker.cycleSection`
- [tempo](#tempo) — `tempo.list`, `tempo.set`, `tempo.move`, `tempo.remove`, `tempo.clear`
- [automation](#automation) — `automation.list`, `automation.create`, `automation.setPoints`, `automation.setPoint`, `automation.removePoint`, `automation.setEnabled`, `automation.setInterpolation`, `automation.remove`
- [controller](#controller) — `controller.list`, `controller.add`, `controller.update`, `controller.remove`, `controller.setPoints`
- [rhythm](#rhythm) — `rhythm.create`, `rhythm.preview`
- [take](#take) — `take.list`, `take.create`, `take.select`, `take.remove`
- [view](#view) — `view.get`, `view.fit`, `view.set`
- [preset](#preset) — `preset.list`, `preset.save`, `preset.load`, `preset.delete`
- [settings](#settings) — `settings.get`, `settings.set`, `settings.reset`
- [audio](#audio) — `audio.devices`, `audio.status`, `audio.allowSpeakerMonitoring`, `audio.setOutput`, `audio.setInput`, `audio.setMidiInput`, `audio.reconnect`
- [ui](#ui) — `ui.screenshot`, `ui.showPanel`, `ui.openPluginWindow`, `ui.closePluginWindow`, `ui.dismissError`, `ui.closePluginWindows`, `ui.musicalTyping`, `ui.setTool`, `ui.status`, `ui.state`
- [app](#app) — `app.info`, `app.checkUpdates`, `app.installUpdate`, `app.quit`, `app.confirm`, `app.openGuide`, `app.relaunch`, `app.logs`, `app.crashReports`, `app.clearCrashReports`, `app.diagnostics`, `app.reportProblem`, `app.whatsNew`, `app.suite`, `app.onboarding`, `app.finishOnboarding`, `app.recent`, `app.openRecent`
- [agent](#agent) — `agent.status`, `agent.configure`, `agent.providers`, `agent.mcp`, `agent.openClient`, `agent.models`, `agent.connection`, `agent.send`, `agent.stop`, `agent.transcript`, `agent.changes`, `agent.revert`, `agent.revertTurn`, `agent.clear`, `agent.conversations`, `agent.newConversation`, `agent.selectConversation`, `agent.renameConversation`, `agent.deleteConversation`, `agent.memory`, `agent.setMemory`, `agent.steer`
- [generate](#generate) — `generate.services`, `generate.audio`, `generate.list`, `generate.preview`, `generate.place`, `generate.delete`
- [export](#export) — `export.toKimchi`
- [handoff](#handoff) — `handoff.inbox`
- [account](#account) — `account.status`, `account.signIn`, `account.signOut`, `account.plans`, `account.manage`
- [harness](#harness) — `harness.brief`, `harness.skills`, `harness.skill`, `harness.context`, `harness.look`, `harness.measure`, `harness.checkpoint`, `harness.checkpoints`, `harness.changes`, `harness.revert`

## session

### `session.info`

Summarise the open session: name, file, transport, counts, selection and history state.

### `session.get`

Return the complete session document as JSON (tracks, clips with notes, sources, strips, transport, view).

### `session.inspect`

Inspect the arrangement, mixer and automation without opaque plugin state. Clips are summaries by default; use clip.get for individual notes.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `includeNotes` | boolean |  | Include all MIDI notes instead of clip summaries (default false). |

### `session.catalog`

List built-in instruments, effects and bundled MIDI loops.

### `session.commands`

Describe every command with its parameters.

### `session.new`

*Edits*

Replace the open session with an empty one (or the bundled Nightfall demo). Unsaved changes are discarded.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `demo` | boolean |  | Load the Nightfall demo instead of an empty session. |

### `session.open`

*Edits*

Open a .ryolune session file, replacing the current session. Unsaved changes are discarded.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Path to a .ryolune file. |

### `session.save`

*Edits*

Save the session as a .ryolune file. Writes atomically; the old file survives a failed save.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string |  | Destination file. Defaults to the file the session was opened from. |

### `session.rename`

*Edits*

Set the session name shown in the title bar and used for exports. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | New session name. |

### `session.bounce`

*Edits*

Render the whole arrangement offline to a stereo 48 kHz / 24-bit WAV file, including a 3-second effect tail.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Destination .wav path. |

### `session.importAudio`

*Edits*

Decode an audio file (WAV, AIFF, FLAC, MP3, Ogg, AAC) into the session and place it as a clip on an audio track.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Audio file to import. |
| `trackId` | string |  | Audio track to place the clip on. Defaults to the selected audio track, or a new one. |
| `startBar` | number |  | Bar to place the clip at. Defaults to the playhead. |

### `session.importMidi`

*Edits*

Import SMF type 0/1 MIDI into new instrument tracks in one undo step. Quarter-note positions are preserved; reports what it could not import.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Source .mid or .midi file. |
| `startBar` | number |  | Zero-based destination bar, default 0. |
| `importTempo` | boolean |  | Make the file's tempo the song's (default false): its first tempo and meter for the whole song, its later tempo changes from startBar on, replacing the song's tempo changes. Later meter changes are reported and ignored. |
| `keepChannels` | boolean |  | One track per track of the file, every note and controller on the MIDI channel it had, for a multitimbral instrument (default false: one track per channel, played on channel 1). |

### `session.exportMidi`

*Edits*

Export arrangement notes as SMF type 1 at 960 PPQ with the meter and every tempo change (a ramp as a step each sixteenth note). Does not convert audio or embed plugins; includes muted tracks.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Destination .mid file, replaced atomically after success. |
| `trackIds` | array |  | MIDI track IDs to export; defaults to all MIDI tracks. |

### `session.exportAudio`

*Edits*

Export an offline stereo WAV, AIFF, FLAC or Ogg Vorbis using the live plugin graph, with chosen rate, format, range and tail. The file type follows the path's extension and every type streams to disk. PCM clips above full scale; float retains headroom. Reports peak/clipping, and the bitrate for Ogg.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Destination .wav, .aiff for AIFF, .flac for lossless FLAC (both pcm16 or pcm24 only) or .ogg for lossy Ogg Vorbis. Replaced atomically after success. |
| `sampleRate` | integer |  | 44100, 48000 (default), or 96000 Hz. |
| `format` | string |  | pcm16, pcm24 (default), or float32. |
| `startBar` | number |  | Zero-based range start bar, default 0. Use bars or beats, never both. |
| `endBar` | number |  | Exclusive end bar; defaults to arrangement end. |
| `startBeat` | number |  | Zero-based start in quarter-note beats instead of bars. |
| `endBeat` | number |  | Exclusive end in quarter-note beats instead of bars. |
| `tailSeconds` | number |  | Effect release tail after range end, 0–120 seconds, default 3. |
| `dither` | boolean |  | TPDF dither for integer PCM, default true. Ignored for float32. |
| `quality` | number |  | Ogg Vorbis quality 0–1, default 0.6 (about 192 kbit/s; 0.4 ≈ 128, 0.8 ≈ 256). Ignored by the other file types, as format and dither are by Ogg. |

### `session.exportStems`

*Edits*

Export one stereo file per track (WAV, or AIFF, FLAC or Ogg Vorbis with `container`) into a new folder, publishing the entire set only on success. Solo-rendered nonlinear/shared effects can prevent exact summation to the full mix.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `directory` | string | yes | New destination folder; an existing folder is never replaced. |
| `trackIds` | array |  | Track IDs to export; defaults to all tracks. Mute/solo are ignored. |
| `includeEffects` | boolean |  | Include track inserts and send/bus processing, default true. |
| `includeMaster` | boolean |  | Apply master inserts/fader to each stem, default false. |
| `sampleRate` | integer |  | 44100, 48000 (default), or 96000 Hz. |
| `container` | string |  | File type of every stem: wav (default), aiff, flac or ogg. aiff and flac need pcm16 or pcm24; ogg uses quality instead of format. |
| `format` | string |  | pcm16, pcm24 (default), or float32. |
| `startBar` | number |  | Zero-based range start bar, default 0. Use bars or beats, never both. |
| `endBar` | number |  | Exclusive end bar; defaults to arrangement end for every stem. |
| `startBeat` | number |  | Zero-based start in quarter-note beats instead of bars. |
| `endBeat` | number |  | Exclusive end in quarter-note beats instead of bars. |
| `tailSeconds` | number |  | Effect release tail after range end, 0–120 seconds, default 3. |
| `dither` | boolean |  | TPDF dither for integer PCM, default true. Ignored for float32. |
| `quality` | number |  | Ogg Vorbis quality 0–1 for container ogg, default 0.6 (about 192 kbit/s). |

### `session.batch`

*Edits*

Run many commands in one request. They share one undo step, and with atomic=true (default) a failing command rolls back every edit made before it. Far faster than separate calls: the window answers the whole list in one frame.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `commands` | array | yes | List of {"command": name, "params": {...}} in the order to run. File, history, application and agent commands are not accepted. |
| `atomic` | boolean |  | Roll back earlier edits when one command fails (default true). |

### `session.saveRecoveredTake`

*Edits · Needs the app*

Write a recording that could not be placed on a track to a WAV file, which frees the window to open other sessions. ui.status reports it as `recoveredTake`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Destination .wav. |

### `session.snapshots`

List recovery snapshots newest first, with paths, titles, times and sizes.

### `session.restoreSnapshot`

*Edits · Needs the app*

Open a recovery snapshot in the window as an unsaved copy.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Snapshot path from session.snapshots. |

### `session.overview`

Everything about the song in one compact answer; call it first. Song (tempo, tempo changes, meter, key, length in bars and seconds), transport (playhead, cycle, metronome), sections (markers), every track with its instrument (name, format, vendor), inserts (plugin, bypass, parameters changed from their defaults as displayed), sends, fader in dB, pan, mute/solo/arm/monitor, problems that keep it silent, clips (bars, names, note counts and pitch ranges, audio sources, fades), automation lanes and controller lanes; the buses, selection, takes, undo history and, in the app, what the window shows (ui.state). Clips per track are capped by maxClips; `truncated` says what was left out and `next` names the commands that give the details.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string |  | Only this track (id or name), with every clip. |
| `maxClips` | integer |  | Clips listed per track, 0-200. Default: 12, fewer in songs with many tracks (about 48 in all); the rest are counted and their bars shown in covers. |
| `parameters` | boolean |  | List changed plugin parameters (default true, at most 6 per plugin). |

### `session.scoreCut`

*Edits*

Score a cut from kimchi: put its audio on a new audio track at bar 1 and its markers on the ruler at the bars where they fall, so the music can follow the picture. Takes a hand-off manifest from kimchi (handoff.inbox) or the audio, length and markers directly. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `manifest` | string |  | A kimchi hand-off manifest (JSON file) from handoff.inbox. |
| `path` | string |  | The cut's audio (WAV, AIFF, FLAC, MP3, Ogg, AAC…), if there is no manifest. |
| `name` | string |  | Name for the audio track (default: the cut's name). |
| `markers` | array |  | Markers of the cut: objects with `time` (seconds) and `label`. |
| `durationSeconds` | number |  | Length of the cut; sets the cycle over it when given. |

### `session.formats`

What ryolune opens from and writes for other music apps: DAWproject (Bitwig Studio, Studio One, Cubase…), MIDI files, audio files and stems, each with what survives the trip; and every app ryolune knows (Ableton Live, Logic Pro, FL Studio, Bitwig Studio, REAPER, Cubase, Studio One, Pro Tools, GarageBand) with the formats it exchanges, how to bring a song over and take it back, and whether it is installed on this computer.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `app` | string |  | Only this app: ableton, logic, fl, bitwig, reaper, cubase, studioone, protools or garageband. |

### `session.importFrom`

*Edits*

Open a song from another app as a new, unsaved song that replaces the open one (unsaved changes are discarded): a DAWproject (.dawproject, with tracks, buses, clips, notes, audio, the mix, markers, tempo and installed plugins), a MIDI file (an instrument track per channel, with its tempo), or audio files (one audio track per file from bar 1, for stems). Returns a report of what came across, what changed and what was left out. Runs as a job in the app.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string |  | The file to open (.dawproject, .mid, or an audio file). |
| `paths` | array |  | Several audio files (stems), each on its own track. |

### `session.exportTo`

*Edits*

Write the song for another app: dawproject (Bitwig Studio, Studio One, Cubase), midi, audio (the mix), stems (a new folder, one WAV per track) or package (a new folder with the MIDI file and one WAV per track, for apps without DAWproject). Files are replaced atomically and folders must be new. Returns a report of what the format could not carry. Runs as a job in the app.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Destination file, or the new folder for stems and package. |
| `format` | string |  | dawproject, midi, audio, stems or package. Default: the app's best, else from the extension (.dawproject, .mid, .wav…; none makes a package). |
| `app` | string |  | The app it is for (see session.formats); picks its best format. |

## plugin

### `plugin.list`

Search a page of installed plugins from the scanner cache. Use query/kind/format to avoid returning a large library; follow nextOffset for more. Channel layouts that a vendor registers as separate plugins ("C1 comp (m)", "(s)", "(m->s)") are one row: its id is the layout a stereo track wants and `layouts` lists the others.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `everyLayout` | boolean |  | List each channel layout as its own row instead (default false). |
| `includeDisabled` | boolean |  | Also list plugins turned off with plugin.disable (each row says `enabled`). |
| `query` | string |  | Case-insensitive name, vendor or plugin ID search. |
| `format` | string |  | stock, native, clap, vst3 or au. |
| `kind` | string |  | instrument or effect. |
| `folder` | string |  | Sound folder from plugin.folders, for example Synths, Drums, Dynamics or Space & Time. |
| `favorite` | boolean |  | Only favourites. |
| `sort` | string |  | name (default) or recent: most recently loaded first. |
| `offset` | integer |  | Zero-based result offset, default 0. |
| `limit` | integer |  | Page size 1-200, default 50. |

### `plugin.scan`

*Edits*

Scan installed plugin directories in isolated child processes and refresh the plugin cache. May take several minutes.

### `plugin.folders`

The sound folders of the plugin library with how many instruments and effects each holds, plus favourites and recents.

### `plugin.setFavorite`

*Edits*

Star or unstar a plugin so it shows under Favourites in the browser.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `pluginId` | string | yes | Plugin id from plugin.list. |
| `favorite` | boolean | yes | Star (true) or unstar (false). |

### `plugin.setFolder`

*Edits*

File a plugin under another sound folder, or a new one of your own. Omit folder to return it to the automatic one.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `pluginId` | string | yes | Plugin id from plugin.list. |
| `folder` | string |  | Folder name, 1-40 characters. |

### `plugin.scaffold`

*Edits*

Start a new ryolune native plugin in Rust at a path of your choice: writes a crate with a working effect or instrument, its plugin.toml, a test that runs it through the real plugin ABI, and build notes. Build it with cargo, then plugin.install. plugin.new does the same in the lsuite sources folder, for plugin.build and plugin.publishLocal.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Directory to create. It must not exist yet. |
| `name` | string | yes | Plugin display name, for example Warm Drive. |
| `kind` | string |  | effect (default) or instrument. |
| `vendor` | string |  | Your name or label, default My Studio. |

### `plugin.install`

*Edits*

Install a built plugin: an lsuite bundle (a folder with plugin.toml and its library) goes to ~/.lsuite/plugins/ryolune and is loaded at once; a bare library (.dylib, .so, .dll or .onplug) is copied into ryolune's plugin folder, then plugin.scan loads it.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | The bundle folder, or the built library such as target/release/libwarm_drive.dylib. |

### `plugin.describe`

Describe an installed plugin without placing it: format, vendor, category, latency, whether it has its own window, its factory programs and ryolune presets, and its parameters with ids, ranges, units, defaults as displayed and whether they can be automated.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `pluginId` | string | yes | Descriptor ID from plugin.list (stock:Space, vst3:…), or the plugin's name. |
| `query` | string |  | Only parameters whose name matches these words. |
| `limit` | integer |  | Parameters to return, 1-10000, default 200. |

### `plugin.info`

One plugin, as the Plugins window shows it: name, kind, format, vendor, version, description, whether it is on, where it came from (its bundle and plugin.toml for lsuite plugins) and its parameters.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Plugin id from plugin.list (stock:Space, native:com.you.drive, clap:…), or its name. |

### `plugin.enable`

*Edits*

Turn a plugin back on: it shows in the browser and agents can load it again.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Plugin id from plugin.list. |

### `plugin.disable`

*Edits*

Turn a plugin off without deleting it (a setting): it leaves the browser and agents cannot load it; songs that use it still play it.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Plugin id from plugin.list. |

### `plugin.remove`

*Edits*

Delete an lsuite plugin you installed (its bundle in ~/.lsuite/plugins/ryolune) and rescan. Stock plugins and CLAP, VST3 or Audio Unit plugins can only be disabled.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Plugin id from plugin.list. |

### `plugin.guide`

How to write a ryolune plugin in Rust, for an agent: the SDK, the kinds, plugin.toml, the rules of the audio thread, an example and the recipe (plugin.toolchain, plugin.new, plugin.writeSource, plugin.build, plugin.publishLocal). Markdown, made from the SDK this ryolune carries.

### `plugin.toolchain`

Whether Rust is installed to build plugins: cargo and rustc paths, the version, ok, and how to install it (rustup) when it is missing.

### `plugin.new`

*Edits*

Start a plugin crate from the SDK template in ~/.lsuite/plugins-src/ryolune/<name>/: a working effect or instrument, its plugin.toml and tests. Returns its path and files.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | Plugin display name, for example Warm Drive. |
| `kind` | string | yes | effect or instrument. |
| `vendor` | string |  | Your name or label, default the person's lsuite name or My Studio. |

### `plugin.writeSource`

*Edits*

Write one whole file of a plugin crate made by plugin.new (src/lib.rs, a new module, plugin.toml). Paths outside the crate are refused.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | The plugin's name or crate name from plugin.new. |
| `path` | string | yes | Path inside the crate, such as src/lib.rs. |
| `contents` | string | yes | The whole file. |

### `plugin.build`

*Edits*

Build a plugin crate (cargo build --release). Returns ok and the compiler's errors as {file, line, column, message, rendered}, never the whole log. The first build takes a minute; later ones seconds. Runs as a job in the app.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | The plugin's name or crate name from plugin.new. |

### `plugin.publishLocal`

*Edits*

Build a plugin crate, install it as an lsuite plugin bundle and load it: it is in plugin.list at once, and songs already using it reload it, without restarting ryolune. A failed build installs nothing and returns its errors.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | The plugin's name or crate name from plugin.new. |

## transport

### `transport.play`

*Edits*

Start playback from the playhead. Needs the ryolune app (live mode).

### `transport.record`

*Edits*

Record armed audio and MIDI tracks in the running app. Disable cycle before recording.

### `transport.stop`

*Edits*

Stop playback and recording, like the Stop button.

### `transport.locate`

*Edits*

Move the playhead. Give one of bar, beats or markerId.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `bar` | number |  | Zero-based bar position. |
| `beats` | number |  | Zero-based beat position. |
| `markerId` | string |  | A marker from marker.list: go to its bar. |

### `transport.returnToStart`

*Edits*

Move the playhead to the beginning.

### `transport.setTempo`

*Edits*

Set the song's starting tempo in beats per minute, like dragging the tempo display. Tempo changes later in the song (tempo.list) keep theirs. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `bpm` | number | yes | Beats per minute, 20-400. |

### `transport.setTimeSignature`

*Edits*

Set the meter. Clips keep their bar positions and automation moves with them, in one undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `numerator` | integer | yes | Beats per bar, 1-32. |
| `denominator` | integer | yes | Beat unit: 1, 2, 4, 8, 16 or 32. |

### `transport.setKey`

*Edits*

Set the song key shown in the transport (a label; it does not transpose anything).

| Parameter | Type | Required | Description |
|---|---|---|---|
| `key` | string | yes | Key label such as "C minor". |

### `transport.setCycle`

*Edits*

Enable or disable cycle (loop) playback and optionally set its range.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `enabled` | boolean | yes | Cycle on or off. |
| `startBar` | number |  | Cycle start bar. |
| `endBar` | number |  | Cycle end bar; must be after startBar. |

### `transport.setMetronome`

*Edits*

Turn the metronome click on or off for playback and recording.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `enabled` | boolean | yes | Metronome on or off. |

### `transport.setSnap`

*Edits*

Set the grid that drags, the playhead and clip.quantize snap to, in notes per bar.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `division` | integer | yes | Notes per bar: 1, 2, 4, 8, 16, 32 or 64. |

### `transport.punch`

*Edits · Needs the app*

Turn record on or off. While the transport is rolling this punches in or out on the armed tracks without stopping playback; while stopped it only sets the record button.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `enabled` | boolean | yes | Record on or off. |

## track

### `track.list`

List tracks in arrangement order with their instrument and clip count.

### `track.add`

*Edits*

Add a track at the end of the arrangement and select it.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `kind` | string | yes | "midi" for an instrument track, "audio", or "bus" for a bus track that other tracks route or send to (see track.setOutput, strip.setSend, track.group). |
| `name` | string |  | Track name. Defaults to Instrument N / Audio N / Bus N. |
| `color` | string |  | CSS colour: #rrggbb or oklch(l c h). Defaults to the palette. |
| `instrument` | string |  | Instrument for a MIDI track; see session.catalog. |

### `track.remove`

*Edits*

Delete a track and every clip on it.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |

### `track.rename`

*Edits*

Rename a track (names can then be used instead of its id). One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `name` | string | yes | New name. |

### `track.setMute`

*Edits*

Mute or unmute a track, like its M button. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `muted` | boolean | yes | Muted or not. |

### `track.setSolo`

*Edits*

Solo or unsolo a track, like its S button: while any track is soloed, the others are silent. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `solo` | boolean | yes | Soloed or not. |

### `track.setArmed`

*Edits*

Arm or disarm an audio or MIDI track for recording.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `armed` | boolean | yes | Armed or not. |

### `track.setMonitor`

*Edits*

Hear the live input through an audio track's inserts, sends and fader. auto monitors while the track is armed and not playing back its own clip (and again while recording); on always; off never. audio.status reports whether the input is routed, the measured latency, and `blocked` when the built-in microphone would feed back through the built-in speakers.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `monitor` | string | yes | off, auto or on. |

### `track.setVolume`

*Edits*

Set a track fader. The scale is the mixer's: 0.75 is 0 dB, 1.0 is +6 dB, 0 is silent. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `volume` | number | yes | 0.0 (silent) to 1.0 (+6 dB); 0.75 is unity. |

### `track.setPan`

*Edits*

Set a track's stereo pan, -100 (left) to 100 (right). One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `pan` | number | yes | -100 (left) to 100 (right). |

### `track.setColor`

*Edits*

Set a track's colour in the arrangement and mixer. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `color` | string | yes | CSS colour: #rrggbb or oklch(l c h). |

### `track.move`

*Edits*

Move a track to another position.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `index` | integer | yes | Zero-based target index. |

### `track.select`

*Edits*

Select a track in the interface.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |

### `track.setOutput`

*Edits*

Route a track's fader to a bus track (a group: drums into a Drums bus) or back to the Stereo Out, like the inspector's Output menu. Bus tracks always feed the Stereo Out. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `output` | string | yes | A bus track's id or name, or "Stereo Out" (also master or none). |

### `track.group`

*Edits*

Make a bus and route tracks to it, like selecting tracks and choosing Group into Bus: a drum group, a vocal group. The bus comes right after the last of them. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackIds` | array | yes | Tracks to route, by id or name (not bus tracks). |
| `name` | string |  | The bus's name. Defaults to "Group N". |

### `track.duplicate`

*Edits*

Copy a track with its strip and clips right after the original.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `name` | string |  | Name for the copy. Defaults to the original name plus " copy". |

## clip

### `clip.list`

List clips (regions) without their notes.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string |  | Only clips on this track. |

### `clip.get`

Return one clip with its notes or audio reference.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |

### `clip.create`

*Edits*

Create a MIDI or audio clip. Audio clips need sourceId (see session.get sources).

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `startBar` | number | yes | Zero-based start bar. |
| `lengthBars` | number | yes | Length in bars, greater than 0. |
| `name` | string |  | Clip name. |
| `notes` | array |  | Array of {start, length, pitch, velocity?} with start/length in beats relative to the clip, pitch 0-127 (60 = C4), velocity 1-127 (default 100). |
| `sourceId` | string |  | Audio source id for a clip on an audio track. |
| `offsetSeconds` | number |  | Seconds into the source where the audio clip starts (default 0). |

### `clip.move`

*Edits*

Move a clip to another bar and/or track of the same kind.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `startBar` | number |  | New zero-based start bar. |
| `trackId` | string |  | Destination track. |

### `clip.resize`

*Edits*

Change a clip's length in bars, keeping its start, like dragging its right edge. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `lengthBars` | number | yes | New length in bars. |

### `clip.rename`

*Edits*

Rename a clip (region); a unique name can then be used instead of its id. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `name` | string | yes | New name. |

### `clip.split`

*Edits*

Split a clip at a bar, keeping notes and audio offsets aligned.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `bar` | number | yes | Absolute bar inside the clip. |

### `clip.duplicate`

*Edits*

Duplicate a clip immediately after itself.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |

### `clip.copy`

*Edits*

Copy a clip to the clipboard the window, the CLI and agents share. The session does not change.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string |  | Clip to copy; defaults to the selected clip. |

### `clip.cut`

*Edits*

Copy a clip to the shared clipboard and remove it, in one undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string |  | Clip to cut; defaults to the selected clip. |

### `clip.paste`

*Edits*

Paste the clipboard as a new clip. MIDI goes on instrument tracks and audio on audio tracks.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string |  | Destination track. Defaults to the selected track when its kind fits, else the track the clip came from. |
| `bar` | number |  | Zero-based start bar; defaults to the bar the playhead is in. |

### `clip.remove`

*Edits*

Delete a clip (region) and its notes or audio placement. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |

### `clip.setNotes`

*Edits*

Replace every note of a MIDI clip in one undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `notes` | array | yes | Array of {start, length, pitch, velocity?} with start/length in beats relative to the clip, pitch 0-127 (60 = C4), velocity 1-127 (default 100). |

### `clip.addLoop`

*Edits*

Insert one of the bundled MIDI loops (see session.catalog) as a new clip, switching the track's instrument to the loop's.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | Loop name from session.catalog. |
| `trackId` | string |  | MIDI track. Defaults to the selected MIDI track, or a new one. |
| `startBar` | number |  | Start bar. Defaults to the playhead's bar. |

### `clip.select`

*Edits*

Select a clip and open it in the editor, optionally selecting one of its notes.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `noteId` | string |  | Note id from note.list to select inside the clip. |

### `clip.trim`

*Edits*

Move a clip's left edge while its content stays where it is on the timeline: notes keep their bar positions and audio keeps its alignment. The right edge does not move unless lengthBars is given.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `startBar` | number | yes | New absolute start bar of the clip. |
| `lengthBars` | number |  | New length in bars. Defaults to keeping the right edge in place. |

### `clip.deselect`

*Edits*

Clear the clip and note selection, like clicking an empty lane. The selected track stays.

### `clip.setFades`

*Edits*

Set an audio clip's fade in, fade out and fade curve, in one undo step. Omitted values keep theirs. Fades are seconds of audio and are kept inside the clip: when they would overlap they shrink in proportion. Split, trim and resize keep them sensible.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `fadeInSeconds` | number |  | Fade-in length in seconds; 0 removes it. |
| `fadeOutSeconds` | number |  | Fade-out length in seconds; 0 removes it. |
| `curve` | string |  | equalPower (default: keeps loudness through a crossfade), linear or exponential (slow start). |

### `clip.setGain`

*Edits*

Set an audio clip's gain, applied before the track's inserts and fader.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `gainDb` | number | yes | Gain in dB, -60 to +24; 0 is unchanged. |

### `clip.humanize`

*Edits*

Humanize MIDI timing and velocity reproducibly without changing pitch, inside region bounds. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `timingMs` | number |  | Maximum timing offset, 0-100 ms (default 10). |
| `velocity` | integer |  | Maximum velocity offset, 0-32 (default 8). |
| `seed` | integer |  | Random seed, 0-4294967295 (default 1). |

### `clip.velocityRamp`

*Edits*

Shape MIDI dynamics from the first to last onset, preserving chords at equal velocity. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `from` | integer | yes | Starting velocity 1-127. |
| `to` | integer | yes | Ending velocity 1-127. |

### `clip.fitScale`

*Edits*

Move MIDI pitches to the closest note in a scale, choosing down on ties. Timing and velocity stay intact.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `root` | integer | yes | Root pitch class 0-11, C=0. |
| `scale` | string | yes | major, minor, dorian, mixolydian, pentatonicMajor or pentatonicMinor. |

### `clip.reverseMidi`

*Edits*

Reverse MIDI note timing within the region, preserving pitch, duration and velocity.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |

### `clip.legato`

*Edits*

Extend MIDI notes to the next distinct onset or region end. Simultaneous chord notes remain together.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |

### `clip.repeat`

*Edits*

Repeat a MIDI or audio region immediately after itself in one undo step, assigning unique IDs.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `count` | integer | yes | Number of additional copies, 1-64. |

### `clip.quantize`

*Edits*

Snap every note start in a MIDI clip to the grid, in one undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `division` | integer |  | Notes per bar: 1, 2, 4, 8, 16, 32 or 64. Defaults to the transport snap. |
| `strength` | number |  | How far to move toward the grid, 0-100 percent (default 100). |
| `lengths` | boolean |  | Also quantize note lengths (default false). |

### `clip.transpose`

*Edits*

Shift every note in a MIDI clip by semitones, clamped to 0-127.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `semitones` | integer | yes | Signed semitones, -48 to 48. |

## note

### `note.list`

List the notes of a MIDI clip.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |

### `note.add`

*Edits*

Add one note to a MIDI clip, like drawing it in the piano roll. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `start` | number | yes | Start in beats relative to the clip. |
| `length` | number | yes | Length in beats, greater than 0. |
| `pitch` | integer | yes | MIDI pitch 0-127 (60 = C4). |
| `velocity` | integer |  | 1-127, default 100. |

### `note.update`

*Edits*

Change a note's timing, pitch or velocity.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `noteId` | string | yes | Note id from note.list. |
| `start` | number |  | Start in beats relative to the clip. |
| `length` | number |  | Length in beats. |
| `pitch` | integer |  | MIDI pitch 0-127. |
| `velocity` | integer |  | Velocity 1-127. |

### `note.remove`

*Edits*

Delete one note from a MIDI clip. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `noteId` | string | yes | Note id from note.list. |

### `note.preview`

*Edits · Needs the app*

Audition one note on a track's instrument, like clicking a piano-roll key.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `pitch` | integer | yes | MIDI pitch 0-127. |
| `velocity` | integer |  | 1-127, default 100. |

### `note.hold`

*Edits · Needs the app*

Hold or release a note on the selected instrument track, like a key on a MIDI keyboard. Held notes are recorded when the transport is recording. Always release what you hold.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `pitch` | integer | yes | MIDI pitch 0-127. |
| `on` | boolean | yes | true presses the key, false releases it. |
| `velocity` | integer |  | 1-127, default 100. |

### `note.releaseAll`

*Edits · Needs the app*

Release every note held with note.hold or musical typing.

## strip

### `strip.get`

Return a track or bus channel strip: instrument, eight inserts and two sends. Bus IDs: master, bus-a, bus-b.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |

### `strip.setInstrument`

*Edits*

Choose the instrument of a MIDI track.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `instrument` | string | yes | Instrument name from session.catalog. |

### `strip.setInsert`

*Edits*

Load, bypass or clear an insert effect slot.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `slot` | integer | yes | Insert slot 0-7. |
| `effect` | string |  | Effect name from session.catalog. Omit it, and bypassed, to empty the slot. |
| `bypassed` | boolean |  | Bypass the effect instead of running it (default false). Without effect, bypasses or enables the effect already in the slot. |

### `strip.setSendLevel`

*Edits*

Set a send's level; sends 0 and 1 feed the reverb (A) and delay (B) buses unless strip.setSend pointed them at a bus track.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `send` | integer | yes | 0 for A · Reverb, 1 for B · Delay (or where they point), 2 or 3 for a further send. |
| `levelDb` | number |  | Level in dB, -100 to 0. Omit or null for off. |

### `strip.setPlugin`

*Edits*

Load a stock or installed external plugin (CLAP, VST3, AU, native) as a MIDI track's instrument (omit slot) or as an insert (slot 0-7, or firstFreeSlot) on a track or bus. Name it by pluginId, or by plugin: a search such as "pro q" or "diva" that must single out one plugin of the right kind.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `slot` | integer |  | Insert slot 0-7. Omit for the instrument. |
| `firstFreeSlot` | boolean |  | Put the effect in the first empty insert slot (default false). |
| `pluginId` | string |  | Stable descriptor ID from plugin.list, for example stock:Space or vst3:… |
| `plugin` | string |  | Plugin name or search words, instead of pluginId. |

### `strip.setBypass`

*Edits*

Bypass or enable a plugin without replacing its settings.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `slot` | integer |  | Insert slot 0-7. Omit for the instrument. |
| `bypassed` | boolean | yes | Whether to bypass the processor. |

### `strip.getState`

Capture and read the selected plugin's current parameters and base64 state, including changes from its native editor.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `slot` | integer |  | Insert slot 0-7. Omit for the instrument. |

### `strip.setState`

*Edits*

Restore base64 state previously captured from this plugin, replacing its explicit parameter overrides.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `slot` | integer |  | Insert slot 0-7. Omit for the instrument. |
| `blob` | string | yes | Base64 plugin state from strip.getState or session.get. |

### `strip.setSend`

*Edits*

Point one of a track's sends at a bus and/or set its level, like a send knob and its menu. Sends 0 and 1 feed A · Reverb and B · Delay unless pointed elsewhere; sends 2 and 3 exist once pointed at a bus. A track sends to bus tracks, A or B; a bus track to A or B. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `send` | integer | yes | Send 0-3. |
| `bus` | string |  | Where it goes: a bus track's id or name, A (A · Reverb) or B (B · Delay). "none" removes send 2 or 3, or gives send 0 or 1 back to A or B, off. |
| `levelDb` | number |  | Level in dB, -100 to 0. Omit to keep it; null for off. |

### `strip.moveInsert`

*Edits*

Move an insert to another slot on the same strip, shifting the others.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `from` | integer | yes | Slot 0-7 to move. |
| `to` | integer | yes | Destination slot 0-7. |

### `strip.parameters`

Read the parameters of the plugin on a strip (a track's instrument or insert, or a bus insert): id, name, plain value, display text as the plugin shows it, normalized 0-1 position, min, max, default, unit, steps, labels, whether it can be automated and the automation lane that drives it. Filter by name with query; pages of `limit`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `slot` | integer |  | Insert slot 0-7. Omit for the MIDI track's instrument. |
| `query` | string |  | Only parameters whose name matches these words, best match first ("filter cutoff"). |
| `changed` | boolean |  | Only parameters set away from their default (default false). |
| `offset` | integer |  | Zero-based offset into the result, default 0. |
| `limit` | integer |  | Parameters to return, 1-10000, default 200. |

### `strip.setParameter`

*Edits*

Set one plugin parameter in one undo step. Name it by parameterId or parameter (its name); give exactly one of value (plain, within min-max), normalized (0-1 of the range) or text (what the plugin displays, such as "-6 dB", "50%" or a label like "Hall"). Answers with the parameter as it now reads.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `slot` | integer |  | Insert slot 0-7. Omit for the MIDI track's instrument. |
| `parameterId` | integer |  | Parameter id from strip.parameters. |
| `parameter` | string |  | Parameter name, matched like a search ("cutoff", "mix"), or its id as text. Use instead of parameterId. |
| `value` | number |  | Plain value within the parameter's min and max. |
| `normalized` | number |  | Position 0-1 along the range, following its log or stepped scale. |
| `text` | string |  | Display text to parse: "-6 dB", "440 Hz", "2.5k", "50%" (of the range), "On", or a label. |

### `strip.setParameters`

*Edits*

Set several plugin parameters atomically in one undo step. Keys are parameter ids or names; each value is a plain number, a display string ("-6 dB", "Hall") or {"normalized": 0-1}.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `slot` | integer |  | Insert slot 0-7. Omit for the MIDI track's instrument. |
| `values` | object | yes | Object mapping parameter ids or names to a plain number, a display string or {normalized}. |

### `strip.programs`

List the programs of the plugin on a strip: its own factory programs (Audio Unit factory presets, a VST3 program list) with the current one, and the ryolune presets saved for it (preset.list). CLAP preset discovery and plugins that only show presets in their own window are not listed.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `slot` | integer |  | Insert slot 0-7. Omit for the MIDI track's instrument. |

### `strip.setProgram`

*Edits*

Load one of the plugin's programs, by index or by name, in one undo step. A name that is not a factory program loads the ryolune preset of that name (preset.load).

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `slot` | integer |  | Insert slot 0-7. Omit for the MIDI track's instrument. |
| `index` | integer |  | Program index from strip.programs. |
| `name` | string |  | Program or preset name. |

### `strip.removeInsert`

*Edits*

Empty an insert slot on a track or bus, removing the plugin, its settings and any automation of it (one undo step).

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `slot` | integer | yes | Insert slot 0-7. |

### `strip.loadSample`

*Edits*

Turn a sound into an instrument: load an audio file, or audio already in the song, into Sample Keys, which plays it across the keyboard from its root note. On the given MIDI track, else on a new one named after the sound. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string |  | MIDI track to play it on (default: a new track). |
| `path` | string |  | Audio file to load (WAV, AIFF, FLAC, MP3, Ogg, AAC…). |
| `sourceId` | string |  | Audio already in the song, by source id from session.overview, instead of a file. |
| `rootNote` | integer |  | The MIDI note that plays the sound at its own pitch, 0-127 (default 60). |
| `name` | string |  | Name of a new track (default: the sound's name). |

## master

### `master.setVolume`

*Edits*

Set the stereo output fader, including offline exports.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `volume` | number | yes | 0.0 (silent) to 1.0 (+6 dB); 0.75 is unity. |

## history

### `history.undo`

*Edits*

Undo the last document edit, or several.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `steps` | integer |  | How many edits to undo, 1-200 (default 1). |

### `history.redo`

*Edits*

Redo the last undone edit, or several.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `steps` | integer |  | How many edits to redo, 1-200 (default 1). |

### `history.info`

Report whether undo and redo are available and the current revision.

## source

### `source.peaks`

The waveform of an audio source as the window draws it: peak magnitudes 0-1, reduced to at most `points` values.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `sourceId` | string | yes | Audio source id, from clip.get or session.inspect. |
| `points` | integer |  | Most values to return, 16-4000 (default 400). |

## marker

### `marker.list`

List the song's markers (sections) in bar order, with the bar where each section ends.

### `marker.add`

*Edits*

Add a marker, the start of a song section, to the ruler.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `bar` | number |  | Zero-based bar. Defaults to the playhead, on the snap grid. |
| `name` | string |  | Section name such as "Verse 1" or "Chorus". Defaults to "Marker N". |
| `color` | string |  | CSS colour: #rrggbb or oklch(l c h). Defaults to the theme's marker colour. |

### `marker.rename`

*Edits*

Rename a marker (a song section such as Verse or Chorus). One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `markerId` | string | yes | Marker id, as listed by marker.list. |
| `name` | string | yes | New name, 1-120 characters. |

### `marker.move`

*Edits*

Move a marker to another bar of the ruler. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `markerId` | string | yes | Marker id, as listed by marker.list. |
| `bar` | number | yes | Zero-based bar. |

### `marker.setColor`

*Edits*

Colour a marker, or give it back the theme's colour.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `markerId` | string | yes | Marker id, as listed by marker.list. |
| `color` | string |  | CSS colour: #rrggbb or oklch(l c h). Omit or null for the theme's colour. |

### `marker.remove`

*Edits*

Delete a marker from the ruler; the music does not change. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `markerId` | string | yes | Marker id, as listed by marker.list. |

### `marker.goto`

*Edits*

Move the playhead to a marker, by id or by name.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `markerId` | string |  | Marker id from marker.list. |
| `name` | string |  | Marker name, case-insensitive, when no id is given. |

### `marker.next`

*Edits*

Move the playhead to the next marker after it. Answers with marker null, and leaves the playhead, when there is none.

### `marker.previous`

*Edits*

Move the playhead to the marker before it. Answers with marker null, and leaves the playhead, when there is none.

### `marker.cycleSection`

*Edits*

Cycle one song section: set the cycle from a marker to the next marker (or the end of the song) and turn cycle on.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `markerId` | string |  | Marker that starts the section. Defaults to the section the playhead is in. |

## tempo

### `tempo.list`

List the song's tempo: the starting tempo and every change after it in bar order, with where each one falls in seconds, and the tempo at the playhead.

### `tempo.set`

*Edits*

Set the tempo from a bar on, like adding or dragging a point on the tempo track. Bar 0 sets the starting tempo; any later bar adds a change there or replaces the one already there. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `bar` | number | yes | Zero-based bar of the tempo change, as listed by tempo.list. |
| `bpm` | number | yes | Beats per minute, 20-400. |
| `ramp` | boolean |  | Glide from the previous tempo to reach this one at the bar, instead of jumping to it there (a ritardando or accelerando). Defaults to the ramp of the change already at that bar, else false. Not for bar 0. |

### `tempo.move`

*Edits*

Move a tempo change to another bar, like dragging its point on the tempo track, keeping its ramp and, unless bpm is given, its tempo. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `bar` | number | yes | Zero-based bar of the tempo change, as listed by tempo.list. |
| `toBar` | number | yes | New zero-based bar, after bar 0 and free of another change. |
| `bpm` | number |  | A new tempo for it at the same time, 20-400. |

### `tempo.remove`

*Edits*

Delete a tempo change: the tempo before it carries on. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `bar` | number | yes | Zero-based bar of the tempo change, as listed by tempo.list. |

### `tempo.clear`

*Edits*

Delete every tempo change, or those in a range of bars, so the song keeps its starting tempo there. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `startBar` | number |  | First bar of the range (default 0). |
| `endBar` | number |  | Bar where the range ends, exclusive (default: the end of the song). |

## automation

### `automation.list`

List automation lanes and absolute beat points. Track/master gain and pan are sample-accurate; plugin changes occur at blocks of at most 256 frames.

### `automation.create`

*Edits*

Create read automation for trackVolume, trackPan, masterVolume or pluginParameter. One lane per target; values use the parameter's plain units.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `target` | string | yes | trackVolume, trackPan, masterVolume or pluginParameter |
| `trackId` | string |  | Track ID; master/bus-a/bus-b also accept plugin parameters |
| `slot` | integer |  | Plugin insert 0-7; omit for the track instrument |
| `parameterId` | integer |  | Plugin parameter ID from strip.parameters |
| `parameter` | string |  | Plugin parameter name instead of parameterId, matched like strip.parameters query |
| `name` | string |  | Lane name |
| `interpolation` | string |  | linear (default) or step |
| `points` | array |  | Optional {beat,value,id?} points in absolute quarter-note beats |

### `automation.setPoints`

*Edits*

Replace all points in a lane atomically, with one undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `laneId` | string | yes | Automation lane ID |
| `points` | array | yes | Array of {beat,value,id?}; beats must be distinct |

### `automation.setPoint`

*Edits*

Add or move one automation point. Beats are absolute quarter notes from the song start (bar × beats per bar); values are the target's plain units: volume 0-1 (0.75 = 0 dB), pan -100 to 100, plugin parameters as strip.parameters lists them.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `laneId` | string | yes | Automation lane ID |
| `pointId` | string |  | Existing point ID to move; omitted creates a point |
| `beat` | number | yes | Absolute quarter-note beat |
| `value` | number | yes | Plain parameter value |

### `automation.removePoint`

*Edits*

Delete one point of an automation lane. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `laneId` | string | yes | Automation lane ID |
| `pointId` | string | yes | Point ID |

### `automation.setEnabled`

*Edits*

Enable read automation or leave the manual control active.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `laneId` | string | yes | Automation lane ID |
| `enabled` | boolean | yes | Whether automation controls its target |

### `automation.setInterpolation`

*Edits*

Choose linear ramps or steps between points.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `laneId` | string | yes | Automation lane ID |
| `interpolation` | string | yes | linear or step |

### `automation.remove`

*Edits*

Delete an automation lane; undo restores it.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `laneId` | string | yes | Automation lane ID |

## controller

### `controller.list`

List a MIDI clip's controller points (control changes, pitch bend, channel and polyphonic pressure) and a summary of its lanes. Each value holds until the next point of its lane.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `kind` | string |  | Only this kind: cc, bend, pressure or poly. |
| `number` | integer |  | Only this controller number (kind cc) or key (kind poly). |
| `channel` | integer |  | Only this MIDI channel, 0-15. |

### `controller.add`

*Edits*

Add a controller point to a MIDI clip. A point already at that time in the same lane takes the new value instead.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `kind` | string | yes | cc (control change), bend (pitch bend), pressure (channel pressure) or poly (polyphonic key pressure). |
| `number` | integer |  | Controller number 0-119 for kind cc: 1 mod wheel, 7 volume, 10 pan, 11 expression, 64 sustain pedal. The key 0-127 for kind poly. Omit for bend and pressure. |
| `channel` | integer |  | MIDI channel 0-15 (channel 1-16 to a musician); default 0. A lane is one kind, number and channel. |
| `time` | number | yes | Beats from the clip start, inside the clip. |
| `value` | integer | yes | 0-127 for cc, pressure and poly; -8192 (down) to 8191 (up) for bend, 0 centred. |

### `controller.update`

*Edits*

Move a controller point or change its value.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `controllerId` | string | yes | Point id from controller.list. |
| `time` | number |  | New time in beats from the clip start. |
| `value` | integer |  | 0-127 for cc, pressure and poly; -8192 (down) to 8191 (up) for bend, 0 centred. |

### `controller.remove`

*Edits*

Delete one controller point (CC, pitch bend or pressure) from a MIDI clip. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `controllerId` | string | yes | Point id from controller.list. |

### `controller.setPoints`

*Edits*

Replace the points of one lane (kind and number) in one undo step: all of them, or only those from `from` up to `to` when given, which is how a drawn curve lands. An empty list clears the lane or the range.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `clipId` | string | yes | Clip id, as listed by clip.list. |
| `kind` | string | yes | cc (control change), bend (pitch bend), pressure (channel pressure) or poly (polyphonic key pressure). |
| `number` | integer |  | Controller number 0-119 for kind cc: 1 mod wheel, 7 volume, 10 pan, 11 expression, 64 sustain pedal. The key 0-127 for kind poly. Omit for bend and pressure. |
| `channel` | integer |  | MIDI channel 0-15 (channel 1-16 to a musician); default 0. A lane is one kind, number and channel. |
| `points` | array | yes | Array of {time, value, id?}: time in beats from the clip start, value as for controller.add. |
| `from` | number |  | Start of the range to replace, in beats (default: the clip start). |
| `to` | number |  | End of the range to replace, in beats, exclusive (default: the clip end). |

## rhythm

### `rhythm.create`

*Edits*

Create a Euclidean drum groove on a new Drum Machine track, in one undo step. Each lane has its own subdivision per bar.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `lanes` | array | yes | 1-8 objects with steps (1-64), pulses (0-steps), rotation (0-steps-1), pitch (0-127), velocity (1-127). |
| `bars` | integer | yes | Groove length, 1-16 bars. |
| `startBar` | number |  | Arrangement start, default zero. |
| `name` | string |  | Groove and track name. |

### `rhythm.preview`

*Edits*

Render a Euclidean groove to a WAV at the session's tempo and meter without creating anything: hear it before rhythm.create. In the app it runs as a job and answers when the file is written.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `lanes` | array | yes | As for rhythm.create. |
| `bars` | integer | yes | 1-4 bars, at most 30 seconds. |
| `path` | string |  | Destination .wav; defaults to one preview file in the data folder that each preview replaces. |
| `inline` | boolean |  | Also return the file as wavBase64 (default false). |

## take

### `take.list`

List creative takes saved inside this project, including the active one.

### `take.create`

*Edits*

Save the current music as a named creative take. Create Original then Variation before experimenting; edits follow the active take. Up to eight takes travel with the saved project.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | Take name, 1-120 characters. |

### `take.select`

*Edits*

Switch to a creative take, preserving edits in the current take. Stops playback; one Undo restores the previous arrangement.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Take ID from take.list. |

### `take.remove`

*Edits*

Remove an inactive creative take. The active arrangement is preserved; undoable.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Inactive take ID from take.list. |

## view

### `view.get`

Read the view: zoom in pixels per bar, first visible bar, the lane width in pixels, follow mode, editor mode, the clip open in the editor, the piano roll's lowest pitch, and the browser's tab and selected row.

### `view.fit`

*Edits*

Zoom the arrangement so the whole song, plus one bar, spans the lanes, and scroll to the first bar. Uses the lane width from view.get.

### `view.set`

*Edits*

Change the arrangement view and the editor. Omitted fields keep their values. Not an undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `pixelsPerBar` | number |  | Arrangement zoom, 12-480 pixels per bar. |
| `scrollBar` | number |  | First visible bar, zero-based. |
| `followPlayhead` | boolean |  | Scroll with the playhead while playing. |
| `editorMode` | string |  | pianoRoll, score or step. |
| `editorClipId` | string |  | Clip to open in the editor; an empty string closes it. |
| `editorLowPitch` | integer |  | Lowest MIDI pitch the piano roll shows, 0-108: its vertical scroll. -1 lets it frame the open clip again. |
| `browserTab` | string |  | instruments, loops, plugins or files. |
| `browserSelection` | string |  | Name of the browser row to select; an empty string clears it. |
| `laneWidth` | number |  | Width of the arrangement lanes in pixels, 50-20000. The window reports it as it resizes; scripts rarely need to. |

## preset

### `preset.list`

List factory and user presets, optionally for one plugin.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `pluginId` | string |  | Only presets for this plugin ID. |

### `preset.save`

*Edits*

Save the plugin in a slot as a named preset: its parameters and, for external plugins, its captured state.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `slot` | integer |  | Insert slot 0-7. Omit for the instrument. |
| `name` | string | yes | Preset name, 1-120 characters. |

### `preset.load`

*Edits*

Apply a preset to the plugin in a slot, in one undo step. The preset must belong to the same plugin.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `slot` | integer |  | Insert slot 0-7. Omit for the instrument. |
| `name` | string | yes | Preset name from preset.list. |

### `preset.delete`

*Edits*

Delete a user preset. Factory presets cannot be deleted.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `pluginId` | string | yes | Plugin ID the preset belongs to. |
| `name` | string | yes | Preset name. |

## settings

### `settings.get`

Read preferences with secrets masked, or one dotted path such as agent.model.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string |  | Dotted setting path. Omit for everything. |

### `settings.set`

*Edits*

Change one preference and save it. The running window applies it at once.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string | yes | Dotted setting path, for example agent.provider or audio.outputDevice. |
| `value` | any | yes | New value: string, number, boolean, list or null. Strings are converted for numbers and booleans. |

### `settings.reset`

*Edits*

Reset one preference, a section, or everything to the defaults.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string |  | Dotted path or section name. Omit to reset everything. |

## audio

### `audio.devices`

List output devices, input devices and MIDI input ports, with the configured and, in live mode, the active selection.

### `audio.status`

*Needs the app*

The audio engine: device, sample rate, CPU load, master and selected-track peaks, MIDI port and live notes. `monitoring` reports input monitoring: state (off, on, blocked, failed), the input device and rate, the measured input and output buffer sizes, frames waiting in the ring, latencyMs computed from them, and frames dropped or underrun.

### `audio.allowSpeakerMonitoring`

*Edits · Needs the app*

Answer the feedback warning: monitoring the built-in microphone through the built-in speakers howls, so it stays muted (audio.status monitoring.state = blocked) until this is called with allow=true. Lasts until the app closes.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `allow` | boolean | yes | true to monitor anyway, false to mute it again. |

### `audio.setOutput`

*Edits · Needs the app*

Switch the output device and reconnect. Omit name for the system default.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `name` | string |  | Output device name from audio.devices. |

### `audio.setInput`

*Edits · Needs the app*

Choose the recording input. Omit name for the system default.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `name` | string |  | Input device name from audio.devices. |

### `audio.setMidiInput`

*Edits · Needs the app*

Connect a MIDI input port for live playing and recording. Omit port to disconnect.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `port` | string |  | MIDI port name from audio.devices. |

### `audio.reconnect`

*Edits · Needs the app*

Reopen the output device, for example after it was unplugged.

## ui

### `ui.screenshot`

*Edits · Needs the app*

Capture the window to a PNG so an agent can see the interface. Returns the file path and size.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `path` | string |  | Destination .png. Defaults to a timestamped file in the app data directory. |

### `ui.showPanel`

*Edits · Needs the app*

Show or hide an interface panel: agent, automation, mixer (every channel, in place of the region editor), controllers (the controller lane under the piano roll), tempo (the tempo track under the ruler), palette (the command palette), settings, plugins (the Plugins window: stock, installed, formats, build with your agent), help, export, recovery, whatsNew (the release notes of this version), diagnostics (Settings › Diagnostics), or master / bus-a / bus-b in the inspector.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `panel` | string | yes | agent, automation, mixer, controllers, tempo, palette, settings, plugins, help, export, recovery, whatsNew, diagnostics, master, bus-a or bus-b. |
| `visible` | boolean |  | Show (default) or hide. |
| `section` | string |  | Settings section: general, audio, interface, agent, generation, plugins, control, updates, diagnostics or about. Plugins window part: stock, installed, formats or build. |

### `ui.openPluginWindow`

*Edits · Needs the app*

Open a plugin's parameter panel in the window, or its native editor with native=true.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `trackId` | string | yes | Track id, as listed by track.list. |
| `slot` | integer |  | Insert slot 0-7. Omit for the instrument. |
| `native` | boolean |  | Open the plugin's own editor window when it has one. |

### `ui.closePluginWindow`

*Edits · Needs the app*

Close one plugin panel and its native editor window. ui.state lists the open ones.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Window id from ui.status pluginWindows. |

### `ui.dismissError`

*Edits · Needs the app*

Dismiss the error shown in the window.

### `ui.closePluginWindows`

*Edits · Needs the app*

Close every plugin panel and native editor.

### `ui.musicalTyping`

*Edits · Needs the app*

Turn musical typing (the computer keyboard as a piano) on or off.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `enabled` | boolean | yes | On or off. |

### `ui.setTool`

*Edits · Needs the app*

Choose the arrangement tool, like keys 1-3: pointer selects and drags, pencil draws clips, scissors splits.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `tool` | string | yes | pointer, pencil or scissors. |

### `ui.status`

*Needs the app*

Window state: open panels, tool, musical typing, plugin panels, status line and any error being shown.

### `ui.state`

*Needs the app*

What the window shows right now: open panels and dialogs (agent, mixer, automation, controllers, palette, settings with its section, help, export, recovery), the prompt waiting for an answer, plugin windows with their track, slot and plugin, the editor (clip, mode, lowest pitch), arrangement zoom and scroll with the visible bars, tool, follow mode, browser tab and selection, selection by name, theme and scale, musical typing, status line and error. Needs the running app.

## app

### `app.info`

Version, platform, executable, data and settings paths, the control discovery file and the host mode.

### `app.checkUpdates`

*Edits · Needs the app*

Check GitHub for a newer release and report it.

### `app.installUpdate`

*Edits · Needs the app*

Download, verify and install the available update. Relaunching is confirmed in the window.

### `app.quit`

*Edits · Needs the app*

Ask the window to quit. Unsaved changes prompt in the window unless discard is true.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `discard` | boolean |  | Quit without saving (default false). |

### `app.confirm`

*Edits · Needs the app*

Answer the unsaved-changes prompt the window shows before New, Open, Quit or Relaunch. ui.status reports it as `prompt`.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `choice` | string | yes | save, discard or cancel. |

### `app.openGuide`

*Edits · Needs the app*

Open one of ryolune's pages, or a sound service's key page, in the web browser.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `guide` | string | yes | plugins: writing native plugins with the Rust SDK. support: donate to ryolune, once or monthly (optional, unlocks nothing). elevenlabs, stability, fal: where to get that service's API key. custom: the contract a custom generation endpoint follows. |

### `app.relaunch`

*Edits · Needs the app*

Relaunch the app, for example after an update was installed. Unsaved changes prompt first.

### `app.logs`

The last lines of ryolune's log (this run's by default, or an earlier run's), with the log folder and every log file. Logs stay on this computer and never hold API keys.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `lines` | integer |  | Lines from the end, 1-2000 (default 100). |
| `file` | string |  | A log file name from `files`, such as ryolune.1.log for the run before. |

### `app.crashReports`

Crash reports newest first: panics that stopped ryolune (crash), panics a background job survived (recovered) and runs that ended without quitting (unclean). With id, one report's full text.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `id` | string |  | A report's file name from the list, to read its text. |

### `app.clearCrashReports`

*Edits*

Delete every crash report in the crashes folder. Logs and recovery snapshots are kept.

### `app.diagnostics`

What a bug report needs: version and build, system, audio device, plugin scan summary, folders, the log file, counts and recent crash reports. Holds no API keys, prompts or songs.

### `app.reportProblem`

*Edits · Needs the app*

Open a new GitHub issue for ryolune in the web browser, with the version, the system and the last crash's summary filled in. Nothing is sent: the person reads and submits it. Only a person can do this.

### `app.whatsNew`

Release notes built into this copy, newest first, in Markdown: this version's by default, one version's (version), every release after one (since), or all of them (all). ui.showPanel panel=whatsNew shows them in the window.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `version` | string |  | One release, such as 0.12.0. |
| `since` | string |  | Every release after this version, up to this copy. |
| `all` | boolean |  | Every release this copy carries (default false). |

### `app.suite`

The lsuite apps installed on this computer (ryolune, kimchi, zenith…) from their discovery files in ~/.lsuite/apps: version, paths of each app and its CLI and MCP server, whether it is running and on which bridge port, and the hand-offs it accepts.

### `app.onboarding`

The first-run setup: whether it was done, the app the person came from and the steps to bring a song from it, whether they want AI features, the agent providers ready to use, the music apps found on this computer, and the steps the setup walks through.

### `app.finishOnboarding`

*Edits*

Finish (or skip) the first-run setup with the person's choices: the app they come from (it decides which steps the import shows first), whether they want AI features, and the agent provider to use. Saved in settings.onboarding. Only a person can do this, from the window or the CLI.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `comingFrom` | string | yes | App id from session.formats (ableton, logic, fl, bitwig, reaper, cubase, studioone, protools, garageband), or none. |
| `ai` | boolean | yes | Whether they want AI features (the agent and sound generation). |
| `agentProvider` | string |  | Agent provider to select when ai is true (agent.providers): lsuite, codex, claude, anthropic, openai, gemini… |
| `skipped` | boolean |  | They skipped the setup; the choices given still apply. |

### `app.recent`

Songs opened recently, newest first: name, folder, path, and whether the file is still there.

### `app.openRecent`

*Edits*

Open a recent song, replacing the open one (unsaved changes are discarded). Give its index in app.recent or its path.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `index` | integer |  | Position in app.recent, 0 for the most recent. |
| `path` | string |  | A path from app.recent. |

## agent

### `agent.status`

*Needs the app*

The built-in agent: provider, model, whether a task is running, turn count and last reply.

### `agent.configure`

*Edits · Needs the app*

Select the agent provider, model and reasoning effort together. Only while idle.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `provider` | string | yes | lsuite (lsuite AI, after account.signIn), codex, claude, anthropic, openai, gemini, openrouter, mistral, groq, deepseek, xai, ollama, lmstudio, compatible or zenith. |
| `model` | string | yes | Model ID; empty uses the provider default. |
| `reasoningEffort` | string | yes | Provider effort level; empty uses its default. |

### `agent.providers`

*Needs the app*

Available agent providers and whether each is configured.

### `agent.mcp`

*Needs the app*

How to connect an outside agent to this window over MCP: the ryolune-mcp command, its environment, whether the bridge is on, and a ready configuration for Claude Code, Codex, Cursor, VS Code, Claude Desktop, Gemini CLI, Windsurf, opencode, Zed and any other MCP client.

### `agent.openClient`

*Edits · Needs the app*

Open an outside agent's install link with ryolune's MCP server filled in (Cursor and VS Code install from a link; the app asks before adding it). Only a person can do this.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `client` | string | yes | cursor or vscode. |

### `agent.models`

*Needs the app*

Discover the models each connected provider offers, grouped by provider. Asks the providers over the network, so it runs as a job and answers when they have.

### `agent.connection`

*Needs the app*

Check that the configured agent provider can be reached and is signed in: provider, state and a message. Runs as a job, like agent.models.

### `agent.send`

*Edits · Needs the app*

Send a prompt to the built-in agent panel, like typing in the window.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `prompt` | string | yes | The request, in plain language. |

### `agent.stop`

*Edits · Needs the app*

Stop the running agent task; finished edits stay in Undo.

### `agent.transcript`

*Needs the app*

The agent conversation: user, assistant and tool entries.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `limit` | integer |  | Newest entries to return, default 40. |

### `agent.changes`

*Needs the app*

What the agent changed, one entry per command: sequence, title, the command as typed, its output, and whether it is currently applied.

### `agent.revert`

*Edits · Needs the app*

Undo back to just before one agent change, or redo up to it. Same as the buttons in the panel's Changes tab.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `sequence` | integer | yes | Change sequence from agent.changes. |
| `redo` | boolean |  | Redo up to the change instead of undoing it (default false). |

### `agent.revertTurn`

*Edits · Needs the app*

Undo the built-in agent's whole last turn in one step, back to the checkpoint taken before its first edit (or, with redo, bring the turn back). Same as Revert turn in the panel's Changes tab; agent.status lists the turn's changes.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `redo` | boolean |  | Bring a reverted turn back instead (default false). |

### `agent.clear`

*Edits · Needs the app*

Clear the agent conversation; the edits it made stay in Undo.

### `agent.conversations`

*Needs the app*

The agent's saved conversations for the open song, newest first: id, title, when it last changed, how many requests and which one is open; also the size of the song's project memory and any error saving them.

### `agent.newConversation`

*Edits · Needs the app*

Start a new agent conversation for this song; the open one is kept and agent.selectConversation goes back to it. Only while the agent is idle.

### `agent.selectConversation`

*Edits · Needs the app*

Open one of the song's saved agent conversations in the panel, as agent.conversations lists them. Only while the agent is idle.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Conversation id from agent.conversations. |

### `agent.renameConversation`

*Edits · Needs the app*

Rename an agent conversation (the open one by default). New conversations are titled from their first request.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `title` | string | yes | The new title, 1 to 120 characters on one line. |
| `id` | string |  | Conversation id from agent.conversations; default the open one. |

### `agent.deleteConversation`

*Edits · Needs the app*

Delete one of the song's agent conversations for good (its edits stay in the song). Deleting the open one opens the newest other. Only a person can do this.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Conversation id from agent.conversations. |

### `agent.memory`

*Needs the app*

The song's project memory: notes the person keeps for the agent (style, key, what to avoid), sent ahead of every request to every provider.

### `agent.setMemory`

*Edits · Needs the app*

Replace the song's project memory, at most 32 KB; empty clears it. It goes ahead of every request as user-maintained context, so only a person can change it.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `text` | string | yes | The whole memory text, plain language. |

### `agent.steer`

*Edits · Needs the app*

Steer the agent while it works: the text joins the conversation now and reaches the agent at its next step, after the tool calls under way, instead of stopping it (Claude Code restarts its run with it).

| Parameter | Type | Required | Description |
|---|---|---|---|
| `text` | string | yes | What to change or add, in plain language. |

## generate

### `generate.services`

The sound generation services ryolune can call (ElevenLabs, Stable Audio, fal.ai and a custom endpoint): the one chosen in Settings > Generation, which have a key, and what each makes best.

### `generate.audio`

*Edits · Needs the app*

Make a sound from a description with a generation service and put it in the song: a song or a loop as an audio clip (a loop follows the song's tempo and key), a sound effect or one-shot as an audio clip, or an instrument note as a Sample Keys track you play from the keyboard. Runs as a job over the network, on the service's credits, and keeps the result in generate.list. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `prompt` | string | yes | What it should sound like, in plain words: genre, instruments, mood, character. |
| `kind` | string |  | song, loop (default), sound or instrument. |
| `seconds` | number |  | Length in seconds. Defaults: song 60, sound 3, instrument 3; a loop defaults to its bars. |
| `bars` | integer |  | For a loop: its length in bars at the song's tempo, 1-32 (default 4). |
| `service` | string |  | elevenlabs, stability, fal or custom (default: the one in Settings > Generation). |
| `instrumental` | boolean |  | No vocals (default true; songs only). |
| `followSong` | boolean |  | Tell the service the song's tempo and key (default true for loops and songs). |
| `seed` | integer |  | Seed for services that take one, to repeat a result. |
| `name` | string |  | Name of the clip or instrument track (default: from the prompt). |
| `place` | boolean |  | Put it in the song (default true); false only keeps it in generate.list. |
| `trackId` | string |  | Track to place it on: an audio track for audio, a MIDI track for an instrument (default: a new track). |
| `startBar` | number |  | Zero-based bar where an audio clip starts (default: the playhead). |
| `rootNote` | integer |  | For an instrument: the MIDI note the sound plays at its own pitch, 0-127 (default 60, middle C). |

### `generate.list`

The sounds generated on this computer, newest first, with the description, service, kind and length of each. They stay in ryolune's data folder until generate.delete.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `limit` | integer |  | At most this many, 1-200 (default 50). |

### `generate.preview`

One generated sound as base64 audio with its MIME type, to audition it before placing it.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Generated sound id, from generate.list. |

### `generate.place`

*Edits*

Put a sound from generate.list in the song: as an audio clip, or as a Sample Keys instrument played from the keyboard. One undo step.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Generated sound id, from generate.list. |
| `as` | string |  | audio or instrument (default: how it was made). |
| `trackId` | string |  | Track to place it on (default: a new track). |
| `startBar` | number |  | Zero-based bar where an audio clip starts (default: the playhead). |
| `rootNote` | integer |  | For an instrument: the MIDI note the sound plays at its own pitch (default 60). |

### `generate.delete`

*Edits*

Delete a generated sound from this computer. Clips already in a song keep their audio.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `id` | string | yes | Generated sound id, from generate.list. |

## export

### `export.toKimchi`

*Edits*

Render the mix (or one stem per track) and put it on a kimchi video project, on a new audio track, ready to cut picture to. When kimchi is open, kimchi places them on its open project itself (its handoff.fromRyolune, one undo step there); when it is closed, they are added to the project file (the previous one is kept as a backup). Runs as a job in the app.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `project` | string |  | When kimchi is closed: the project id or name (default: the one changed most recently). When it is open, its open project. |
| `stems` | boolean |  | One file and one kimchi track per ryolune track instead of the mix (default false). |
| `trackIds` | array |  | With stems: the tracks to send (default all). |
| `startSeconds` | number |  | Where the audio starts on kimchi's timeline, in seconds (default 0). |
| `startBar` | number |  | Zero-based bar the render starts at (default 0). |
| `endBar` | number |  | Exclusive bar the render ends at (default: the end of the song). |
| `tailSeconds` | number |  | Effect tail after the end, 0-120 seconds (default 3). |

## handoff

### `handoff.inbox`

Hand-offs other lsuite apps left for ryolune (a cut from kimchi to score), oldest first, from ~/.lsuite/handoff/ryolune (kimchi's handoff.toRyolune writes `<name>.kimchi-cut.json` there). Pass a manifest to session.scoreCut.

## account

### `account.status`

The lsuite account this computer is signed in to (shared by every lsuite app): email, plan, the allowance used this month (`summary` reads like "Pro · 38 % used · resets 1 Nov"), the plan's models and where to manage it. Asks the lsuite server unless check is false. Never shows the token.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `check` | boolean |  | Ask the server for the plan and allowance (default true); false reads only the account file. |

### `account.signIn`

*Edits*

Sign in to lsuite AI so the agent works without any other setup, for every lsuite app on this computer. Without key, the window opens the browser to sign in (or create the account and pick a plan) and waits for it; with key, uses the key shown on the account page (lsk_…), which also works from ryolune-cli. Only a person can do this.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `key` | string |  | The key from your lsuite account page (lsk_…), for the CLI and headless use. |

### `account.signOut`

*Edits*

Sign out of lsuite AI on this computer (every lsuite app): the server forgets the token and the account file is removed. Only a person can do this.

### `account.plans`

The lsuite AI plans as the server offers them: prices, models and monthly allowances (a demo for now: no payment is taken).

### `account.manage`

*Edits*

Open the lsuite account page (plan, allowance, key) in the web browser; headless, returns its address. Only a person can do this.

## harness

### `harness.brief`

The agent's expert brief (markdown): the music producer's role in ryolune, the song's mental model, the commands for common jobs, the quality bar (levels, loudness targets), the finish routine and the index of skills. The built-in agent and ryolune-mcp's instructions use this same text.

### `harness.skills`

The playbooks for music jobs (compose, drums, bass and chords, melody, arrangement, sound design, automation, mixing, mastering, export, scoring to picture, writing a plugin, review): [{name, title, when}]. Load one with harness.skill.

### `harness.skill`

One skill's playbook (markdown): when to use it, the steps with the exact commands, and the checks that prove the job worked.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `name` | string | yes | Skill name from harness.skills, for example mixing. |

### `harness.context`

The live context an agent gets before each step: the song in brief (tempo, key, meter, length, sections), every track in one line, the selection, the playhead, the newest checkpoint and what changed since this agent's last command (the person's edits while it was thinking).

| Parameter | Type | Required | Description |
|---|---|---|---|
| `key` | string |  | Whose last look to compare with: `agent` (the built-in agent, default) or `mcp` (an outside agent). The window moves it after each of that agent's commands. |

### `harness.look`

Look at and listen to a bar range: renders it offline like an export and returns a picture (waveform with the bar grid and sections, short-term loudness, average spectrum against a pink slope, piano roll of the notes in track colours) plus the numbers: integrated/short-term/momentary loudness (LUFS), loudness range, true peak (dBTP), sample peak, clipped samples, energy per band, and findings that name the fix. The picture reaches the model as an image (built-in agent with a vision model; MCP image content) and is written as a PNG (`image.path`).

| Parameter | Type | Required | Description |
|---|---|---|---|
| `fromBar` | number |  | Zero-based first bar (default 0). |
| `toBar` | number |  | Exclusive end bar (default the end of the song). At most 600 seconds. |
| `trackId` | string |  | Only this track, soloed (its buses and the master chain still apply). |
| `view` | string |  | all (default), mix (waveform, loudness, spectrum) or notes (piano roll only: no render, quick). |
| `targetLufs` | number |  | A loudness target to draw and compare with, for example -14. |
| `tailSeconds` | number |  | Seconds after the range to include (reverb tails), 0-30, default 0. |
| `path` | string |  | Where to write the PNG. Defaults to a new file in the app data folder (looks/). |

### `harness.measure`

Measure loudness without a picture: integrated, short-term max and momentary max loudness (LUFS, ITU-R BS.1770 / EBU R128 gating), loudness range (LU), true peak (dBTP, 4x oversampled), sample peak (dBFS), clipped samples, energy per band (sub, bass, low mids, high mids, air) and findings. tracks=true measures each track on its own as well (gain staging).

| Parameter | Type | Required | Description |
|---|---|---|---|
| `fromBar` | number |  | Zero-based first bar (default 0). |
| `toBar` | number |  | Exclusive end bar (default the end of the song). At most 600 seconds. |
| `trackId` | string |  | Only this track, soloed. |
| `tracks` | boolean |  | Also measure every track on its own (ranges up to 120 seconds). |
| `targetLufs` | number |  | Integrated loudness to aim for; findings say how far off it is. |
| `tailSeconds` | number |  | Seconds after the range to include, 0-30, default 0. |

### `harness.checkpoint`

Take a checkpoint of the song before a job, so the whole job can be reverted in one step (harness.revert) and its changes listed (harness.changes). The built-in agent takes one at the start of every turn. Checkpoints last until another song is opened.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `label` | string |  | What the job is, for example "Mix pass". |

### `harness.checkpoints`

The checkpoints of this song, oldest first, each with how many changes were made since.

### `harness.changes`

What changed since a checkpoint (default the newest), in plain words: tracks, clips, notes, sounds, levels, sections, tempo and key.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `checkpoint` | string |  | Checkpoint id from harness.checkpoints (default the newest). |

### `harness.revert`

*Edits*

Return the song to a checkpoint (default the newest) in one undo step: everything changed since is undone together, and history.undo brings it back. Answers with the changes it undid.

| Parameter | Type | Required | Description |
|---|---|---|---|
| `checkpoint` | string |  | Checkpoint id from harness.checkpoints (default the newest). |
