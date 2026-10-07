---
name: compose-from-brief
title: Compose a track from a brief
when: The person describes a piece to make ("a lo-fi beat in A minor", "90 seconds of tense synthwave") and the song is empty or they want a new idea.
---
# Compose a track from a brief

## Plan (before any edit)

Write down, for yourself: style, tempo, key and mode, meter, form with bar counts, and the parts
(drums, bass, harmony, lead or hook, texture). Defaults when the brief says nothing: 4/4, a
tempo typical of the style (lo-fi 75-90, house 120-126, hip hop 85-95, drum and bass 170-175,
ballad 60-75, pop 95-120), a key that suits it, and a form in multiples of 4 or 8 bars, for
example Intro 4 · A 8 · B 8 · A 8 · Outro 4.

## Steps

1. `harness.checkpoint label="Compose: <style>"` when you are not the built-in agent.
2. `session.overview`. If the song already has tracks, keep them: put your parts on new tracks
   and do not overwrite clips unless asked.
3. `transport.setTempo bpm=…`, `transport.setKey key="A minor"`, and
   `transport.setTimeSignature` if not 4/4.
4. Sections: `marker.add name="Intro" bar=0`, `marker.add name="Verse" bar=4`, … one per section.
5. Tracks, one per part, with fitting stock instruments: `track.add kind=midi name=Drums
   instrument="Drum Machine"`, `Bass` (Analog Bass or Sub Bass 808), `Keys` (E-Piano Mk I,
   Glass Keys, Tonewheel Organ), `Pad` (Choir Pad, String Ensemble), `Lead` (ryolune Synth).
   The empty session already has Drums, Bass and Vocals tracks: reuse Drums and Bass.
6. Write each part as whole clips (load `drum-programming` and `bass-and-chords` and
   `melody-and-hooks` for the craft): one `clip.create` per section and part with the full
   `notes` array, or write a 4- or 8-bar clip and `clip.repeat` it, then vary the copies with
   `clip.setNotes`. Use `session.batch` to land many clips as one undo step.
7. Arrange (skill `arrangement`): not every part plays everywhere. Thin the intro, build into
   the chorus, drop parts for a break, end on a resolved chord.
8. Space and balance: `strip.setSendLevel send=0 levelDb=-14` on keys and pads (reverb),
   faders by role (`track.setVolume`, 0.75 is 0 dB): drums and bass near 0 dB, pads -6 to -10 dB.

## Checks

- `harness.look view=notes` over the whole song: every section has its parts, registers do not
  collide, notes stay inside their clips, nothing is left empty.
- `harness.measure`: not silent, no clipping, true peak under 0 dBTP. If it clips, lower the
  loudest faders (`harness.measure tracks=true` names them) before anything else.
- `session.overview`: no `problems`; tempo, key and length as planned.
- Report: tempo, key, form with bars, the parts and their instruments, the loudness.
