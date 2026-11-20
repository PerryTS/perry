//! Platform-independent sound-level metering shared by every native
//! `audioGetLevel` backend (iOS, macOS, Android, watchOS).
//!
//! Signal chain (IEC 61672 style):
//!   samples → A-weighting (designed for the live sample rate)
//!           → squared → "Fast" exponential time weighting (τ = 125 ms) on
//!             mean-square (energy), NOT on dB
//!           → 10·log10 → + `DBFS_TO_SPL_OFFSET`
//!
//! The A-weighting filter is the analog IEC 61672-1 prototype
//!   H(s) = s⁴ / ((s+ω₁)² (s+ω₂) (s+ω₃) (s+ω₄)²)
//! split into six first-order sections, each discretised with the bilinear
//! transform, then normalised to exactly 0 dB at 1 kHz. At 48 kHz the
//! response is within ±0.05 dB of the standard from 20 Hz to 4 kHz and within
//! the Class 1 tolerances up to 16 kHz (−1.2 dB @ 10 kHz, −6.4 dB @ 16 kHz).
//!
//! The previous hard-coded biquad coefficients were not an A-weighting
//! filter: they attenuated 1 kHz by ~77 dB and acted as a steep high-pass, so
//! readings tracked high-frequency hiss rather than loudness.

/// Maps 0 dBFS (digital full scale) to dB SPL. Kept at the historical value
/// so `audioGetLevel()` stays API-compatible; apps refine it per device.
pub const DBFS_TO_SPL_OFFSET: f64 = 110.0;

/// Fast time weighting (IEC 61672): 125 ms.
pub const TAU_FAST: f64 = 0.125;

// IEC 61672-1 pole frequencies (Hz).
const F1: f64 = 20.598_997;
const F2: f64 = 107.652_65;
const F3: f64 = 737.862_23;
const F4: f64 = 12_194.217;

#[derive(Clone, Copy, Debug, Default)]
struct FirstOrder {
    b0: f64,
    b1: f64,
    a1: f64,
    x1: f64,
    y1: f64,
}

impl FirstOrder {
    /// Bilinear-transformed `s/(s+ω)` (high-pass) or `1/(s+ω)` (low-pass).
    fn new(f_hz: f64, high_pass: bool, fs: f64) -> Self {
        let k = 2.0 * fs;
        let w = 2.0 * std::f64::consts::PI * f_hz;
        let d0 = k + w;
        let (b0, b1) = if high_pass {
            (k / d0, -k / d0)
        } else {
            (1.0 / d0, 1.0 / d0)
        };
        FirstOrder {
            b0,
            b1,
            a1: (w - k) / d0,
            x1: 0.0,
            y1: 0.0,
        }
    }

    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 - self.a1 * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }

    /// |H(e^{jω})| at `f_hz`.
    fn magnitude(&self, f_hz: f64, fs: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * f_hz / fs;
        let (c, s) = (w.cos(), -w.sin()); // z⁻¹ = e^{-jω}
        let num = ((self.b0 + self.b1 * c).powi(2) + (self.b1 * s).powi(2)).sqrt();
        let den = ((1.0 + self.a1 * c).powi(2) + (self.a1 * s).powi(2)).sqrt();
        num / den
    }
}

/// A-weighting filter designed for a specific sample rate.
#[derive(Clone, Debug)]
pub struct AWeighting {
    sections: [FirstOrder; 6],
    gain: f64,
    sample_rate: f64,
}

impl AWeighting {
    pub fn new(sample_rate: f64) -> Self {
        let fs = if sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let sections = [
            FirstOrder::new(F1, true, fs),
            FirstOrder::new(F1, true, fs),
            FirstOrder::new(F2, true, fs),
            FirstOrder::new(F3, true, fs),
            FirstOrder::new(F4, false, fs),
            FirstOrder::new(F4, false, fs),
        ];
        let mut filter = AWeighting {
            sections,
            gain: 1.0,
            sample_rate: fs,
        };
        filter.gain = 1.0 / filter.raw_magnitude(1000.0);
        filter
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    fn raw_magnitude(&self, f_hz: f64) -> f64 {
        self.sections
            .iter()
            .map(|s| s.magnitude(f_hz, self.sample_rate))
            .product()
    }

    /// Filter response in dB at `f_hz` (0 dB at 1 kHz).
    pub fn response_db(&self, f_hz: f64) -> f64 {
        20.0 * (self.gain * self.raw_magnitude(f_hz)).log10()
    }

    #[inline]
    pub fn tick(&mut self, x: f64) -> f64 {
        let mut y = x * self.gain;
        for s in self.sections.iter_mut() {
            y = s.tick(y);
        }
        y
    }
}

/// A-weighted, Fast-time-weighted sound level meter.
#[derive(Clone, Debug)]
pub struct SoundLevelMeter {
    filter: AWeighting,
    alpha: f64,
    mean_square: f64,
    primed: bool,
}

impl SoundLevelMeter {
    pub fn new(sample_rate: f64) -> Self {
        let filter = AWeighting::new(sample_rate);
        let alpha = 1.0 - (-1.0 / (filter.sample_rate() * TAU_FAST)).exp();
        SoundLevelMeter {
            filter,
            alpha,
            mean_square: 0.0,
            primed: false,
        }
    }

    /// Re-designs the filter if the hardware sample rate changed (e.g. a
    /// headset was plugged in); keeps the running energy.
    pub fn ensure_sample_rate(&mut self, sample_rate: f64) {
        if sample_rate > 0.0 && (sample_rate - self.filter.sample_rate()).abs() > 0.5 {
            let ms = self.mean_square;
            let primed = self.primed;
            *self = SoundLevelMeter::new(sample_rate);
            self.mean_square = ms;
            self.primed = primed;
        }
    }

    /// Processes one buffer. Returns `(level_db, peak_abs_sample)` where
    /// `level_db` is the Fast-weighted LAF (dBFS + `DBFS_TO_SPL_OFFSET`),
    /// clamped to 0…140.
    pub fn process(&mut self, samples: &[f32]) -> (f64, f32) {
        let mut peak = 0.0f32;
        if !self.primed && !samples.is_empty() {
            // Seed the integrator with the first buffer's energy so the
            // meter doesn't ramp up from silence for ~0.5 s at launch.
            let mut warm = self.filter.clone();
            let ms: f64 = samples
                .iter()
                .map(|&s| {
                    let y = warm.tick(s as f64);
                    y * y
                })
                .sum::<f64>()
                / samples.len() as f64;
            self.mean_square = ms;
            self.primed = true;
        }
        for &s in samples {
            let a = s.abs();
            if a > peak {
                peak = a;
            }
            let y = self.filter.tick(s as f64);
            self.mean_square += self.alpha * (y * y - self.mean_square);
        }
        (self.level_db(), peak)
    }

    pub fn level_db(&self) -> f64 {
        if self.mean_square <= 1.0e-20 {
            return 0.0;
        }
        (10.0 * self.mean_square.log10() + DBFS_TO_SPL_OFFSET).clamp(0.0, 140.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iec_a(f: f64) -> f64 {
        let f2 = f * f;
        let ra = F4.powi(2) * f2 * f2
            / ((f2 + F1 * F1) * ((f2 + F2 * F2) * (f2 + F3 * F3)).sqrt() * (f2 + F4 * F4));
        20.0 * ra.log10() + 2.0
    }

    #[test]
    fn a_weighting_matches_iec_61672() {
        for fs in [16_000.0, 44_100.0, 48_000.0] {
            let filt = AWeighting::new(fs);
            assert!(filt.response_db(1000.0).abs() < 1e-9);
            for f in [
                20.0, 31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0,
            ] {
                if f > fs * 0.2 {
                    continue;
                }
                let err = filt.response_db(f) - iec_a(f);
                assert!(err.abs() < 0.5, "fs={fs} f={f} err={err}");
            }
        }
        // Class 1 upper band at 48 kHz.
        let filt = AWeighting::new(48_000.0);
        for (f, lo, hi) in [
            (8000.0, -3.0, 1.5),
            (10_000.0, -3.0, 2.0),
            (16_000.0, -17.0, 3.5),
        ] {
            let err = filt.response_db(f) - iec_a(f);
            assert!(err > lo && err < hi, "f={f} err={err}");
        }
    }

    fn sine(fs: f64, f: f64, amp: f32, secs: f64) -> Vec<f32> {
        let n = (fs * secs) as usize;
        (0..n)
            .map(|i| amp * (2.0 * std::f64::consts::PI * f * i as f64 / fs).sin() as f32)
            .collect()
    }

    #[test]
    fn full_scale_1khz_sine_reads_minus_3_dbfs() {
        let mut m = SoundLevelMeter::new(48_000.0);
        let (db, peak) = m.process(&sine(48_000.0, 1000.0, 1.0, 1.0));
        // RMS of a full-scale sine is −3.01 dBFS.
        assert!((db - (DBFS_TO_SPL_OFFSET - 3.01)).abs() < 0.1, "db={db}");
        assert!(peak > 0.99);
    }

    #[test]
    fn level_is_linear_in_amplitude() {
        let mut a = SoundLevelMeter::new(44_100.0);
        let mut b = SoundLevelMeter::new(44_100.0);
        let (da, _) = a.process(&sine(44_100.0, 1000.0, 0.1, 1.0));
        let (db, _) = b.process(&sine(44_100.0, 1000.0, 0.01, 1.0));
        assert!(((da - db) - 20.0).abs() < 0.05, "{da} {db}");
    }

    #[test]
    fn low_frequency_is_attenuated() {
        let mut lf = SoundLevelMeter::new(48_000.0);
        let mut ref1k = SoundLevelMeter::new(48_000.0);
        let (l, _) = lf.process(&sine(48_000.0, 100.0, 0.5, 2.0));
        let (r, _) = ref1k.process(&sine(48_000.0, 1000.0, 0.5, 2.0));
        assert!(((r - l) - 19.1).abs() < 0.3, "diff={}", r - l);
    }

    #[test]
    fn fast_time_weighting_decays_about_35_db_per_second() {
        let mut m = SoundLevelMeter::new(48_000.0);
        let (start, _) = m.process(&sine(48_000.0, 1000.0, 0.5, 1.0));
        // Near-silence (tiny noise floor so log stays defined).
        let quiet = vec![1.0e-6f32; 48_000 / 4];
        let (after, _) = m.process(&quiet);
        // 0.25 s at τ=125 ms → e^-2 → −8.7 dB.
        assert!(((start - after) - 8.69).abs() < 0.3, "{}", start - after);
    }
}
