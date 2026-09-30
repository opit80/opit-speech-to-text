//! Band-limited sample-rate conversion.

use std::f64::consts::PI;

/// Kernel half-width measured in output samples.
const HALF_WIDTH_OUT: f64 = 16.0;
/// Fractional positions precomputed per kernel.
const PHASES: usize = 256;

/// Converts `input` from `from` Hz to `to` Hz.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() || from == 0 || to == 0 {
        return input.to_vec();
    }
    let scale = (f64::from(to) / f64::from(from)).min(1.0);
    let half = (HALF_WIDTH_OUT / scale).ceil() as usize;
    let taps = 2 * half;
    let table = kernel_table(half, 0.95 * scale);
    let step = f64::from(from) / f64::from(to);
    let out_len = (input.len() as f64 * f64::from(to) / f64::from(from)).floor() as usize;

    let mut out = Vec::with_capacity(out_len);
    for n in 0..out_len {
        let t = n as f64 * step;
        let base = t.floor();
        let phase = ((t - base) * PHASES as f64).round() as usize;
        let row = &table[phase * taps..(phase + 1) * taps];
        let first = base as isize - half as isize + 1;
        let mut acc = 0.0f32;
        for (k, weight) in row.iter().enumerate() {
            let index = first + k as isize;
            if index >= 0 && (index as usize) < input.len() {
                acc += input[index as usize] * weight;
            }
        }
        out.push(acc);
    }
    out
}

/// Blackman-windowed sinc weights for `PHASES + 1` fractional offsets,
/// each row normalized to unity DC gain.
fn kernel_table(half: usize, cutoff: f64) -> Vec<f32> {
    let taps = 2 * half;
    let width = half as f64;
    let mut table = vec![0.0f32; (PHASES + 1) * taps];
    let mut weights = vec![0.0f64; taps];
    for phase in 0..=PHASES {
        let frac = phase as f64 / PHASES as f64;
        for (k, weight) in weights.iter_mut().enumerate() {
            let x = k as f64 - width + 1.0 - frac;
            let sinc = if x.abs() < 1e-12 { 1.0 } else { (PI * cutoff * x).sin() / (PI * cutoff * x) };
            let window = if x.abs() >= width {
                0.0
            } else {
                0.42 + 0.5 * (PI * x / width).cos() + 0.08 * (2.0 * PI * x / width).cos()
            };
            *weight = sinc * window;
        }
        let sum: f64 = weights.iter().sum();
        let row = &mut table[phase * taps..(phase + 1) * taps];
        for (dst, weight) in row.iter_mut().zip(&weights) {
            *dst = (weight / sum) as f32;
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, seconds: f32, amp: f32) -> Vec<f32> {
        let n = (rate as f32 * seconds) as usize;
        (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin()).collect()
    }

    /// RMS of the middle half, away from edge effects.
    fn mid_rms(x: &[f32]) -> f32 {
        let mid = &x[x.len() / 4..x.len() * 3 / 4];
        (mid.iter().map(|s| s * s).sum::<f32>() / mid.len() as f32).sqrt()
    }

    #[test]
    fn same_rate_is_identity() {
        let x = vec![0.1, -0.2, 0.3];
        assert_eq!(resample(&x, 16_000, 16_000), x);
    }

    #[test]
    fn output_length_follows_the_ratio() {
        assert_eq!(resample(&vec![0.0; 48_000], 48_000, 16_000).len(), 16_000);
        assert_eq!(resample(&vec![0.0; 44_100], 44_100, 16_000).len(), 16_000);
        assert_eq!(resample(&vec![0.0; 8_000], 8_000, 16_000).len(), 16_000);
    }

    #[test]
    fn passband_tone_keeps_its_level() {
        let expected = 0.5 / 2f32.sqrt();
        for from in [48_000, 44_100, 8_000] {
            let out = resample(&sine(1_000.0, from, 1.0, 0.5), from, 16_000);
            let got = mid_rms(&out);
            assert!((got - expected).abs() / expected < 0.03, "{from}: {got}");
        }
    }

    #[test]
    fn tone_above_new_nyquist_is_removed() {
        let out = resample(&sine(10_000.0, 48_000, 1.0, 0.5), 48_000, 16_000);
        assert!(mid_rms(&out) < 0.01, "{}", mid_rms(&out));
    }
}
