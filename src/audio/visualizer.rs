// SPDX-License-Identifier: GPL-3.0-only

//! Real-time pure-Rust FFT audio analyzer and particle/pulse physics for the background visualizer.

use serde::{Deserialize, Serialize};
use std::f32::consts::PI;
use std::sync::Mutex;

pub const FFT_SIZE: usize = 512;
pub const NUM_VIZ_BANDS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VizMode {
    #[default]
    AuroraOrbit,
    Kaleidoscope,
    Nebula,
    Lyrics,
    Off,
}

impl VizMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::AuroraOrbit => Self::Kaleidoscope,
            Self::Kaleidoscope => Self::Nebula,
            Self::Nebula => Self::Lyrics,
            Self::Lyrics => Self::Off,
            Self::Off => Self::AuroraOrbit,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::AuroraOrbit => "Aurora",
            Self::Kaleidoscope => "Kaleidoscope",
            Self::Nebula => "Nebula",
            Self::Lyrics => "Lyrics",
            Self::Off => "Viz Off",
        }
    }
}

/// Shared audio tap fed by the playback thread and sampled by the UI at ~45–60 FPS.
pub struct SharedAudioTap {
    inner: Mutex<TapBuffer>,
}

struct TapBuffer {
    ring: [f32; FFT_SIZE],
    write_pos: usize,
    sample_rate: u32,
}

impl Default for SharedAudioTap {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedAudioTap {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(TapBuffer {
                ring: [0.0; FFT_SIZE],
                write_pos: 0,
                sample_rate: 44_100,
            }),
        }
    }

    /// Appends a small batch of mono samples without blocking the audio thread if contended.
    pub fn push_batch(&self, samples: &[f32], sample_rate: u32) {
        if let Ok(mut buf) = self.inner.try_lock() {
            buf.sample_rate = sample_rate.max(8_000);
            for &s in samples {
                let idx = buf.write_pos;
                buf.ring[idx] = s;
                buf.write_pos = (idx + 1) % FFT_SIZE;
            }
        }
    }

    /// Copies the latest `FFT_SIZE` mono samples in chronological order.
    pub fn snapshot(&self) -> ([f32; FFT_SIZE], u32) {
        let Ok(buf) = self.inner.lock() else {
            return ([0.0; FFT_SIZE], 44_100);
        };
        let mut out = [0.0_f32; FFT_SIZE];
        let pos = buf.write_pos;
        let first_len = FFT_SIZE - pos;
        out[..first_len].copy_from_slice(&buf.ring[pos..]);
        out[first_len..].copy_from_slice(&buf.ring[..pos]);
        (out, buf.sample_rate)
    }

    pub fn clear(&self) {
        if let Ok(mut buf) = self.inner.lock() {
            buf.ring.fill(0.0);
        }
    }
}

/// A single floating luminous particle orbiting behind the album art.
#[derive(Debug, Clone)]
pub struct VizParticle {
    pub angle: f32,
    pub radius: f32, // normalized 0.18 .. 1.05 of min(w,h)/2
    pub speed: f32,
    pub size: f32,
    pub hue_shift: f32, // 0.0 .. 1.0
    pub phase: f32,
}

/// Beat-triggered expanding halo ring.
#[derive(Debug, Clone)]
pub struct PulseRing {
    pub radius: f32, // starts ~0.28, expands to ~1.25
    pub alpha: f32,
    pub hue_shift: f32,
    pub thickness: f32,
}

/// Frame-by-frame animated state consumed by the Canvas visualizer.
#[derive(Debug, Clone)]
pub struct VizFrameState {
    pub time: f32,
    pub bands: [f32; NUM_VIZ_BANDS],
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub energy: f32,
    pub beat_flash: f32,
    bass_avg: f32,
    beat_cooldown: f32,
    pub particles: Vec<VizParticle>,
    pub rings: Vec<PulseRing>,
}

impl Default for VizFrameState {
    fn default() -> Self {
        let mut particles = Vec::with_capacity(42);
        for i in 0..42 {
            let fi = i as f32;
            particles.push(VizParticle {
                angle: (fi * 2.399_963) % (2.0 * PI),
                radius: 0.30 + ((fi * 0.618_034) % 0.72),
                speed: 0.14 + ((fi * 0.37) % 0.28) * if i % 2 == 0 { 1.0 } else { -0.8 },
                size: 2.0 + ((fi * 1.73) % 4.5),
                hue_shift: (fi * 0.19) % 1.0,
                phase: fi * 0.85,
            });
        }
        Self {
            time: 0.0,
            bands: [0.0; NUM_VIZ_BANDS],
            bass: 0.0,
            mid: 0.0,
            treble: 0.0,
            energy: 0.0,
            beat_flash: 0.0,
            bass_avg: 0.08,
            beat_cooldown: 0.0,
            particles,
            rings: Vec::new(),
        }
    }
}

impl VizFrameState {
    /// Advances FFT analysis, beat detection, particles, and expanding rings.
    pub fn step(&mut self, tap: &SharedAudioTap, is_playing: bool, dt: f32) {
        let dt = dt.clamp(0.005, 0.1);
        self.time += dt * (1.0 + self.mid * 1.6);

        let raw_bands = if is_playing {
            let (samples, sr) = tap.snapshot();
            compute_log_bands(&samples, sr)
        } else {
            [0.0; NUM_VIZ_BANDS]
        };

        // Smooth attack (fast rise) and musical decay (graceful fall).
        for i in 0..NUM_VIZ_BANDS {
            let target = raw_bands[i];
            let cur = self.bands[i];
            let coeff = if target > cur { 0.52 } else { 0.16 };
            self.bands[i] = cur + (target - cur) * coeff;
        }

        let raw_bass = avg(&self.bands[0..5]);
        let raw_mid = avg(&self.bands[5..17]);
        let raw_treble = avg(&self.bands[17..NUM_VIZ_BANDS]);

        self.bass = lerp(self.bass, raw_bass, 0.38);
        self.mid = lerp(self.mid, raw_mid, 0.28);
        self.treble = lerp(self.treble, raw_treble, 0.32);
        self.energy = (self.bass * 0.45 + self.mid * 0.35 + self.treble * 0.20).clamp(0.0, 1.0);

        // Adaptive beat / kick onset detection
        self.bass_avg = lerp(self.bass_avg, self.bass, 0.06);
        self.beat_cooldown = (self.beat_cooldown - dt).max(0.0);
        self.beat_flash = (self.beat_flash - dt * 2.8).max(0.0);

        if is_playing
            && self.beat_cooldown <= 0.0
            && self.bass > 0.24
            && self.bass > self.bass_avg * 1.32 + 0.05
        {
            self.beat_flash = 1.0;
            self.beat_cooldown = 0.22;
            if self.rings.len() < 8 {
                self.rings.push(PulseRing {
                    radius: 0.34,
                    alpha: (0.35 + self.bass * 0.35).min(0.68),
                    hue_shift: (self.time * 0.23) % 1.0,
                    thickness: 1.5 + self.bass * 2.5,
                });
            }
        }

        // Advance expanding beat rings
        for ring in &mut self.rings {
            ring.radius += dt * (0.48 + self.bass * 0.45);
            ring.alpha *= 1.0 - (dt * 1.55).min(0.8);
        }
        self.rings.retain(|r| r.alpha > 0.015 && r.radius < 1.45);

        // Advance orbiting particles
        for (i, p) in self.particles.iter_mut().enumerate() {
            let band = self.bands[i % NUM_VIZ_BANDS];
            p.angle += dt * p.speed * (0.65 + self.mid * 1.8 + band * 1.4);
            p.phase += dt * (1.2 + self.treble * 3.0);
        }
    }
}

fn avg(slice: &[f32]) -> f32 {
    if slice.is_empty() {
        return 0.0;
    }
    slice.iter().sum::<f32>() / slice.len() as f32
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Computes `NUM_VIZ_BANDS` logarithmically-spaced normalized spectrum magnitudes (0.0..1.0).
pub fn compute_log_bands(samples: &[f32; FFT_SIZE], sample_rate: u32) -> [f32; NUM_VIZ_BANDS] {
    let mut re = [0.0_f32; FFT_SIZE];
    let mut im = [0.0_f32; FFT_SIZE];

    // Apply Hann window to eliminate spectral leakage.
    let denom = (FFT_SIZE - 1) as f32;
    for i in 0..FFT_SIZE {
        let w = 0.5 * (1.0 - (2.0 * PI * i as f32 / denom).cos());
        re[i] = samples[i] * w;
    }

    fft_in_place(&mut re, &mut im);

    let half = FFT_SIZE / 2;
    let nyquist = (sample_rate as f32 * 0.5).max(4_000.0);
    let min_hz = 40.0_f32;
    let max_hz = nyquist.min(16_000.0);
    let log_min = min_hz.ln();
    let log_max = max_hz.ln();

    let mut bands = [0.0_f32; NUM_VIZ_BANDS];
    for (b, slot) in bands.iter_mut().enumerate() {
        let t0 = b as f32 / NUM_VIZ_BANDS as f32;
        let t1 = (b + 1) as f32 / NUM_VIZ_BANDS as f32;
        let f0 = (log_min + t0 * (log_max - log_min)).exp();
        let f1 = (log_min + t1 * (log_max - log_min)).exp();

        let bin0 = ((f0 / nyquist) * half as f32).floor() as usize;
        let bin1 = (((f1 / nyquist) * half as f32).ceil() as usize)
            .max(bin0 + 1)
            .min(half);

        let mut peak = 0.0_f32;
        for bin in bin0.clamp(1, half - 1)..bin1 {
            let mag = (re[bin] * re[bin] + im[bin] * im[bin]).sqrt() / (FFT_SIZE as f32 * 0.25);
            if mag > peak {
                peak = mag;
            }
        }

        // Gentle perceptual weighting + soft logarithmic compression into 0..1.
        let tilt = 1.0 + (b as f32 / NUM_VIZ_BANDS as f32) * 1.15;
        let scaled = (peak * tilt * 3.8).max(0.0);
        *slot = (scaled.ln_1p() / 2.2).clamp(0.0, 1.0);
    }

    bands
}

/// In-place Radix-2 Cooley-Tukey FFT (`re.len()` must be a power of 2).
fn fft_in_place(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    // Bit-reversal permutation
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let mut len = 2usize;
    while len <= n {
        let half = len >> 1;
        let ang = -2.0 * PI / len as f32;
        let wlen_re = ang.cos();
        let wlen_im = ang.sin();

        let mut i = 0usize;
        while i < n {
            let mut w_re = 1.0_f32;
            let mut w_im = 0.0_f32;
            for k in 0..half {
                let u_re = re[i + k];
                let u_im = im[i + k];
                let v_re = re[i + k + half] * w_re - im[i + k + half] * w_im;
                let v_im = re[i + k + half] * w_im + im[i + k + half] * w_re;

                re[i + k] = u_re + v_re;
                im[i + k] = u_im + v_im;
                re[i + k + half] = u_re - v_re;
                im[i + k + half] = u_im - v_im;

                let next_re = w_re * wlen_re - w_im * wlen_im;
                let next_im = w_re * wlen_im + w_im * wlen_re;
                w_re = next_re;
                w_im = next_im;
            }
            i += len;
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_detects_low_vs_high_sine_wave() {
        let sr = 44_100_u32;
        let mut low_samples = [0.0_f32; FFT_SIZE];
        let mut high_samples = [0.0_f32; FFT_SIZE];
        for i in 0..FFT_SIZE {
            let t = i as f32 / sr as f32;
            low_samples[i] = (2.0 * PI * 100.0 * t).sin();
            high_samples[i] = (2.0 * PI * 6_000.0 * t).sin();
        }
        let low_bands = compute_log_bands(&low_samples, sr);
        let high_bands = compute_log_bands(&high_samples, sr);

        assert!(avg(&low_bands[0..6]) > avg(&low_bands[20..32]));
        assert!(avg(&high_bands[20..32]) > avg(&high_bands[0..6]));
    }
}
