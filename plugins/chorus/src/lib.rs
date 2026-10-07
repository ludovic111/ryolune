//! Simple Chorus: an example ryolune plugin written on the SDK (`ryolune-plugin`). Two
//! voices read a short delay line at times that a slow sine sweeps, one per side a quarter
//! turn apart, for width. It shows a delay line made once in `new` (never in `process`), a
//! latency of zero and a tail the host renders after the last note.

use ryolune_plugin::{export_plugins, prelude::*};
use std::f64::consts::TAU;

pub struct Chorus {
    rate: f64,
    line: Delay,
    phase: f64,
    speed: f64,
    depth_ms: f64,
    delay_ms: f64,
    mix: Smoother,
    feedback: f32,
}

impl Plugin for Chorus {
    const INFO: Info = Info::effect(
        "org.ryolune.examples.chorus",
        "Simple Chorus",
        "ryolune Examples",
        "Modulation",
    )
    .describe("Two modulated voices, spread left and right, for width and shimmer.");
    fn params() -> Vec<ParamSpec> {
        vec![
            hz("Rate", 0.05, 5.0, 0.8),
            param("Depth", 0.0, 10.0, 3.0, "ms"),
            param("Delay", 5.0, 30.0, 12.0, "ms"),
            param("Feedback", 0.0, 70.0, 0.0, "%"),
            param("Mix", 0.0, 100.0, 50.0, "%"),
        ]
    }
    fn new(rate: f64) -> Self {
        Self {
            rate,
            // The longest delay plus the deepest sweep, with room to spare.
            line: Delay::new(0.05, rate as u32),
            phase: 0.0,
            speed: 0.8,
            depth_ms: 3.0,
            delay_ms: 12.0,
            mix: Smoother::new(rate, 0.02, 0.5),
            feedback: 0.0,
        }
    }
    fn set_param(&mut self, index: usize, value: f64) {
        match index {
            0 => self.speed = value,
            1 => self.depth_ms = value,
            2 => self.delay_ms = value,
            3 => self.feedback = (value / 100.0) as f32,
            4 => self.mix.set((value / 100.0) as f32),
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.line.clear();
        self.phase = 0.0;
    }
    fn tail_seconds(&self) -> f64 {
        0.05
    }
    fn process(&mut self, audio: &mut [[f32; 2]], _: &[NoteEvent], _: &ProcessContext) {
        let step = self.speed / self.rate;
        let samples = self.rate / 1000.0;
        for frame in audio {
            self.phase = (self.phase + step).fract();
            let left = self.delay_ms + self.depth_ms * (self.phase * TAU).sin();
            let right = self.delay_ms + self.depth_ms * ((self.phase + 0.25) * TAU).sin();
            let wet = [
                self.line.read(left * samples)[0],
                self.line.read(right * samples)[1],
            ];
            self.line.write([
                frame[0] + wet[0] * self.feedback,
                frame[1] + wet[1] * self.feedback,
            ]);
            let mix = self.mix.step();
            for (sample, wet) in frame.iter_mut().zip(wet) {
                *sample += (wet - *sample) * mix;
            }
        }
    }
}

export_plugins!(Chorus);

#[cfg(test)]
mod tests {
    use super::*;
    use ryolune_plugin::testing::Bench;

    #[test]
    fn a_click_comes_back_delayed_and_spread() {
        let mut bench = Bench::<Chorus>::new(48_000.0);
        bench.set("Mix", 100.0).set("Depth", 0.0).set("Delay", 10.0);
        let mut audio = vec![[0.0f32; 2]; 2048];
        audio[0] = [1.0, 1.0];
        bench.process(&mut audio, &[]);
        let loudest = audio
            .iter()
            .enumerate()
            .max_by(|a, b| a.1[0].abs().total_cmp(&b.1[0].abs()))
            .unwrap()
            .0;
        // 10 ms at 48 kHz.
        assert!((470..=490).contains(&loudest), "{loudest}");
        assert!(audio.iter().all(|f| f[0].is_finite() && f[1].is_finite()));
    }
}
