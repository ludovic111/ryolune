---
name: bass-and-chords
title: Write bass and chords
when: Writing a chord progression, pads, keys comping or a bass line that follows the harmony.
---
# Write bass and chords

## Harmony

Pick a progression that suits the style, in the song's key. Useful ones (Roman numerals; minor
keys in lowercase): pop I-V-vi-IV; melancholy vi-IV-I-V; minor i-VI-III-VII; i-iv-v (or V);
jazz ii-V-I with sevenths; lo-fi Imaj7-vi7-ii7-V7; house i-VII-VI-VII. One chord per bar is a
safe pace; two per bar for energy. Check the key: C major pitch classes 0 2 4 5 7 9 11, A minor
the same set rooted on 9.

## Voicing chords (keys, pads)

- Keep chords between C3 (48) and C5 (72). Three or four notes; add sevenths and ninths for
  lo-fi, jazz and soul.
- Voice-lead: keep common tones, move the others by a step; avoid all voices jumping in parallel.
- Pads: whole-bar notes (length 4 at 4/4), velocity 60-80, slight overlap (legato:
  `clip.legato`). Keys comping: rhythmic chords (eighths, off-beats), velocity 70-95.

## Bass

- Lives under C3: typically E1 (28) to G2 (43). Sub Bass 808 for hip hop and trap (long notes,
  can glide), Analog Bass for house, funk and pop.
- Root on the chord's downbeat; move to the next root with a passing note on the last eighth.
  Lock with the kick: put bass notes where the kick plays and leave space where the snare hits.
- Octave jumps and syncopation (a note on 1.5 or 3.75) give drive; long notes give weight.

## Steps

1. `track.add kind=midi name=Keys instrument="E-Piano Mk I"` (or Pad, Choir Pad, String Ensemble).
2. `clip.create trackId=Keys startBar=… lengthBars=… notes=[…]` with the whole progression.
3. `clip.create trackId=Bass …` with the bass line on the same bars.
4. If notes stray out of key, `clip.fitScale clipId=… root=9 scale=minor`.
5. `track.setVolume`: bass near 0.72-0.75, keys and pads lower (0.6-0.68).

## Checks

- `harness.look view=notes` over the section: bass lowest, chords in the middle, no two parts
  stacked on the same notes for long; every chord change lines up with the bass root.
- `harness.measure` with `tracks=true`: bass and kick together carry most of the low end; if
  `sub` dominates the bands, lower the 808 or shorten its notes.
