//! Bitcrusher: an example ryolune plugin written on the SDK (`ryolune-plugin`), the kind of
//! effect a person gets by asking their agent ("a bitcrusher with a mix knob"). It shows the
//! whole shape of an effect: parameters in plain units, smoothing where a jump would click,
//! and a `process` that never allocates. `plugin.toml` beside this crate makes the built
//! library an lsuite plugin bundle (`plugin.publishLocal`).

use ryolune_plugin::{export_plugins, prelude::*};

pub struct Bitcrusher {
    rate: f64,
    /// Quantisation steps per unit of amplitude (2^(bits-1)).
    steps: f32,
    /// How many input samples each held sample lasts, as a fraction (1 = no reduction).
    hold: f64,
    /// Where the hold counter is, and the sample being held.
    counter: f64,
    held: [f32; 2],
    mix: Smoother,
    output: Smoother,
}

impl Bitcrusher {
    fn steps_for(bits: f64) -> f32 {
        2f32.powf((bits.clamp(1.0, 24.0) - 1.0) as f32)
    }
}

impl Plugin for Bitcrusher {
    const INFO: Info = Info::effect(
        "org.ryolune.examples.bitcrusher",
        "Bitcrusher",
        "ryolune Examples",
        "Distortion",
    )
    .describe("Fewer bits and a lower sample rate: from warm grit to broken 8-bit.");
    fn params() -> Vec<ParamSpec> {
        vec![
            param("Bits", 1.0, 16.0, 8.0, "bits"),
            hz("Sample Rate", 500.0, 48000.0, 11025.0),
            param("Mix", 0.0, 100.0, 100.0, "%"),
            param("Output", -24.0, 12.0, 0.0, "dB"),
        ]
    }
    fn new(rate: f64) -> Self {
        Self {
            rate,
            steps: Self::steps_for(8.0),
            hold: rate / 11025.0,
            counter: 0.0,
            held: [0.0; 2],
            mix: Smoother::new(rate, 0.02, 1.0),
            output: Smoother::new(rate, 0.02, 1.0),
        }
    }
    fn set_param(&mut self, index: usize, value: f64) {
        match index {
            0 => self.steps = Self::steps_for(value),
            1 => self.hold = (self.rate / value.max(1.0)).max(1.0),
            2 => self.mix.set((value / 100.0) as f32),
            3 => self.output.set(db_to_gain(value)),
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.counter = 0.0;
        self.held = [0.0; 2];
    }
    fn process(&mut self, audio: &mut [[f32; 2]], _: &[NoteEvent], _: &ProcessContext) {
        for frame in audio {
            self.counter -= 1.0;
            if self.counter <= 0.0 {
                self.counter += self.hold;
                for c in 0..2 {
                    self.held[c] = (frame[c] * self.steps).round() / self.steps;
                }
            }
            let (mix, output) = (self.mix.step(), self.output.step());
            for c in 0..2 {
                frame[c] = (frame[c] + (self.held[c] - frame[c]) * mix) * output;
            }
        }
    }
}

export_plugins!(Bitcrusher);

#[cfg(test)]
mod tests {
    use super::*;
    use ryolune_plugin::testing::Bench;

    #[test]
    fn crushes_to_the_chosen_steps_and_mixes_back() {
        let mut bench = Bench::<Bitcrusher>::new(48_000.0);
        bench.set("Bits", 2.0).set("Sample Rate", 48_000.0);
        let mut audio: Vec<[f32; 2]> = (0..256).map(|i| [i as f32 / 256.0; 2]).collect();
        bench.process(&mut audio, &[]);
        // Two bits: steps of 0.5.
        for frame in &audio[64..] {
            assert!(
                (frame[0] * 2.0 - (frame[0] * 2.0).round()).abs() < 1e-4,
                "{}",
                frame[0]
            );
        }
        let mut dry = Bench::<Bitcrusher>::new(48_000.0);
        dry.set("Mix", 0.0);
        let mut audio = vec![[0.3f32, -0.3]; 4096];
        dry.process(&mut audio, &[]);
        assert!((audio[4095][0] - 0.3).abs() < 1e-4);
    }
}
