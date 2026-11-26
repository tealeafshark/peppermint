// SPDX-License-Identifier: GPL-3.0-only

//! Real-time 10-band Graphic Equalizer DSP using cascaded RBJ biquad peaking filters.

use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};

pub const NUM_BANDS: usize = 10;

/// Classic Winamp 10-band center frequencies (Hz).
pub const EQ_BANDS_HZ: [f64; NUM_BANDS] = [
    60.0, 170.0, 310.0, 600.0, 1_000.0, 3_000.0, 6_000.0, 12_000.0, 14_000.0, 16_000.0,
];

pub const EQ_BAND_LABELS: [&str; NUM_BANDS] = [
    "60", "170", "310", "600", "1K", "3K", "6K", "12K", "14K", "16K",
];

pub const EQ_MIN_DB: f32 = -12.0;
pub const EQ_MAX_DB: f32 = 12.0;
const PEAKING_Q: f64 = 1.41;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqPreset {
    pub name: String,
    pub preamp_db: f32,
    pub bands_db: [f32; NUM_BANDS],
}

impl EqPreset {
    pub fn builtins() -> Vec<Self> {
        vec![
            Self {
                name: "Flat".into(),
                preamp_db: 0.0,
                bands_db: [0.0; NUM_BANDS],
            },
            Self {
                name: "Rock".into(),
                preamp_db: 0.0,
                bands_db: [4.5, 3.0, -2.5, -4.0, -1.5, 2.0, 4.5, 5.5, 5.5, 5.5],
            },
            Self {
                name: "Pop".into(),
                preamp_db: 0.0,
                bands_db: [-1.5, 2.5, 4.0, 4.5, 3.0, -0.5, -1.5, -1.5, -1.0, -1.0],
            },
            Self {
                name: "Bass Boost".into(),
                preamp_db: -2.0,
                bands_db: [6.5, 5.5, 4.0, 2.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0],
            },
            Self {
                name: "Electronic".into(),
                preamp_db: -1.0,
                bands_db: [4.5, 3.5, 1.0, 0.0, -2.0, 2.0, 1.0, 2.5, 4.5, 5.0],
            },
            Self {
                name: "Jazz".into(),
                preamp_db: 0.0,
                bands_db: [3.0, 2.0, 1.0, 2.0, -1.5, -1.5, 0.0, 1.5, 2.5, 3.0],
            },
            Self {
                name: "Classical".into(),
                preamp_db: 0.0,
                bands_db: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -3.5, -3.5, -3.5, -5.0],
            },
            Self {
                name: "Vocal".into(),
                preamp_db: 0.0,
                bands_db: [-2.0, -3.0, -2.0, 1.5, 4.0, 4.0, 2.5, 1.0, 0.0, -1.0],
            },
            Self {
                name: "Full Bass & Treble".into(),
                preamp_db: -1.5,
                bands_db: [4.5, 3.5, 0.0, -3.5, -2.5, 1.0, 4.5, 6.0, 6.5, 6.5],
            },
            Self {
                name: "Laptop / Headphones".into(),
                preamp_db: 0.0,
                bands_db: [3.0, 5.5, 2.5, -1.5, -1.0, 1.0, 2.5, 5.0, 6.5, 7.0],
            },
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqSettings {
    pub enabled: bool,
    pub preset_name: String,
    pub preamp_db: f32,
    pub bands_db: [f32; NUM_BANDS],
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            preset_name: "Flat".into(),
            preamp_db: 0.0,
            bands_db: [0.0; NUM_BANDS],
        }
    }
}

/// Lock-free shared EQ parameters read by the audio thread and written by the UI thread.
pub struct SharedEqParams {
    enabled: AtomicBool,
    preamp_millidb: AtomicI32,
    bands_millidb: [AtomicI32; NUM_BANDS],
    generation: AtomicU64,
}

impl SharedEqParams {
    pub fn new(settings: &EqSettings) -> Self {
        let params = Self {
            enabled: AtomicBool::new(settings.enabled),
            preamp_millidb: AtomicI32::new(db_to_millidb(settings.preamp_db)),
            bands_millidb: std::array::from_fn(|i| AtomicI32::new(db_to_millidb(settings.bands_db[i]))),
            generation: AtomicU64::new(1),
        };
        params
    }

    pub fn apply_settings(&self, settings: &EqSettings) {
        self.enabled.store(settings.enabled, Ordering::Relaxed);
        self.preamp_millidb
            .store(db_to_millidb(settings.preamp_db), Ordering::Relaxed);
        for (i, &db) in settings.bands_db.iter().enumerate() {
            self.bands_millidb[i].store(db_to_millidb(db), Ordering::Relaxed);
        }
        self.generation.fetch_add(1, Ordering::Release);
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn snapshot(&self) -> (bool, f64, [f64; NUM_BANDS]) {
        let enabled = self.enabled.load(Ordering::Relaxed);
        let preamp = f64::from(self.preamp_millidb.load(Ordering::Relaxed)) / 1000.0;
        let bands = std::array::from_fn(|i| {
            f64::from(self.bands_millidb[i].load(Ordering::Relaxed)) / 1000.0
        });
        (enabled, preamp, bands)
    }
}

fn db_to_millidb(db: f32) -> i32 {
    (db.clamp(EQ_MIN_DB, EQ_MAX_DB) * 1000.0).round() as i32
}

/// Direct Form II Transposed biquad filter coefficients (normalized by a0).
#[derive(Debug, Clone, Copy)]
pub struct BiquadCoeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl BiquadCoeffs {
    pub const IDENTITY: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    /// Audio EQ Cookbook peaking EQ filter.
    pub fn peaking_eq(sample_rate: f64, freq_hz: f64, q: f64, gain_db: f64) -> Self {
        if gain_db.abs() < 0.01 || sample_rate <= 0.0 {
            return Self::IDENTITY;
        }
        // Keep center frequency safely below Nyquist to avoid filter instability.
        let nyquist_safe = sample_rate * 0.46;
        let f0 = freq_hz.min(nyquist_safe).max(10.0);

        let a = 10.0_f64.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * f0 / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Magnitude response (linear) at a given frequency.
    pub fn magnitude_at(&self, sample_rate: f64, freq_hz: f64) -> f64 {
        let w = 2.0 * PI * freq_hz / sample_rate;
        let cos_w = w.cos();
        let sin_w = w.sin();
        let cos_2w = (2.0 * w).cos();
        let sin_2w = (2.0 * w).sin();

        let num_re = self.b0 + self.b1 * cos_w + self.b2 * cos_2w;
        let num_im = -(self.b1 * sin_w + self.b2 * sin_2w);
        let den_re = 1.0 + self.a1 * cos_w + self.a2 * cos_2w;
        let den_im = -(self.a1 * sin_w + self.a2 * sin_2w);

        let num_mag = (num_re * num_re + num_im * num_im).sqrt();
        let den_mag = (den_re * den_re + den_im * den_im).sqrt().max(1e-12);
        num_mag / den_mag
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct BiquadState {
    z1: f64,
    z2: f64,
}

impl BiquadState {
    #[inline]
    fn process(&mut self, x: f64, c: &BiquadCoeffs) -> f64 {
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y
    }
}

/// Per-channel 10-band EQ processor for the audio thread.
pub struct GraphicEqProcessor {
    sample_rate: f64,
    channels: usize,
    enabled: bool,
    preamp_linear: f64,
    coeffs: [BiquadCoeffs; NUM_BANDS],
    /// Indexed as `states[channel][band]`.
    states: Vec<[BiquadState; NUM_BANDS]>,
    last_generation: u64,
}

impl GraphicEqProcessor {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        let ch = (channels as usize).max(1);
        Self {
            sample_rate: f64::from(sample_rate.max(8_000)),
            channels: ch,
            enabled: false,
            preamp_linear: 1.0,
            coeffs: [BiquadCoeffs::IDENTITY; NUM_BANDS],
            states: vec![[BiquadState::default(); NUM_BANDS]; ch],
            last_generation: 0,
        }
    }

    /// Recomputes filter coefficients if parameters changed.
    #[inline]
    pub fn sync_from(&mut self, shared: &SharedEqParams) {
        let generation = shared.generation();
        if generation == self.last_generation {
            return;
        }
        self.last_generation = generation;
        let (enabled, preamp_db, bands_db) = shared.snapshot();
        self.enabled = enabled;
        self.preamp_linear = 10.0_f64.powf(preamp_db / 20.0);
        for i in 0..NUM_BANDS {
            self.coeffs[i] =
                BiquadCoeffs::peaking_eq(self.sample_rate, EQ_BANDS_HZ[i], PEAKING_Q, bands_db[i]);
        }
    }

    #[inline]
    pub fn process_sample(&mut self, channel_idx: usize, sample: f32) -> f32 {
        if !self.enabled {
            return sample;
        }
        let ch = channel_idx % self.channels;
        let mut x = f64::from(sample) * self.preamp_linear;
        let ch_states = &mut self.states[ch];
        for band in 0..NUM_BANDS {
            x = ch_states[band].process(x, &self.coeffs[band]);
        }
        // Gentle soft-clipping above ±0.98 so heavy bass boosts never harshly wrap.
        let out = if x.abs() > 0.98 {
            x.signum() * (0.98 + 0.02 * ((x.abs() - 0.98) / 0.02).tanh())
        } else {
            x
        };
        out as f32
    }
}

/// Computes the combined EQ response (in dB) at `freq_hz` for rendering the EQ curve preview.
pub fn combined_response_db(settings: &EqSettings, freq_hz: f64) -> f32 {
    if !settings.enabled {
        return 0.0;
    }
    let sample_rate = 48_000.0;
    let mut mag = 10.0_f64.powf(f64::from(settings.preamp_db) / 20.0);
    for i in 0..NUM_BANDS {
        let c = BiquadCoeffs::peaking_eq(
            sample_rate,
            EQ_BANDS_HZ[i],
            PEAKING_Q,
            f64::from(settings.bands_db[i]),
        );
        mag *= c.magnitude_at(sample_rate, freq_hz);
    }
    (20.0 * mag.max(1e-6).log10()) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_eq_passes_signal_unchanged() {
        let settings = EqSettings::default();
        let shared = SharedEqParams::new(&settings);
        let mut proc = GraphicEqProcessor::new(44_100, 2);
        proc.sync_from(&shared);

        for i in 0..100 {
            let input = (i as f32 * 0.1).sin() * 0.5;
            let output = proc.process_sample(i % 2, input);
            assert!((input - output).abs() < 1e-5);
        }
    }

    #[test]
    fn bass_boost_increases_low_frequency_response() {
        let mut settings = EqSettings::default();
        settings.bands_db[0] = 6.0;
        let low = combined_response_db(&settings, 60.0);
        let high = combined_response_db(&settings, 8_000.0);
        assert!(low > 5.5 && low < 6.5, "expected ~+6 dB at 60Hz, got {low}");
        assert!(high.abs() < 0.5, "expected ~0 dB at 8kHz, got {high}");
    }
}
