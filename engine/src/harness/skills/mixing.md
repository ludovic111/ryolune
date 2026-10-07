---
name: mixing
title: Mix (gain staging, EQ, compression, buses, space)
when: Balancing levels, cleaning up mud or harshness, compressing, grouping tracks into buses, adding reverb and delay, or any "make it sound better" request on an existing song.
---
# Mix

## Steps

1. **Measure first**: `harness.measure tracks=true` over a dense section (the chorus or drop,
   at most 120 s). It lists each track's loudness and peaks with its fader.
2. **Gain staging**: bring every track's peak to about -10 to -6 dBFS with `track.setVolume`
   (0.75 = 0 dB; each 0.05 is roughly 1.5-2 dB near unity, so re-measure) or the instrument's
   Level parameter. Nothing may clip on its own.
3. **Balance by role** (relative integrated loudness, from the per-track numbers): kick and
   bass loudest in the low end; lead or vocal 0-3 LU under the loudest element; drums
   (snare) close to the lead; keys and pads 6-10 LU under; effects and textures further down.
   Start with all faders down a little and bring parts up in that order.
4. **Pan** for space (`track.setPan`): kick, snare, bass, lead centred; hats 15-30 off centre;
   keys and pads or doubled parts left and right (-40 / +40); keep low end centred.
5. **EQ** (Channel EQ, or Filter in high-pass mode): cut before you boost. Take lows out of
   everything that is not kick or bass (high-pass 100-200 Hz on pads, keys, leads); cut mud at
   250-400 Hz (Mid Gain -2 to -4 dB, Mid Freq ~300 Hz) when `lowMids` dominate; add presence
   at 2-5 kHz or air with the High shelf only where a part must cut through.
6. **Compression** (ryolune Comp): drums and bass to steady them (Ratio 3-4:1, Threshold so
   gain reduction is 3-6 dB, Attack 10-30 ms to keep the transient, Release 80-150 ms);
   presets "Drum bus", "Bass tighten", "Vocal glue". Add Makeup to restore the level.
7. **Buses**: group related tracks with `track.group trackIds=[…] name="Drums"` and process
   the group once (a Comp "Drum bus" on it).
8. **Space**: reverb on send A, delay on send B, at -20 to -10 dB per track; more on pads and
   leads, little or none on kick and bass. Set the return's character on `bus-a` / `bus-b`
   (`strip.parameters trackId=bus-a slot=0`).
9. **Sidechain feel**: Pump on pads and bass for the ducking of dance music.

## Checks

- `harness.measure tracks=true` again: no track clips; the order of loudness matches the roles.
- `harness.measure` on the mix (no trackId): sample peak below -3 dBFS before mastering (leave
  headroom for the master chain), no clipping, `findings` empty or understood.
- `harness.look view=mix`: the spectrum roughly follows the dashed pink slope (no big bump in
  low mids, no hole in the highs); the waveform is not a flat brick.
- Report the changes per track (fader, EQ, compression, sends) in a few lines.
