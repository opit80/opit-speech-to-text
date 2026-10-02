//! Recording → transcript: prepare audio, call the provider (retry, fallback),
//! then post-process with the rule layer.

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::audio::encode::{EncodeError, EncodedAudio, encode};
use crate::audio::gate::{self, GateVerdict};
use crate::audio::{self, Recording, TARGET_RATE};
use crate::provider::retry::with_retry;
use crate::provider::{ProviderError, RawTranscript, TranscribeRequest, Transcriber};
use crate::rules::prompt::{build_prompt, keywords};
use crate::rules::{RuleHit, RuleSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptStatus {
    Ok,
    /// The provider returned nothing.
    Empty,
    /// The whole output matched a known silence hallucination.
    Hallucination,
}

impl TranscriptStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Empty => "empty",
            Self::Hallucination => "hallucination",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ok" => Some(Self::Ok),
            "empty" => Some(Self::Empty),
            "hallucination" => Some(Self::Hallucination),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Transcript {
    /// Provider output before rules.
    pub raw_text: String,
    /// Text to paste.
    pub text: String,
    pub status: TranscriptStatus,
    /// Profile that produced the text (differs from the primary after a fallback).
    pub profile_id: String,
    pub used_fallback: bool,
    pub audio_ms: u64,
    /// Wall time from the first request to the final answer, retries included.
    pub request_ms: u64,
    pub hits: Vec<RuleHit>,
}

/// 16 kHz mono audio that passed the silence gate; kept for "Try again".
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedAudio {
    pub samples: Vec<f32>,
    pub audio_ms: u64,
    pub speech_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PipelineError {
    #[error("the recording is too short")]
    TooShort,
    #[error("no speech was detected")]
    NoSpeech,
    #[error(transparent)]
    Encode(#[from] EncodeError),
    #[error(transparent)]
    Provider(#[from] ProviderError),
}

pub struct PipelineContext<'a, T> {
    pub primary: &'a T,
    pub fallback: Option<&'a T>,
    pub rules: &'a RuleSet,
    pub prompt_context: &'a str,
    pub retry_delay: Duration,
    pub numbers_as_words: bool,
}

pub fn prepare(recording: &Recording) -> Result<PreparedAudio, PipelineError> {
    let samples = audio::to_mono_16k(recording);
    match gate::check(&samples) {
        GateVerdict::TooShort => Err(PipelineError::TooShort),
        GateVerdict::NoSpeech => Err(PipelineError::NoSpeech),
        GateVerdict::Speech { speech_ms } => {
            let audio_ms = samples.len() as u64 * 1000 / u64::from(TARGET_RATE);
            Ok(PreparedAudio { samples, audio_ms, speech_ms })
        }
    }
}

pub async fn transcribe<T: Transcriber>(
    audio: &PreparedAudio,
    ctx: &PipelineContext<'_, T>,
) -> Result<Transcript, PipelineError> {
    let started = Instant::now();
    let primary_audio = encode(&audio.samples, ctx.primary.profile().audio_format)?;
    let (raw, used, used_fallback) = match call(ctx.primary, &primary_audio, audio, ctx, true).await {
        Ok(raw) => (raw, ctx.primary, false),
        Err(err) => match ctx.fallback {
            Some(fallback) if err.is_retryable() => {
                let format = fallback.profile().audio_format;
                let fallback_audio;
                let encoded = if format == primary_audio.format {
                    &primary_audio
                } else {
                    fallback_audio = encode(&audio.samples, format)?;
                    &fallback_audio
                };
                (call(fallback, encoded, audio, ctx, false).await?, fallback, true)
            }
            _ => return Err(err.into()),
        },
    };

    let profile = used.profile();
    let (text, status, hits) =
        postprocess_with_numbers(&raw.text, ctx.rules, profile.apply_rules, ctx.numbers_as_words, &profile.language);
    Ok(Transcript {
        raw_text: raw.text,
        text,
        status,
        profile_id: profile.id.clone(),
        used_fallback,
        audio_ms: audio.audio_ms,
        request_ms: started.elapsed().as_millis() as u64,
        hits,
    })
}

pub fn postprocess(raw: &str, rules: &RuleSet, apply_rules: bool) -> (String, TranscriptStatus, Vec<RuleHit>) {
    postprocess_with_numbers(raw, rules, apply_rules, false, "tr")
}

pub fn postprocess_with_numbers(
    raw: &str,
    rules: &RuleSet,
    apply_rules: bool,
    numbers_as_words: bool,
    language: &str,
) -> (String, TranscriptStatus, Vec<RuleHit>) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (String::new(), TranscriptStatus::Empty, Vec::new());
    }
    if !apply_rules {
        return (trimmed.to_string(), TranscriptStatus::Ok, Vec::new());
    }
    if rules.is_hallucination(trimmed) {
        return (String::new(), TranscriptStatus::Hallucination, Vec::new());
    }
    let (mut text, mut hits) = rules.apply_traced(trimmed);
    if numbers_as_words {
        let (converted, number_hits) = crate::rules::numbers::spell_integers(&text, language);
        text = converted;
        hits.extend(number_hits);
    }
    (text, TranscriptStatus::Ok, hits)
}

pub async fn run<T: Transcriber>(
    recording: &Recording,
    ctx: &PipelineContext<'_, T>,
) -> Result<Transcript, PipelineError> {
    let audio = prepare(recording)?;
    transcribe(&audio, ctx).await
}

async fn call<T: Transcriber>(
    transcriber: &T,
    encoded: &EncodedAudio,
    audio: &PreparedAudio,
    ctx: &PipelineContext<'_, T>,
    retry: bool,
) -> Result<RawTranscript, ProviderError> {
    let profile = transcriber.profile();
    let prompt = if profile.send_prompt {
        build_prompt(ctx.prompt_context, ctx.rules.terms(), &profile.language).prompt
    } else {
        None
    };
    let keyword_list = if profile.send_keywords { keywords(ctx.rules.terms()) } else { Vec::new() };
    let request = TranscribeRequest {
        audio: encoded,
        audio_ms: audio.audio_ms,
        prompt: prompt.as_deref(),
        keywords: &keyword_list,
    };
    if retry {
        with_retry(ctx.retry_delay, || transcriber.transcribe(&request)).await
    } else {
        transcriber.transcribe(&request).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::provider::{Profile, presets};
    use crate::rules::builtin::assemble;

    struct Scripted {
        profile: Profile,
        script: Mutex<VecDeque<Result<RawTranscript, ProviderError>>>,
        calls: AtomicUsize,
        prompts: Mutex<Vec<Option<String>>>,
    }

    impl Scripted {
        fn new(id: &str, script: Vec<Result<&str, ProviderError>>) -> Self {
            let script = script
                .into_iter()
                .map(|r| r.map(|text| RawTranscript { text: text.into(), dropped_segments: 0 }))
                .collect();
            Self {
                profile: presets::custom(id, id, "http://unused.invalid/v1", "m"),
                script: Mutex::new(script),
                calls: AtomicUsize::new(0),
                prompts: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl Transcriber for Scripted {
        fn profile(&self) -> &Profile {
            &self.profile
        }

        fn transcribe(
            &self,
            request: &TranscribeRequest<'_>,
        ) -> impl Future<Output = Result<RawTranscript, ProviderError>> + Send {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.prompts.lock().unwrap().push(request.prompt.map(str::to_string));
            let next = self.script.lock().unwrap().pop_front().expect("unexpected extra call");
            async move { next }
        }
    }

    fn tone_recording(seconds: f32) -> Recording {
        let frames = (48_000.0 * seconds) as usize;
        let samples = (0..frames)
            .flat_map(|i| {
                let s = 0.3 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 48_000.0).sin();
                [s, s]
            })
            .collect();
        Recording { samples, sample_rate: 48_000, channels: 2 }
    }

    fn speech() -> PreparedAudio {
        prepare(&tone_recording(1.0)).unwrap()
    }

    fn ctx<'a>(
        primary: &'a Scripted,
        fallback: Option<&'a Scripted>,
        rules: &'a RuleSet,
    ) -> PipelineContext<'a, Scripted> {
        PipelineContext {
            primary,
            fallback,
            rules,
            prompt_context: "",
            retry_delay: Duration::ZERO,
            numbers_as_words: false,
        }
    }

    fn tech_rules() -> RuleSet {
        RuleSet::compile(&assemble(None, &["tr-core".to_string(), "tr-tech".to_string()])).0
    }

    #[test]
    fn prepare_rejects_short_and_silent_recordings() {
        assert_eq!(prepare(&tone_recording(0.2)).unwrap_err(), PipelineError::TooShort);
        let silent = Recording { samples: vec![0.0; 96_000], sample_rate: 48_000, channels: 2 };
        assert_eq!(prepare(&silent).unwrap_err(), PipelineError::NoSpeech);
        let ok = speech();
        assert_eq!((ok.audio_ms, ok.samples.len()), (1_000, 16_000));
    }

    #[test]
    fn status_strings_round_trip() {
        for status in [TranscriptStatus::Ok, TranscriptStatus::Empty, TranscriptStatus::Hallucination] {
            assert_eq!(TranscriptStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(TranscriptStatus::parse("nope"), None);
    }

    #[test]
    fn number_setting_preserves_raw_output_when_off_or_rules_disabled() {
        let rules = RuleSet::empty();
        assert_eq!(postprocess_with_numbers("12 kişi", &rules, true, true, "tr").0, "On iki kişi");
        assert_eq!(postprocess_with_numbers("12 kişi", &rules, true, false, "tr").0, "12 kişi");
        assert_eq!(postprocess_with_numbers("12 kişi", &rules, false, true, "tr").0, "12 kişi");
        assert_eq!(postprocess_with_numbers(" ", &rules, true, true, "tr").1, TranscriptStatus::Empty);
    }

    #[tokio::test]
    async fn retries_the_primary_once() {
        let rules = RuleSet::empty();
        let primary = Scripted::new("a", vec![Err(ProviderError::Server(503)), Ok("merhaba")]);
        let t = transcribe(&speech(), &ctx(&primary, None, &rules)).await.unwrap();
        assert_eq!((t.text.as_str(), t.used_fallback, primary.calls()), ("merhaba", false, 2));
        assert_eq!(t.profile_id, "a");
    }

    #[tokio::test]
    async fn falls_back_after_the_retry_fails() {
        let rules = RuleSet::empty();
        let primary = Scripted::new("a", vec![Err(ProviderError::Server(502)), Err(ProviderError::Timeout)]);
        let fallback = Scripted::new("b", vec![Ok("yedek")]);
        let t = transcribe(&speech(), &ctx(&primary, Some(&fallback), &rules)).await.unwrap();
        assert_eq!((t.text.as_str(), t.used_fallback, t.profile_id.as_str()), ("yedek", true, "b"));
        assert_eq!((primary.calls(), fallback.calls()), (2, 1));
    }

    #[tokio::test]
    async fn number_spelling_uses_the_profile_that_produced_the_transcript() {
        let rules = RuleSet::empty();
        let mut primary = Scripted::new("a", vec![Err(ProviderError::Server(502)), Err(ProviderError::Timeout)]);
        primary.profile.language = "en".into();
        let mut fallback = Scripted::new("b", vec![Ok("12 kişi")]);
        fallback.profile.language = "tr".into();
        let mut context = ctx(&primary, Some(&fallback), &rules);
        context.numbers_as_words = true;
        let transcript = transcribe(&speech(), &context).await.unwrap();
        assert_eq!(transcript.raw_text, "12 kişi");
        assert_eq!(transcript.text, "On iki kişi");
        assert!(transcript.used_fallback);
        assert_eq!(transcript.profile_id, "b");
    }

    #[tokio::test]
    async fn fallback_gets_a_single_attempt() {
        let rules = RuleSet::empty();
        let primary = Scripted::new("a", vec![Err(ProviderError::RateLimited), Err(ProviderError::RateLimited)]);
        let fallback = Scripted::new("b", vec![Err(ProviderError::Server(500))]);
        let err = transcribe(&speech(), &ctx(&primary, Some(&fallback), &rules)).await.unwrap_err();
        assert_eq!(err, PipelineError::Provider(ProviderError::Server(500)));
        assert_eq!(fallback.calls(), 1);
    }

    #[tokio::test]
    async fn auth_errors_skip_retry_and_fallback() {
        let rules = RuleSet::empty();
        let primary = Scripted::new("a", vec![Err(ProviderError::Unauthorized(401))]);
        let fallback = Scripted::new("b", vec![Ok("x")]);
        let err = transcribe(&speech(), &ctx(&primary, Some(&fallback), &rules)).await.unwrap_err();
        assert_eq!(err, PipelineError::Provider(ProviderError::Unauthorized(401)));
        assert_eq!((primary.calls(), fallback.calls()), (1, 0));
    }

    #[tokio::test]
    async fn rules_run_on_the_provider_text() {
        let rules = tech_rules();
        let primary = Scripted::new("a", vec![Ok("cloud code'u aç")]);
        let t = transcribe(&speech(), &ctx(&primary, None, &rules)).await.unwrap();
        assert_eq!(t.raw_text, "cloud code'u aç");
        assert_eq!(t.text, "Claude Code'u aç");
        assert_eq!(t.status, TranscriptStatus::Ok);
        assert!(!t.hits.is_empty());
    }

    #[tokio::test]
    async fn rules_are_skipped_when_the_profile_says_so() {
        let rules = tech_rules();
        let mut primary = Scripted::new("a", vec![Ok(" cloud code'u aç ")]);
        primary.profile.apply_rules = false;
        let t = transcribe(&speech(), &ctx(&primary, None, &rules)).await.unwrap();
        assert_eq!(t.text, "cloud code'u aç");
        assert!(t.hits.is_empty());
    }

    #[tokio::test]
    async fn empty_and_hallucinated_outputs_are_flagged() {
        let rules = tech_rules();
        let blank = Scripted::new("a", vec![Ok("   ")]);
        let t = transcribe(&speech(), &ctx(&blank, None, &rules)).await.unwrap();
        assert_eq!((t.text.as_str(), t.status), ("", TranscriptStatus::Empty));

        let ghost = Scripted::new("a", vec![Ok("Altyazı M.K.")]);
        let t = transcribe(&speech(), &ctx(&ghost, None, &rules)).await.unwrap();
        assert_eq!((t.text.as_str(), t.status), ("", TranscriptStatus::Hallucination));
        assert_eq!(t.raw_text, "Altyazı M.K.");
    }

    #[tokio::test]
    async fn prompt_follows_the_profile() {
        let rules = tech_rules();
        let primary = Scripted::new("a", vec![Ok("x")]);
        let mut with_context = ctx(&primary, None, &rules);
        with_context.prompt_context = "Yazılım konuşması";
        transcribe(&speech(), &with_context).await.unwrap();
        let prompt = primary.prompts.lock().unwrap()[0].clone().unwrap();
        assert!(prompt.starts_with("Yazılım konuşması. Geçen terimler: Claude Code, Claude,"), "{prompt}");

        let mut quiet = Scripted::new("b", vec![Ok("x")]);
        quiet.profile.send_prompt = false;
        transcribe(&speech(), &ctx(&quiet, None, &rules)).await.unwrap();
        assert_eq!(quiet.prompts.lock().unwrap()[0], None);
    }
}
