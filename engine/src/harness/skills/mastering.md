---
name: mastering
title: Master to a loudness target
when: The person asks for a master, a loudness target ("-14 LUFS for Spotify", "club loud"), a final limiter, or to fix clipping on the output.
---
# Master to a loudness target

Targets unless the person names one: streaming -14 LUFS integrated, true peak at most -1 dBTP;
club -8 LUFS at -1 dBTP; broadcast -23 LUFS (EBU R128), true peak -1 dBTP; podcast -16 LUFS.
Tolerance: within ±1 LU of the target, true peak at or below the ceiling, zero clipped samples.

## Steps

1. Measure the whole song first: `harness.measure targetLufs=-14` (no range = the whole song;
   ranges longer than 600 s must be measured in parts). Note integrated, true peak, LRA.
2. If the mix clips or peaks above -3 dBFS before the master chain, fix the mix first (skill
   `mixing`): lower the loudest tracks; a master fader cut only hides it.
3. Build the master chain with `strip.setPlugin trackId=master plugin=… slot=…`, in order:
   - slot 0 `Channel EQ` (gentle: ±1-2 dB shelves, only if the spectrum asks for it);
   - slot 1 `ryolune Comp` for glue: Ratio 2:1, Threshold for 1-3 dB of reduction, Attack
     30 ms, Release 150 ms, Mix 100 %;
   - last slot (slot 2 or later) `Limiter`: `strip.setParameter trackId=master slot=2
     parameter=Ceiling text="-1 dB"` (use -1.5 dB for loud masters: lossy encoding adds peaks),
     Release 80-150 ms.
4. Reach the target with the Limiter's **Input** gain: set it to (target − integrated) dB as a
   first guess, for example integrated -20 LUFS → target -14 → `parameter=Input text="6 dB"`.
5. Measure again: `harness.measure targetLufs=-14`. Adjust Input by the difference that remains
   (findings say how much). Repeat until within ±1 LU, at most three passes.
6. If more than about 8 dB of Input is needed for the target, the mix is too quiet: raise the
   tracks or add Comp makeup rather than pushing the Limiter harder (it flattens the music).

## Checks

- `harness.measure targetLufs=…`: integrated within ±1 LU of the target, `truePeakDbtp` at or
  below the ceiling, `clippedSamples` 0.
- `harness.look view=mix targetLufs=…`: the short-term curve sits around the target line in
  the loudest sections; the waveform is not squared off everywhere.
- Report: target, integrated, true peak, loudness range, the chain and its settings.
