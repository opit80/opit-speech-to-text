//! Audio preparation: everything between the microphone buffer and the upload.

pub mod encode;
pub mod gate;
pub mod resample;

/// Sample rate sent to providers.
pub const TARGET_RATE: u32 = 16_000;

/// Raw microphone capture: interleaved `f32` samples at the device rate.
#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Recording {
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }

    pub fn duration_ms(&self) -> u64 {
        if self.sample_rate == 0 {
            return 0;
        }
        self.frames() as u64 * 1000 / u64::from(self.sample_rate)
    }
}

/// Replaces NaN/∞ with silence and clamps to [-1, 1].
pub fn sanitize(sample: f32) -> f32 {
    if sample.is_finite() { sample.clamp(-1.0, 1.0) } else { 0.0 }
}

/// Averages interleaved channels into sanitized mono.
pub fn downmix(samples: &[f32], channels: u16) -> Vec<f32> {
    let channels = usize::from(channels.max(1));
    if channels == 1 {
        return samples.iter().copied().map(sanitize).collect();
    }
    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().copied().map(sanitize).sum::<f32>() / channels as f32)
        .collect()
}

pub fn to_mono_16k(recording: &Recording) -> Vec<f32> {
    let mono = downmix(&recording.samples, recording.channels);
    resample::resample(&mono, recording.sample_rate, TARGET_RATE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_clamps_and_zeroes_non_finite() {
        assert_eq!(sanitize(1.5), 1.0);
        assert_eq!(sanitize(-3.0), -1.0);
        assert_eq!(sanitize(f32::NAN), 0.0);
        assert_eq!(sanitize(f32::INFINITY), 0.0);
        assert_eq!(sanitize(0.25), 0.25);
    }

    #[test]
    fn downmix_averages_channels_and_sanitizes() {
        assert_eq!(downmix(&[0.25, 0.75, 1.0, f32::NAN], 2), vec![0.5, 0.5]);
        assert_eq!(downmix(&[2.0, 0.5], 1), vec![1.0, 0.5]);
    }

    #[test]
    fn duration_uses_frames_not_samples() {
        let rec = Recording { samples: vec![0.0; 96_000], sample_rate: 48_000, channels: 2 };
        assert_eq!(rec.frames(), 48_000);
        assert_eq!(rec.duration_ms(), 1_000);
        let broken = Recording { samples: vec![0.0; 10], sample_rate: 0, channels: 0 };
        assert_eq!(broken.duration_ms(), 0);
    }

    #[test]
    fn to_mono_16k_converts_stereo_48k() {
        let rec = Recording { samples: vec![0.0; 96_000], sample_rate: 48_000, channels: 2 };
        assert_eq!(to_mono_16k(&rec).len(), 16_000);
    }
}
