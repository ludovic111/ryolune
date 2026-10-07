---
name: melody-and-hooks
title: Write a melody or hook
when: Writing a lead line, a topline, a riff, an arpeggio or a counter-melody over existing harmony.
---
# Write a melody or hook

## Craft

- Read the harmony first: `note.list` on the chord clips, or `harness.look view=notes`.
- A hook is short (1-2 bars), repeated with small changes: call and answer, the answer ending
  on a chord tone. Repetition with variation beats constant novelty.
- Strong beats get chord tones (root, third, fifth); passing and neighbour notes on weak beats.
- Mostly steps, with one or two leaps per phrase; after a leap, move back by step.
- Range about an octave and a half, above the chords (C4-C6, 60-84). Leave rests: a melody that
  never breathes tires the ear.
- Rhythm: mix long and short notes; syncopate (start on 0.5 or 1.75) for groove; land phrase
  ends on beat 0 or 2 of a bar.
- Arpeggios: chord tones in sixteenths or eighths across two octaves, following each chord.

## Steps

1. `track.add kind=midi name=Lead instrument="ryolune Synth"` (or Glass Keys, E-Piano Mk I).
2. Write the hook: `clip.create trackId=Lead startBar=… lengthBars=2 notes=[…]`; repeat it with
   `clip.repeat`, then vary the last repeat's ending with `clip.setNotes`.
3. Keep it in key: `clip.fitScale root=… scale=…` if needed.
4. Shape dynamics: `clip.velocityRamp from=80 to=110` across a build; `clip.humanize`.
5. Sound: for a synth lead set the filter and release (`strip.parameters trackId=Lead
   query="cutoff release"`, `strip.setParameter`), add Echo on send B (`strip.setSendLevel
   send=1 levelDb=-16`) for depth.

## Checks

- `harness.look view=notes` over the section: the lead sits above the chords, phrases end on
  chord tones (compare with the chord notes at the same bar), the hook repeats where meant.
- `harness.measure trackId=Lead`: it should be clearly audible but not the loudest track;
  compare with `harness.measure tracks=true`.
