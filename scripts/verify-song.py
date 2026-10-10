#!/usr/bin/env python3
"""Compose, edit, mix, save, reopen and bounce through MCP; verify with the CLI.

Default mode owns an isolated headless session. --live explicitly replaces the
session in the app discovered through RYOLUNE_CONTROL; use a dedicated QA window.
No third-party plugin is required. Optional IDs must already be scanned.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import random
import struct
import subprocess
import sys
import time
import wave


def read_wav(path):
    """(channels, bytes per sample, sample rate, PCM bytes) of a WAV file, PCM or
    WAVE_FORMAT_EXTENSIBLE (the `wave` module reads the latter only from Python 3.12; the
    Mac runner's Python is 3.9)."""
    data = Path(path).read_bytes()
    if data[:4] != b'RIFF' or data[8:12] != b'WAVE':
        raise ValueError(f'{path} is not a WAV file')
    fmt = pcm = None
    offset = 12
    while offset + 8 <= len(data):
        chunk, size = data[offset:offset + 4], struct.unpack('<I', data[offset + 4:offset + 8])[0]
        body = data[offset + 8:offset + 8 + size]
        if chunk == b'fmt ':
            fmt = body
        elif chunk == b'data':
            pcm = body
        offset += 8 + size + (size & 1)
    if fmt is None or pcm is None:
        raise ValueError(f'{path} has no fmt or data chunk')
    tag, channels, rate = struct.unpack('<HHI', fmt[:8])
    bits = struct.unpack('<H', fmt[14:16])[0]
    if tag == 0xFFFE:  # WAVE_FORMAT_EXTENSIBLE: the sub-format GUID starts with the real tag
        tag = struct.unpack('<H', fmt[24:26])[0]
    if tag != 1:
        raise ValueError(f'{path} is not integer PCM (format {tag})')
    return channels, bits // 8, rate, pcm


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bin-dir', type=Path, default=Path('target/release'))
    parser.add_argument('--output', type=Path, default=Path('artifacts/song-verification'))
    parser.add_argument('--live', action='store_true')
    parser.add_argument('--instrument', help='Installed external instrument descriptor ID')
    parser.add_argument('--effect', help='Installed external effect descriptor ID')
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    bins = args.bin_dir.resolve()
    suffix = '.exe' if os.name == 'nt' else ''
    err = (out / 'mcp.log').open('w')
    proc = subprocess.Popen([str(bins / ('ryolune-mcp' + suffix)), '--live' if args.live else '--headless'],
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=err, text=True, encoding="utf-8")
    seq = 0
    calls = []

    def rpc(method, params):
        nonlocal seq
        seq += 1
        proc.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': seq, 'method': method, 'params': params}) + '\n')
        proc.stdin.flush()
        line = proc.stdout.readline()
        if not line:
            raise RuntimeError('MCP exited; inspect ' + str(out / 'mcp.log'))
        reply = json.loads(line)
        assert reply.get('id') == seq, reply
        if 'error' in reply:
            raise RuntimeError(reply['error'])
        return reply['result']

    def call(command, **params):
        start = time.perf_counter()
        for attempt in range(100):
            result = rpc('tools/call', {'name': command.replace('.', '_', 1), 'arguments': params})
            if not result.get('isError'):
                break
            message = result['content'][0]['text']
            if any(s in message.lower() for s in ['updating', 'still loading', 'busy', 'retry']):
                time.sleep(0.05)
            else:
                raise RuntimeError(f'{command}: {message}')
        else:
            raise RuntimeError(f'{command} timed out: {message}')
        value = result.get('structuredContent')
        if value is None:
            value = json.JSONDecoder().raw_decode(result['content'][0]['text'])[0]
        calls.append({'command': command, 'milliseconds': round((time.perf_counter() - start) * 1000, 3)})
        return value

    try:
        init = rpc('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {},
                                 'clientInfo': {'name': 'ryolune song verification', 'version': '1'}})
        assert init['protocolVersion'] == '2025-06-18'
        proc.stdin.write(json.dumps({'jsonrpc': '2.0', 'method': 'notifications/initialized'}) + '\n')
        proc.stdin.flush()
        tools = rpc('tools/list', {})['tools']
        call('session.new')
        call('session.rename', name='Afterglow — integration session')
        call('transport.setTempo', bpm=100)
        call('transport.setCycle', enabled=False)
        call('transport.setMetronome', enabled=False)
        call('transport.setKey', key='A minor')
        for track in call('track.list'):
            call('track.remove', trackId=track['id'])
        tracks = {}
        for name, instrument, level, pan in [
            ('Warm keys', 'E-Piano Mk I', 0.48, -18), ('Bass', 'Sub Bass 808', 0.52, 0),
            ('Drums', 'Drum Machine', 0.57, 0), ('Lead', 'Glass Keys', 0.42, 16),
        ]:
            track = call('track.add', kind='midi', name=name, instrument=instrument)
            tracks[name] = track['id']
            call('track.setVolume', trackId=track['id'], volume=level)
            call('track.setPan', trackId=track['id'], pan=pan)
        if args.instrument:
            call('strip.setPlugin', trackId=tracks['Lead'], pluginId=args.instrument)
        chord_roots = [45, 41, 48, 43]
        chords = [[57, 60, 64, 67], [53, 57, 60, 64], [55, 60, 64, 67], [55, 59, 62, 65]]
        lead_pattern = [76, 72, 69, 67, 69, 72, 74, 71]
        clips = []
        for section in range(4):
            keys, bass, drums, melody = [], [], [], []
            for bar in range(4):
                beat = bar * 4
                keys.extend({'start': beat + j * 0.018, 'length': 3.6, 'pitch': pitch, 'velocity': 67 + j * 4}
                            for j, pitch in enumerate(chords[bar]))
                for offset, length, pitch in [(0, 1.2, chord_roots[bar] - 12), (1.5, 0.35, chord_roots[bar]),
                                               (2, 1.4, chord_roots[bar] - 12), (3.5, 0.25, chord_roots[bar] - 5)]:
                    bass.append({'start': beat + offset, 'length': length, 'pitch': pitch, 'velocity': 87})
                for offset, pitch in [(0, 36), (1.5, 36), (2, 36), (1, 38), (3, 38)]:
                    drums.append({'start': beat + offset, 'length': 0.14, 'pitch': pitch, 'velocity': 88})
                for eighth in range(8):
                    drums.append({'start': beat + eighth * 0.5 + (0.025 if eighth % 2 else 0),
                                  'length': 0.10, 'pitch': 42, 'velocity': 40 if eighth % 2 else 57})
                if section > 0:
                    for j in range(4):
                        melody.append({'start': beat + j * 0.75, 'length': 0.48,
                                       'pitch': lead_pattern[(bar * 2 + j) % 8], 'velocity': 74})
            for name, notes in [('Warm keys', keys), ('Bass', bass), ('Drums', drums), ('Lead', melody)]:
                if notes:
                    clip = call('clip.create', trackId=tracks[name], startBar=section * 4,
                                lengthBars=4, name=f'{name} {section + 1}', notes=notes)
                    clips.append(clip['id'])
        # Exercise editing and a shared undo/redo step, not only initial construction.
        call('clip.split', clipId=clips[0], bar=2)
        call('history.undo')
        call('history.redo')
        call('strip.setPlugin', trackId=tracks['Warm keys'], slot=0, pluginId='stock:Channel EQ')
        call('strip.setPlugin', trackId=tracks['Warm keys'], slot=7, pluginId='stock:Utility')
        if args.effect:
            call('strip.setPlugin', trackId=tracks['Warm keys'], slot=1, pluginId=args.effect)
        params = call('strip.parameters', trackId=tracks['Warm keys'], slot=7)['parameters']
        parameter = params[0]
        value = parameter['min'] + (parameter['max'] - parameter['min']) * 0.45
        call('strip.setParameter', trackId=tracks['Warm keys'], slot=7, parameterId=parameter['id'], value=value)
        call('history.undo')
        call('history.redo')
        call('strip.setSendLevel', trackId=tracks['Warm keys'], send=0, levelDb=-18)
        call('strip.setSendLevel', trackId=tracks['Lead'], send=1, levelDb=-20)
        call('strip.setPlugin', trackId='master', slot=0, pluginId='stock:Limiter')
        call('master.setVolume', volume=0.68)
        # The same automation data must survive editing, undo and document reload,
        # and feed the renderer used for the final mix.
        lane = call('automation.create', target='masterVolume', name='Song fade', points=[
            {'beat': 0, 'value': 0.68}, {'beat': 56, 'value': 0.68}, {'beat': 64, 'value': 0.05}
        ])['lane']
        call('automation.setPoint', laneId=lane['id'], pointId=lane['points'][-1]['id'], beat=64, value=0)
        call('history.undo')
        call('history.redo')
        assert call('automation.list')['lanes'][0]['points'][-1]['value'] == 0
        # A generated PCM shaker exercises file decoding/embedding and audio-region placement.
        shaker = out / 'shaker-source.wav'
        rng = random.Random(29)
        with wave.open(str(shaker), 'wb') as wav:
            wav.setparams((1, 2, 48000, 0, 'NONE', 'not compressed'))
            block = bytearray()
            for i in range(round(38.4 * 48000)):
                pulse = (i / 48000) % 0.3
                sample = rng.uniform(-1, 1) * math.exp(-pulse * 95) * 0.06
                block.extend(struct.pack('<h', int(sample * 32767)))
                if len(block) >= 96000:
                    wav.writeframes(block)
                    block.clear()
            wav.writeframes(block)
        call('session.importAudio', path=str(shaker), startBar=0)
        call('transport.locate', beats=0)
        if args.live:
            call('transport.play')
            time.sleep(1.25)
            playing = call('session.info')
            assert playing['transport']['playing'], playing
            assert playing['transport']['positionBeats'] > 0.5, playing
            call('transport.stop')
            call('transport.locate', beats=0)
        project, mix = out / 'Afterglow.ryolune', out / 'Afterglow.wav'
        call('session.save', path=str(project))
        before = call('session.get')
        call('session.open', path=str(project))
        after = call('session.get')
        assert before['tracks'] == after['tracks']
        assert before['clips'] == after['clips']
        assert before['automation'] == after['automation']
        # Loading assigns stable IDs to unused legacy slots. Compare every active
        # plugin, parameter and saved state exactly; empty-slot IDs do not host DSP.
        def strips(document):
            value = json.loads(json.dumps(document['strips']))
            for strip in value.values():
                for insert in strip.get('inserts', []):
                    if insert['state'] == 'empty':
                        insert['id'] = ''
            return value
        assert strips(before) == strips(after)
        call('session.bounce', path=str(mix))
        midi = out / 'Afterglow.mid'
        midi_report = call('session.exportMidi', path=str(midi))
        expected_notes = sum(len(c['data'].get('notes', [])) for c in after['clips'])
        assert midi.read_bytes()[:4] == b'MThd'
        assert midi_report['noteCount'] == expected_notes, midi_report
        imported = call('session.importMidi', path=str(midi), startBar=20)
        assert len(call('track.list')) > len(after['tracks']), imported
        call('history.undo')
        assert len(call('track.list')) == len(after['tracks'])
        export_report = call('session.exportAudio', path=str(out / 'Afterglow-range-float96.wav'),
                             sampleRate=96000, format='float32', startBeat=4, endBeat=8,
                             tailSeconds=0.25, dither=False)
        assert export_report['sampleRate'] == 96000 and export_report['format'] == 'float32'
        assert export_report['peak'] > 0.001 and export_report['clippedSamples'] == 0, export_report
        stem_dir = out / ('stems-' + str(time.time_ns()))
        stems = call('session.exportStems', directory=str(stem_dir), sampleRate=44100, format='pcm16',
                     endBeat=4, tailSeconds=0.1, includeMaster=False, includeEffects=True)
        stem_files = list(stem_dir.glob('*.wav'))
        assert len(stem_files) == len(after['tracks']), stems
        for stem in stem_files:
            assert read_wav(stem)[:3] == (2, 2, 44100), stem
        call('session.save', path=str(project))
        # A file-backed MCP host owns its project until exit. The native app keeps
        # ownership, so verify that window through --live instead of opening a
        # second writer behind its back.
        if not args.live:
            proc.stdin.close()
            proc.wait(timeout=10)
        cli_mode = ['--live'] if args.live else ['--file', str(project)]
        cli = subprocess.run([str(bins / ('ryolune-cli' + suffix)), *cli_mode, '--compact', 'session.get'],
                             capture_output=True, text=True, encoding="utf-8", check=True)
        restored = json.loads(cli.stdout)
        assert restored['clips'] == after['clips']
        validated = subprocess.run([str(bins / ('ryolune' + suffix)), '--validate', str(project)],
                                   capture_output=True, text=True, encoding="utf-8", check=True)
        channels, width, rate, pcm = read_wav(mix)
        assert (channels, width, rate) == (2, 3, 48000)
        frames = len(pcm) // (channels * width)
        samples = [int.from_bytes(pcm[i:i+3], 'little', signed=True) / 8388608 for i in range(0, len(pcm), 3)]
        peak = max(map(abs, samples))
        rms = math.sqrt(sum(x*x for x in samples) / len(samples))
        assert 0.001 < rms < 0.95 and peak <= 1, (peak, rms)
        report = {'mode': 'live' if args.live else 'headless', 'version': init['serverInfo']['version'],
                  'registeredTools': len(tools), 'tracks': len(restored['tracks']), 'clips': len(restored['clips']),
                  'notes': sum(len(c['data'].get('notes', [])) for c in restored['clips']),
                  'seconds': frames / 48000, 'sampleRate': 48000, 'bitDepth': 24,
                  'peak': peak, 'rms': rms, 'sha256': hashlib.sha256(mix.read_bytes()).hexdigest(),
                  'instrument': args.instrument or 'stock', 'effect': args.effect or 'stock',
                  'automationLanes': len(after['automation']), 'midiExport': midi_report,
                  'rangeExport': export_report, 'stems': stems,
                  'calls': calls, 'validation': validated.stdout.strip()}
        (out / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps({k: v for k, v in report.items() if k != 'calls'}, indent=2))
    finally:
        proc.stdin.close()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.terminate()
            proc.wait(timeout=10)
        err.close()


if __name__ == '__main__':
    main()
