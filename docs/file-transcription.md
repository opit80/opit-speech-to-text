# File transcription in Opit Speech to Text

Open **Files** (`#/files`), choose an audio or video file, review the active profile and
options, then choose **Transcribe file**. Dictation remains available during the job.
Only one file job runs at a time. Closing the window does not cancel it; reopen Files
to recover its progress or result. Cancel stops requests and retry waits; an active
decode/resample operation finishes its current CPU work before stopping.

## Formats and limits

Opit decodes locally in pure Rust. No external executable or system codec installation
is required. Supported combinations include WAV/PCM, MP3, FLAC, OGG/Vorbis, M4A/MP4
with AAC or ALAC, and MKV/WebM with a supported audio codec. Only an audio track is
used from video files, preferring the first supported audio track. A container name
alone does not guarantee codec support: Opus, encrypted tracks, damaged files and
unsupported codec variants return an error naming the format.

Audio is downmixed to mono and resampled to 16 kHz with Opit's existing resampler.
The file path is accepted only from the native file picker, never from a text field.
The decoder holds native-rate mono audio in memory, bounded at 128 million frames
(512 MiB), and at most two hours. For example, the frame limit allows about 46 minutes
at 48 kHz, or about 140 minutes at 16 kHz (subject to the two-hour cap). Resampling,
encoding and results use additional memory. Split larger files before importing.

Files are split at exact sample boundaries, looking for quiet 30 ms frames within the
last 10% (at most 15 seconds) of each target chunk. Chunks are conservatively capped
at roughly 6 minutes 15 seconds to leave room below a 24 MB upload ceiling even for
uncompressed audio and encoding overhead. They use the successful profile's FLAC or
WAV setting and the same quick retry, fallback and patient retry ladder as dictation.
Servers with smaller upload or duration limits can still reject a chunk.

## Text, subtitles and cleanup

The default language comes from the active profile. An override accepts `auto` or a
supported ISO language code, such as `tr`, `en` or `de`. Local deterministic rules
and the number-spelling setting apply when enabled for the successful profile.

Opit requests segment timestamps through `verbose_json` on compatible providers.
For a server that rejects that format with HTTP 400 or 422, it retries that request
once as JSON. ElevenLabs uses word timestamps grouped into short segments. Every
chunk's sample offset is added to its timings. Missing, invalid, overlapping or
incomplete timestamps disable SRT and TXT timestamp prefixes for the entire result;
the plain transcript remains available. Opit never invents timings.

TXT can include `[mm:ss]` or `[h:mm:ss]` line prefixes. SRT uses sequential cue numbers,
`hh:mm:ss,mmm` times, and physical text lines of at most 42 Unicode characters.
Review subtitles before publishing; wrapping does not retime or split cues.

AI cleanup is an explicit per-job option using the cleanup service and credential
configured in Settings. It uses separate original English/Turkish subtitle prompts,
not the dictation prompt override. The prompt preserves each marked line and its
meaning. Opit validates marker order, line count and non-shortening word counts;
invalid or failed answers keep the original rules-applied text and show a warning.
This structural validation cannot guarantee factual correctness. Review names and facts.

Speaker labels are offered only for an ElevenLabs profile and request `diarize=true`.
Identifiers are prefixed with the chunk number because provider speaker identities
cannot be matched reliably across separate requests. A fallback without diarization
can return unlabeled segments. Labels are identifiers, not inferred personal names.

## Privacy and cost

The selected transcription service receives encoded audio. Optional cleanup also
sends transcript text and the rule glossary to the configured cleanup service.
Provider charges, paid diarization, optional keyterms, cleanup and retries can add
cost. Opit does not make a cost estimate or guarantee a provider quota.

**File transcripts are never added to dictation history.** Source audio is read only;
results are retained only in memory until another file/job replaces them or the app
exits. Save as opens a native destination dialog and writes UTF-8 TXT or SRT. Copy
places the selected output on the clipboard. Transcript text, filenames and API keys
are not logged. Existing dictation history and audio retention are unaffected.

## Reusable engine

`opit_core::files::decode::decode` returns 16 kHz mono samples;
`opit_core::files::transcribe` takes a `PipelineContext`, optional `FileCleanup`,
cancellation flag, target chunk size and progress callback. `render_txt` and
`render_srt` export the result without filesystem side effects. These functions
have no Tauri or Windows dependency and can be called by a later CLI package.
Decode/encode are CPU and I/O work: desktop callers use `spawn_blocking` and run the
async provider work there with the runtime handle. `opit_app::file_jobs::start`
provides the desktop lifecycle hook independently of invoke command wrappers.
