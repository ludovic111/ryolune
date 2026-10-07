---
name: review-and-fix
title: Review a song and fix what is wrong
when: "Why is it silent", "it sounds bad", "check my mix", "something is off" – diagnosing a song before changing it.
---
# Review a song and fix what is wrong

## Steps

1. `session.overview`: read `problems` first (a solo elsewhere, a mute, a bypassed or missing
   instrument, a zero fader, clips on a track without an instrument, a missing plugin). These
   are the usual "silent track" causes.
2. Look at the whole song: `harness.look view=notes` (structure, empty sections, notes out of
   their clips' range) and `harness.look view=mix` (levels, clipping in red, loudness over time,
   spectrum).
3. Measure the loudest section with `harness.measure tracks=true`: which track clips, which is
   far too loud or quiet for its role.
4. Check the key: compare chord and melody notes with `transport` key; `clip.fitScale` the
   clips that stray (only when the wrong notes are clearly accidental).
5. Write the findings as a short list, worst first, each with its fix. If the person asked for
   a review only, stop here and ask before changing anything.
6. Fix with the matching skills (`mixing`, `mastering`, `arrangement`, `sound-design`), one
   problem at a time, measuring after each.

## Common findings and fixes

- Silent: a solo on another track (`track.setSolo solo=false`), mute, fader at 0, instrument
  bypassed (`strip.setBypass bypassed=false`), a missing external plugin (choose another).
- Clipping: lower the loudest tracks first, then a Limiter last on the master.
- Muddy: too much `lowMids`; high-pass pads and keys, cut 250-400 Hz.
- Harsh: `highMids` too strong; lower bright synths' cutoff, de-ess, cut 3-5 kHz.
- Thin: little `bass`; check the bass part plays and is loud enough; add a sub layer.
- Static: one loop for the whole song (skill `arrangement`).

## Checks

- `session.overview` has no `problems` left that you were asked to fix.
- `harness.measure`: no clipping; the findings you fixed are gone.
