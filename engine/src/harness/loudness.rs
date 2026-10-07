//! Loudness as broadcasters and streaming services measure it: ITU-R BS.1770-4 / EBU R128.
//!
//! [`Meter`] takes stereo frames one block at a time (no allocation per frame) and keeps:
//!
//! - the K-weighting filter (a high shelf then the RLB high-pass) per channel,
//! - the energy of each 100 ms segment, from which momentary (400 ms) and short-term (3 s)
//!   loudness are read with 75 % and 97 % overlap,
//! - the gated integrated loudness (absolute gate -70 LUFS, relative gate -10 LU) and the
//!   loudness range (short-term blocks, relative gate -20 LU, 10th to 95th percentile),
//! - the sample peak, the samples above full scale, and the true peak from 4x oversampling
//!   with a 48-tap windowed-sinc interpolator (BS.1770-4 Annex 2 asks for at least 4x).
//!
//! Checked against the EBU Tech 3341 / 3342 reference cases in the tests: a 1 kHz sine at
//! -20 dBFS in both channels reads -20 LUFS (within 0.1 LU) and so on.

/// Loudness of a mean-square energy (sum of the channel weights times their mean square).
pub fn lufs(energy: f64) -> f64 {
    if energy <= 0.0 {
        f64::NEG_INFINITY
    } else {
        -0.691 + 10.0 * energy.log10()
    }
}
fn energy_of(lufs: f64) -> f64 {
    10f64.powf((lufs + 0.691) / 10.0)
}

#[derive(Clone, Copy, Debug, Default)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}
impl Biquad {
    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}
/// The two K-weighting stages for a sample rate (the closed forms libebur128 uses; at 48 kHz
/// they give the coefficients printed in BS.1770).
fn k_weighting(rate: f64) -> [Biquad; 2] {
    use std::f64::consts::PI;
    let (f0, gain, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (PI * f0 / rate).tan();
    let vh = 10f64.powf(gain / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b0: (vh + vb * k / q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
        ..Biquad::default()
    };
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (PI * f0 / rate).tan();
    let a0 = 1.0 + k / q + k * k;
    let highpass = Biquad {
        b0: 1.0,
        b1: -2.0,
        b2: 1.0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
        ..Biquad::default()
    };
    [shelf, highpass]
}

const TAPS_PER_PHASE: usize = 12;
const PHASES: usize = 4;

/// 4x oversampling for the true peak: polyphase windowed sinc, each phase normalised to unity
/// gain at DC.
struct TruePeak {
    phases: [[f64; TAPS_PER_PHASE]; PHASES],
    history: [[f64; TAPS_PER_PHASE]; 2],
    at: usize,
    peak: f64,
}
impl TruePeak {
    fn new() -> Self {
        let n = TAPS_PER_PHASE * PHASES;
        let centre = (n - 1) as f64 / 2.0;
        let mut phases = [[0.0; TAPS_PER_PHASE]; PHASES];
        for (i, tap) in (0..n).map(|i| (i, i as f64 - centre)) {
            let x = tap / PHASES as f64;
            let sinc = if x.abs() < 1e-12 {
                1.0
            } else {
                (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
            };
            // Blackman-Harris window over the whole filter.
            let w = {
                let t = 2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64;
                0.35875 - 0.48829 * t.cos() + 0.14128 * (2.0 * t).cos() - 0.01168 * (3.0 * t).cos()
            };
            phases[i % PHASES][i / PHASES] = sinc * w;
        }
        for phase in &mut phases {
            let sum: f64 = phase.iter().sum();
            if sum.abs() > 1e-9 {
                for c in phase.iter_mut() {
                    *c /= sum;
                }
            }
        }
        Self {
            phases,
            history: [[0.0; TAPS_PER_PHASE]; 2],
            at: 0,
            peak: 0.0,
        }
    }
    #[inline]
    fn push(&mut self, frame: [f64; 2]) {
        self.at = (self.at + TAPS_PER_PHASE - 1) % TAPS_PER_PHASE;
        for (channel, &x) in frame.iter().enumerate() {
            self.history[channel][self.at] = x;
            self.peak = self.peak.max(x.abs());
            for phase in &self.phases {
                let mut y = 0.0;
                for (k, c) in phase.iter().enumerate() {
                    y += c * self.history[channel][(self.at + k) % TAPS_PER_PHASE];
                }
                self.peak = self.peak.max(y.abs());
            }
        }
    }
}

/// What a [`Meter`] measured.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// Gated integrated loudness, LUFS (`-inf` for silence).
    pub integrated: f64,
    pub momentary_max: f64,
    pub short_term_max: f64,
    /// EBU Tech 3342 loudness range in LU.
    pub range: f64,
    /// dBTP from 4x oversampling.
    pub true_peak: f64,
    /// dBFS of the largest sample.
    pub sample_peak: f64,
    /// Samples (per channel) above full scale.
    pub clipped: u64,
    /// Short-term loudness every 100 ms from the start (LUFS; `-inf` before 3 s of signal is
    /// silent, otherwise the window seen so far).
    pub short_term: Vec<f64>,
    pub seconds: f64,
}

pub struct Meter {
    rate: f64,
    filters: [[Biquad; 2]; 2],
    segment_len: usize,
    segment_fill: usize,
    segment_energy: f64,
    /// Energy (mean square, weighted) of each finished 100 ms segment.
    segments: Vec<f64>,
    true_peak: TruePeak,
    sample_peak: f64,
    clipped: u64,
    frames: u64,
}
impl Meter {
    pub fn new(rate: u32) -> Self {
        let rate = rate.max(8000) as f64;
        let k = k_weighting(rate);
        Self {
            rate,
            filters: [k, k],
            segment_len: (rate / 10.0).round() as usize,
            segment_fill: 0,
            segment_energy: 0.0,
            segments: Vec::new(),
            true_peak: TruePeak::new(),
            sample_peak: 0.0,
            clipped: 0,
            frames: 0,
        }
    }
    pub fn push(&mut self, block: &[[f32; 2]]) {
        for frame in block {
            let x = [frame[0] as f64, frame[1] as f64];
            let mut weighted = 0.0;
            for (channel, &sample) in x.iter().enumerate() {
                if sample.abs() > 1.0 {
                    self.clipped += 1;
                }
                self.sample_peak = self.sample_peak.max(sample.abs());
                let [shelf, highpass] = &mut self.filters[channel];
                let y = highpass.run(shelf.run(sample));
                weighted += y * y;
            }
            self.true_peak.push(x);
            self.segment_energy += weighted;
            self.segment_fill += 1;
            self.frames += 1;
            if self.segment_fill == self.segment_len {
                self.segments
                    .push(self.segment_energy / self.segment_len as f64);
                self.segment_energy = 0.0;
                self.segment_fill = 0;
            }
        }
    }
    /// Mean energy of the `n` segments ending at `end` (exclusive), over what exists.
    fn window(&self, end: usize, n: usize) -> f64 {
        let start = end.saturating_sub(n);
        let slice = &self.segments[start..end];
        if slice.is_empty() {
            0.0
        } else {
            slice.iter().sum::<f64>() / n.max(1) as f64
        }
    }
    pub fn reading(&self) -> Reading {
        let count = self.segments.len();
        // Momentary blocks: 400 ms, every 100 ms (75 % overlap), only complete ones.
        let momentary: Vec<f64> = (4..=count).map(|end| self.window(end, 4)).collect();
        let integrated = gated_mean(&momentary, -70.0, 10.0).map_or(f64::NEG_INFINITY, lufs);
        let short: Vec<f64> = (30..=count).map(|end| self.window(end, 30)).collect();
        let range = loudness_range(&short);
        let short_term = (1..=count).map(|end| lufs(self.window(end, 30))).collect();
        let max_of = |blocks: &[f64]| {
            blocks
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, |a, b| a.max(lufs(b)))
        };
        let db = |v: f64| {
            if v > 0.0 {
                20.0 * v.log10()
            } else {
                f64::NEG_INFINITY
            }
        };
        Reading {
            integrated,
            momentary_max: max_of(&momentary),
            short_term_max: if short.is_empty() {
                // Under 3 s: the whole signal is the short-term window.
                lufs(self.window(count, 30))
            } else {
                max_of(&short)
            },
            range,
            true_peak: db(self.true_peak.peak),
            sample_peak: db(self.sample_peak),
            clipped: self.clipped,
            short_term,
            seconds: self.frames as f64 / self.rate,
        }
    }
}

/// BS.1770 gating: blocks above the absolute gate, then above (their mean - `relative` LU);
/// the mean energy of what is left.
fn gated_mean(blocks: &[f64], absolute: f64, relative: f64) -> Option<f64> {
    let floor = energy_of(absolute);
    let above: Vec<f64> = blocks.iter().copied().filter(|&e| e > floor).collect();
    if above.is_empty() {
        return None;
    }
    let mean = above.iter().sum::<f64>() / above.len() as f64;
    let gate = energy_of(lufs(mean) - relative);
    let kept: Vec<f64> = above.into_iter().filter(|&e| e > gate).collect();
    (!kept.is_empty()).then(|| kept.iter().sum::<f64>() / kept.len() as f64)
}

/// EBU Tech 3342: short-term blocks above -70 LUFS and above their mean - 20 LU; the spread
/// between the 10th and 95th percentile of their loudness.
fn loudness_range(short: &[f64]) -> f64 {
    let floor = energy_of(-70.0);
    let above: Vec<f64> = short.iter().copied().filter(|&e| e > floor).collect();
    if above.is_empty() {
        return 0.0;
    }
    let mean = above.iter().sum::<f64>() / above.len() as f64;
    let gate = energy_of(lufs(mean) - 20.0);
    let mut kept: Vec<f64> = above.into_iter().filter(|&e| e > gate).map(lufs).collect();
    if kept.len() < 2 {
        return 0.0;
    }
    kept.sort_by(|a, b| a.total_cmp(b));
    let at = |p: f64| kept[((kept.len() - 1) as f64 * p).round() as usize];
    at(0.95) - at(0.10)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f64, dbfs: f64, seconds: f64, phase_offset: f64) -> Vec<[f32; 2]> {
        let amplitude = 10f64.powf(dbfs / 20.0);
        (0..(rate as f64 * seconds) as usize)
            .map(|i| {
                let v = amplitude
                    * (2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64 + phase_offset)
                        .sin();
                [v as f32, v as f32]
            })
            .collect()
    }
    fn measure(rate: u32, signal: &[[f32; 2]]) -> Reading {
        let mut meter = Meter::new(rate);
        for block in signal.chunks(256) {
            meter.push(block);
        }
        meter.reading()
    }

    #[test]
    fn a_minus_23_dbfs_sine_reads_minus_23_lufs() {
        // EBU Tech 3341 case 1: 1 kHz sine at -23 dBFS in both channels, 20 s: -23.0 LUFS.
        for rate in [44100, 48000, 96000] {
            let r = measure(rate, &sine(rate, 1000.0, -23.0, 20.0, 0.0));
            assert!(
                (r.integrated + 23.0).abs() < 0.1,
                "{rate}: {}",
                r.integrated
            );
            assert!((r.short_term_max + 23.0).abs() < 0.1);
            assert!((r.momentary_max + 23.0).abs() < 0.1);
            assert!(r.range < 0.1);
            assert_eq!(r.clipped, 0);
        }
    }

    #[test]
    fn the_relative_gate_ignores_quiet_passages() {
        // Tech 3341 case 3 shape: 10 s at -36, 60 s at -23, 10 s at -36 dBFS: -23.0 LUFS.
        let rate = 48000;
        let mut signal = sine(rate, 1000.0, -36.0, 10.0, 0.0);
        signal.extend(sine(rate, 1000.0, -23.0, 60.0, 0.0));
        signal.extend(sine(rate, 1000.0, -36.0, 10.0, 0.0));
        let r = measure(rate, &signal);
        assert!((r.integrated + 23.0).abs() < 0.1, "{}", r.integrated);
        // Silence alone is below the absolute gate.
        let silent = measure(rate, &vec![[0.0; 2]; rate as usize * 5]);
        assert!(silent.integrated.is_infinite());
    }

    #[test]
    fn loudness_range_follows_tech_3342() {
        // Tech 3342 case 1: 20 s at -20 dBFS then 20 s at -30 dBFS: LRA 10 LU (±1).
        let rate = 48000;
        let mut signal = sine(rate, 1000.0, -20.0, 20.0, 0.0);
        signal.extend(sine(rate, 1000.0, -30.0, 20.0, 0.0));
        let r = measure(rate, &signal);
        assert!((r.range - 10.0).abs() < 1.0, "{}", r.range);
    }

    #[test]
    fn true_peak_sees_the_peak_between_samples() {
        // A sine at a quarter of the rate, sampled 45 degrees off its crest: every sample is
        // at 0.707 of the amplitude, the true peak is the amplitude.
        let rate = 48000;
        let signal = sine(rate, 12000.0, -6.0, 1.0, std::f64::consts::FRAC_PI_4);
        let r = measure(rate, &signal);
        assert!((r.sample_peak + 9.0).abs() < 0.2, "{}", r.sample_peak);
        assert!((r.true_peak + 6.0).abs() < 0.5, "{}", r.true_peak);
        // Over full scale is counted as clipping.
        let hot = sine(rate, 1000.0, 1.0, 1.0, 0.0);
        let r = measure(rate, &hot);
        assert!(r.clipped > 0);
        assert!(r.true_peak > 0.9);
    }
}
