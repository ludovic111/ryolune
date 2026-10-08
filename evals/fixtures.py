"""Starting songs for the evals, built with ryolune-cli so they follow the current format."""
import json
import math
import struct
import wave
from pathlib import Path

from song import Cli


def chord(start, length, pitches, velocity=80):
    return [{"start": start, "length": length, "pitch": p, "velocity": velocity} for p in pitches]


def empty(cli: Cli, path: Path, work: Path):
    cli.run(path, "session.new")


def demo(cli: Cli, path: Path, work: Path):
    cli.run(path, "session.new", {"demo": True})


def chords_c(cli: Cli, path: Path, work: Path):
    """C major, 100 BPM: a 4-bar progression on keys and a bass line; the lead is missing."""
    notes = (chord(0, 4, [48, 52, 55, 60]) + chord(4, 4, [45, 48, 52, 57]) +
             chord(8, 4, [41, 45, 48, 53]) + chord(12, 4, [43, 47, 50, 55]))
    bass = [{"start": b * 4 + s, "length": 1.5, "pitch": p, "velocity": 96}
            for b, p in enumerate([36, 33, 29, 31]) for s in (0, 2)]
    cli.run(path, "session.new")
    cli.batch(path, [
        {"command": "transport.setTempo", "params": {"bpm": 100}},
        {"command": "transport.setKey", "params": {"key": "C major"}},
        {"command": "track.add", "params": {"kind": "midi", "name": "Keys", "instrument": "E-Piano Mk I"}},
        {"command": "clip.create", "params": {"trackId": "Keys", "startBar": 0, "lengthBars": 4, "name": "Chords", "notes": notes}},
        {"command": "clip.create", "params": {"trackId": "Bass", "startBar": 0, "lengthBars": 4, "name": "Bass line", "notes": bass}},
    ])


def hot_mix(cli: Cli, path: Path, work: Path):
    """The demo with every fader and the master pushed to +6 dB: it clips."""
    demo(cli, path, work)
    song = cli.run(path, "session.get")
    lines = [{"command": "track.setVolume", "params": {"trackId": t["id"], "volume": 1.0}}
             for t in song["tracks"]]
    lines.append({"command": "master.setVolume", "params": {"volume": 1.0}})
    cli.batch(path, lines)
    measured = cli.run(path, "harness.measure")
    assert measured["loudness"]["clippedSamples"] > 0, "the hot mix fixture must clip"


def quiet_mix(cli: Cli, path: Path, work: Path):
    """The demo, balanced but far under any delivery loudness, with no master chain."""
    demo(cli, path, work)
    cli.run(path, "master.setVolume", {"volume": 0.5})


def pad_song(cli: Cli, path: Path, work: Path):
    """A bright pad over 8 bars (A minor, 84 BPM)."""
    notes = []
    for bar, pitches in enumerate([[57, 60, 64, 69], [53, 57, 60, 65], [48, 52, 55, 60], [55, 59, 62, 67]] * 2):
        notes += chord(bar * 4, 4, pitches, 90)
    cli.run(path, "session.new")
    cli.batch(path, [
        {"command": "transport.setTempo", "params": {"bpm": 84}},
        {"command": "transport.setKey", "params": {"key": "A minor"}},
        {"command": "track.add", "params": {"kind": "midi", "name": "Pad", "instrument": "ryolune Synth"}},
        {"command": "clip.create", "params": {"trackId": "Pad", "startBar": 0, "lengthBars": 8, "name": "Pad", "notes": notes}},
        {"command": "strip.setParameter", "params": {"trackId": "Pad", "parameter": "Cutoff", "normalized": 0.95}},
    ])


def kimchi_cut(cli: Cli, path: Path, work: Path):
    """An empty song and a 24-second cut from kimchi waiting in the inbox, with three scenes."""
    cli.run(path, "session.new")
    audio = work / "cut.wav"
    rate = 48000
    with wave.open(str(audio), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        frames = bytearray()
        for i in range(rate * 24):
            # Quiet room tone with a soft tone: dialogue stand-in.
            v = 0.05 * math.sin(2 * math.pi * 220 * i / rate) * (0.5 + 0.5 * math.sin(i / rate))
            frames += struct.pack("<h", int(v * 32767))
        w.writeframes(bytes(frames))
    inbox = Path(cli.env["LSUITE_HOME"]) / "handoff" / "ryolune"
    inbox.mkdir(parents=True, exist_ok=True)
    (inbox / "Teaser.kimchi-cut.json").write_text(json.dumps({
        "format": 1, "from": "kimchi", "project": "Teaser", "audio": str(audio),
        "seconds": 24.0, "durationSeconds": 24.0, "range": {"from": 0.0, "to": 24.0}, "fps": 30.0,
        "markers": [{"time": 0.0, "label": "Opening"}, {"time": 8.0, "label": "Chase"},
                    {"time": 16.0, "label": "Reveal"}],
    }))


FIXTURES = {f.__name__: f for f in [empty, demo, chords_c, hot_mix, quiet_mix, pad_song, kimchi_cut]}
