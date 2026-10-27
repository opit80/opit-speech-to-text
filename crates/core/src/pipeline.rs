//! Recording → transcript: prepare audio, call the provider (retry, fallback),
//! then post-process with the rule layer.

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::audio::encode::{EncodeError, EncodedAudio, encode};
use crate::audio::gate::{self, GateVerdict};
use crate::audio::{self, Recording, TARGET_RATE};
use crate::provider::retry::{PATIENT_DELAYS, RetryWait};
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
    /// Transcription failed; retained audio can be retried.
    Failed,
}

impl TranscriptStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Empty => "empty",
            Self::Hallucination => "hallucination",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ok" => Some(Self::Ok),
            "empty" => Some(Self::Empty),
            "hallucination" => Some(Self::Hallucination),
            "failed" => Some(Self::Failed),
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
    #[error("retry cancelled; audio retained for another attempt")]
    RetryCancelled,
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
    transcribe_with_wait(audio, ctx, |_, delay| async move {
        tokio::time::sleep(delay).await;
        true
    })
    .await
}

/// Waits are injectable, including the quick retry. Returning false cancels before
/// another request. Each patient rung calls primary once, then fallback once.
pub async fn transcribe_with_wait<T, W, F>(
    audio: &PreparedAudio,
    ctx: &PipelineContext<'_, T>,
    mut wait: W,
) -> Result<Transcript, PipelineError>
where
    T: Transcriber,
    W: FnMut(RetryWait, Duration) -> F,
    F: Future<Output = bool>,
{
    let started = Instant::now();
    let primary_audio = encode(&audio.samples, ctx.primary.profile().audio_format)?;
    let mut fallback_audio = None;
    let mut result = call(ctx.primary, &primary_audio, audio, ctx).await;
    if let Err(err) = &result
        && err.is_retryable()
    {
        let delay = ctx.retry_delay.max(err.retry_after().unwrap_or_default());
        if !wait(RetryWait { kind: err.code(), seconds: delay.as_secs(), attempt: 1, total: 5 }, delay).await {
            return Err(PipelineError::RetryCancelled);
        }
        result = call(ctx.primary, &primary_audio, audio, ctx).await;
    }
    let mut used = ctx.primary;
    let mut used_fallback = false;
    let mut delays = PATIENT_DELAYS.into_iter().enumerate();
    loop {
        // A fallback failure must not discard the primary's rate-limit deadline.
        let primary_retry_after = result.as_ref().err().and_then(ProviderError::retry_after).unwrap_or_default();
        if let Err(err) = &result
            && err.is_retryable()
            && let Some(fallback) = ctx.fallback
        {
            let format = fallback.profile().audio_format;
            let encoded = if format == primary_audio.format {
                &primary_audio
            } else {
                if fallback_audio.is_none() {
                    fallback_audio = Some(encode(&audio.samples, format)?);
                }
                fallback_audio.as_ref().expect("fallback audio was encoded above")
            };
            result = call(fallback, encoded, audio, ctx).await;
            used = fallback;
            used_fallback = true;
        }
        let err = match &result {
            Ok(_) => break,
            Err(err) if err.is_retryable() => err,
            Err(err) => return Err(err.clone().into()),
        };
        let Some((rung, seconds)) = delays.next() else { return Err(err.clone().into()) };
        let delay = Duration::from_secs(seconds).max(primary_retry_after).max(err.retry_after().unwrap_or_default());
        let progress = RetryWait { kind: err.code(), seconds: delay.as_secs(), attempt: rung + 2, total: 5 };
        if !wait(progress, delay).await {
            return Err(PipelineError::RetryCancelled);
        }
        result = call(ctx.primary, &primary_audio, audio, ctx).await;
        used = ctx.primary;
        used_fallback = false;
    }
    let raw = result?;

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
    transcriber.transcribe(&request).await
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

    #[tokio::test]
    async fn patient_ladder_waits_are_injected_and_cancel_stops_requests() {
        let rules = RuleSet::empty();
        let primary = Scripted::new("a", vec![Err(ProviderError::Timeout); 6]);
        let mut waits = Vec::new();
        let err = transcribe_with_wait(&speech(), &ctx(&primary, None, &rules), |progress, delay| {
            waits.push((progress, delay));
            async { true }
        })
        .await
        .unwrap_err();
        assert_eq!(err, PipelineError::Provider(ProviderError::Timeout));
        assert_eq!(primary.calls(), 6);
        assert_eq!(waits.iter().map(|(_, d)| d.as_secs()).collect::<Vec<_>>(), [0, 10, 30, 60, 120]);
        assert_eq!(waits.last().unwrap().0.attempt, 5);
        let primary = Scripted::new("a", vec![Err(ProviderError::Timeout); 2]);
        let err = transcribe_with_wait(&speech(), &ctx(&primary, None, &rules), |progress, _| async move {
            progress.attempt == 1
        })
        .await
        .unwrap_err();
        assert_eq!(err, PipelineError::RetryCancelled);
        assert_eq!(primary.calls(), 2);
    }

    #[tokio::test]
    async fn retry_after_is_honoured_and_permanent_errors_never_wait() {
        let rules = RuleSet::empty();
        let primary =
            Scripted::new("a", vec![Err(ProviderError::RateLimitedAfter(Duration::from_secs(300))), Ok("ok")]);
        let mut waits = Vec::new();
        transcribe_with_wait(&speech(), &ctx(&primary, None, &rules), |_, delay| {
            waits.push(delay.as_secs());
            async { true }
        })
        .await
        .unwrap();
        assert_eq!(waits, [120]);
        for status in [400, 401, 403, 404, 413] {
            let primary = Scripted::new("a", vec![Err(ProviderError::Http { status, message: "private".into() })]);
            let err = transcribe_with_wait(&speech(), &ctx(&primary, None, &rules), |_, _| async {
                panic!("permanent errors must not wait")
            })
            .await;
            assert!(err.is_err());
            assert_eq!(primary.calls(), 1);
        }
    }

    #[tokio::test]
    async fn a_fallback_failure_preserves_the_primary_retry_after() {
        let rules = RuleSet::empty();
        let primary = Scripted::new(
            "primary",
            vec![
                Err(ProviderError::Timeout),
                Err(ProviderError::RateLimitedAfter(Duration::from_secs(90))),
                Ok("recovered"),
            ],
        );
        let fallback = Scripted::new("fallback", vec![Err(ProviderError::Server(503))]);
        let mut waits = Vec::new();
        let transcript = transcribe_with_wait(&speech(), &ctx(&primary, Some(&fallback), &rules), |_, delay| {
            waits.push(delay.as_secs());
            async { true }
        })
        .await
        .unwrap();
        assert_eq!(waits, [0, 90]);
        assert_eq!(transcript.profile_id, "primary");
        assert_eq!(transcript.text, "recovered");
        assert_eq!((primary.calls(), fallback.calls()), (3, 1));
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
        for status in
            [TranscriptStatus::Ok, TranscriptStatus::Empty, TranscriptStatus::Hallucination, TranscriptStatus::Failed]
        {
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
        let primary = Scripted::new("a", vec![Err(ProviderError::RateLimited); 6]);
        let fallback = Scripted::new("b", vec![Err(ProviderError::Server(500)); 5]);
        let err = transcribe_with_wait(&speech(), &ctx(&primary, Some(&fallback), &rules), |_, _| async { true })
            .await
            .unwrap_err();
        assert_eq!(err, PipelineError::Provider(ProviderError::Server(500)));
        assert_eq!(fallback.calls(), 5);
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
