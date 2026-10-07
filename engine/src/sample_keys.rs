//! Sample Keys: the stock sampler. It plays one recorded or generated sound across the
//! keyboard, repitched from its root note, with an envelope, sustain pedal and pitch bend.
//! The sound lives in the plugin's state (`Insert.blob`), so it travels inside the song;
//! `strip.loadSample` and `generate.audio kind=instrument` write it. With no sound loaded
//! it plays a soft sine bell so a new track is never silent.

use crate::audio::AudioBuffer;
use crate::dsp::HEADROOM;
use ryolune_plugin::prelude::*;

const VOICES: usize = 24;
const BEND_SEMITONES: f64 = 2.0;
const MAGIC: &[u8; 4] = b"RYSK";
const VERSION: u8 = 1;
/// The longest sound a sampler keeps: longer ones are cut, with a short fade.
pub const MAX_SECONDS: f64 = 20.0;
const MODES: &[&str] = &["Gate", "One-shot"];

/// Parameter order; `set_param` and `strip.loadSample` use these indices.
pub const ROOT: usize = 0;
const TUNE: usize = 1;
const ATTACK: usize = 2;
const RELEASE: usize = 3;
const MODE: usize = 4;
const LEVEL: usize = 5;

/// A sound ready for the sampler: interleaved stereo at its own rate.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sample {
    pub rate: u32,
    pub frames: Vec<[f32; 2]>,
}

/// Encode a decoded buffer as sampler state: 16-bit stereo, at most `MAX_SECONDS`, with
/// leading silence trimmed so the note speaks at once and a 10 ms fade if it was cut.
pub fn encode(buffer: &AudioBuffer) -> Result<Vec<u8>, String> {
    if buffer.sample_rate == 0 || buffer.frames.is_empty() {
        return Err("The sound is empty".into());
    }
    let start = buffer
        .frames
        .iter()
        .position(|f| f[0].abs().max(f[1].abs()) > 0.002)
        .ok_or("The sound is silent")?;
    let max = (MAX_SECONDS * buffer.sample_rate as f64) as usize;
    let mut frames: Vec<[f32; 2]> = buffer.frames[start..].iter().take(max).copied().collect();
    if buffer.frames.len() - start > max {
        let fade = (buffer.sample_rate as usize / 100).min(frames.len());
        let n = frames.len();
        for (i, frame) in frames[n - fade..].iter_mut().enumerate() {
            let g = 1.0 - i as f32 / fade as f32;
            frame[0] *= g;
            frame[1] *= g;
        }
    }
    let mut out = Vec::with_capacity(13 + frames.len() * 4);
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&buffer.sample_rate.to_le_bytes());
    out.extend_from_slice(&(frames.len() as u32).to_le_bytes());
    for frame in &frames {
        for s in frame {
            out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
        }
    }
    Ok(out)
}

/// The insert blob the host stores for Sample Keys with `state` loaded: the host's native
/// state document, parameter values (defaults, the root note set) and the sound beside them.
pub fn insert_blob(state: &[u8], root_note: u8) -> String {
    let mut values: Vec<f64> = <SampleKeys as Plugin>::params()
        .iter()
        .map(|p| p.default)
        .collect();
    values[ROOT] = f64::from(root_note);
    let document = serde_json::json!({
        "values": values,
        "state": crate::host::encode_blob(state),
    });
    crate::host::encode_blob(document.to_string().as_bytes())
}

/// Read what `encode` wrote.
pub fn decode(state: &[u8]) -> Result<Sample, String> {
    if state.len() < 13 || &state[..4] != MAGIC {
        return Err("This is not a Sample Keys sound".into());
    }
    if state[4] != VERSION {
        return Err("This sound was saved by a newer ryolune".into());
    }
    let rate = u32::from_le_bytes(state[5..9].try_into().unwrap_or_default());
    let count = u32::from_le_bytes(state[9..13].try_into().unwrap_or_default()) as usize;
    let body = &state[13..];
    if !(8_000..=384_000).contains(&rate) || body.len() != count * 4 {
        return Err("The Sample Keys sound is damaged".into());
    }
    let frames = body
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| {
            [
                i16::from_le_bytes([c[0], c[1]]) as f32 / 32767.0,
                i16::from_le_bytes([c[2], c[3]]) as f32 / 32767.0,
            ]
        })
        .collect();
    Ok(Sample { rate, frames })
}

#[derive(Clone, Copy)]
struct Voice {
    pitch: u8,
    /// Read position in sample frames.
    position: f64,
    velocity: f32,
    /// Envelope level and whether the key (or the pedal) still holds it.
    level: f32,
    held: bool,
    serial: u64,
}

pub struct SampleKeys {
    rate: f64,
    sample: Sample,
    state: Vec<u8>,
    voices: [Option<Voice>; VOICES],
    sustained: [bool; VOICES],
    pedal: bool,
    bend: f64,
    serial: u64,
    root: f64,
    tune: f64,
    attack: f32,
    release: f32,
    one_shot: bool,
    gain: f32,
}

impl SampleKeys {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        self.serial += 1;
        let voice = Voice {
            pitch,
            position: 0.0,
            velocity: (velocity as f32 / 127.0).powf(1.4),
            level: 0.0,
            held: true,
            serial: self.serial,
        };
        let slot = self
            .voices
            .iter()
            .position(Option::is_none)
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .filter_map(|(i, v)| v.map(|v| (i, v)))
                    .min_by_key(|(_, v)| (v.held, v.serial))
                    .map_or(0, |(i, _)| i)
            });
        self.voices[slot] = Some(voice);
        self.sustained[slot] = false;
    }
    fn note_off(&mut self, pitch: u8) {
        for (slot, voice) in self.voices.iter_mut().enumerate() {
            if let Some(v) = voice.as_mut().filter(|v| v.pitch == pitch && v.held) {
                if self.pedal {
                    self.sustained[slot] = true;
                } else {
                    v.held = false;
                }
            }
        }
    }
    fn apply(&mut self, e: Event) {
        match e.kind {
            event::NOTE_ON if e.value > 0 => self.note_on(e.key, e.value),
            event::NOTE_ON | event::NOTE_OFF => self.note_off(e.key),
            event::CONTROL => match e.key {
                64 => {
                    self.pedal = e.value >= 64;
                    if !self.pedal {
                        for (slot, voice) in self.voices.iter_mut().enumerate() {
                            if std::mem::take(&mut self.sustained[slot]) {
                                if let Some(v) = voice {
                                    v.held = false;
                                }
                            }
                        }
                    }
                }
                120 => self.reset(),
                121 => {
                    self.bend = 0.0;
                    self.pedal = false;
                }
                123 => {
                    for v in self.voices.iter_mut().flatten() {
                        v.held = false;
                    }
                }
                _ => {}
            },
            event::PITCH_BEND => self.bend = e.bend_amount() as f64 * BEND_SEMITONES,
            _ => {}
        }
    }
    fn render(&mut self, frame: &mut [f32; 2]) {
        let attack_step = 1.0 / (self.attack * self.rate as f32).max(1.0);
        let release_step = 1.0 / (self.release * self.rate as f32).max(1.0);
        let mut out = [0.0f32; 2];
        for (slot, voice) in self.voices.iter_mut().enumerate() {
            let Some(v) = voice else { continue };
            let semitones = v.pitch as f64 - self.root + self.tune / 100.0 + self.bend;
            let ratio = 2f64.powf(semitones / 12.0);
            let gate = v.held || self.one_shot;
            v.level = if gate {
                (v.level + attack_step).min(1.0)
            } else {
                (v.level - release_step).max(0.0)
            };
            let (l, r, done) = if self.sample.frames.is_empty() {
                // The empty sampler: a soft sine bell at the note's pitch.
                let hz = 440.0 * 2f64.powf((v.pitch as f64 - 69.0 + self.bend) / 12.0);
                let t = v.position / self.rate;
                let s = ((t * hz * std::f64::consts::TAU).sin() * (-t * 3.0).exp()) as f32;
                v.position += 1.0;
                (s, s, t > 4.0)
            } else {
                let frames = &self.sample.frames;
                let i = v.position as usize;
                let (l, r) = hermite(frames, i, (v.position - i as f64) as f32);
                v.position += ratio * self.sample.rate as f64 / self.rate;
                (l, r, v.position as usize + 2 >= frames.len())
            };
            let g = v.level * v.velocity;
            out[0] += l * g;
            out[1] += r * g;
            if done || (!gate && v.level <= 0.0) {
                *voice = None;
                self.sustained[slot] = false;
            }
        }
        let gain = self.gain * HEADROOM;
        for (o, s) in frame.iter_mut().zip(out) {
            let s = s * gain;
            *o += if s.is_finite() { s } else { 0.0 };
        }
    }
}

/// Four-point Hermite interpolation between frame `i` and `i + 1`.
fn hermite(frames: &[[f32; 2]], i: usize, t: f32) -> (f32, f32) {
    let at = |k: isize| {
        let k = (i as isize + k).clamp(0, frames.len() as isize - 1) as usize;
        frames[k]
    };
    let (a, b, c, d) = (at(-1), at(0), at(1), at(2));
    let one = |ch: usize| {
        let c0 = b[ch];
        let c1 = 0.5 * (c[ch] - a[ch]);
        let c2 = a[ch] - 2.5 * b[ch] + 2.0 * c[ch] - 0.5 * d[ch];
        let c3 = 0.5 * (d[ch] - a[ch]) + 1.5 * (b[ch] - c[ch]);
        ((c3 * t + c2) * t + c1) * t + c0
    };
    (one(0), one(1))
}

impl Plugin for SampleKeys {
    const INFO: Info = Info::instrument("org.ryolune.stock.samplekeys", "Sample Keys", "ryolune")
        .describe(
            "Plays one sound across the keyboard: a recording, a file or a generated sample.",
        );
    fn params() -> Vec<ParamSpec> {
        vec![
            param("Root", 0.0, 127.0, 60.0, "note"),
            param("Tune", -100.0, 100.0, 0.0, "ct"),
            param("Attack", 0.0, 2000.0, 2.0, "ms"),
            param("Release", 5.0, 5000.0, 250.0, "ms"),
            choice("Mode", MODES, 0),
            param("Level", -24.0, 6.0, 0.0, "dB"),
        ]
    }
    fn new(rate: f64) -> Self {
        Self {
            rate,
            sample: Sample::default(),
            state: Vec::new(),
            voices: [None; VOICES],
            sustained: [false; VOICES],
            pedal: false,
            bend: 0.0,
            serial: 0,
            root: 60.0,
            tune: 0.0,
            attack: 0.002,
            release: 0.25,
            one_shot: false,
            gain: 1.0,
        }
    }
    fn set_param(&mut self, index: usize, value: f64) {
        match index {
            ROOT => self.root = value.round(),
            TUNE => self.tune = value,
            ATTACK => self.attack = (value / 1000.0) as f32,
            RELEASE => self.release = (value / 1000.0) as f32,
            MODE => self.one_shot = value.round() as usize == 1,
            LEVEL => self.gain = db_to_gain(value),
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.voices = [None; VOICES];
        self.sustained = [false; VOICES];
        self.pedal = false;
        self.bend = 0.0;
    }
    fn process(&mut self, audio: &mut [[f32; 2]], notes: &[NoteEvent], _: &ProcessContext) {
        let mut next = 0;
        for (i, frame) in audio.iter_mut().enumerate() {
            while let Some(note) = notes.get(next).filter(|n| n.frame as usize <= i) {
                if note.on && note.velocity > 0 {
                    self.note_on(note.pitch, note.velocity);
                } else {
                    self.note_off(note.pitch);
                }
                next += 1;
            }
            self.render(frame);
        }
    }
    fn process_events(
        &mut self,
        audio: &mut [[f32; 2]],
        events: &[Event],
        params: &[TimedParam],
        _: &ProcessContext,
    ) {
        let (mut next, mut next_param) = (0, 0);
        for (i, frame) in audio.iter_mut().enumerate() {
            while let Some(change) = params.get(next_param).filter(|c| c.frame as usize <= i) {
                if change.value.is_finite() {
                    self.set_param(change.index as usize, change.value);
                }
                next_param += 1;
            }
            while let Some(event) = events.get(next).filter(|e| e.frame as usize <= i) {
                self.apply(*event);
                next += 1;
            }
            self.render(frame);
        }
        for change in &params[next_param..] {
            if change.value.is_finite() {
                self.set_param(change.index as usize, change.value);
            }
        }
        for event in &events[next..] {
            self.apply(*event);
        }
    }
    fn save(&self) -> Vec<u8> {
        self.state.clone()
    }
    fn load(&mut self, state: &[u8]) -> Result<(), String> {
        if state.is_empty() {
            self.sample = Sample::default();
            self.state.clear();
            return Ok(());
        }
        self.sample = decode(state)?;
        self.state = state.to_vec();
        Ok(())
    }
    fn tail_seconds(&self) -> f64 {
        self.release as f64 + 0.05
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(seconds: f64, rate: u32) -> AudioBuffer {
        let n = (seconds * rate as f64) as usize;
        AudioBuffer {
            sample_rate: rate,
            frames: (0..n)
                .map(|i| {
                    let s = (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5;
                    [s, s]
                })
                .collect(),
            peaks: vec![],
        }
    }

    #[test]
    fn a_sound_survives_its_state_and_long_ones_are_cut() {
        let state = encode(&buffer(1.0, 44_100)).unwrap();
        let sample = decode(&state).unwrap();
        assert_eq!(sample.rate, 44_100);
        assert!(sample.frames.len() > 44_000);
        let long = decode(&encode(&buffer(30.0, 8_000)).unwrap()).unwrap();
        assert_eq!(long.frames.len(), (MAX_SECONDS * 8_000.0) as usize);
        assert!(decode(b"nope").is_err());
        assert!(encode(&buffer(0.0, 44_100)).is_err());
    }

    #[test]
    fn a_note_an_octave_up_reads_the_sound_twice_as_fast_and_releases() {
        let mut keys = SampleKeys::new(48_000.0);
        keys.load(&encode(&buffer(2.0, 48_000)).unwrap()).unwrap();
        let mut audio = vec![[0.0f32; 2]; 480];
        keys.process_events(
            &mut audio,
            &[Event::note_on(0, 72, 100)],
            &[],
            &Default::default(),
        );
        let voice = keys.voices.iter().flatten().next().copied().unwrap();
        assert!((voice.position - 960.0).abs() < 1.0, "{}", voice.position);
        assert!(audio.iter().any(|f| f[0].abs() > 0.01));
        keys.process_events(
            &mut audio,
            &[Event::note_off(0, 72)],
            &[],
            &Default::default(),
        );
        let mut tail = vec![[0.0f32; 2]; 48_000];
        keys.process_events(&mut tail, &[], &[], &Default::default());
        assert!(keys.voices.iter().all(Option::is_none));
    }

    #[test]
    fn an_empty_sampler_still_sounds() {
        let mut keys = SampleKeys::new(48_000.0);
        let mut audio = vec![[0.0f32; 2]; 4_800];
        keys.process_events(
            &mut audio,
            &[Event::note_on(0, 60, 100)],
            &[],
            &Default::default(),
        );
        assert!(audio.iter().any(|f| f[0].abs() > 0.01));
    }
}
