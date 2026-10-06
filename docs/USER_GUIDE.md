# ryolune user guide

This guide covers everything you can do in the ryolune window, from a first beat to an exported
mix. Every action here also exists as a command, so the built-in agent, `ryolune-cli` and MCP
clients can do the same things: see [AI_CONTROL.md](AI_CONTROL.md) and the generated
[command reference](COMMANDS.md). Keyboard shortcuts are listed in [SHORTCUTS.md](SHORTCUTS.md)
and in the app under Help > Shortcuts and Help (⌘/). ⌘ is Ctrl on Windows and Linux.

## Contents

1. [The window](#the-window)
2. [Your first song in five minutes](#your-first-song-in-five-minutes)
3. [Tracks](#tracks)
4. [Instruments, loops and the browser](#instruments-loops-and-the-browser)
5. [Regions in the arrangement](#regions-in-the-arrangement)
6. [Editing MIDI](#editing-midi)
7. [Tempo](#tempo)
8. [Recording](#recording)
9. [Mixing](#mixing)
10. [Automation](#automation)
11. [Plugins](#plugins)
12. [Files: save, import, export, recover](#files-save-import-export-recover)
13. [Coming from another app](#coming-from-another-app)
14. [The agent](#the-agent)
15. [Appearance](#appearance)
16. [Settings](#settings)
17. [Updates, what's new and problems](#updates-whats-new-and-problems)
17. [Limits](#limits)

## The window

From top to bottom and left to right:

- **Title bar**: the menus (File, Edit, Track, Mix, Agent, View, Help), the song's name (a `*`
  means unsaved changes) and update notices.
- **Transport**: go to start, rewind and forward one bar, Play, Stop, Record and Cycle; the
  display with position (bars · beats · ticks), SMPTE time, tempo, time signature and key (click
  or drag a value to change it); Click (metronome) and Snap; the master meter, CPU and the Agent
  button. On a narrow window the least important readouts hide first.
- **Browser** (left): tabs for Instruments, Loops, Plugins and Files, with a search field.
- **Arrangement** (centre): the toolbar (Pointer, Pencil and Scissors tools, grid, cycle range,
  Follow, Zoom), the ruler with markers and the cycle range, the tempo track when shown
  (View > Tempo Track, `⇧T`), the track headers and the lanes.
- **Editor** (below the arrangement): Piano Roll, Score or Step for the selected MIDI region, with
  an optional controller lane. The Mixer (View > Mixer, `X`) takes its place when open.
- **Inspector** (right): the selected track's instrument, input and output, Channel EQ, the eight
  insert slots, its sends, pan and fader, and the selected region's properties.
- **Agent panel** (right edge): a conversation with the built-in assistant. It folds into a thin
  rail when closed (`⌘J` toggles it).

The command palette (`⌘P` or View > Command Palette) finds any action by name.

## Your first song in five minutes

1. **File > New session** (`⌘N`). A new song has three tracks: **Drums** on the Drum Machine,
   **Bass** on Analog Bass and an audio track, **Vocals**, to record into.
2. **Add a beat**: open the browser's **Loops** tab and double-click *Four Floor 124*, or drag it
   onto the Drums lane. Loops are ordinary MIDI regions you can edit.
3. **Write a bass line**: choose the Pencil tool (`2`), drag across the Bass lane to draw a
   two-bar region, then click in the piano roll below to place notes. Drag a note to move it,
   drag its right edge to change its length.
4. **Hear it**: press Space. Press `C` to loop the cycle range; drag across the ruler to set it.
5. **Name the sections**: move the playhead and press `⇧M` to add a marker, double-click it to
   rename it (*Verse*, *Chorus*…).
6. **Mix**: select a track and use the inspector: pick an effect in an insert slot, turn the
   sends to the reverb (A) and delay (B) buses, set pan and level. `X` opens the full mixer.
7. **Change the tempo** where the song needs it: `⇧T` shows the tempo track under the ruler;
   click it to add a change, right-click a point to make it ramp (see [Tempo](#tempo)).
8. **Save** with `⌘S`, then **export** with File > Export audio… (`⌘B`): WAV, AIFF, FLAC or Ogg
   Vorbis, as a stereo mix or one file per track.

## Tracks

- **Add** an instrument track or an audio track from the Track menu or the `+` above the track
  headers (`⌥⌘S` and `⌥⌘A`). Importing a file or double-clicking a loop also adds a track when
  needed.
- **Header controls**: M (mute), S (solo), record arm, and on audio tracks the monitoring button;
  the small fader and pan dial mirror the inspector.
- **Rename** in the inspector. Right-click a header to duplicate, recolour, move or delete the
  track, or to ask the agent about it.
- **Select** a track by clicking its header; the inspector and the editor follow the selection.
- **Duplicate** (Track > Duplicate) copies the regions, the instrument, the inserts and their
  settings.
- Keyboard: `M`, `S` and `A` mute, solo and arm the selected track; `I` cycles its monitoring.

## Instruments, loops and the browser

- **Instruments** tab: the stock instruments (ryolune Synth, E-Piano Mk I, Drum Machine, Sampler,
  Sub Bass 808, Glass Keys, Choir Pad, Riser, Tonewheel Organ, String Ensemble, Analog Bass,
  Sample Keys) and every installed instrument plugin, filed in colour-coded sound folders.
  **Sample Keys** plays one sound across the keyboard from its root note (Root, Tune, Attack,
  Release, Gate or One-shot, Level): load a file with `strip.loadSample`, or make an instrument
  in the agent panel's Generate tab. The sound is saved inside the song. Double-click to put it
  on the selected instrument track (or a new one), or drag it onto a track.
- **Loops** tab: ready-made MIDI phrases, each with the instrument it was written for.
- **Plugins** tab: every effect and instrument (stock, ryolune native, CLAP, VST3 and Audio Units),
  by sound folder, with Favourites and Recent at the top. Click the star to favourite a plugin;
  right-click to move it to another folder, including folders you create.
- **Files** tab: audio and MIDI files to import.
- **Preview** (bottom of the browser) plays a sound before you choose it.
- Search matches names, vendors and what a plugin is for ("reverb", "compressor"). One row
  stands for all of a plugin's formats and channel layouts: ryolune loads CLAP, then VST3, then
  AU, in the layout a stereo track needs.

## Regions in the arrangement

- **Create** a MIDI region by drawing with the Pencil tool or double-clicking an empty MIDI lane.
  Audio regions come from recordings, imports and dropped files.
- **Select** by clicking; **move** by dragging (also to another track of the same kind);
  **resize/trim** by dragging either edge; **split** at the playhead with `⌘T` or click with the
  Scissors tool; **duplicate** with `⌘D`; delete with `⌫`.
- **Copy, cut and paste** (`⌘C`, `⌘X`, `⌘V`) paste at the playhead on the selected track. The
  clipboard is shared with the CLI and the agent.
- **Open in the editor** with a double-click or `E`.
- **Audio fades**: drag the handles at an audio region's top corners, or type the fade lengths in
  the inspector. Choose equal power, linear or exponential curves. **Region gain** runs from −60
  to +24 dB; the waveform shows the result.
- **Markers**: `⇧M` adds one at the playhead, `⇧N` and `⇧B` jump to the next and previous, `⇧C`
  loops the section you are in. Drag a marker to move it, double-click to rename, right-click to
  recolour or delete.
- **Cycle**: drag across the ruler to set the range, `C` to toggle it.
- **Zoom and scroll**: the Zoom slider, `⌘=` / `⌘-`, `Z` to fit the whole song; `F` toggles
  following the playhead.
- **Edit menu** on the selected MIDI region: quantize, humanize, velocity crescendo and
  diminuendo, legato, reverse, fit to a scale, repeat, and transposition (`⌥↑`/`⌥↓` by a semitone,
  with `⇧` by an octave).

## Editing MIDI

- **Piano Roll**: click to add a note (at the editor's velocity), drag to move, drag the right
  edge to resize, right-click for velocity or deletion. Quantize and the scale guide are in the
  editor header.
- **Score** shows the region as notation; **Step** toggles notes on a grid, handy for drums.
- **Controller lane** (View > Controller Lane, `L`): choose Mod Wheel, Expression, Sustain, Pitch
  Bend, Pressure, Volume, Pan, Breath or any CC number. Click to add a point, drag to move, draw
  a curve by dragging on empty space, Option-click or right-click to delete.
- **Musical typing** (`⌘K`): the letter row from A to ; plays the selected instrument; Z and X
  change the octave. A hardware MIDI keyboard works the same way (Settings > Audio > MIDI Input):
  notes, the mod wheel, pitch bend, the sustain pedal, pressure and other controllers play live.

## Tempo

A song starts at the tempo in the transport display and can change it anywhere after that.

- **Tempo track**: View > Tempo Track (`⇧T`) shows it under the ruler. The line is the tempo over
  the song, with each change as a point. Click the track to add a change at that bar (at the
  height you clicked); drag a point sideways to move it and up or down to change its tempo (`⇧`
  for tenths of a BPM, `⌥` off the grid); double-click a point to type a tempo.
- **Ramps**: right-click a point and choose *Ramp from the Previous Tempo* for a ritardando or
  an accelerando: the tempo glides from the one before to reach the point's tempo at its bar.
- **The transport display** shows the tempo at the playhead. Dragging or typing it changes the
  tempo in force there: the starting tempo, or the change the playhead is after.
- **Music stays on its bars**: notes, regions, markers and automation keep their bars and beats;
  only the speed changes. Audio regions keep their bars too and play their audio at its own
  speed, so a region covers the seconds between its start and end.
- **MIDI files** carry tempo changes both ways: export writes them (a ramp as a step every
  sixteenth note), and import with *Use the file's tempo* follows them.
- Time signature changes inside a song are not supported yet.

## Recording

1. Choose your interface in Settings > Audio (input, output, buffer size).
2. Arm the track (the round button, or `A`). Audio tracks record the input; instrument tracks
   record MIDI from the keyboard or from musical typing.
3. Press Record (`R`), then Play, or just Record: a count-in (Settings > Audio > Count In Bars,
   one bar by default) clicks before the take while the song stays parked. The status line shows
   "Count-in…".
4. Stop to keep the take. Audio takes are also kept as separate WAV files in ryolune's data folder.

**Monitoring** lets you hear the input through the track's inserts, sends and fader. The button
beside Arm cycles Off, Auto (while armed and while recording, until the track plays its own
region) and On; `I` does the same. A built-in microphone into built-in speakers would howl, so
that pairing stays muted until you choose *Monitor Anyway*; headphones are not affected.

MIDI takes keep controllers (mod wheel, bend, sustain, pressure…) in the region, where the
controller lane shows them.

## Mixing

- **Inspector**: Channel EQ, eight insert slots, sends, the Output menu, pan and fader for the
  selected track. Click an insert to open its panel: stock plugins have a front panel with a live display;
  external plugins show their parameters and can open their own window.
- **Mixer** (`X`): one strip per track plus the reverb bus (A), the delay bus (B) and the master,
  each with inserts, sends, pan, fader, mute, solo and meter.
- **Buses A and B**: A is a reverb and B a delay by default; any effect can go on them. Show
  their strips from the Track menu.
- **Your own buses**: Track > Add bus makes a bus track. It holds no regions; it sums what reaches
  it through its own inserts, fader, pan, mute and solo, then goes to the Stereo Out.
  - *Groups*: route tracks to a bus with the inspector's **Output** menu (or right-click a track
    and choose *Route Track to a New Bus*). One fader, one compressor for all the drums.
  - *Aux returns*: point a send at a bus by clicking the send's name, or add one with **+ Send**
    (up to four sends per track). The first two sends feed A and B until you point them elsewhere.
  - Buses feed the Stereo Out, A and B, never another bus, so the mix cannot loop.
  - Solo on a bus keeps the tracks that feed it audible, and solo on a track keeps its bus.
  - Stems follow the routing: a track's stem goes through its bus when effects are included, and
    a bus's stem is everything routed to it.
- **Master**: its own eight inserts (put a limiter last) and the master fader.
- **Faders**: 0 dB is unity. Double-click a dial to reset it; drag with ⇧ for fine steps.
- **Plugin delay compensation** keeps parallel paths aligned.
- Stock effects glide between parameter values in about 10 ms, so moving a dial or fast
  automation does not click.

## Automation

View > Automation opens lanes for track volume and pan, the master fader, and any parameter of
any instrument or insert, including those on the buses and the master. Double-click to add a
point, drag to move, right-click to delete, or type exact values. Each lane can be linear or
stepped and can be switched off. Parameter changes land on their exact sample for CLAP, VST3,
native and stock plugins; Audio Units follow in short slices. Hover a stock plugin's dial and
click its automation button to open that parameter's lane.

## Plugins

- **Formats**: stock plugins (built in), ryolune native plugins written in Rust with the SDK
  ([NATIVE_PLUGINS.md](NATIVE_PLUGINS.md)), CLAP, VST3 and, on macOS, Audio Units. VST2 and AAX
  are not supported.
- **Scanning** runs in a separate process, so a crashing plugin cannot take the app down. Mix >
  Rescan plugins refreshes the list; Settings > Plugins adds folders and can scan at start.
- **Presets**: every stock plugin has factory presets and you can save your own from its panel.
- **State**: an external plugin's own state is saved with the song, so its sound comes back when
  you reopen it and when you export.
- See [PLUGINS.md](PLUGINS.md) for hosting details and limits.

## Files: save, import, export, recover

- **Save** (`⌘S`) and **Save as** (`⇧⌘S`) write a `.ryolune` file with the audio embedded. Saving
  never leaves a half-written file: the old one is kept if anything fails.
- **Open** (`⌘O`) and **Open demo** (File menu) load a song. One song has one editing owner at a
  time, across windows and scripts. **Open Recent…** lists the songs you opened lately; one that
  was moved or deleted says so.
- **Import audio** (`⌘I`, or drop files): WAV, AIFF/AIFC, CAF, FLAC, MP3, AAC/M4A/MP4, Ogg Vorbis
  and the sound of video files (MKV, WebM…). Surround files are folded to stereo.
- **Import and export MIDI** (File menu): notes, controllers and tempo changes, optionally the
  file's tempo and time signature. Each MIDI channel becomes a track on channel 1; from a script,
  `keepChannels=true` keeps one track per file track with every channel, for a multitimbral
  instrument.
- **Export audio** (`⌘B`): a stereo mix or one stem per track; WAV, AIFF, FLAC or Ogg Vorbis;
  44.1, 48 or 96 kHz; 16 or 24-bit PCM or 32-bit float for WAV; optional dither; the whole song
  or a bar range; up to 120 seconds of release tail. The report lists the files, the peak and any
  clipping. There is no MP3 export.
- **Recovery**: an edited song gets a recovery copy every 30 seconds (Settings > General).
  File > Recover session… lists them; opening one never overwrites your original.

## Coming from another app

The first time ryolune starts, a short setup asks which app you made music in, whether you want
the AI features (the agent and sound generation; nothing is hidden either way), lets you connect
an agent provider and check the sound output, and starts you on the demo song, an empty song or
a song from your old app. Help > Set Up ryolune… shows it again. If you used ryolune before this
setup existed, it counts as done.

**File > Import from Another App…** opens a song as a new, unsaved song (ryolune asks to save the
open one first): a **DAWproject** (`.dawproject`), a **MIDI file**, or **audio files** (choose
several stems at once: each becomes an audio track from bar 1). **File > Export for Another
App…** writes the song for an app: a DAWproject, or for the apps without it a folder with the
song as a MIDI file and one WAV per track (Stems), or just the MIDI, the mix or the stems. Both
end with a report: what came across, what changed on the way, and what was left out.

**DAWproject** is the open format of Bitwig Studio, Studio One, Cubase 14 and others. It carries
the tracks, buses and sends, volume, pan, mute and solo, MIDI clips with their notes and
controllers, audio clips with their audio and fades, markers, the tempo and its changes, volume
and pan automation, and plugins with their settings. CLAP, VST3 and Audio Unit plugins load when
they are installed here; the others are named in the report and on the track's note, and an
instrument track without its instrument plays ryolune Synth. ryolune's own instruments and
effects travel as named devices: another app keeps their place, but chooses its own sound.
What does not travel: clip gain, fade curves (the other app uses its own), automation of plugin
parameters, the clip launcher, looped clips (written out repeat by repeat), time-stretched audio
(ryolune plays audio at its own speed) and meter changes (ryolune keeps one meter).

How to bring a song over and take it back, app by app:

| App | Bring your song to ryolune | Take it back |
|---|---|---|
| **Bitwig Studio** (5.0.9+) | Export the project as a DAWproject from the File menu, then Import from Another App… in ryolune. | Export for Another App… › Bitwig Studio writes a `.dawproject`; open it in Bitwig with File › Open…. |
| **Studio One** (6.5+) | Export the song as a DAWproject from the File menu, then import it. | Export for Another App… › Studio One, then open the `.dawproject` in Studio One. |
| **Cubase** (14+) | File › Export › DAWproject…, then import it. Older Cubase: Audio Mixdown with Channel Batch Export for stems, File › Export › MIDI File… for the notes. | Export for Another App… › Cubase, then File › Import › DAWproject… in Cubase. |
| **Ableton Live** | File › Export Audio/Video… with Rendered Track set to All Individual Tracks (one WAV per track); Export MIDI Clip… on a clip for its notes. Import the WAVs together, or the `.mid`. | Export for Another App… › Ableton Live: a folder with the MIDI and one WAV per track; drag them into the Arrangement at bar 1. |
| **Logic Pro** | File › Export › All Tracks as Audio Files…, and File › Export › Selection as MIDI File… for the MIDI regions. | Export for Another App… › Logic Pro, then File › Import › MIDI File… and drag the WAVs in at bar 1. |
| **FL Studio** | File › Export › Wave file… with Split mixer tracks, and File › Export › MIDI file…. | Export for Another App… › FL Studio, then File › Import › MIDI file… and drag the WAVs into the Playlist. |
| **REAPER** | File › Render… with Source set to Stems (selected tracks), and File › Export project MIDI…. (The free ProjectConverter turns a `.rpp` into a `.dawproject`.) | Export for Another App… › REAPER, then Insert › Media file… for each at the project start. |
| **Pro Tools** | Bounce each track (Track Bounce), and File › Export › MIDI…. | Export for Another App… › Pro Tools, then File › Import › Audio… and File › Import › MIDI…. |
| **GarageBand** | Share › Export Song to Disk… exports the mix only: solo each track and export it in turn, or open the project in Logic Pro for MIDI and stems. | Export for Another App… › GarageBand, then drag the `.mid` and the WAVs into the tracks area. |

Audio files and MIDI carry no mix and no plugins, and audio files no tempo: after importing stems,
set the tempo they were made at before editing to the grid.

## The agent

The panel at the right edge is a music assistant that works inside your song. Choose a service in
Settings > Agent: Codex or Claude Code (they use their own sign-in); an API key for Anthropic,
OpenAI, Google Gemini, OpenRouter, Mistral, Groq, DeepSeek or xAI; Ollama or LM Studio running on
this computer; any OpenAI-compatible server; or **Zenith · lsuite**, the agents you use in zenith.
Then describe what you want in your own words and
language: "a busier bass line in the second verse", "glue the drums a little", "why is the keys
track silent?".

- Each step the agent takes appears in the conversation in plain words; the **Changes** tab lists
  every edit it made with Undo and Redo. Everything it does is an ordinary undo step.
- **Conversations** belong to the song and come back when you open it again. Click the title at
  the top of the panel to switch between them, start a new one (the **+** does too, and keeps the
  old one), rename or delete one, or edit the song's **Project memory**: notes the agent reads
  before every request (its key, the style, what to leave alone; up to 32 KB). Only you can change
  the memory.
- **Steer** while the agent works: type and press Enter (the Send button becomes Steer, with Stop
  beside it). The agent reads it at its next step and keeps what it has already done; Claude Code
  restarts its run with your note, the others take it without stopping.
- **Ask Agent About Selection** (`⇧⌘J`, or right-click a region, lane or track) sends what you
  selected along with your message.
- **Takes A/B** keeps a protected original while the agent explores a variation.
- **Generate** makes a loop that fits your bars, tempo and key, a song idea, a one-shot or a
  playable instrument from a description, with the sound service in Settings > Generation
  (ElevenLabs, Stable Audio, fal.ai or your own endpoint; you pay the service directly). Each
  result lands in the song in one undo step and stays under **Your sounds** to listen to, add
  again, turn into Sample Keys or delete. You can also just ask the agent for it.
- **Use another agent** (Settings > Agent) connects Claude Code, Codex, Cursor, VS Code, Claude
  Desktop, Gemini CLI and other MCP apps to the open window, with each one's configuration ready
  to copy, or one click for Cursor and VS Code.
- **Permissions** (Settings > Agent) decide whether the agent may touch files, the transport,
  replace the session, change settings, control the application or generate sounds. They also apply to MCP
  clients.

Scripts and external AI tools control ryolune through the same commands: see
[AI_CONTROL.md](AI_CONTROL.md).

## Appearance

ryolune has one theme, in a dark and a light mode. Settings > Interface shows each as a live
miniature: **Dark** (white ink on black, for long sessions), **Light** (black ink on paper, for
daylight) and **Auto**, which follows the system while the window is open.

ryolune wears the lsuite design system (v2), shared with kimchi and zenith: black and white,
cut square, with grain. The chrome (title bar, transport, browser, inspector, agent panel) sits
on a page of film grain and dithered light; the work (arrangement, editors, mixer strips) stays
solid. The accent is the ink of the mode, white in the dark and black in the light: it marks the
playhead, selection and focus, and whatever you choose is inverted (the open tab, the lit
switch, the selected tool, the menu item under the pointer). Red is kept for recording, record
arm and errors. Menus, popovers and dialogs cast a hard shadow, and dialogs sit inside corner
brackets like a viewfinder. Your track colours stay on your clips and notes, and the logos of
other services keep their own colours.

Every area has a title bar with its name, what it shows and its tools, boxed by kind: undo and
redo, the views (mixer, automation, commands), the agent and the app in the title bar; the edit
tools, follow and cycle, the tempo track and markers, and zoom over the arrangement. In a narrow
window the tools keep their icons and drop their labels (the tooltip still names them). Track
headers show the track's number and its whole name, on two lines if needed, with the mute, solo,
arm and monitoring keys always in view. Knobs show their value as a ring; a centred knob such
as pan fills from the top. With macOS's Reduce transparency setting on, the chrome turns opaque.

Scripts and the agent switch the mode too (`settings.set` with `interface.mode` set to `dark`,
`light` or `auto`). Themes chosen in earlier versions (Modern, Skeuomorphic, Frutiger Aero,
Console, Ink, Neon) open as ryolune in the mode they had. The interface scale, tooltips and
following the playhead are set there too.

The heart at the right of the title bar opens ryolune's GitHub Sponsors page. ryolune is free
and nothing is locked; sponsoring, once or monthly, is how it is paid for.

## Settings

Settings (`⌘,`) is organised in sections:

- **General**: reopen the last song, confirm before quitting, recovery interval.
- **Interface**: appearance (dark, light or auto), scale, tooltips, open the agent panel at start, follow playhead.
- **Audio**: output and input devices, buffer size (64 to 2048 frames), count-in bars, input meter
  on armed tracks, MIDI input and connecting it at start.
- **Plugins**: extra CLAP, VST3 and native folders, scan at start.
- **Agent**: provider, model, reasoning effort, keys, custom instructions, limits, permissions and
  the configurations for outside agents.
- **Generation**: the sound service, its key, the fal.ai model or your endpoint's address.
- **Control**: the local bridge that `ryolune-cli` and `ryolune-mcp` use to reach the window.
- **Updates**: check at start (and every six hours while ryolune is open), install
  automatically, What's new.
- **Diagnostics**: crash reports, this run's log, the data folder, Copy diagnostics and Report a
  Problem (see below).

Settings live in `settings.json` in ryolune's data folder, readable only by you; keys are never
shown in full once saved.

ryolune is free, every update included. Help > Support ryolune… (also in Settings > About) opens
the page where you can donate, once or monthly, if you want to; nothing is locked either way. The window asks one time, after your
third export, and never again. It counts exports in `settings.json` only; nothing is sent.

## Updates, what's new and problems

- **Updates**: ryolune asks GitHub for a newer release when it starts and again every six hours
  while it stays open (Settings > Updates turns this off; so do `--no-update-check` and
  `RYOLUNE_NO_UPDATE=1`). An update is offered in a sheet with its notes; while the song plays,
  the offer waits until you stop. Installing downloads the release and checks its signature
  before anything is replaced; **Restart now** (in the sheet or Settings > Updates) starts the
  new version, asking to save first. Every update is free.
- **What's new**: the first time a new version starts, a sheet lists what changed in every
  release since the one you had. Help > What's New, the command palette and Settings > Updates
  open it again; Earlier versions lists every release this copy carries.
- **When something goes wrong**: ryolune keeps a log of each run and writes a crash report when it
  runs into a bug. If it did not quit properly last time (a crash, a forced quit, a power cut), it
  says so at the next start and writes a report too; File > Recover session… has the snapshots of
  an edited song. Settings > Diagnostics (Help > Logs and Crash Reports…) lists the reports (View,
  Copy report, Delete all), shows the end of this run's log and opens the folders.
- **Report a Problem** (Help menu, or Settings > Diagnostics) opens a new GitHub issue with the
  version and system filled in. Nothing is sent automatically: read it, add what happened and
  submit it yourself. **Copy diagnostics** copies what a report needs (version, system, audio
  device, plugin scan, recent crash reports) without keys, prompts or songs, to paste into it.
- Logs live in `logs/` and reports in `crashes/` in ryolune's data folder, and never leave your
  computer. The last four runs' logs are kept, each up to 8 MB.

## Limits

- No time stretching, comping or time signature changes inside a song yet. Buses do not feed
  other buses.
- Recording latency is not measured or compensated automatically.
- External plugin windows open on macOS; on Windows and Linux external plugins show their
  parameter list.
- Windows builds are not code-signed, so SmartScreen may ask before the first launch. macOS
  builds are notarized from 0.11.0.
