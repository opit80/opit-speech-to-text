//! UI sounds synthesized in code and played with `PlaySoundW` (no asset files).

use std::f32::consts::TAU;
use std::sync::OnceLock;

use opit_core::audio::encode::encode_wav;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows::core::PCWSTR;

use crate::platform::{Sound, Sounds};

const RATE: u32 = 44_100;
const AMPLITUDE: f32 = 0.25;
const FADE_MS: u32 = 5;

/// One note: frequency in Hz (0 = silence) and length in ms.
type Note = (f32, u32);

const START: &[Note] = &[(660.0, 70), (880.0, 70)];
const DONE: &[Note] = &[(784.0, 70), (1175.0, 110)];
const CANCEL: &[Note] = &[(440.0, 110)];
const ERROR: &[Note] = &[(330.0, 90), (0.0, 40), (262.0, 130)];

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
    notes.iter().flat_map(|&(freq, ms)| tone(freq, ms)).collect()
}

/// A sine note with linear fade in/out, mono 16-bit at [`RATE`].
fn tone(freq: f32, ms: u32) -> Vec<i16> {
    let len = (RATE * ms / 1000) as usize;
    let fade = ((RATE * FADE_MS / 1000) as usize).min(len / 2).max(1);
    (0..len)
        .map(|i| {
            if freq <= 0.0 {
                return 0;
            }
            let envelope = (i.min(len - 1 - i) as f32 / fade as f32).min(1.0);
            let sample = (TAU * freq * i as f32 / RATE as f32).sin() * AMPLITUDE * envelope;
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
        let pcm = tone(660.0, 70);
        assert_eq!(pcm.len(), 3087);
        let max = (AMPLITUDE * f32::from(i16::MAX)) as i16;
        assert!(peak(&pcm) <= max + 1 && peak(&pcm) > max / 10 * 9, "peak {}", peak(&pcm));
        assert_eq!(pcm[0], 0, "starts silent");
        assert!(pcm[pcm.len() - 1].abs() < 50, "ends near silence");
        assert!(peak(&pcm[..20]) < max / 2, "fade-in");
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
            assert!((100..=300).contains(&ms), "sound {i} is {ms} ms");
        }
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
