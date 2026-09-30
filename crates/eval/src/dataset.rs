//! Loads `name.wav` + `name.txt` pairs.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use opit_core::audio::Recording;

pub struct Sample {
    pub name: String,
    pub recording: Recording,
    pub reference: String,
}

/// Loads `name.wav` + `name.txt` pairs from `dir`, sorted by name. A WAV file
/// without a transcript is skipped and reported in the returned warnings.
pub fn load_dir(dir: &Path) -> Result<(Vec<Sample>, Vec<String>)> {
    let mut wavs: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("wav")))
        .collect();
    wavs.sort();

    let mut samples = Vec::new();
    let mut warnings = Vec::new();
    for wav in wavs {
        let name = wav.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let Ok(reference) = std::fs::read_to_string(wav.with_extension("txt")) else {
            warnings.push(format!("{name}: no {name}.txt next to the WAV file, skipped"));
            continue;
        };
        let recording = read_wav(&wav).with_context(|| format!("cannot decode {}", wav.display()))?;
        samples.push(Sample { name, recording, reference: reference.trim().to_string() });
    }
    Ok((samples, warnings))
}

pub fn read_wav(path: &Path) -> Result<Recording> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
        }
    };
    Ok(Recording { samples, sample_rate: spec.sample_rate, channels: spec.channels })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, samples: &[i16]) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for &s in samples {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn loads_pairs_and_warns_about_missing_transcripts() {
        let dir = tempfile::tempdir().unwrap();
        write_wav(&dir.path().join("b.wav"), &[0; 160]);
        write_wav(&dir.path().join("a.wav"), &[16_384; 160]);
        std::fs::write(dir.path().join("a.txt"), "  Merhaba dünya.\n").unwrap();
        std::fs::write(dir.path().join("notes.md"), "ignored").unwrap();

        let (samples, warnings) = load_dir(dir.path()).unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].name, "a");
        assert_eq!(samples[0].reference, "Merhaba dünya.");
        assert_eq!(samples[0].recording.sample_rate, 16_000);
        assert!((samples[0].recording.samples[0] - 0.5).abs() < 1e-6);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("b:"), "{}", warnings[0]);
    }

    #[test]
    fn missing_dir_is_an_error() {
        assert!(load_dir(Path::new("definitely/not/here")).is_err());
    }
}
