---
name: arrangement
title: Arrange a song (structure and energy)
when: Turning a loop or a few parts into a song, extending or restructuring sections, adding intros, breaks, builds, transitions and an ending.
---
# Arrange a song

## Shape

- Sections in 4- or 8-bar units, marked on the ruler (`marker.add name=… bar=…`). Typical
  forms: pop Intro 4 · Verse 8 · Pre 4 · Chorus 8 · Verse 8 · Chorus 8 · Bridge 8 · Chorus 8 ·
  Outro 4; electronic Intro 16 · Build 8 · Drop 16 · Break 8 · Build 8 · Drop 16 · Outro 16;
  hip hop Intro 4 · Verse 16 · Hook 8 · Verse 16 · Hook 8.
- An energy curve: each section is denser, wider, higher or louder than the one before, until a
  break resets it. Change something every 4 or 8 bars: add or drop a part, open the hats, move
  the chords up an octave, double the lead.
- Transitions: a fill or a drop-out in the last bar, a Riser (stock instrument, one long note
  over 2-4 bars, ending on the downbeat), a crash (49) on the first beat of the new section,
  an Auto Filter sweep (automate its cutoff, skill `automation-and-movement`).
- An ending: a final chord held on a downbeat, or a fade (`automation.create target=masterVolume
  points=[{"beat":…,"value":0.75},{"beat":…,"value":0}]`), never a loop that just stops.

## Steps

1. `session.overview` and `marker.list`: what exists, how long, which parts play where.
2. Plan the form as a table: section, start bar, length, which parts play.
3. Lay out the sections: `marker.add` for each.
4. Copy parts across: `clip.duplicate`, `clip.repeat count=…`, `clip.copy` + `clip.paste`, or
   `clip.create` with the pattern at the new bar; `clip.move` to place, `clip.trim`/`clip.resize`
   to shorten, `clip.remove` to drop a part from a section.
5. Vary the repeats: `clip.setNotes` on a copy (fewer notes in the verse, an octave up in the
   last chorus); a fill in each section's last bar.
6. Use `session.batch` for the whole layout so it is one undo step.

## Checks

- `harness.look view=notes` over the whole song: the picture should show the sections (dashed
  marker lines) with visibly different density; no hole you did not mean.
- `harness.look view=mix` over the whole song: the short-term loudness rises into each chorus or
  drop and falls in the breaks; the end decays instead of cutting.
- `session.overview`: the length in bars and seconds matches the plan.
