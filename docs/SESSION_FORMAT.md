# The `.ryolune` session format

A ryolune song is one JSON file. It holds the arrangement, the mixer, plugin state and the
audio itself (as base64 WAV), so a single file moves between computers. This page describes
the file as the code reads and writes it: `engine/src/document.rs` (the envelope) and
`engine/src/model.rs` (the session), with `engine/src/tempo.rs` and
`engine/src/automation.rs` for tempo changes and automation lanes.

To check a file: `ryolune --validate song.ryolune`. It loads the file exactly as the window
does and prints the track and clip counts, or the error.

## Envelope

```json
{
  "format": "ryolune-session",
  "version": 1,
  "session": { ... },
  "audio": { "<source id>": "<base64 WAV>", ... }
}
```

| Field | Type | Notes |
|---|---|---|
| `format` | string | `ryolune-session`. Files from before the rename carry `ondera-session`; both load. ryolune always writes `ryolune-session`. |
| `version` | integer | Must be `1`. Any other value is refused ("Unsupported ryolune session format or version"). |
| `session` | object | The song, described below. |
| `audio` | object | Required, may be empty. Maps a source id to its audio as base64 WAV. ryolune writes 32-bit float stereo WAV at the source's sample rate (`audio::encode_wav`). Sources with `origin: "generated"` are not stored here. |

There has only ever been version 1. The format grows by adding optional fields that are
absent when they hold their default, so files without a newer feature have the same shape
they always had, and older files load unchanged. Fields ryolune does not know are ignored on
load; on the `session`, a track, a strip and the view they are kept and written back (see
[Unknown fields](#unknown-fields)).

### Extensions

- `.ryolune` is the extension (`document::EXTENSION`).
- `.ondera` (`document::LEGACY_EXTENSION`) is the extension from before the rename. These files
  open unchanged and keep their name when saved again; the saved content says
  `ryolune-session`.
- Extensions are matched without regard to case (`document::is_session_path`).

### Size limits

| Limit | Value |
|---|---|
| File size on load | 768 MiB |
| Embedded audio on save (base64) | 700 MiB |
| Decoded audio of a song | 1 GiB |
| One source | 14,400 seconds, 8,000-384,000 Hz, 1 or 2 channels |

### Loading and saving

`document::load` reads the file, checks `format` and `version`, normalizes the session (see
[Normalization](#normalization-on-load)), validates it (`Session::validate`), decodes every
embedded source and synthesizes the generated ones. `transport.playing` and
`transport.recording` are always false after a load.

`document::save` validates first, then writes through `document::atomic_write`: a temporary
file beside the target, flushed and synced, then renamed over it. A failed save leaves the old
file intact. The written file keeps the old file's permissions (0644 for a new file) and a
symlinked target is written through.

While a song is open, ryolune holds an advisory lock on a sibling file named
`.<file name>.ryolune-lock` (mode 0600, never deleted; `engine/src/session_file.rs`), so the
window, `ryolune-cli` and `ryolune-mcp` do not edit one file at once.

## Session

All keys are camelCase.

| Key | Type | Default when absent | Notes |
|---|---|---|---|
| `id` | string | `""` | Stable id of the song (0.14+), kept across saves and renames; the agent's saved conversations and project memory belong to it. Older files get one derived from their path when opened and keep it from the next save. Absent when empty. |
| `name` | string | required | Song name, often the file name. |
| `tracks` | array of [Track](#tracks) | required | Top to bottom. At most 128. |
| `clips` | array of [Clip](#clips) | required | At most 50,000. |
| `sources` | object: id to [Source](#sources) | required | At most 10,000. |
| `strips` | object: key to [Strip](#strips) | required | Mixer strips by track id, plus `master`, `bus-a`, `bus-b`. |
| `automation` | array of [lanes](#automation) | `[]` | Absent when empty. |
| `transport` | [Transport](#transport) | required | |
| `view` | [View](#view) | required | |
| `masterVolume` | number 0-1 | `0.75` | Stereo Out fader position. Always written. |
| `markers` | array of [Marker](#markers) | `[]` | Absent when empty. |
| `tempoChanges` | array of [TempoPoint](#tempo-changes) | `[]` | Absent when empty. |

Positions are zero-based. Bars (`startBar`, `bar`, `cycleStartBar`) and beats are floats. Every
time value must be finite and between 0 and 1,000,000 (`model::valid_time`).

## Tracks

| Key | Type | Default | Notes |
|---|---|---|---|
| `id` | string | required | Unique. Cannot be `master`, `bus-a` or `bus-b`. |
| `name` | string | required | |
| `color` | string | required | CSS colour. |
| `armed` | bool | required | Record arm. Must be false on a bus track. |
| `monitor` | `"off"`, `"auto"`, `"on"` | `"off"` | Input monitoring for audio tracks. Absent when off. Must be off on a bus track. |
| `kind` | `"audio"`, `"midi"`, `"bus"` | required | |
| `volume` | number 0-1 | required | Fader position; 0.75 is unity gain, 1.0 is +6 dB (`model::fader_gain`). |
| `pan` | number -100 to 100 | required | |
| `mute` | bool | required | |
| `solo` | bool | required | |
| `output` | string | absent | The bus track this track feeds. Absent means the Stereo Out. |

### Bus tracks and routing

A track with `kind: "bus"` holds no clips and is never armed. It sums the tracks that route to
it (`output`) or send to it (a strip's `sends[].bus`) through its own inserts, fader and pan.
A song holds at most 32 bus tracks.

`Session::validate_routing` keeps the graph one-way:

- A track's `output` must name a bus track. A bus track has no `output`: it always feeds the
  Stereo Out.
- A track's sends go to a bus track, `bus-a` or `bus-b`. A bus track's sends go to `bus-a` or
  `bus-b` only. The fixed strips (`master`, `bus-a`, `bus-b`) send nowhere else.

When a bus track is removed, `Session::prune_routing` sends its tracks back to the Stereo Out
and resets sends that pointed at it.

## Clips

| Key | Type | Default | Notes |
|---|---|---|---|
| `id` | string | required | Unique. |
| `name` | string | required | |
| `agent` | bool | `false` | Made by an agent; the window marks it. Always written. |
| `trackId` | string | required | An existing track of the matching kind, never a bus track. |
| `startBar` | number | required | |
| `lengthBars` | number > 0 | required | |
| `data` | object | required | Tagged by `kind`: `"midi"` or `"audio"`. |

### MIDI clip data

```json
{ "kind": "midi", "notes": [ ... ], "controllers": [ ... ], "polyPressure": [ ... ] }
```

| Key | Type | Default | Notes |
|---|---|---|---|
| `notes` | array of Note | required | |
| `controllers` | array of Controller | `[]` | Control changes, pitch bend, channel pressure. Absent when empty. |
| `polyPressure` | array of Controller | `[]` | Polyphonic key pressure points only. Absent when empty. |

In memory, polyphonic pressure lives in `controllers` with every other controller. The file
keeps it in its own `polyPressure` list (`ClipDataFile` on read, `ClipDataOut` on write) so
older ryolune versions, which do not know the `poly` kind, never meet it. On load the two
lists are merged.

Note:

| Key | Type | Default | Notes |
|---|---|---|---|
| `id` | string | required | |
| `start` | number | required | Beats from the clip start. |
| `length` | number > 0 | required | Beats. |
| `pitch` | integer 0-127 | required | 60 is C4. |
| `velocity` | integer 1-127 | required | |
| `agent` | bool | `false` | Always written. |
| `channel` | integer 0-15 | `0` | MIDI channel (channel 1 to a musician is 0). Absent on channel 0. |

Controller:

| Key | Type | Default | Notes |
|---|---|---|---|
| `id` | string | required | |
| `kind` | `"cc"`, `"bend"`, `"pressure"`, `"poly"` | required | |
| `number` | integer 0-127 | absent | The controller number for `cc`, the key for `poly`. Required for those two, absent for `bend` and `pressure`. |
| `time` | number | required | Beats from the clip start. |
| `value` | integer | required | 0-127; `bend` is -8192 to 8191 with 0 centred. |
| `agent` | bool | `false` | Absent when false. |
| `channel` | integer 0-15 | `0` | Absent on channel 0. |

A value holds until the next point of the same lane, where a lane is (kind, number, channel).
A song holds at most 200,000 notes and controller points together.

### Audio clip data

```json
{ "kind": "audio", "sourceId": "src-1", "offsetSeconds": 0,
  "fadeInSeconds": 0.01, "fadeOutSeconds": 0.5, "fadeCurve": "linear", "gainDb": -3 }
```

| Key | Type | Default | Notes |
|---|---|---|---|
| `sourceId` | string | required | A key of `sources`. |
| `offsetSeconds` | number | required | Where in the source the clip starts. |
| `fadeInSeconds` | number | `0` | Absent when 0. |
| `fadeOutSeconds` | number | `0` | Absent when 0. |
| `fadeCurve` | `"equalPower"`, `"linear"`, `"exponential"` | `"equalPower"` | Absent when equalPower. Fade-outs mirror fade-ins. |
| `gainDb` | number -60 to 24 | `0` | Clip gain before the track's inserts. Absent when 0. |

Fades are in seconds, not bars: audio is not stretched with the tempo, so a fade keeps its
sound when the tempo changes. Edits clamp fades to the clip's length (`model::clamp_fades`);
the file itself is not clamped on load.

## Sources

The audio a clip plays. Keyed by id in `sources`; the key must equal the source's `id`.

| Key | Type | Notes |
|---|---|---|
| `id` | string | Same as the map key. |
| `name` | string | |
| `sampleRate` | integer | 8,000-384,000. |
| `channels` | integer | 1 or 2. |
| `fileName` | string or null | The imported file's name. Written as `null` when unknown. |
| `durationSeconds` | number | At most 14,400. |
| `origin` | `"file"`, `"recording"`, `"generated"` | |
| `seed` | integer or null | For generated sources. Written as `null` otherwise. |
| `waveKind` | string or null | For generated sources: `"drums"` makes a drum loop, anything else a pad. Written as `null` otherwise. |

`file` and `recording` sources must have their audio in the envelope's `audio` map.
`generated` sources are synthesized at load from `seed` and `waveKind` at 48 kHz
(`audio::generate`); the bundled demo song uses them. Sounds made by `generate.audio` are
imported as ordinary audio, not as `generated` sources.

## Strips

`strips` maps a key to a channel strip. The keys are track ids plus three fixed strips:
`master` (Stereo Out), `bus-a` (A · Reverb) and `bus-b` (B · Delay). A track may have no
strip; it then plays with defaults (a MIDI track uses the stock `ryolune Synth`).

| Key | Type | Default | Notes |
|---|---|---|---|
| `instrument` | string | `"ryolune Synth"` | Stock instrument for a MIDI track, used when `synth` is absent. |
| `inserts` | array of [Insert](#inserts) | `[]` | Effects in order. At most 8. |
| `sends` | array of Send | `[]` | At most 4. |
| `synth` | Insert | absent | An instrument plugin (stock, native, CLAP, VST3 or AU). Absent plays `instrument`. |

Send:

| Key | Type | Default | Notes |
|---|---|---|---|
| `levelDb` | number -100 to 0, or null | required key, may be `null` | Send level. `null` means the send is off. |
| `name` | string | `""` | Display name. |
| `bus` | string | absent | A bus track id, `bus-a` or `bus-b`. Absent: the first send feeds `bus-a`, the second `bus-b`. A third or fourth send must name its bus. |

## Inserts

One plugin slot, used for `inserts` and for `synth`.

| Key | Type | Default | Notes |
|---|---|---|---|
| `name` | string | required | Display name; also the stock plugin's name when `plugin` is empty. |
| `state` | `"active"`, `"bypassed"`, `"empty"` | required | `empty` is a free slot. |
| `meta` | string | `""` | Free text shown under the name. Always written. |
| `id` | string | `""` | Rack key, unique across the song. Filled in on load when missing or duplicated. Always written. |
| `plugin` | string | `""` | Plugin id. Absent when empty, which means `stock:<name>`. |
| `params` | object | `{}` | Parameter id (as a JSON string key) to plain value. Absent when empty. Values must be finite. |
| `blob` | string | `""` | Base64 plugin state, captured on save and bounce. Absent when empty. At most 64 MiB. |

Plugin ids (`plugin::Descriptor`): `stock:<name>`, `native:<plugin id>`, `clap:<plugin id>`,
`vst3:<class id hex>`, `au:<type>:<subtype>:<manufacturer>`.

Parameter values are document state, so they undo. Plugins whose sound is not fully described
by parameters (most external plugins, the Sample Keys instrument) also keep `blob`.

## Automation

`automation` is a list of lanes, one per target.

| Key | Type | Default | Notes |
|---|---|---|---|
| `id` | string | required | Unique, 1-256 bytes. |
| `name` | string | required | At most 512 bytes. |
| `target` | object | required | See below. |
| `min`, `max` | number | required | Value range, `min < max`. Track volume and master volume lanes must be 0 to 1; track pan lanes -100 to 100. |
| `manualValue` | number | `0` | The target's own value when the lane was made; a plugin parameter returns to it when its lane goes away. Must lie in `min..max`. |
| `interpolation` | `"linear"`, `"step"` | `"linear"` | |
| `enabled` | bool | `true` | A lane that is off, or has no points, leaves the target at its own value. |
| `points` | array of `{id, beat, value}` | `[]` | `beat` is absolute quarter-note beats, strictly increasing; `value` within `min..max`. |

Targets are tagged by `kind`:

```json
{ "kind": "trackVolume", "trackId": "bass" }
{ "kind": "trackPan", "trackId": "bass" }
{ "kind": "masterVolume" }
{ "kind": "pluginParameter", "trackId": "bass", "insertId": "insert-3",
  "pluginId": "stock:ryolune Comp", "parameterId": 2 }
```

A lane whose target no longer exists fails validation. At most 256 lanes and 65,536 points
in a song.

## Transport

| Key | Type | Default | Notes |
|---|---|---|---|
| `playing` | bool | `false` | Always written false. |
| `recording` | bool | `false` | Always written false. |
| `positionBeats` | number | required | Playhead. |
| `key` | string | required | Song key, such as `"C min"`. |
| `snapDivision` | integer | required | One of 1, 2, 4, 8, 16, 32, 64. |
| `tempo` | number 20-400 | required | Tempo at the start of the song. |
| `timeSignature` | `{numerator, denominator}` | required | Numerator 1-32; denominator 1, 2, 4, 8, 16 or 32. One meter for the whole song. |
| `cycle` | bool | required | |
| `cycleStartBar`, `cycleEndBar` | number | required | End after start. |
| `metronome` | bool | required | |

## Tempo changes

`tempoChanges` lists the tempo changes after the start, in bar order. Absent when the song
keeps one tempo.

| Key | Type | Default | Notes |
|---|---|---|---|
| `bar` | number | required | Zero-based, after bar 0, strictly increasing. |
| `bpm` | number 20-400 | required | |
| `ramp` | bool | `false` | Glide from the previous tempo to reach `bpm` at `bar` (linear in beats). Absent when false. |

At most 1,000. `engine/src/tempo.rs` (`TempoMap`) turns beats into seconds and back.

## Markers

`markers` lists named positions on the ruler, in bar order. Absent when empty.

| Key | Type | Default | Notes |
|---|---|---|---|
| `id` | string | required | Unique, not empty. |
| `bar` | number | required | Zero-based. |
| `name` | string | required | At most 120 characters. |
| `color` | string | absent | CSS colour. Absent uses the theme's marker colour. |

At most 1,000.

## View

What the window last showed. Saved with the song so it reopens the same way.

| Key | Type | Default | Notes |
|---|---|---|---|
| `selectedTrackId`, `selectedClipId`, `editorClipId`, `selectedNoteId` | string or null | `null` | Written as `null` when nothing is selected. |
| `pixelsPerBar` | number 12-480 | required | Zoom. |
| `scrollBars` | number | required | First visible bar. |
| `followPlayhead` | bool | required | |
| `editorMode` | string | required | `pianoRoll`, `score` or `step`. |
| `browserTab` | string | `"instruments"` | `instruments`, `loops`, `plugins` or `files`. |
| `browserSelection` | string or null | `null` | |
| `editorLowPitch` | integer | absent | Lowest pitch the piano roll shows (capped at 108). Absent lets it frame the clip. |

## Unknown fields

`Session`, `Track`, `Strip` and `View` keep fields they do not know in a flattened `extra` map
and write them back. Files from older versions carry such fields (for example `agentActive`
on tracks, `input` and `output` on strips, `agent` and `meters` on the session). Unknown fields
on clips, notes, sources, inserts, the transport or the envelope are dropped on load.

## Normalization on load

`Session::normalize` runs on every load, before validation:

- Stock names from before the rename are updated: `Ondera Synth` and `Ondera Comp` become
  `ryolune Synth` and `ryolune Comp` (in `instrument`, insert names and `stock:` plugin ids),
  and an insert `meta` of `Ondera` becomes `ryolune`.
- Inserts with a missing or duplicate `id` get a fresh `insert-<n>`.
- `editorLowPitch` is capped at 108.
- Missing fixed strips are created: `bus-a` with the stock `Space` reverb (parameter 4, mix,
  at 100), `bus-b` with the stock `Echo` delay (parameter 5, mix, at 100), and an empty
  `master`.

## A minimal valid file

One MIDI track, one audio track routed to a bus track, a tempo ramp, a marker and a volume
lane. The audio track plays a generated source, so the `audio` map can stay empty. The fixed
strips are left out; they are created on load.

```json
{
  "format": "ryolune-session",
  "version": 1,
  "session": {
    "name": "Sketch",
    "tracks": [
      { "id": "keys", "name": "Keys", "color": "#b191ea", "armed": false,
        "kind": "midi", "volume": 0.75, "pan": 0, "mute": false, "solo": false },
      { "id": "drums", "name": "Drums", "color": "#ed835e", "armed": false,
        "kind": "audio", "volume": 0.75, "pan": 0, "mute": false, "solo": false,
        "output": "group" },
      { "id": "group", "name": "Group", "color": "#6ab3fd", "armed": false,
        "kind": "bus", "volume": 0.75, "pan": 0, "mute": false, "solo": false }
    ],
    "clips": [
      { "id": "keys-1", "name": "Chords", "trackId": "keys", "startBar": 0, "lengthBars": 2,
        "data": {
          "kind": "midi",
          "notes": [
            { "id": "n1", "start": 0, "length": 4, "pitch": 60, "velocity": 100 },
            { "id": "n2", "start": 4, "length": 4, "pitch": 64, "velocity": 90, "channel": 1 }
          ],
          "controllers": [
            { "id": "c1", "kind": "cc", "number": 64, "time": 0, "value": 127 },
            { "id": "c2", "kind": "bend", "time": 2, "value": -4096 }
          ],
          "polyPressure": [
            { "id": "p1", "kind": "poly", "number": 60, "time": 1, "value": 40 }
          ]
        } },
      { "id": "drums-1", "name": "Beat", "trackId": "drums", "startBar": 0, "lengthBars": 2,
        "data": { "kind": "audio", "sourceId": "src-1", "offsetSeconds": 0,
                  "fadeOutSeconds": 0.5, "gainDb": -3 } }
    ],
    "sources": {
      "src-1": { "id": "src-1", "name": "Beat", "sampleRate": 48000, "channels": 2,
                 "durationSeconds": 4, "origin": "generated", "seed": 1, "waveKind": "drums" }
    },
    "strips": {
      "keys": { "instrument": "ryolune Synth", "inserts": [],
                "sends": [ { "levelDb": -12, "name": "A · Reverb" } ] }
    },
    "automation": [
      { "id": "auto-1", "name": "Keys volume",
        "target": { "kind": "trackVolume", "trackId": "keys" },
        "min": 0, "max": 1,
        "points": [ { "id": "a1", "beat": 0, "value": 0.5 },
                    { "id": "a2", "beat": 8, "value": 0.75 } ] }
    ],
    "transport": {
      "positionBeats": 0, "key": "C maj", "snapDivision": 16, "tempo": 120,
      "timeSignature": { "numerator": 4, "denominator": 4 },
      "cycle": false, "cycleStartBar": 0, "cycleEndBar": 4, "metronome": false
    },
    "view": { "pixelsPerBar": 48, "scrollBars": 0, "followPlayhead": true,
              "editorMode": "pianoRoll" },
    "markers": [ { "id": "m1", "bar": 0, "name": "Intro" } ],
    "tempoChanges": [ { "bar": 1, "bpm": 100, "ramp": true } ]
  },
  "audio": {}
}
```

When ryolune saves this song it writes the same structure with defaults filled in where they
are always written: `"agent": false` on clips and notes, `"masterVolume": 0.75`,
`"playing": false` and `"recording": false`, `null` for the view's empty selections and
`browserSelection`, `"browserTab": "instruments"`, `"meta": ""` and an `id` on every insert,
`"fileName": null` on the source, and the three fixed strips.
