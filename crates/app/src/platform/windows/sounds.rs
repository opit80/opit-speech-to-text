//! UI sounds synthesized in code and played with `PlaySoundW` (no asset files).

use std::f32::consts::TAU;
use std::sync::OnceLock;

use opit_core::audio::encode::encode_wav;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows::core::PCWSTR;

use crate::platform::{Sound, Sounds};

const RATE: u32 = 44_100;
const AMPLITUDE: f32 = 0.065;
const ATTACK_MS: u32 = 30;
const RELEASE_MS: u32 = 90;
const ROOM_TAPS: [(u32, f32); 2] = [(19, 0.10), (43, 0.06)];

/// Overlapping mallet notes, with independent onset, duration and gain.
#[derive(Clone, Copy)]
struct Note {
    hz: f32,
    start_ms: u32,
    duration_ms: u32,
    gain: f32,
}

const START: &[Note] = &[
    Note { hz: 349.23, start_ms: 0, duration_ms: 200, gain: 0.8 },
    Note { hz: 392.0, start_ms: 75, duration_ms: 250, gain: 0.65 },
];
const DONE: &[Note] = &[
    Note { hz: 392.0, start_ms: 0, duration_ms: 200, gain: 0.7 },
    Note { hz: 523.25, start_ms: 92, duration_ms: 260, gain: 0.7 },
];
const CANCEL: &[Note] = &[Note { hz: 329.63, start_ms: 0, duration_ms: 180, gain: 0.65 }];
const ERROR: &[Note] = &[
    Note { hz: 293.66, start_ms: 0, duration_ms: 220, gain: 0.8 },
    Note { hz: 261.63, start_ms: 99, duration_ms: 250, gain: 0.7 },
];

/// WAV files for every [`Sound`], built once. `SND_MEMORY | SND_ASYNC` reads the buffer while
/// playing, so it must live for the whole process: `'static` storage guarantees that.
static WAVS: OnceLock<[Vec<u8>; 4]> = OnceLock::new();

/// [`Sounds`] via the Win32 `PlaySoundW` API.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinSounds;

impl WinSounds {
    pub fn new() -> Self {
        Self
    }

    /// Builds the WAV cache ahead of the first `play`.
    pub fn warm_up(&self) {
        wavs();
    }
}

impl Sounds for WinSounds {
    fn play(&self, sound: Sound) {
        let wav = &wavs()[index(sound)];
        if wav.is_empty() {
            return;
        }
        // SAFETY: `wav` is a complete RIFF/WAVE image in 'static memory, as SND_MEMORY requires.
        // With SND_ASYNC the call returns immediately; a later call replaces the playing sound.
        let ok = unsafe { PlaySoundW(PCWSTR(wav.as_ptr().cast()), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT) };
        if !ok.as_bool() {
            tracing::debug!(?sound, "PlaySoundW failed");
        }
    }
}

fn index(sound: Sound) -> usize {
    match sound {
        Sound::Start => 0,
        Sound::Done => 1,
        Sound::Cancel => 2,
        Sound::Error => 3,
    }
}

fn wavs() -> &'static [Vec<u8>; 4] {
    WAVS.get_or_init(|| {
        [START, DONE, CANCEL, ERROR].map(|notes| {
            encode_wav(&melody(notes), RATE).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "could not build a UI sound");
                Vec::new()
            })
        })
    })
}

fn melody(notes: &[Note]) -> Vec<i16> {
    let duration_ms = notes.iter().map(|note| note.start_ms + note.duration_ms).max().unwrap_or(0);
    let mut dry = vec![0.0; (RATE * duration_ms / 1000) as usize];
    for note in notes {
        let offset = (RATE * note.start_ms / 1000) as usize;
        for (index, sample) in tone(note.hz, note.duration_ms).into_iter().enumerate() {
            dry[offset + index] += f32::from(sample) * note.gain;
        }
    }
    // Very quiet early reflections soften the notes without a long reverberant tail.
    let tail = ROOM_TAPS.iter().map(|(ms, _)| RATE * ms / 1000).max().unwrap_or(0) as usize;
    let mut wet = vec![0.0; dry.len() + tail];
    wet[..dry.len()].copy_from_slice(&dry);
    for (delay_ms, gain) in ROOM_TAPS {
        let delay = (RATE * delay_ms / 1000) as usize;
        for (index, sample) in dry.iter().enumerate() {
            wet[index + delay] += sample * gain;
        }
    }
    wet.into_iter().map(|sample| sample.round() as i16).collect()
}

/// A rounded mallet note: smooth edges, decaying partials and subtle pitch movement.
fn tone(freq: f32, ms: u32) -> Vec<i16> {
    let len = (RATE * ms / 1000) as usize;
    let attack = ((RATE * ATTACK_MS / 1000) as usize).min(len / 2).max(1);
    let release = ((RATE * RELEASE_MS / 1000) as usize).min(len / 2).max(1);
    let mut phase = 0.0;
    (0..len)
        .map(|i| {
            if freq <= 0.0 {
                return 0;
            }
            let time = i as f32 / RATE as f32;
            let fade_in = ((i as f32 / attack as f32).min(1.0) * TAU / 4.0).sin().powi(2);
            let fade_out = (((len - 1 - i) as f32 / release as f32).min(1.0) * TAU / 4.0).sin().powi(2);
            let envelope = fade_in * fade_out * (-4.8 * time).exp();
            // A tiny settling glide and slow flutter keep the body from sounding like a fixed beep.
            let cents = -10.0 * (-time / 0.08).exp() + 4.0 * (TAU * 3.7 * time).sin();
            phase += TAU * freq * 2.0f32.powf(cents / 1200.0) / RATE as f32;
            let body = 0.82 * phase.sin()
                + 0.12 * (phase * 2.003).sin() * (-9.0 * time).exp()
                + 0.05 * (phase * 3.99).sin() * (-16.0 * time).exp()
                + 0.01 * (phase * 6.12).sin() * (-24.0 * time).exp();
            let flutter = 0.96 + 0.04 * (TAU * 5.2 * time).cos();
            let sample = body * AMPLITUDE * envelope * flutter;
            (sample * f32::from(i16::MAX)).round() as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(pcm: &[i16]) -> i16 {
        pcm.iter().map(|s| s.saturating_abs()).max().unwrap_or(0)
    }

    #[test]
    fn tone_has_expected_length_peak_and_fades() {
        let pcm = tone(392.0, 250);
        assert_eq!(pcm.len(), 11025);
        let max = (AMPLITUDE * f32::from(i16::MAX)) as i16;
        assert!(peak(&pcm) <= max && peak(&pcm) > max / 4, "peak {}", peak(&pcm));
        assert_eq!(pcm[0], 0, "starts silent");
        assert!(pcm[pcm.len() - 1].abs() < 50, "ends near silence");
        assert!(peak(&pcm[..20]) < max / 2, "fade-in");
        assert!(peak(&pcm[pcm.len() - 1000..]) < peak(&pcm[2000..4000]) / 4, "gentle decay");
    }

    #[test]
    fn rest_is_silent() {
        assert!(tone(0.0, 40).iter().all(|&s| s == 0));
        assert_eq!(tone(0.0, 40).len(), 1764);
    }

    #[test]
    fn every_sound_is_a_short_wav() {
        for (i, wav) in wavs().iter().enumerate() {
            assert_eq!(&wav[..4], b"RIFF", "sound {i}");
            assert_eq!(&wav[8..12], b"WAVE", "sound {i}");
            let ms = (wav.len() - 44) as u32 / 2 * 1000 / RATE;
            assert!((150..=500).contains(&ms), "sound {i} is {ms} ms");
        }
    }

    #[test]
    fn cues_are_quiet_and_have_no_abrupt_edges() {
        for notes in [START, DONE, CANCEL, ERROR] {
            let pcm = melody(notes);
            assert_eq!(pcm[0], 0);
            assert_eq!(pcm[pcm.len() - 1], 0);
            assert!(peak(&pcm) < (0.115 * f32::from(i16::MAX)) as i16, "cue is too loud");
            let largest_step =
                pcm.windows(2).map(|pair| (i32::from(pair[1]) - i32::from(pair[0])).abs()).max().unwrap();
            assert!(largest_step < 400, "abrupt sample transition: {largest_step}");
        }
    }

    #[test]
    #[ignore = "writes a listening preview"]
    fn export_listening_preview() {
        let dir = std::path::PathBuf::from(std::env::var_os("OPIT_SOUND_PREVIEW_DIR").expect("preview directory"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut combined = vec![0i16; RATE as usize / 4];
        for (name, notes) in [("start", START), ("done", DONE), ("cancel", CANCEL), ("error", ERROR)] {
            let pcm = melody(notes);
            std::fs::write(dir.join(format!("{name}.wav")), encode_wav(&pcm, RATE).unwrap()).unwrap();
            combined.extend(pcm);
            combined.resize(combined.len() + RATE as usize * 3 / 5, 0);
        }
        std::fs::write(dir.join("gentle-chimes.wav"), encode_wav(&combined, RATE).unwrap()).unwrap();
    }

    /// Plays each sound. Run with `--ignored` (audible).
    #[test]
    #[ignore = "plays audio"]
    fn smoke_play_all() {
        let sounds = WinSounds::new();
        sounds.warm_up();
        for sound in [Sound::Start, Sound::Done, Sound::Cancel, Sound::Error] {
            println!("playing {sound:?}");
            sounds.play(sound);
            std::thread::sleep(std::time::Duration::from_millis(600));
        }
    }
}
