//! In-process audio-track decoding. No external processes or system codecs.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecParameters, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::{FileError, check_cancel};
use crate::audio::{TARGET_RATE, downmix, resample::resample};

/// Bounds memory on malformed or exceptionally long inputs: at most 128M native mono
/// frames (512 MiB), with at most two hours of audio. No microphone silence gate.
pub fn decode(path: &Path, cancel: &AtomicBool) -> Result<Vec<f32>, FileError> {
    check_cancel(cancel)?;
    let source = std::fs::File::open(path).map_err(|_| FileError::Decode("read"))?;
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let stream = MediaSourceStream::new(Box::new(source), Default::default());
    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|_| FileError::Decode("container"))?;
    let mut format = probed.format;
    // Video tracks lack an audio sample rate. Prefer the first supported audio track.
    let tracks = format.tracks().iter().filter(|t| t.codec_params.sample_rate.is_some()).collect::<Vec<_>>();
    let track = tracks
        .iter()
        .find(|t| symphonia::default::get_codecs().get_codec(t.codec_params.codec).is_some())
        .copied()
        .or_else(|| tracks.first().copied())
        .ok_or(FileError::Decode("audio track"))?;
    let params: CodecParameters = track.codec_params.clone();
    let id = track.id;
    let rate = params.sample_rate.filter(|r| (8000..=192000).contains(r)).ok_or(FileError::Decode("sample rate"))?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|_| FileError::Decode(codec_name(params.codec)))?;
    let mut mono = Vec::new();
    loop {
        check_cancel(cancel)?;
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(_) => return Err(FileError::Decode("packet")),
        };
        if packet.track_id() != id {
            continue;
        }
        let decoded = decoder.decode(&packet).map_err(|_| FileError::Decode(codec_name(params.codec)))?;
        if decoded.spec().rate != rate {
            return Err(FileError::Decode("sample rate change"));
        }
        let channels = decoded.spec().channels.count();
        if channels == 0 || channels > 32 {
            return Err(FileError::Decode("channels"));
        }
        if mono.len().saturating_add(decoded.frames()) > (128 * 1024 * 1024).min(rate as usize * 7200) {
            return Err(FileError::Limit);
        }
        mono.try_reserve(decoded.frames()).map_err(|_| FileError::Limit)?;
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        buffer.copy_interleaved_ref(decoded);
        mono.extend(downmix(buffer.samples(), channels as u16));
    }
    if mono.is_empty() {
        return Err(FileError::Decode("empty audio"));
    }
    check_cancel(cancel)?;
    let samples = resample(&mono, rate, TARGET_RATE);
    check_cancel(cancel)?;
    Ok(samples)
}

fn codec_name(codec: symphonia::core::codecs::CodecType) -> &'static str {
    use symphonia::core::codecs::*;
    match codec {
        CODEC_TYPE_OPUS => "Opus",
        CODEC_TYPE_AAC => "AAC",
        CODEC_TYPE_VORBIS => "Vorbis",
        CODEC_TYPE_FLAC => "FLAC",
        CODEC_TYPE_MP3 => "MP3",
        CODEC_TYPE_ALAC => "ALAC",
        _ => "audio codec",
    }
}
