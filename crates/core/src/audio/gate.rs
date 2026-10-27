//! Decides whether a recording contains enough speech to be worth sending.
//! This is the main defence against hallucinations on silence.

use super::TARGET_RATE;
use crate::config::RecordingConfig;

pub const FRAME_MS: u64 = 30;
pub const MIN_RECORDING_MS: u64 = 400;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GateLevels {
    pub noise_floor_db: f32,
    /// 90th-percentile frame level, including when no speech passed the gate.
    pub speech_db: f32,
    pub voiced_seconds: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GateResult {
    pub verdict: GateVerdict,
    pub levels: GateLevels,
}

/// Finite meter floor for digital silence, suitable for IPC and overlays.
pub fn rms_to_db(level: f32) -> f32 {
    if !level.is_finite() || level <= 0.000_001 { -120.0 } else { 20.0 * level.log10() }
}

pub fn db_to_rms(db: f64) -> f32 {
    10.0_f64.powf(db / 20.0) as f32
}

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
    analyze(samples_16k, &RecordingConfig::default()).verdict
}

pub fn analyze(samples_16k: &[f32], config: &RecordingConfig) -> GateResult {
    let duration_ms = samples_16k.len() as u64 * 1000 / u64::from(TARGET_RATE);
    let frame_len = (u64::from(TARGET_RATE) * FRAME_MS / 1000) as usize;
    let levels: Vec<f32> = samples_16k.chunks(frame_len).map(rms).collect();
    let mut sorted = levels.to_vec();
    sorted.sort_by(f32::total_cmp);
    let floor = sorted.get(sorted.len() / 10).copied().unwrap_or(0.0);
    let loud = sorted.get(sorted.len() * 9 / 10).copied().unwrap_or(0.0);
    let minimum = db_to_rms(config.speech_floor_db);
    let margin = db_to_rms(config.noise_margin_db);
    // The original loud-speech escape hatch was 0.02 / 0.008 = 2.5x the floor.
    let loud_speech = db_to_rms(config.speech_floor_db + 7.958800173440752);
    // Flat broadband hiss must not become speech when a user lowers the floor.
    // Zero crossings distinguish hiss from sustained voiced sounds; bursts still pass.
    let crossings = samples_16k.windows(2).filter(|w| w[0].is_sign_positive() != w[1].is_sign_positive()).count();
    let flat_hiss =
        rms_to_db(loud) - rms_to_db(floor) < 3.0 && crossings as f64 / samples_16k.len().max(1) as f64 > 0.25;
    // On flat hiss, count only frames that rise clearly above its floor. This
    // preserves sparse speech bursts without letting one click admit all the hiss.
    let dynamic_margin = if flat_hiss { margin.max(db_to_rms(3.0)) } else { margin };
    let speech_frames = levels
        .iter()
        .filter(|&&level| level > minimum && (level > floor * dynamic_margin || (!flat_hiss && level > loud_speech)))
        .count();
    let speech_ms = speech_frames as u64 * FRAME_MS;
    let verdict = if duration_ms < MIN_RECORDING_MS {
        GateVerdict::TooShort
    } else if (speech_ms as f64) < config.min_speech_seconds * 1000.0 {
        GateVerdict::NoSpeech
    } else {
        GateVerdict::Speech { speech_ms }
    };
    GateResult {
        verdict,
        levels: GateLevels {
            noise_floor_db: rms_to_db(floor),
            speech_db: rms_to_db(loud),
            voiced_seconds: speech_ms as f64 / 1000.0,
        },
    }
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
    fn db_defaults_and_measurements_match_linear_thresholds() {
        let config = crate::config::RecordingConfig::default();
        assert_eq!(db_to_rms(config.speech_floor_db), 0.008);
        assert_eq!(db_to_rms(config.noise_margin_db), 2.0);
        assert_eq!(db_to_rms(config.speech_floor_db + 7.958800173440752), 0.02);
        assert!((rms_to_db(0.01) + 40.0).abs() < 0.001);
        assert_eq!(rms_to_db(0.0), -120.0);
        let mut samples = vec![0.001; 16_000];
        samples.extend(vec![0.03; 16_000]);
        let result = analyze(&samples, &config);
        assert!(matches!(result.verdict, GateVerdict::Speech { .. }));
        assert!((result.levels.noise_floor_db + 60.0).abs() < 0.01);
        assert!((result.levels.speech_db - rms_to_db(0.03)).abs() < 0.01);
        assert!(result.levels.voiced_seconds >= 0.99);
    }

    #[test]
    fn calibration_allows_quiet_speech_but_rejects_flat_hiss_at_multiple_levels() {
        let config = crate::config::RecordingConfig { speech_floor_db: -65.0, ..Default::default() };
        for amp in [0.001, 0.01, 0.026, 0.05, 0.2] {
            assert_eq!(analyze(&noise(2.0, amp), &config).verdict, GateVerdict::NoSpeech);
        }
        let mut quiet = silence(1.0);
        quiet.extend(tone(0.6, 0.003));
        assert_eq!(analyze(&quiet, &Default::default()).verdict, GateVerdict::NoSpeech);
        assert!(matches!(analyze(&quiet, &config).verdict, GateVerdict::Speech { .. }));
        let strict = crate::config::RecordingConfig { min_speech_seconds: 1.0, ..config };
        assert_eq!(analyze(&quiet, &strict).verdict, GateVerdict::NoSpeech);
    }

    #[test]
    fn margin_controls_speech_below_the_loud_bypass_and_empty_levels_are_finite() {
        let mut samples = vec![0.01; 16_000];
        samples.extend(tone(1.0, 0.025));
        assert_eq!(check(&samples), GateVerdict::NoSpeech);
        let calibrated = RecordingConfig { noise_margin_db: 2.0, ..Default::default() };
        assert!(matches!(analyze(&samples, &calibrated).verdict, GateVerdict::Speech { .. }));
        let empty = analyze(&[], &calibrated);
        assert_eq!(empty.verdict, GateVerdict::TooShort);
        assert_eq!(empty.levels, GateLevels { noise_floor_db: -120.0, speech_db: -120.0, voiced_seconds: 0.0 });
        let silence = analyze(&silence(1.0), &calibrated);
        assert_eq!(silence.verdict, GateVerdict::NoSpeech);
        assert_eq!(silence.levels, empty.levels);
    }

    #[test]
    fn sparse_speech_over_hiss_passes_but_one_click_does_not_open_the_gate() {
        let mut samples = noise(10.0, 0.05);
        let speech: Vec<f32> = tone(0.6, 0.2).iter().zip(noise(0.6, 0.05)).map(|(s, n)| s + n).collect();
        samples.extend(speech);
        let result = analyze(&samples, &RecordingConfig::default());
        assert!(matches!(result.verdict, GateVerdict::Speech { .. }));
        assert!(result.levels.voiced_seconds < 1.0);
        let mut click = noise(10.0, 0.05);
        click.extend(tone(0.01, 0.5));
        assert_eq!(check(&click), GateVerdict::NoSpeech);
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

    #[test]
    fn speech_over_moderate_noise_passes() {
        let mut samples = noise(1.0, 0.02);
        let speech: Vec<f32> = tone(1.0, 0.035).iter().zip(noise(1.0, 0.02)).map(|(s, n)| s + n).collect();
        samples.extend(speech);
        assert!(matches!(check(&samples), GateVerdict::Speech { .. }), "{:?}", check(&samples));
    }

    #[test]
    fn continuous_quiet_speech_passes() {
        assert!(matches!(check(&tone(1.0, 0.035)), GateVerdict::Speech { .. }));
    }
}
