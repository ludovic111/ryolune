//! What `harness.look` and `harness.measure` read from a render besides loudness: the
//! waveform as min/max columns and the average spectrum (Welch: Hann-windowed 4096-point
//! frames, power averaged), with the energy of five bands a mix engineer talks about.

const FFT: usize = 4096;

/// In-place iterative radix-2 FFT on (re, im) pairs; `data.len()` must be a power of two.
pub fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two() && im.len() == n);
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * std::f64::consts::PI / len as f64;
        let (w_re, w_im) = (angle.cos(), angle.sin());
        for start in (0..n).step_by(len) {
            let (mut c_re, mut c_im) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let t_re = re[b] * c_re - im[b] * c_im;
                let t_im = re[b] * c_im + im[b] * c_re;
                re[b] = re[a] - t_re;
                im[b] = im[a] - t_im;
                re[a] += t_re;
                im[a] += t_im;
                let next = c_re * w_re - c_im * w_im;
                c_im = c_re * w_im + c_im * w_re;
                c_re = next;
            }
        }
        len <<= 1;
    }
}

/// The five bands reported as numbers (name, low Hz, high Hz).
pub const BANDS: [(&str, f64, f64); 5] = [
    ("sub", 20.0, 60.0),
    ("bass", 60.0, 250.0),
    ("lowMids", 250.0, 2000.0),
    ("highMids", 2000.0, 6000.0),
    ("air", 6000.0, 20000.0),
];

/// Averages the power spectrum of the mid signal over the frames it is given. To bound the
/// work on long ranges only every `stride`-th frame is analysed.
pub struct Spectrum {
    rate: f64,
    window: Vec<f64>,
    frame: Vec<f64>,
    fill: usize,
    power: Vec<f64>,
    frames: u64,
    stride: u64,
    seen: u64,
}
impl Spectrum {
    pub fn new(rate: u32, total_frames: u64) -> Self {
        let window = (0..FFT)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / FFT as f64).cos())
            .collect();
        // At most about 300 analysed frames, whatever the length.
        let stride = (total_frames / FFT as u64 / 300).max(1);
        Self {
            rate: rate as f64,
            window,
            frame: vec![0.0; FFT],
            fill: 0,
            power: vec![0.0; FFT / 2 + 1],
            frames: 0,
            stride,
            seen: 0,
        }
    }
    pub fn push(&mut self, block: &[[f32; 2]]) {
        for f in block {
            self.frame[self.fill] = (f[0] as f64 + f[1] as f64) * 0.5;
            self.fill += 1;
            if self.fill == FFT {
                self.fill = 0;
                self.seen += 1;
                if self.seen % self.stride == 0 {
                    self.analyse();
                }
            }
        }
    }
    fn analyse(&mut self) {
        let mut re: Vec<f64> = self
            .frame
            .iter()
            .zip(&self.window)
            .map(|(x, w)| x * w)
            .collect();
        let mut im = vec![0.0; FFT];
        fft(&mut re, &mut im);
        for (bin, p) in self.power.iter_mut().enumerate() {
            *p += re[bin] * re[bin] + im[bin] * im[bin];
        }
        self.frames += 1;
    }
    /// Mean power per bin with its frequency, normalised so a full-scale sine reads about
    /// 0 dB at its bin.
    pub fn bins(&self) -> Vec<(f64, f64)> {
        if self.frames == 0 {
            return vec![];
        }
        // A Hann-windowed full-scale sine peaks at (N/4)^2 in one bin.
        let reference = (FFT as f64 / 4.0).powi(2);
        self.power
            .iter()
            .enumerate()
            .skip(1)
            .map(|(bin, p)| {
                let hz = bin as f64 * self.rate / FFT as f64;
                (hz, p / self.frames as f64 / reference)
            })
            .collect()
    }
    /// dB of each band's share of the total energy between 20 Hz and 20 kHz (all <= 0), and
    /// the spectral centroid.
    pub fn bands(&self) -> (Vec<(&'static str, f64)>, Option<f64>) {
        let bins = self.bins();
        let audible: Vec<&(f64, f64)> = bins
            .iter()
            .filter(|(hz, _)| (20.0..20000.0).contains(hz))
            .collect();
        let total: f64 = audible.iter().map(|(_, p)| p).sum();
        if total <= 0.0 {
            return (
                BANDS.iter().map(|b| (b.0, f64::NEG_INFINITY)).collect(),
                None,
            );
        }
        let centroid = audible.iter().map(|(hz, p)| hz * p).sum::<f64>() / total;
        let bands = BANDS
            .iter()
            .map(|(name, low, high)| {
                let e: f64 = audible
                    .iter()
                    .filter(|(hz, _)| *hz >= *low && *hz < *high)
                    .map(|(_, p)| p)
                    .sum();
                (
                    *name,
                    if e > 0.0 {
                        10.0 * (e / total).log10()
                    } else {
                        f64::NEG_INFINITY
                    },
                )
            })
            .collect();
        (bands, Some(centroid))
    }
}

/// Min and max of the louder channel's signed sample per column of the picture, and whether
/// a column clipped.
pub struct Waveform {
    columns: Vec<(f32, f32, bool)>,
    per_column: f64,
    at: u64,
}
impl Waveform {
    pub fn new(columns: usize, total_frames: u64) -> Self {
        Self {
            columns: vec![(0.0, 0.0, false); columns.max(1)],
            per_column: (total_frames.max(1) as f64) / columns.max(1) as f64,
            at: 0,
        }
    }
    pub fn push(&mut self, block: &[[f32; 2]]) {
        let last = self.columns.len() - 1;
        for f in block {
            let column = ((self.at as f64 / self.per_column) as usize).min(last);
            let c = &mut self.columns[column];
            for &s in f {
                c.0 = c.0.min(s);
                c.1 = c.1.max(s);
                c.2 |= s.abs() > 1.0;
            }
            self.at += 1;
        }
    }
    pub fn columns(&self) -> &[(f32, f32, bool)] {
        &self.columns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_lands_in_its_band_and_bin() {
        let rate = 48000;
        let n = rate as usize * 2;
        let signal: Vec<[f32; 2]> = (0..n)
            .map(|i| {
                let v = (2.0 * std::f64::consts::PI * 100.0 * i as f64 / rate as f64).sin() as f32;
                [v, v]
            })
            .collect();
        let mut spectrum = Spectrum::new(rate, n as u64);
        spectrum.push(&signal);
        let (bands, centroid) = spectrum.bands();
        let bass = bands.iter().find(|b| b.0 == "bass").unwrap().1;
        assert!(bass > -0.5, "{bands:?}");
        assert!((centroid.unwrap() - 100.0).abs() < 15.0);
        let peak = spectrum
            .bins()
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        assert!((peak.0 - 100.0).abs() < 12.0);
        assert!(
            (10.0 * peak.1.log10()).abs() < 2.0,
            "{}",
            10.0 * peak.1.log10()
        );

        let mut wave = Waveform::new(10, n as u64);
        wave.push(&signal);
        assert!(wave
            .columns()
            .iter()
            .all(|c| c.1 > 0.99 && c.0 < -0.99 && !c.2));
    }
}
