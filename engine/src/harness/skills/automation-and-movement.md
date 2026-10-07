---
name: automation-and-movement
title: Automate movement (fades, sweeps, rides)
when: Volume rides and fades, filter sweeps into a drop, panning moves, effect throws, or anything that should change over time.
---
# Automate movement

Automation lanes hold points `{beat, value}` in **absolute beats** from the song's start (bar ×
beats per bar) and the parameter's plain units: track and master volume in fader units (0.75 =
0 dB, 0 = silent), pan -100 to 100, plugin parameters in their own units (Hz, %, dB).

## Steps

1. Find the target: for a plugin parameter, `strip.parameters trackId=… slot=… query=cutoff`
   (id, range, unit).
2. Create the lane with its points in one call:
   - fade out the song over its last 4 bars (4/4, ending at bar 32):
     `automation.create target=masterVolume points=[{"beat":112,"value":0.75},{"beat":128,"value":0}]`;
   - a filter sweep into the drop at bar 16: put an Auto Filter or Filter on the track, then
     `automation.create target=pluginParameter trackId=Pad slot=0 parameter=Cutoff
     points=[{"beat":48,"value":300},{"beat":64,"value":12000}]`;
   - a volume dip under dialogue: four points, down 4-8 dB (about 0.6 from 0.75) and back.
3. `interpolation=step` for switches and stutters; `linear` (default) for ramps.
4. Edit later with `automation.setPoints laneId=…` (replaces all points) or
   `automation.setPoint`; `automation.list` shows the lanes.
5. A lane overrides the fader or knob while it is enabled: set the levels you want inside the
   points, and `automation.setEnabled` to compare.

## Checks

- `harness.look view=mix fromBar=… toBar=…` around the move: the short-term loudness falls or
  rises where you placed it (a fade reaches silence at its end).
- `harness.measure trackId=…` before and after a sweep section: the bands move as intended.
