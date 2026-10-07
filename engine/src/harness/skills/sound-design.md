---
name: sound-design
title: Design sounds with the stock instruments and effects
when: Choosing or shaping a sound ("darker pad", "punchier kick", "a pluck", "make it wider"), building an effect chain, or using installed plugins.
---
# Design sounds

## What is in the box

`session.catalog` lists them. Instruments: ryolune Synth (subtractive: waveform, cutoff,
resonance, envelope), Analog Bass, Sub Bass 808, E-Piano Mk I, Glass Keys, Tonewheel Organ,
String Ensemble, Choir Pad, Riser, Drum Machine, Sampler, Sample Keys. Effects: ryolune Comp,
Channel EQ, Tape Sat, Overdrive, Bitcrusher, Lo-Fi, Chorus, Flanger, Phaser, Space (reverb),
Echo (delay), Filter, Auto Filter, Tremolo, Auto Pan, Stereo Width, Utility, Transient, Gate,
De-Esser, Pump, Pitch Shift, Limiter. Factory presets: `strip.programs` then
`strip.setProgram` (for example ryolune Synth "Plucky bass", Space "Small room", Echo "Dotted
eighth", Tape Sat "Warm").

## Steps

1. Read before you turn: `strip.parameters trackId=… query="cutoff"` (omit slot for the
   instrument; slot N for an insert) gives names, ranges and the current display text.
2. Change by meaning, with display text: `strip.setParameter trackId=Lead parameter=Cutoff
   text="1.2 kHz"`, `parameter=Release text="400 ms"`, `parameter=Mix text="25%"`.
3. Effects go in insert slots in signal order: `strip.setPlugin trackId=Keys plugin="Tape Sat"
   slot=0`, then Channel EQ, then ryolune Comp, then modulation; reverb and delay are better on
   the A/B sends (`strip.setSendLevel`) so several tracks share one space.
4. External plugins load the same way (`plugin.list query="…"`, `strip.setPlugin plugin="…"`);
   their parameters come from the plugin: never guess ids.

## Recipes

- **Darker / warmer**: lower the synth Cutoff, Channel EQ High Gain -3 to -6 dB, Tape Sat "Warm".
- **Brighter / more present**: raise Cutoff, Channel EQ High Gain +2 to +4 dB, a touch of Overdrive.
- **Pluck**: short Decay (100-250 ms), Sustain 0 %, filter envelope amount up, Echo on send B.
- **Pad**: slow Attack (300-900 ms), long Release (1-3 s), Chorus, Space with Size 70-90 %.
- **Punchier drums**: Transient (more attack), ryolune Comp "Drum bus", Tape Sat for density.
- **Wider**: Stereo Width 130-150 % on pads and keys only; keep bass and kick mono
  (Utility "Bass Mono" 120 Hz).
- **Lo-fi**: Lo-Fi or Bitcrusher, Filter low-pass around 6-8 kHz, Tape Sat, slow Tremolo.

## Checks

- `harness.measure trackId=…` before and after: a brighter sound shows more `highMids`/`air`,
  a darker one less; the level should not jump (match it with the fader or the plugin's output).
- `harness.look view=mix trackId=…`: the spectrum moved where you meant; no clipping (red).
