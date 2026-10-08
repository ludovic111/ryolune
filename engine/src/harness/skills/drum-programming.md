---
name: drum-programming
title: Program drums
when: Writing or fixing a beat or groove, fills, or making drums feel human.
---
# Program drums

The Drum Machine plays General MIDI pitches: 36 kick, 38 snare, 39 clap, 37 rim, 42 closed hat,
44 pedal hat, 46 open hat, 49 crash. At 4/4 a bar is 4 beats; a sixteenth is 0.25 beats.

## Grooves by style (one bar; positions in beats)

- **Four on the floor** (house, disco, techno, 120-128 BPM): kick 0, 1, 2, 3; clap or snare 1
  and 3; closed hats on the off-beats 0.5, 1.5, 2.5, 3.5; open hat on 3.5 every other bar.
- **Backbeat** (pop, rock, 90-130): kick 0 and 2.5 (and 2 or 1.75 for push), snare 1 and 3, hats
  on eighths.
- **Hip hop / lo-fi** (75-95): kick 0, 1.75, 2.5; snare 1 and 3 (lay it back: start 1.02-1.04);
  hats in eighths with swing: move every second sixteenth late by 0.04-0.08 beats.
- **Trap** (130-150 felt half time): kick 0, 0.75, 2.5; snare or clap 2 only; hats in
  sixteenths with rolls (32nds, 0.125 beats) before the snare.
- **Drum and bass** (170-175): kick 0 and 2.5, snare 1 and 3, hats in eighths, ghost snares
  on 1.75 and 3.75 at low velocity.

## Steps

1. `rhythm.create` builds a Euclidean groove in one call when "a groove" is all that is asked:
   `lanes=[{"steps":16,"pulses":4,"pitch":36,"velocity":110},{"steps":16,"pulses":2,"rotation":4,"pitch":38},{"steps":16,"pulses":8,"pitch":42,"velocity":80}] bars=4`.
2. Otherwise write it: `clip.create trackId=Drums startBar=0 lengthBars=4 notes=[…]` with
   lengths of 0.25 beats for hits.
3. Velocity is the feel: accents 105-120, main hits 90-105, hats 60-90 alternating strong and
   weak, ghost notes 35-60. `clip.humanize timingMs=6 velocity=8` after writing loosens it.
4. Fills: the last bar (or half bar) of a section changes: snare or tom run in sixteenths with
   a velocity ramp (`clip.velocityRamp`), a crash (49) on the downbeat of the next section.
5. Variation: duplicate the groove for each section and change one element (open hats in the
   chorus, kick drops in the break). Avoid one bar looped for the whole song.

## Checks

- `harness.look view=notes trackId=Drums fromBar=… toBar=…`: kick on the beats you meant, fills
  at section ends, crashes at section starts.
- `harness.measure trackId=Drums`: a drum bus should peak around -6 dBFS before the master; if
  it clips, lower the track (`track.setVolume`) or put `ryolune Comp` with the "Drum bus" preset.
