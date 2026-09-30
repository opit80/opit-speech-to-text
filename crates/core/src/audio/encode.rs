//! 16-bit mono encoders for provider uploads.

use std::io::Cursor;

use flacenc::component::BitRepr;
use flacenc::error::Verify;
use serde::{Deserialize, Serialize};

use super::{TARGET_RATE, sanitize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    Flac,
    Wav,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedAudio {
    pub bytes: Vec<u8>,
    pub format: AudioFormat,
}

impl EncodedAudio {
    pub fn mime(&self) -> &'static str {
        match self.format {
            AudioFormat::Flac => "audio/flac",
            AudioFormat::Wav => "audio/wav",
        }
    }

    pub fn file_name(&self) -> &'static str {
        match self.format {
            AudioFormat::Flac => "audio.flac",
            AudioFormat::Wav => "audio.wav",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("audio encoding failed: {0}")]
pub struct EncodeError(pub String);

pub fn to_pcm16(samples: &[f32]) -> Vec<i16> {
    samples.iter().map(|&s| (sanitize(s) * f32::from(i16::MAX)).round() as i16).collect()
}

pub fn encode_wav(pcm: &[i16], sample_rate: u32) -> Result<Vec<u8>, EncodeError> {
    let spec =
        hound::WavSpec { channels: 1, sample_rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut bytes = Vec::with_capacity(44 + pcm.len() * 2);
    let err = |e: hound::Error| EncodeError(e.to_string());
    let mut writer = hound::WavWriter::new(Cursor::new(&mut bytes), spec).map_err(err)?;
    for &sample in pcm {
        writer.write_sample(sample).map_err(err)?;
    }
    writer.finalize().map_err(err)?;
    Ok(bytes)
}

pub fn encode_flac(pcm: &[i16], sample_rate: u32) -> Result<Vec<u8>, EncodeError> {
    let config = flacenc::config::Encoder::default().into_verified().map_err(|e| EncodeError(format!("{e:?}")))?;
    let samples: Vec<i32> = pcm.iter().map(|&s| i32::from(s)).collect();
    let source = flacenc::source::MemSource::from_samples(&samples, 1, 16, sample_rate as usize);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|e| EncodeError(format!("{e:?}")))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).map_err(|e| EncodeError(format!("{e:?}")))?;
    Ok(sink.into_inner())
}

/// Encodes 16 kHz mono samples in `format`.
pub fn encode(samples_16k: &[f32], format: AudioFormat) -> Result<EncodedAudio, EncodeError> {
    let pcm = to_pcm16(samples_16k);
    let bytes = match format {
        AudioFormat::Wav => encode_wav(&pcm, TARGET_RATE)?,
        AudioFormat::Flac => encode_flac(&pcm, TARGET_RATE)?,
    };
    Ok(EncodedAudio { bytes, format })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn tone_pcm() -> Vec<i16> {
        let samples: Vec<f32> =
            (0..16_000).map(|i| 0.3 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16_000.0).sin()).collect();
        to_pcm16(&samples)
    }

    #[test]
    fn pcm16_clamps_and_silences_bad_samples() {
        assert_eq!(to_pcm16(&[2.0, -2.0, f32::NAN, 0.5, 0.0]), vec![32767, -32767, 0, 16384, 0]);
    }

    #[test]
    fn wav_round_trip() {
        let pcm = tone_pcm();
        let wav = encode_wav(&pcm, 16_000).unwrap();
        let mut reader = hound::WavReader::new(Cursor::new(&wav)).unwrap();
        let spec = reader.spec();
        assert_eq!((spec.channels, spec.sample_rate, spec.bits_per_sample), (1, 16_000, 16));
        let decoded: Vec<i16> = reader.samples::<i16>().map(Result::unwrap).collect();
        assert_eq!(decoded, pcm);
    }

    #[test]
    fn flac_round_trip_and_smaller_than_wav() {
        let pcm = tone_pcm();
        let flac = encode_flac(&pcm, 16_000).unwrap();
        assert_eq!(&flac[..4], b"fLaC");
        let mut reader = claxon::FlacReader::new(Cursor::new(&flac)).unwrap();
        let decoded: Vec<i32> = reader.samples().map(Result::unwrap).collect();
        assert_eq!(decoded, pcm.iter().map(|&s| i32::from(s)).collect::<Vec<_>>());
        assert!(flac.len() < encode_wav(&pcm, 16_000).unwrap().len());
    }

    #[test]
    fn encode_sets_format_mime_and_file_name() {
        let wav = encode(&[0.0; 1600], AudioFormat::Wav).unwrap();
        assert_eq!((wav.mime(), wav.file_name()), ("audio/wav", "audio.wav"));
        let flac = encode(&[0.0; 1600], AudioFormat::Flac).unwrap();
        assert_eq!((flac.mime(), flac.file_name()), ("audio/flac", "audio.flac"));
        assert_eq!(flac.format, AudioFormat::Flac);
    }

    #[test]
    fn max_length_recording_fits_the_upload_limit_as_wav() {
        let ten_minutes = vec![0.0f32; 600 * 16_000];
        let wav = encode(&ten_minutes, AudioFormat::Wav).unwrap();
        assert!(wav.bytes.len() < 25 * 1024 * 1024, "{}", wav.bytes.len());
    }
}
