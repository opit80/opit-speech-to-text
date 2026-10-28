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

/// Incremental mono conversion with the same anti-aliasing kernel as batch preparation.
/// Retains only the kernel overlap; output positions never restart at chunk boundaries.
pub struct StreamingResampler {
    from: u32,
    to: u32,
    half: usize,
    table: Vec<f32>,
    buffer: Vec<f32>,
    offset: usize,
    received: usize,
    produced: usize,
}

impl StreamingResampler {
    pub fn new(from: u32, to: u32) -> Option<Self> {
        if from == 0 || to == 0 || from > 384_000 || to > 384_000 {
            return None;
        }
        let scale = (f64::from(to) / f64::from(from)).min(1.0);
        let half = (HALF_WIDTH_OUT / scale).ceil() as usize;
        Some(Self {
            from,
            to,
            half,
            table: if from == to { Vec::new() } else { kernel_table(half, 0.95 * scale) },
            buffer: Vec::new(),
            offset: 0,
            received: 0,
            produced: 0,
        })
    }

    pub fn push(&mut self, input: &[f32]) -> Vec<i16> {
        if self.from == self.to {
            return input.iter().map(|s| pcm16(*s)).collect();
        }
        self.received += input.len();
        self.buffer.extend_from_slice(input);
        self.emit(false)
    }

    pub fn finish(&mut self) -> Vec<i16> {
        self.emit(true)
    }

    fn emit(&mut self, finish: bool) -> Vec<i16> {
        let mut output = Vec::new();
        let end = (self.received as u64 * u64::from(self.to) / u64::from(self.from)) as usize;
        let step = f64::from(self.from) / f64::from(self.to);
        while self.produced < end {
            let position = self.produced as f64 * step;
            let base = position.floor() as usize;
            if !finish && base + self.half >= self.received {
                break;
            }
            let phase = ((position - base as f64) * PHASES as f64).round() as usize;
            let taps = 2 * self.half;
            let first = base as isize - self.half as isize + 1;
            let mut value = 0.0;
            for (k, weight) in self.table[phase * taps..(phase + 1) * taps].iter().enumerate() {
                let index = first + k as isize;
                if index >= self.offset as isize && (index as usize) < self.received {
                    value += self.buffer[index as usize - self.offset] * weight;
                }
            }
            output.push(pcm16(value));
            self.produced += 1;
        }
        let retain_from = ((self.produced as f64 * step).floor() as usize).saturating_sub(self.half);
        let discard = retain_from.saturating_sub(self.offset).min(self.buffer.len());
        self.buffer.drain(..discard);
        self.offset += discard;
        output
    }
}

fn pcm16(value: f32) -> i16 {
    if value.is_finite() { (value.clamp(-1.0, 1.0) * 32767.0).round() as i16 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incremental_matches_batch_across_arbitrary_boundaries_and_retains_only_overlap() {
        for rate in [8_000, 16_000, 44_100, 48_000, 96_000] {
            let input = sine(700.0, rate, 0.75, 0.5);
            let expected: Vec<_> = resample(&input, rate, 16_000).into_iter().map(pcm16).collect();
            let mut stream = StreamingResampler::new(rate, 16_000).unwrap();
            let mut actual = Vec::new();
            for chunk in input.chunks(137) {
                actual.extend(stream.push(chunk));
                assert!(stream.buffer.len() < 2 * stream.half + 10);
            }
            actual.extend(stream.finish());
            assert_eq!(actual, expected, "rate {rate}");
        }
    }

    #[test]
    fn incremental_rejects_invalid_rates_and_sanitizes_samples() {
        assert!(StreamingResampler::new(0, 16_000).is_none());
        let mut stream = StreamingResampler::new(16_000, 16_000).unwrap();
        assert_eq!(stream.push(&[f32::NAN, f32::INFINITY, 2.0, -2.0]), [0, 0, 32767, -32767]);
        assert!(stream.finish().is_empty());
    }

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
