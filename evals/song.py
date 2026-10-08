"""Reading a .ryolune song through ryolune-cli for the evals' checks."""
import json
import subprocess
from pathlib import Path

SCALES = {
    "major": [0, 2, 4, 5, 7, 9, 11],
    "minor": [0, 2, 3, 5, 7, 8, 10],
}
NOTE = {"c": 0, "c#": 1, "db": 1, "d": 2, "d#": 3, "eb": 3, "e": 4, "f": 5, "f#": 6, "gb": 6,
        "g": 7, "g#": 8, "ab": 8, "a": 9, "a#": 10, "bb": 10, "b": 11}


class Cli:
    def __init__(self, cli: Path, env: dict):
        self.cli, self.env = cli, env

    def run(self, path: Path, command: str, params: dict | None = None, timeout=600):
        args = [str(self.cli), "--file", str(path), "--compact", command]
        if params:
            args += ["--params", json.dumps(params)]
        out = subprocess.run(args, env=self.env, capture_output=True, text=True, timeout=timeout)
        if out.returncode != 0:
            raise RuntimeError(f"{command} failed: {out.stderr.strip()}")
        return json.loads(out.stdout)

    def batch(self, path: Path, lines: list[dict]):
        text = "\n".join(json.dumps(l) for l in lines) + "\n"
        out = subprocess.run([str(self.cli), "--file", str(path), "--compact", "batch"], input=text,
                             env=self.env, capture_output=True, text=True, timeout=600)
        if out.returncode != 0:
            raise RuntimeError(f"batch failed: {out.stdout[-2000:]} {out.stderr[-2000:]}")
        return [json.loads(l) for l in out.stdout.splitlines() if l.strip()]


class Song:
    """The session document with helpers for checks."""

    def __init__(self, cli: Cli, path: Path):
        self.cli, self.path = cli, path
        self.doc = cli.run(path, "session.get")
        self._measures = {}

    # Song-level facts.
    @property
    def tempo(self):
        return self.doc["transport"]["tempo"]

    @property
    def key(self):
        return self.doc["transport"].get("key", "")

    @property
    def beats_per_bar(self):
        ts = self.doc["transport"]["timeSignature"]
        return ts["numerator"] * 4 / ts["denominator"]

    @property
    def tracks(self):
        return self.doc["tracks"]

    @property
    def clips(self):
        return self.doc["clips"]

    @property
    def markers(self):
        return self.doc.get("markers", [])

    def end_bar(self):
        return max([1.0] + [c["startBar"] + c["lengthBars"] for c in self.clips])

    def track(self, name_or_id):
        for t in self.tracks:
            if t["id"] == name_or_id or t["name"].lower() == str(name_or_id).lower():
                return t
        return None

    def instrument(self, track):
        strip = self.doc.get("strips", {}).get(track["id"], {})
        synth = strip.get("synth")
        return synth["name"] if synth else strip.get("instrument", "ryolune Synth")

    def notes(self, track=None, from_bar=None, to_bar=None):
        """Every sounding note as (absolute beat, length, pitch, velocity, track id)."""
        bpb = self.beats_per_bar
        out = []
        for c in self.clips:
            data = c["data"]
            if data.get("kind") != "midi":
                continue
            if track is not None and c["trackId"] != track["id"]:
                continue
            start, end = c["startBar"] * bpb, (c["startBar"] + c["lengthBars"]) * bpb
            for n in data.get("notes", []):
                at = start + n["start"]
                if at >= end:
                    continue
                if from_bar is not None and at < from_bar * bpb:
                    continue
                if to_bar is not None and at >= to_bar * bpb:
                    continue
                out.append((at, n["length"], n["pitch"], n.get("velocity", 100), c["trackId"]))
        return out

    def midi_tracks_with_notes(self):
        return [t for t in self.tracks if self.notes(t)]

    def in_key(self, root_name, scale, track=None):
        pcs = SCALES[scale]
        root = NOTE[root_name.lower()]
        notes = [n for n in self.notes(track) if self.instrument(self.track(n[4])) != "Drum Machine"]
        if not notes:
            return 0.0
        inside = sum(1 for n in notes if (n[2] - root) % 12 in pcs)
        return inside / len(notes)

    def master_inserts(self):
        return [i.get("name", "") for i in self.doc.get("strips", {}).get("master", {}).get("inserts", []) if i.get("name")]

    def measure(self, **params):
        key = json.dumps(params, sort_keys=True)
        if key not in self._measures:
            self._measures[key] = self.cli.run(self.path, "harness.measure", params)
        return self._measures[key]

    def loudness(self, **params):
        return self.measure(**params)["loudness"]
