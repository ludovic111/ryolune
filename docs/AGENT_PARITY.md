# Agent parity: everything a person can do, a command can do

ryolune's command registry (`engine/src/control.rs` and the `control_*.rs` families, live-only
actions in `desktop/src/control.rs`) is the contract. The window's buttons, menus, drags and
dialogs call registry commands by name, so the CLI (`ryolune-cli`), MCP (`ryolune-mcp`) and the
built-in agent can do the same thing with the same undo step. This file is the audit of that
contract: every interaction in the window, the command(s) behind it, and the few things that
are deliberately window-only, with the reason.

Checked by `engine/tests/agent_parity.rs`:

- every action id in `desktop/src/ui/actions.rs` (menus, shortcuts, command palette, the
  shortcut sheet) has an entry in `docs/agent-parity.json` and a row below, and names only
  registry commands;
- every `family.action` string the window's GPUI views (`desktop/src/ui`) send is a registry
  command (or one of the shared lsuite aliases in `control::ALIASES`);
- every command named in this file exists;
- every command and parameter has a description (what it does, units, ranges).

Legend: **covered** means the command already existed; **added** means this audit added it or
the missing capability to it; **window-only** means there is deliberately no command, and why.

## Summary

| | Count |
|---|---|
| Interactions audited (rows below, private handlers aside) | 219 |
| Covered by an existing command | 184 |
| Gap closed by a new command or a new capability of one | 23 |
| Window-only by design | 12 |

New commands: `session.overview`, `ui.state`, `strip.programs`, `strip.setProgram`,
`strip.removeInsert`. New capabilities: names for ids everywhere (`trackId: "Bass"`),
`strip.parameters` with display text, normalized position, automatable flag, automation lane,
search and paging; `strip.setParameter` / `strip.setParameters` by parameter name and by
display text or 0-1 position; `strip.setPlugin` by plugin name and into the first free slot;
`plugin.list` ranked search; `plugin.describe` by name, with programs; `automation.create`
by parameter name.

## Start here: the overview an agent reads first

| Need | Command |
|---|---|
| Everything at a glance: song (tempo, meter, key, length in bars and seconds), transport, sections, every track (kind, instrument with format and vendor, inserts with bypass and changed parameters as displayed, sends, fader in dB, pan, mute/solo/arm/monitor, problems that keep it silent, clips with bars, note counts, pitch ranges, audio sources and fades, automation and controller lanes), buses, project audio, takes, selection, undo history, and in the app what the window shows | `session.overview` (added) |
| One track in full | `session.overview trackId=Bass` (added) |
| What the window shows: panels, prompt, plugin windows by track and slot, editor clip and mode, zoom and visible bars, tool, browser, selection, theme | `ui.state` (added) |
| The window as pixels | `ui.screenshot` |
| The list of commands with parameters | `session.commands` |

Output is bounded: clips per track adapt to the track count (about 48 in all, `maxClips` to
change it), plugin parameters are the changed ones (at most 6 per plugin), `truncated` says
what was left out and `next` names the commands that drill down.

## Actions: menus, shortcuts and the command palette (`desktop/src/ui/actions.rs`)

| Action | Shortcut | Command(s) | |
|---|---|---|---|
| `undo` | ⌘Z | `history.undo` (steps) | covered |
| `redo` | ⇧⌘Z | `history.redo` | covered |
| `togglePlay` | Space | `transport.play`, `transport.stop` | covered |
| `stop` | 0 | `transport.stop` | covered |
| `record` | R | `transport.punch`, `transport.record` | covered |
| `cycle` | C | `transport.setCycle` | covered |
| `returnToStart` | Enter | `transport.returnToStart` | covered |
| `rewind` | , | `transport.locate` (bar) | covered |
| `forward` | . | `transport.locate` (bar) | covered |
| `metronome` | K | `transport.setMetronome` | covered |
| `deleteSelection` | ⌫ | `note.remove`, `clip.remove` | covered |
| `duplicateClip` | ⌘D | `clip.duplicate` | covered |
| `splitAtPlayhead` | ⌘T | `clip.split` | covered |
| `openInEditor` | E | `view.set` (editorClipId) | covered |
| `addAudioTrack` | ⌥⌘A | `track.add`, `track.move` | covered |
| `addMidiTrack` | ⌥⌘S | `track.add`, `track.move` | covered |
| `removeSelectedTrack` | ⌘⌫ | `track.remove` | covered |
| `muteSelectedTrack` | M | `track.setMute` | covered |
| `soloSelectedTrack` | S | `track.setSolo` | covered |
| `armSelectedTrack` | A | `track.setArmed` | covered |
| `quantizeRegion` | | `clip.quantize` | covered |
| `cycleMonitorSelectedTrack` | I | `track.setMonitor` | covered |
| `zoomIn` | ⌘= | `view.set` (pixelsPerBar, scrollBar) | covered |
| `zoomOut` | ⌘- | `view.set` | covered |
| `zoomToFit` | Z | `view.fit` | covered |
| `followPlayhead` | F | `view.set` (followPlayhead) | covered |
| `toggleAgentPanel` | ⌘J | `ui.showPanel` panel=agent | covered |
| `askAgent` | ⇧⌘J | `ui.showPanel` panel=agent, `agent.send` | covered |
| `stopAgent` | | `agent.stop` | covered (its menu item was never enabled: fixed, it now follows the running agent) |
| `editorPianoRoll` | | `view.set` editorMode=pianoRoll | covered |
| `editorScore` | | `view.set` editorMode=score | covered |
| `editorStep` | | `view.set` editorMode=step | covered |
| `toolPointer` | 1 | `ui.setTool` | covered |
| `toolPencil` | 2 | `ui.setTool` | covered |
| `toolScissors` | 3 | `ui.setTool` | covered |
| `copy` | ⌘C | `clip.copy` | covered |
| `cut` | ⌘X | `clip.cut` | covered |
| `paste` | ⌘V | `clip.paste` | covered |
| `duplicateTrack` | ⇧⌘D | `track.duplicate` | covered |
| `transposeUp` | ⌥↑ | `clip.transpose`, `note.update` | covered |
| `transposeDown` | ⌥↓ | `clip.transpose`, `note.update` | covered |
| `transposeOctaveUp` | ⇧⌥↑ | `clip.transpose`, `note.update` | covered |
| `transposeOctaveDown` | ⇧⌥↓ | `clip.transpose`, `note.update` | covered |
| `toggleMixer` | X | `ui.showPanel` panel=mixer | covered |
| `toggleControllerLane` | L | `ui.showPanel` panel=controllers | covered |
| `toggleTempoTrack` | ⇧T | `ui.showPanel` panel=tempo | covered |
| `addBusTrack` | | `track.add` kind=bus | added in 0.10 |
| `groupSelectedTrack` | | `track.group` trackIds | added in 0.10 |
| `commandPalette` | ⌘P | `ui.showPanel` panel=palette; an agent reads `session.commands` | covered |
| `showShortcuts` | ⌘/ | `ui.showPanel` panel=help; `session.commands` | covered |
| `addMarker` | ⇧M | `marker.add` | covered |
| `nextMarker` | ⇧N | `marker.next` | covered |
| `previousMarker` | ⇧B | `marker.previous` | covered |
| `cycleSection` | ⇧C | `marker.cycleSection` | covered |

Musical typing plays the letter keys while it is on (`note.hold`, `note.releaseAll`).

## Menu actions (since 0.13 every menu item is an action of the table)

| Action | Menu item | Command(s) | |
|---|---|---|---|
| `newSession`, `openDemo` | File › New session / Open demo | `session.new` (demo) | covered; the window asks about unsaved changes first, a script answers that prompt with `app.confirm` |
| `openSession` | File › Open… | `session.open` path | covered; the file chooser is window-only (a script gives the path) |
| `save`, `saveAs` | File › Save / Save as… | `session.save` (path) | covered |
| `importAudio` | File › Import audio… | `session.importAudio` path, trackId, startBar | covered |
| `importMidi` | File › Import MIDI… | `session.importMidi` | covered |
| `exportAudio` | File › Export audio… | `ui.showPanel` panel=export, then `session.exportAudio` or `session.exportStems` | covered |
| `exportMidi` | File › Export MIDI… | `session.exportMidi` | covered |
| `recoverSession` | File › Recover session… | `session.snapshots`, `session.restoreSnapshot` | covered |
| `settings` | File › Settings… (⌘,) | `ui.showPanel` panel=settings section=… | covered |
| `quit` | File › Quit | `app.quit` (discard) | covered |
| `humanize` | Edit › Humanize | `clip.humanize` | covered |
| `crescendo`, `diminuendo` | Edit › Velocity crescendo / diminuendo | `clip.velocityRamp` | covered |
| `legato` | Edit › Legato | `clip.legato` | covered; disabled for audio regions |
| `reverseMidi` | Edit › Reverse MIDI phrase | `clip.reverseMidi` | covered; disabled for audio regions |
| `fitMajor`, `fitMinor` | Edit › Fit to C major / minor | `clip.fitScale` | covered; disabled for audio regions |
| `repeatRegion` | Edit › Repeat region × 4 | `clip.repeat` | covered |
| `showMaster`, `showBusA`, `showBusB` | Track › Show master strip / reverb bus / delay bus | `ui.showPanel` panel=master, bus-a, bus-b | covered |
| `saveRecoveredTake` | Mix › Save recovered take… | `session.saveRecoveredTake` | covered |
| `reconnectOutput` | Mix › Reconnect output | `audio.reconnect` | covered |
| `audioSettings` | Mix › Audio and MIDI devices… | `audio.setOutput`, `audio.setInput`, `audio.setMidiInput`; `ui.showPanel` panel=settings section=audio | covered |
| `musicalTyping` | Mix › Musical typing (⌘K) | `ui.musicalTyping` | covered |
| `rescanPlugins` | Mix › Rescan plugins | `plugin.scan` | covered |
| `agentSettings` | Agent › Agent settings… | `ui.showPanel` panel=settings section=agent | covered |
| `toggleAutomation` | View › Automation | `ui.showPanel` panel=automation | covered |
| `checkUpdates` | Help › Check for updates… | `app.checkUpdates`, `app.installUpdate`, `app.relaunch` (also app.restart, the lsuite name for it) | covered; an open window also checks every six hours |
| `pluginGuide` | Help › Native plugin SDK… | `app.openGuide` guide=plugins | covered |
| `support` | Help › Support ryolune…, Settings › About, the one-time ask after the third export | `app.openGuide` guide=support; the ask is `settings.get`/`settings.set path=general.exportsCompleted` and `general.supportAsked` | covered |
| `whatsNew` | Help › What's New, Settings › Updates, and by itself once after an update | `ui.showPanel` panel=whatsNew; the text is `app.whatsNew` (version, since, all); the version that ran last is `general.lastRunVersion` | added in 0.14 |
| `diagnostics` | Help › Logs and Crash Reports… | `ui.showPanel` panel=diagnostics; `app.logs`, `app.crashReports`, `app.clearCrashReports`, `app.diagnostics` | added in 0.14 |
| `reportProblem` | Help › Report a Problem…, Settings › Diagnostics | `app.reportProblem` | added in 0.14; refused to agents (it opens a GitHub issue for the person to submit), which read `app.diagnostics` instead |

## Arrangement

| Interaction | Command(s) | |
|---|---|---|
| Click the ruler | `transport.locate` | covered |
| Drag on the ruler / a cycle edge | `transport.setCycle` startBar endBar | covered |
| Click, drag, rename, delete a marker flag | `marker.goto`, `marker.move`, `marker.rename`, `marker.remove` | covered |
| Marker menu: Cycle this section / Add marker here | `marker.cycleSection` markerId, `marker.add` bar name | covered |
| Marker colour | `marker.setColor` | covered (no window control yet) |
| Tempo track: click to add a change, drag a point (bar and tempo), double-click to type, Ramp from the Previous Tempo, Delete, Clear | `tempo.set` bar bpm ramp, `tempo.move` bar toBar bpm, `tempo.remove`, `tempo.clear`, `tempo.list` | added in 0.10 |
| Press a clip | `clip.select` | covered |
| Drag a clip (same-kind tracks) | `clip.move` startBar trackId | covered |
| Drag a clip edge | `clip.trim` (left edge), `clip.resize` | covered |
| Drag a fade handle | `clip.setFades` | covered |
| Pencil on an empty MIDI lane | `clip.create` | covered |
| Scissors on a clip | `clip.split` | covered |
| Click an empty lane | `clip.deselect`, `track.select` | covered |
| Double-click a MIDI clip | `view.set` editorClipId | covered |
| Clip menu: open, rename, cut, copy, duplicate, split, delete, ask agent | `view.set`, `clip.rename`, `clip.cut`, `clip.copy`, `clip.duplicate`, `clip.split`, `clip.remove`, `agent.send` | covered |
| Lane menu: paste, new tracks, import audio | `clip.paste`, `track.add`, `session.importAudio` | covered |
| ⌘-wheel / pinch zoom, shift-wheel scroll | `view.set` pixelsPerBar scrollBar | covered |
| Vertical scroll of the track list | none | window-only: tracks are addressed by id or name, never by where they are on screen; `ui.state` and `ui.screenshot` show the view |
| Empty project buttons | `track.add`, `session.importAudio`, `session.new` demo, `ui.showPanel` agent | covered |
| Lane width as the window resizes | `view.set` laneWidth | covered |
| Tool segment | `ui.setTool` | covered |
| Follow button, zoom rail | `view.set` | covered |
| Track header: select, rename | `track.select`, `track.rename` | covered |
| M / S / arm / monitor | `track.setMute`, `track.setSolo`, `track.setArmed`, `track.setMonitor` | covered |
| Volume slider, pan knob | `track.setVolume`, `track.setPan` | covered |
| Header menu: duplicate, move up/down, colour, delete | `track.duplicate`, `track.move`, `track.setColor`, `track.remove` | covered |

## Region editor

| Interaction | Command(s) | |
|---|---|---|
| Mode: piano roll, score, step | `view.set` editorMode | covered |
| Click a piano key | `note.preview` | covered |
| Click, drag, resize a note | `clip.select` noteId, `note.update` | covered |
| Draw a note, step entry | `note.add` | covered |
| Note menu: velocity, delete | `note.update` velocity, `note.remove` | covered |
| Velocity for new notes | `note.add` velocity | covered (the editor's default is its own) |
| Vertical scroll of the piano roll | `view.set` editorLowPitch | covered |
| A whole pattern at once | `clip.setNotes`, `clip.create` notes | covered |
| Region tools | `clip.quantize`, `clip.transpose`, `clip.humanize`, `clip.velocityRamp`, `clip.legato`, `clip.reverseMidi`, `clip.fitScale`, `clip.repeat` | covered |
| Controller lane: show | `ui.showPanel` panel=controllers | covered |
| Controller lane: which controller it shows | none | window-only: `controller.list` returns every lane of the clip at once |
| Controller lane: click, draw a stroke, drag, delete a point | `controller.add`, `controller.setPoints`, `controller.update`, `controller.remove` | covered |
| MIDI learn | none | not a window feature: controllers are recorded from a MIDI input (`audio.setMidiInput`) or drawn as controller lanes; plugin parameters move with automation |

## Inspector and mixer

| Interaction | Command(s) | |
|---|---|---|
| Choose a stock instrument | `strip.setInstrument` | covered |
| Load any instrument (stock, CLAP, VST3, AU, native) | `strip.setPlugin` pluginId or plugin=name | covered; by name added |
| Instrument parameters button | `ui.openPluginWindow` | covered |
| Empty insert slot → effect | `strip.setInsert` (stock), `strip.setPlugin` slot or firstFreeSlot | covered; firstFreeSlot added |
| Insert LED / Bypass | `strip.setBypass` | covered |
| Insert menu: parameters, plugin window | `ui.openPluginWindow` (native) | covered |
| Insert menu: move up/down | `strip.moveInsert` | covered |
| Insert menu: remove | `strip.removeInsert` | added (was only `strip.setInsert` without an effect) |
| Send A/B knobs | `strip.setSendLevel` | covered |
| Fader, pan, M/S/arm/monitor | `track.setVolume`, `track.setPan`, `track.setMute`, `track.setSolo`, `track.setArmed`, `track.setMonitor` | covered |
| Master fader | `master.setVolume` | covered |
| Region fields: name, bar, length | `clip.rename`, `clip.move`, `clip.resize` | covered |
| Audio region: gain, fade curve, fade in/out | `clip.setGain`, `clip.setFades` | covered |
| Mixer: show / hide, master inserts | `ui.showPanel` panel=mixer / master | covered |
| Meters | `audio.status` (master and selected-track peaks) | covered |

## Browser

| Interaction | Command(s) | |
|---|---|---|
| Tabs, select a row | `view.set` browserTab browserSelection | covered |
| Search box | `plugin.list` query | covered; now ranked by relevance and matching words |
| Folder collapse | none | window-only: presentation; `plugin.folders` lists the folders |
| Double-click an instrument | `track.add`, `strip.setPlugin` | covered |
| Double-click an effect (first free slot) | `strip.setPlugin` firstFreeSlot | added |
| Double-click a loop | `clip.addLoop` | covered |
| Double-click a project audio file | `clip.create` sourceId; `session.overview` lists the sources | covered |
| Row menu: Load / Insert as another format (CLAP, VST3, Audio Unit) | `plugin.list` row `formats`, `strip.setPlugin` pluginId | added in 0.10 |
| Star / favourites | `plugin.setFavorite` | covered |
| Move to folder / new folder / automatic | `plugin.setFolder` | covered |
| Import audio, scan plugins | `session.importAudio`, `plugin.scan` | covered |
| Preview ▶ | `note.preview` | covered |
| Recent plugins | `plugin.list` sort=recent, `plugin.folders` | covered |

## Plugins

| Interaction | Command(s) | |
|---|---|---|
| See every parameter with its value as the plugin displays it | `strip.parameters` | covered; display text, normalized, automatable, automation lane added |
| Find a parameter by name | `strip.parameters` query=… | added |
| Only what was changed | `strip.parameters` changed=true | added |
| Turn a knob | `strip.setParameter` parameterId or parameter=name, value / normalized / text | covered; name, normalized and text added |
| Type a value ("-6 dB", "2.5k", "Hall") | `strip.setParameter` text | added |
| Several at once | `strip.setParameters` | covered; keys by name and values as text or {normalized} added |
| Presets menu: load, save | `preset.load`, `preset.save`, `preset.list` | covered |
| Delete a user preset | `preset.delete` | covered (no window control) |
| The plugin's own factory programs | `strip.programs`, `strip.setProgram` | added |
| Open the plugin's own window | `ui.openPluginWindow` native=true | covered |
| Close one / all plugin windows | `ui.closePluginWindow`, `ui.closePluginWindows` | covered |
| Which plugin windows are open | `ui.state` pluginWindows (track, slot, plugin) | added |
| Automate a parameter ("A") | `automation.create` target=pluginParameter parameterId or parameter=name | covered; by name added |
| Plugin state (what its own window changed) | `strip.getState`, `strip.setState` | covered |
| Describe a plugin before loading it | `plugin.describe` (id or name, query, programs) | covered; name, query and programs added |
| Rescan | `plugin.scan` (in the `--scan-plugin` child processes) | covered |

### External plugins by format

| | CLAP | VST3 | AU | ryolune native | Stock |
|---|---|---|---|---|---|
| List, search, favourites, folders | yes | yes | yes | yes | yes |
| Load as instrument or insert | yes | yes | yes | yes | yes |
| Parameters with ranges and units | yes | yes (normalized 0-1, units from the plugin) | yes | yes | yes |
| Display text of a value | the plugin's (`value_to_text`) | the plugin's (`getParamStringByValue`) | ryolune's, from unit and range | ryolune's | ryolune's |
| Value from display text | the plugin's (`text_to_value`), then labels and numbers | the plugin's (`getParamValueByString`), then labels | labels, numbers with the unit, percentages | same | same |
| Automatable flag | `CLAP_PARAM_IS_AUTOMATABLE` | `kCanAutomate` | always | always | always |
| Factory programs | not listed | program list behind the `kIsProgramChange` parameter, chosen by setting it (undoable) | factory presets (`kAudioUnitProperty_FactoryPresets`), loaded into a fresh instance and restored as state (undoable) | none | ryolune factory presets |
| ryolune presets (params + state) | yes | yes | yes | yes | yes |
| State get/set | yes | yes | yes | yes | yes |
| Own window | yes | yes | yes | when the plugin has one | no (parameter panel) |

Not implemented, and why: CLAP preset discovery (a separate factory and indexer; plugins
that use it still save and restore through ryolune presets and state), VST3 `IUnitInfo`
program lists that are not attached to a program-change parameter, and presets a plugin only
shows inside its own window. Parameters flagged hidden (CLAP, VST3) or expert/read-only (AU)
are not listed, as in the window.

## Automation

| Interaction | Command(s) | |
|---|---|---|
| Panel open / close | `ui.showPanel` panel=automation | covered |
| Lanes and points | `automation.list` | covered |
| Add a lane (volume, pan, master, plugin parameter) | `automation.create` | covered |
| Read on/off, interpolation, remove lane | `automation.setEnabled`, `automation.setInterpolation`, `automation.remove` | covered |
| Double-click / drag a point, edit beat and value | `automation.setPoint` | covered |
| Delete a point | `automation.removePoint` | covered |
| Replace a whole curve | `automation.setPoints` | covered |
| Which lane the panel shows | none | window-only: `automation.list` returns all of them |

## Transport and title bar

| Interaction | Command(s) | |
|---|---|---|
| Return, rewind, forward | `transport.returnToStart`, `transport.locate` | covered |
| Play, stop, record | `transport.play`, `transport.stop`, `transport.punch`, `transport.record` | covered |
| Cycle, click | `transport.setCycle`, `transport.setMetronome` | covered |
| Snap menu | `transport.setSnap` | covered |
| Tempo drag or typed (the tempo at the playhead: the starting one, or the change in force there) | `transport.setTempo`, `tempo.set` bar | covered |
| Signature, key | `transport.setTimeSignature`, `transport.setKey` | covered |
| Count-in | `settings.set path=audio.countInBars` | covered |
| Song name | `session.rename` | covered |
| Menus, window drag | none | window-only: the menu bar is how a person finds commands; an agent has `session.commands` |

## Recording and input

| Interaction | Command(s) | |
|---|---|---|
| Arm, monitor | `track.setArmed`, `track.setMonitor` | covered |
| Record a take | `transport.record`, `transport.punch` | covered |
| Play the computer keyboard | `ui.musicalTyping`; `note.hold` pitch on | covered |
| Octave of musical typing (Z / X) | `note.hold` takes any pitch | covered |
| Feedback warning: monitor anyway | `audio.allowSpeakerMonitoring` | covered |
| Devices | `audio.devices`, `audio.setOutput`, `audio.setInput`, `audio.setMidiInput`, `audio.status` | covered |

## Dialogs

| Interaction | Command(s) | |
|---|---|---|
| Unsaved changes: save, don't save, cancel | `app.confirm` | covered; `ui.state` prompt says it is waiting |
| Error dialog OK | `ui.dismissError` | covered |
| Export dialog fields and Export | `session.exportAudio` (rate, format, dither, quality, range, tail), `session.exportStems` (tracks, effects, master, container) | covered |
| Recovery dialog | `session.snapshots`, `session.restoreSnapshot` | covered |
| Help | `ui.showPanel` panel=help | covered |
| Settings sections | `ui.showPanel` panel=settings section=… | covered |
| Settings › General | `settings.set path=general.recoveryIntervalSeconds`, `settings.set path=general.confirmBeforeQuit`, `settings.set path=general.reopenLastSession` | covered |
| Settings › Audio | `settings.set path=audio.outputDevice` (and inputDevice, midiInput, connectMidiOnStart, countInBars, meterInputWhenArmed, bufferFrames) | covered |
| Settings › Interface (theme, dark/light, scale, tooltips, follow) | `settings.set path=interface.appearance` (and mode, scale, agentPanelOpenOnStart, showTooltips, followPlayhead) | covered |
| Settings › Plugins | `settings.set path=plugins.scanOnStart` (and extra paths), `plugin.scan` | covered |
| Settings › Control | `settings.set path=control.enableBridge` | covered for people; refused to agents |
| Settings › Updates | `settings.set path=general.checkUpdatesOnStart`, `app.checkUpdates`, `app.installUpdate`, `app.relaunch` (Restart now) | covered |
| Settings › Diagnostics: crash reports (View, Copy, Show folder, Delete all), this run's log (Copy, Refresh, Show folder), data folder | `app.crashReports` (id), `app.clearCrashReports`, `app.logs` (lines, file), `app.diagnostics` (paths) | added in 0.14; Delete all needs fileOperations for agents; Show folder is window-only (opens the file manager for the person) |
| Settings › Diagnostics: Copy diagnostics, Report a Problem… | `app.diagnostics`, `app.reportProblem` | added in 0.14; Report a Problem is refused to agents |
| What's New sheet: Earlier versions, All releases, Continue | `app.whatsNew` all=true, `ui.showPanel` panel=whatsNew visible=false | added in 0.14; All releases opens the GitHub releases page for the person |
| Settings › Agent: provider, model, effort, keys, permissions | `agent.configure`, `settings.set path=agent.…` | window-only for agents: connections and permissions are changed by the person (enforced in `run_control_command`); the CLI and MCP can |
| Settings › Agent › Use another agent: recipes, Copy | `agent.mcp` | covered (Copy is the clipboard of the person's computer) |
| Settings › Agent › Use another agent: Add to Cursor / VS Code | `agent.openClient` | refused to agents: it opens another app for the person |
| Settings › Generation: service, keys, fal model, endpoint | `settings.set path=generation.…`, `generate.services` | window-only for agents (the endpoint and keys are the person's); the CLI and MCP can |
| Settings › Generation: Get a key | `app.openGuide` guide=elevenlabs, stability, fal or custom | covered |
| Sign in to a provider | none | window-only: credentials stay with the person (`daw_signin`) |
| Provider help page | none | window-only: opens a web page for the person (`daw_agent_help`) |
| Reset preferences | `settings.reset` | covered (no window control) |
| Modal position, local confirmations | none | window-only: presentation |

## Agent panel

| Interaction | Command(s) | |
|---|---|---|
| Open / close, settings gear | `ui.showPanel` panel=agent / settings | covered |
| Send, stop | `agent.send`, `agent.stop` | covered |
| Conversation | `agent.transcript` | covered |
| Changes tab: undo from here, redo to here | `agent.changes`, `agent.revert` | covered |
| New conversation | `agent.clear` | covered |
| Model picker | `agent.models`, `agent.configure`, `agent.providers` | covered |
| Connection check | `agent.connection`, `agent.status` | covered |
| Takes: create, switch, remove, listen | `take.create`, `take.select`, `take.remove`, `take.list`, `transport.play` | covered |
| Generate tab: make a loop, song, sound or instrument | `generate.audio`, `generate.services` | covered |
| Generate tab: your sounds (listen, Add, Keys, delete) | `generate.list`, `generate.preview`, `generate.place`, `generate.delete` | covered |
| Rhythm Lab (removed from the panel in 0.12) | `rhythm.preview`, `rhythm.create` | commands kept for agents, the CLI and MCP |
| Tabs, draft, selection chip, slash menu | none | window-only: the composer of the panel itself |

## Window plumbing (GPUI)

Since 0.13 the window is Rust (GPUI): views read the host directly and change it only through
registry commands (`Daw::run`), so there are no private handlers left. What remains
window-only is plumbing, not capability:

| Window mechanism | Registry equivalent |
|---|---|
| A pointer drag grouped into one undo step (`Daw::gesture`) | `session.batch` |
| Musical-typing key events | `note.hold`, `note.releaseAll` |
| Native file and folder choosers | commands take paths |
| Window capture | `ui.screenshot` |
| Provider sign-in through Codex or Claude Code | window-only (see Dialogs) |

## Names instead of ids

Every command that takes `trackId`, `clipId` or `markerId` also takes the exact name
(ignoring case) when one track, clip or marker has it: `track.setMute trackId=Bass`,
`clip.get clipId="Bass verse"`, `strip.get trackId=master`. Buses answer to master, bus-a,
bus-b and their names. A wrong name answers with the names that exist and the closest one
("Unknown track `Bas`. Tracks: Drums (drums), Bass (bass)… Did you mean Bass?"); a name two
tracks share asks for the id.

## Permissions for agents

Settings › Agent (`settings.agent.permissions`) is enforced in `run_control_command` for
every agent request. The commands added here are document edits (one undo step each) or
queries, so they need no permission: `session.overview`, `ui.state`, `strip.programs`,
`strip.setProgram`, `strip.removeInsert`. File operations (`session.save`, exports,
`plugin.scan`, `preset.save`, …), transport, replacing the session, settings and application
control keep their switches.
