---
name: score-to-picture
title: Score a video cut from kimchi
when: Writing music for a kimchi cut (a hand-off in the inbox, "score this video", "music under these scenes"), or sending music back to kimchi.
---
# Score a video cut from kimchi

kimchi (lsuite's video editor) hands a cut to ryolune as a manifest: its audio, its length and
its markers (scene changes, beats to hit). ryolune puts the cut's audio on a track and its
markers on the ruler at the bars where they fall, so the music can follow the picture.

## Steps

1. `handoff.inbox`: the cuts waiting, oldest first. Pick the one named (or the newest).
2. Choose a tempo first (the markers are placed in bars at the current tempo): pick one that
   suits the mood; if a key moment must land on a downbeat, choose the tempo so that it does:
   tempo = 60 × beats ÷ seconds-to-the-moment, rounded to a sensible value.
   `transport.setTempo bpm=…`.
3. `session.scoreCut manifest=<path from handoff.inbox>`: one undo step, a new audio track with
   the cut's sound at bar 0, markers at each cue, the cycle over the cut's length.
4. Read the cues: `marker.list` gives each marker's bar. Plan the music per cue: where it
   starts, builds, hits and resolves. Keep the cut's own audio (dialogue) clear: lower music
   under speech (skill `automation-and-movement`, volume dips of 4-8 dB).
5. Compose with the skills `compose-from-brief`, `arrangement`, `mixing`: sections start on the
   markers; hits (crash, chord change, drop) land exactly on the cue bars; the music ends with
   the cut (`durationSeconds` from the manifest).
6. Mute the reference audio track before sending the music back if kimchi already has it.
7. Send it back: `export.toKimchi` (the mix, or `stems=true` for one kimchi track per ryolune
   track), `startSeconds` where it belongs on kimchi's timeline. It renders as a job.

## Checks

- `harness.look view=notes` over the cut's length: section changes and hits sit on the dashed
  marker lines.
- `harness.measure` over the cut: music loud enough but under dialogue (music around -20 to
  -16 LUFS under speech, louder where there is none); no clipping.
- `session.overview`: the song is as long as the cut (plus a short tail).
