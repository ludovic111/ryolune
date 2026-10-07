---
name: stems-and-export
title: Export the mix, stems or MIDI
when: The person asks for a file: a mix (WAV, AIFF, FLAC, Ogg), stems per track, a section, or the MIDI.
---
# Export the mix, stems or MIDI

Exports write files: run them only when the person asks for a file, and only to the path or
folder they name (or a clear one next to the song, which you then report).

## Steps

1. Check before rendering: `harness.measure` over the range to export. Fix clipping first
   (skills `mixing`, `mastering`); an export of a clipping mix clips.
2. **Mix**: `session.exportAudio path=/…/Song.wav` (48 kHz, 24-bit by default). Options:
   `sampleRate` 44100/48000/96000, `format` pcm16/pcm24/float32, a range with
   `startBar`/`endBar` (zero-based, end exclusive), `tailSeconds` for reverb tails (default 3).
   The extension picks the type: `.wav`, `.aiff`, `.flac`, `.ogg` (lossy, `quality` 0-1).
   CD and most distributors: 44100 Hz, pcm16 (dither is on by default); masters for later
   processing: 48000 Hz, pcm24 or float32.
3. **Stems**: `session.exportStems directory=/…/Song stems` writes one file per track into a
   new folder (it never replaces an existing one). `trackIds` to choose, `includeEffects`
   (default true: inserts and sends), `includeMaster` (default false: leave the master chain
   off stems so they sum before mastering). Stems ignore mute and solo.
4. **MIDI**: `session.exportMidi path=/…/Song.mid` (all MIDI tracks, the tempo map included).
5. Read the result: each export reports `peak`, `clippedSamples` and `warnings` (a plugin tail
   longer than `tailSeconds`, clipping in integer formats). Act on them.

## Checks

- The export returned a path and no clipping warning; `clippedSamples` is 0.
- For stems: the folder lists one file per requested track.
- Report the path(s), the format, the length and the peak or loudness.
