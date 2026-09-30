//! Decides whether a recording contains enough speech to be worth sending.
//! This is the main defence against hallucinations on silence.

use super::TARGET_RATE;

pub const FRAME_MS: u64 = 30;
pub const MIN_RECORDING_MS: u64 = 400;
pub const MIN_SPEECH_MS: u64 = 300;

const MIN_THRESHOLD: f32 = 0.008;
const MAX_THRESHOLD: f32 = 0.03;
const NOISE_FLOOR_FACTOR: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateVerdict {
    Speech { speech_ms: u64 },
    TooShort,
    NoSpeech,
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Classifies 16 kHz mono samples.
pub fn check(samples_16k: &[f32]) -> GateVerdict {
    let duration_ms = samples_16k.len() as u64 * 1000 / u64::from(TARGET_RATE);
    if duration_ms < MIN_RECORDING_MS {
        return GateVerdict::TooShort;
    }
    let frame_len = (u64::from(TARGET_RATE) * FRAME_MS / 1000) as usize;
    let levels: Vec<f32> = samples_16k.chunks(frame_len).map(rms).collect();
    let threshold = speech_threshold(&levels);
    let speech_ms = levels.iter().filter(|&&level| level > threshold).count() as u64 * FRAME_MS;
    if speech_ms < MIN_SPEECH_MS { GateVerdict::NoSpeech } else { GateVerdict::Speech { speech_ms } }
}

fn speech_threshold(levels: &[f32]) -> f32 {
    let mut sorted = levels.to_vec();
    sorted.sort_by(f32::total_cmp);
    let noise_floor = sorted[sorted.len() / 10];
    (noise_floor * NOISE_FLOOR_FACTOR).clamp(MIN_THRESHOLD, MAX_THRESHOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f32, amp: f32) -> Vec<f32> {
        let n = (16_000.0 * seconds) as usize;
        (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 16_000.0).sin()).collect()
    }

    fn silence(seconds: f32) -> Vec<f32> {
        vec![0.0; (16_000.0 * seconds) as usize]
    }

    /// Deterministic uniform noise in [-amp, amp].
    fn noise(seconds: f32, amp: f32) -> Vec<f32> {
        let mut state: u32 = 12_345;
        (0..(16_000.0 * seconds) as usize)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((state >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0) * amp
            })
            .collect()
    }

    #[test]
    fn rms_values() {
        assert_eq!(rms(&[]), 0.0);
        assert!((rms(&[0.5, -0.5]) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn short_recordings_are_too_short() {
        assert_eq!(check(&tone(0.3, 0.3)), GateVerdict::TooShort);
        assert_eq!(check(&[]), GateVerdict::TooShort);
    }

    #[test]
    fn silence_and_quiet_noise_have_no_speech() {
        assert_eq!(check(&silence(1.0)), GateVerdict::NoSpeech);
        assert_eq!(check(&noise(2.0, 0.004)), GateVerdict::NoSpeech);
    }

    #[test]
    fn steady_fan_noise_has_no_speech() {
        assert_eq!(check(&noise(2.0, 0.026)), GateVerdict::NoSpeech);
    }

    #[test]
    fn continuous_speech_passes() {
        assert!(matches!(check(&tone(1.0, 0.3)), GateVerdict::Speech { speech_ms } if speech_ms >= 900));
    }

    #[test]
    fn quiet_speech_after_silence_passes() {
        let mut samples = silence(1.5);
        samples.extend(tone(0.5, 0.02));
        assert!(matches!(check(&samples), GateVerdict::Speech { .. }));
    }

    #[test]
    fn a_short_burst_is_not_enough() {
        let mut samples = silence(1.8);
        samples.extend(tone(0.2, 0.3));
        assert_eq!(check(&samples), GateVerdict::NoSpeech);
    }
}
