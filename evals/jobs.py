"""The scripted music jobs. Each starts from a fixture, gives the agent one request, and is
scored by automatic checks on the song it leaves (and the files it writes)."""
import statistics
import wave
from pathlib import Path


def check(name):
    def wrap(fn):
        fn.check_name = name
        return fn
    return wrap


def ok(condition, detail=""):
    return bool(condition), detail


# Checks shared by most jobs -------------------------------------------------------------

@check("not silent and no clipping")
def clean_render(song, ctx):
    l = song.loudness()
    return ok(l["integratedLufs"] is not None and l["clippedSamples"] == 0,
              f"{l['integratedLufs']} LUFS, {l['clippedSamples']} clipped")


@check("looked or measured before finishing (finish routine)")
def finished_with_eyes(song, ctx):
    used = [t for t in ctx["tools"] if t.split("__")[-1] in ("harness_look", "harness_measure")]
    return ok(used, f"{len(used)} looks/measures")


def tempo_is(bpm, tolerance=0.5):
    @check(f"tempo {bpm} BPM")
    def fn(song, ctx):
        return ok(abs(song.tempo - bpm) <= tolerance, f"{song.tempo}")
    return fn


def tempo_between(low, high):
    @check(f"tempo {low}-{high} BPM")
    def fn(song, ctx):
        return ok(low <= song.tempo <= high, f"{song.tempo}")
    return fn


def key_is(root, scale):
    @check(f"key label {root} {scale}")
    def fn(song, ctx):
        k = song.key.lower().replace("♭", "b").replace("♯", "#")
        return ok(k.startswith(root.lower()) and (scale in k or (scale == "minor" and "m" in k[len(root):])), song.key)
    return fn


def notes_in_key(root, scale, share=0.9):
    @check(f"pitched notes in {root} {scale} (>= {int(share * 100)}%)")
    def fn(song, ctx):
        r = song.in_key(root, scale)
        return ok(r >= share, f"{r:.0%}")
    return fn


def at_least_bars(bars):
    @check(f"song at least {bars} bars")
    def fn(song, ctx):
        return ok(song.end_bar() >= bars - 0.01, f"{song.end_bar()}")
    return fn


def tracks_preserved(song, ctx):
    before = {t["id"] for t in ctx["before"].tracks}
    after = {t["id"] for t in song.tracks}
    return ok(before <= after, f"missing {sorted(before - after)}")
tracks_preserved = check("the person's tracks are kept")(tracks_preserved)


def music_preserved(song, ctx):
    before = sorted(n[2] for n in ctx["before"].notes())
    after = sorted(n[2] for n in song.notes())
    return ok(before == after, f"{len(before)} notes before, {len(after)} after")
music_preserved = check("the notes are unchanged")(music_preserved)


# The jobs -------------------------------------------------------------------------------

def _drums_house():
    @check("kick on every beat of 4 bars")
    def kicks(song, ctx):
        bpb = song.beats_per_bar
        beats = {round(n[0], 2) for n in song.notes() if n[2] in (35, 36) and n[0] < 4 * bpb}
        return ok(all(float(b) in beats for b in range(16)), f"{len(beats)} kick onsets")

    @check("hats and a snare or clap")
    def kit(song, ctx):
        pitches = {n[2] for n in song.notes()}
        return ok(pitches & {42, 44, 46} and pitches & {38, 39, 40}, f"{sorted(pitches)}")

    @check("on a Drum Machine")
    def machine(song, ctx):
        return ok(any(song.instrument(t) == "Drum Machine" and song.notes(t) for t in song.tracks))
    return [tempo_is(124), kicks, kit, machine, clean_render, finished_with_eyes]


def _bass_chords():
    @check("a bass line under chords")
    def layers(song, ctx):
        tracks = song.midi_tracks_with_notes()
        if len(tracks) < 2:
            return ok(False, f"{len(tracks)} tracks with notes")
        medians = sorted(statistics.median(n[2] for n in song.notes(t)) for t in tracks)
        return ok(medians[0] < 48 and medians[-1] >= 48, f"medians {medians}")

    @check("chords: 3+ notes together")
    def chords(song, ctx):
        onsets = {}
        for n in song.notes():
            onsets.setdefault((n[4], round(n[0], 2)), set()).add(n[2])
        return ok(sum(1 for v in onsets.values() if len(v) >= 3) >= 4)
    return [tempo_is(90), key_is("A", "minor"), notes_in_key("A", "minor"), at_least_bars(8),
            layers, chords, clean_render, finished_with_eyes]


def _compose_lofi():
    @check("drums, bass and keys (3+ tracks with notes)")
    def parts(song, ctx):
        return ok(len(song.midi_tracks_with_notes()) >= 3, f"{len(song.midi_tracks_with_notes())}")

    @check("sections marked (2+ markers)")
    def sections(song, ctx):
        return ok(len(song.markers) >= 2, f"{[m['name'] for m in song.markers]}")
    return [tempo_between(70, 92), key_is("D", "minor"), notes_in_key("D", "minor", 0.85),
            at_least_bars(16), parts, sections, clean_render, finished_with_eyes]


def _arrange():
    @check("4+ named sections")
    def sections(song, ctx):
        names = [m["name"].lower() for m in song.markers]
        return ok(len(names) >= 4 and any("chorus" in n for n in names) and any("intro" in n for n in names), f"{names}")

    @check("density changes between sections")
    def contrast(song, ctx):
        bars = int(song.end_bar())
        counts = [len(song.notes(from_bar=b, to_bar=b + 4)) for b in range(0, bars, 4)]
        return ok(len(set(counts)) >= 3, f"notes per 4 bars {counts}")
    return [at_least_bars(32), sections, contrast, tracks_preserved, clean_render, finished_with_eyes]


def _fix_clipping():
    @check("no clipping and peak under -1 dBFS")
    def peak(song, ctx):
        l = song.loudness()
        return ok(l["clippedSamples"] == 0 and l["samplePeakDbfs"] is not None and l["samplePeakDbfs"] <= -1.0,
                  f"peak {l['samplePeakDbfs']} dBFS, {l['clippedSamples']} clipped")

    @check("still loud enough (> -24 LUFS)")
    def loud(song, ctx):
        i = song.loudness()["integratedLufs"]
        return ok(i is not None and i > -24, f"{i}")
    return [peak, loud, music_preserved, tracks_preserved, finished_with_eyes]


def _master(target, ceiling):
    @check(f"integrated {target} LUFS ±1.5")
    def integrated(song, ctx):
        i = song.loudness(targetLufs=target)["integratedLufs"]
        return ok(i is not None and abs(i - target) <= 1.5, f"{i}")

    @check(f"true peak <= {ceiling + 0.2} dBTP, no clipping")
    def true_peak(song, ctx):
        l = song.loudness(targetLufs=target)
        return ok(l["truePeakDbtp"] is not None and l["truePeakDbtp"] <= ceiling + 0.2 and l["clippedSamples"] == 0,
                  f"{l['truePeakDbtp']} dBTP")

    @check("a Limiter on the master")
    def limiter(song, ctx):
        return ok(any("Limiter" in n for n in song.master_inserts()), f"{song.master_inserts()}")
    return [integrated, true_peak, limiter, music_preserved, finished_with_eyes]


def _darker_pad():
    @check("the pad has less top end")
    def darker(song, ctx):
        before = ctx["before"].measure(trackId="Pad")["spectrum"]["bandsDb"]
        after = song.measure(trackId="Pad")["spectrum"]["bandsDb"]
        b = (before["highMids"] or -99) + (before["air"] or -99)
        a = (after["highMids"] or -99) + (after["air"] or -99)
        return ok(a < b - 3, f"highMids+air {b:.1f} -> {a:.1f} dB")

    @check("the pad keeps its notes and is audible")
    def kept(song, ctx):
        pad = song.track("Pad")
        i = song.loudness(trackId="Pad")["integratedLufs"] if pad else None
        return ok(pad and len(song.notes(pad)) == len(ctx["before"].notes(ctx["before"].track("Pad"))) and i is not None and i > -40, f"{i}")
    return [darker, kept, clean_render, finished_with_eyes]


def _melody():
    @check("a new lead track with 8+ notes over the chords")
    def lead(song, ctx):
        old = {t["id"] for t in ctx["before"].tracks}
        new = [t for t in song.tracks if t["id"] not in old and song.notes(t)]
        if not new:
            return ok(False, "no new track with notes")
        lead_median = statistics.median(n[2] for t in new for n in song.notes(t))
        keys = song.track("Keys")
        keys_median = statistics.median(n[2] for n in song.notes(keys))
        count = sum(len(song.notes(t)) for t in new)
        return ok(count >= 8 and lead_median > keys_median, f"{count} notes, median {lead_median} vs keys {keys_median}")
    return [notes_in_key("C", "major", 0.9), lead, tracks_preserved, clean_render, finished_with_eyes]


def _score_cut():
    @check("the cut is placed with its 3 scene markers")
    def placed(song, ctx):
        audio = [t for t in song.tracks if t["kind"] == "audio" and any(c["trackId"] == t["id"] for c in song.clips)]
        return ok(audio and len(song.markers) >= 3, f"{len(audio)} audio tracks with clips, {len(song.markers)} markers")

    @check("2+ music tracks covering the cut")
    def music(song, ctx):
        tracks = song.midi_tracks_with_notes()
        last = max([n[0] + n[1] for n in song.notes()] or [0])
        seconds = song.doc  # length check by beats at the song tempo
        covered = last * 60 / song.tempo
        return ok(len(tracks) >= 2 and covered >= 20, f"{len(tracks)} tracks, notes until {covered:.1f}s")

    @check("music changes at the scene markers")
    def follows(song, ctx):
        bpb = song.beats_per_bar
        bars = sorted(m["bar"] for m in song.markers)[1:3]
        starts = {round(c["startBar"], 1) for c in song.clips}
        hits = [b for b in bars if any(abs(b - s) <= 0.5 for s in starts)]
        return ok(len(hits) >= 1, f"marker bars {bars}, clip starts {sorted(starts)[:12]}")
    return [placed, music, follows, clean_render, finished_with_eyes]


def _export():
    @check("a 44.1 kHz 16-bit WAV mix")
    def mix(song, ctx):
        path = Path(ctx["work"]) / "out" / "mix.wav"
        if not path.exists():
            return ok(False, "no mix.wav")
        with wave.open(str(path)) as w:
            return ok(w.getframerate() == 44100 and w.getsampwidth() == 2 and w.getnframes() > 44100, f"{w.getframerate()} Hz {8 * w.getsampwidth()}-bit")

    @check("one stem per track")
    def stems(song, ctx):
        folder = Path(ctx["work"]) / "out" / "stems"
        files = list(folder.glob("*")) if folder.exists() else []
        return ok(len(files) >= len([t for t in song.tracks if t["kind"] != "bus"]), f"{len(files)} files")
    # Files are what this job makes, and every export measures what it wrote (peak, clipped
    # samples, warnings): the skill's checks are reading those. The demo's mix clips, so a
    # checked result reports it, or the agent measured before exporting.
    @check("checked the files before finishing (finish routine)")
    def checked(song, ctx):
        reply = ctx.get("reply", "").lower()
        reported = "clip" in reply and ("dbfs" in reply or "peak" in reply)
        measured = finished_with_eyes(song, ctx)[0]
        return ok(measured or reported, f"measured {measured}, reported peak/clipping {reported}")
    return [mix, stems, music_preserved, checked]


def _ritardando():
    @check("tempo glides down to 70 BPM near the end")
    def rit(song, ctx):
        changes = song.doc.get("tempoChanges", [])
        end = song.end_bar()
        good = [c for c in changes if abs(c["bpm"] - 70) <= 1 and c["bar"] >= end - 6]
        return ok(good, f"{changes}")
    return [rit, music_preserved, tracks_preserved, finished_with_eyes]


def _fade_out():
    @check("the last bars fade to silence")
    def fade(song, ctx):
        bpb = song.beats_per_bar
        end = song.end_bar()
        lanes = song.doc.get("automation", [])
        ends_silent = any(l.get("points") and l["points"][-1]["value"] <= 0.02 and l["points"][-1]["beat"] >= (end - 1) * bpb - 0.01
                          for l in lanes)
        tail = song.loudness(fromBar=max(0, end - 1), toBar=end)["integratedLufs"]
        body = song.loudness(fromBar=max(0, end - 8), toBar=max(1, end - 4))["integratedLufs"]
        quieter = tail is None or (body is not None and tail < body - 6)
        return ok(ends_silent and quieter, f"automation ends silent: {ends_silent}, last bar {tail} vs body {body} LUFS")
    return [fade, music_preserved, finished_with_eyes]


JOBS = [
    {"id": "drums-house", "fixture": "empty", "skill": "drum-programming",
     "prompt": "Program a 4-bar four-on-the-floor house beat at 124 BPM on the Drums track: kick on every beat, clap on 2 and 4, off-beat hats.",
     "checks": _drums_house()},
    {"id": "bass-chords", "fixture": "empty", "skill": "bass-and-chords",
     "prompt": "Write an 8-bar chord progression on a keys track and a bass line that follows it, in A minor at 90 BPM.",
     "checks": _bass_chords()},
    {"id": "compose-lofi", "fixture": "empty", "skill": "compose-from-brief",
     "prompt": "Compose a 16-bar lo-fi hip hop beat in D minor, around 80 BPM, with drums, bass and keys, an intro and a main section.",
     "checks": _compose_lofi()},
    {"id": "arrange-demo", "fixture": "demo", "skill": "arrangement",
     "prompt": "Turn this loop into a full song of at least 32 bars with an intro, a verse, a chorus and an outro, marked on the ruler. Keep my tracks.",
     "checks": _arrange()},
    {"id": "fix-clipping", "fixture": "hot_mix", "skill": "mixing",
     "prompt": "This mix clips. Fix it and balance the levels without changing the music.",
     "checks": _fix_clipping()},
    {"id": "master-streaming", "fixture": "quiet_mix", "skill": "mastering",
     "prompt": "Master this song for streaming: -14 LUFS integrated, true peak at most -1 dBTP.",
     "checks": _master(-14.0, -1.0)},
    {"id": "master-broadcast", "fixture": "quiet_mix", "skill": "mastering",
     "prompt": "Prepare this for broadcast to EBU R128: -23 LUFS integrated, true peak at most -1 dBTP.",
     "checks": _master(-23.0, -1.0)},
    {"id": "darker-pad", "fixture": "pad_song", "skill": "sound-design",
     "prompt": "The pad is too bright and harsh. Make it darker and warmer, same notes.",
     "checks": _darker_pad()},
    {"id": "melody", "fixture": "chords_c", "skill": "melody-and-hooks",
     "prompt": "Add a catchy lead melody over my chords on a new track, for the 4 bars.",
     "checks": _melody()},
    {"id": "score-cut", "fixture": "kimchi_cut", "skill": "score-to-picture",
     "prompt": "kimchi sent a cut to score. Write music for it that changes at each scene.",
     "checks": _score_cut()},
    {"id": "export-stems", "fixture": "demo", "skill": "stems-and-export",
     "prompt": "Export a 44.1 kHz 16-bit WAV mix to {work}/out/mix.wav and one stem per track into the folder {work}/out/stems.",
     "checks": _export()},
    {"id": "ritardando", "fixture": "demo", "skill": "arrangement",
     "prompt": "Make the song slow down gradually to 70 BPM over its last 4 bars.",
     "checks": _ritardando()},
    {"id": "fade-out", "fixture": "demo", "skill": "automation-and-movement",
     "prompt": "Fade the song out over its last 4 bars.",
     "checks": _fade_out()},
]
