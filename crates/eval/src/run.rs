//! Rules on/off evaluation loop.

use std::time::Duration;

use clap::ValueEnum;
use opit_core::pipeline::{self, PipelineContext, PreparedAudio};
use opit_core::provider::{OpenAiCompatible, Profile, ProviderError};
use opit_core::rules::RuleSet;

use crate::dataset::Sample;
use crate::metrics::{TermStats, WerStats, term_hits, wer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RulesMode {
    On,
    Off,
    Both,
}

pub struct EvalConfig {
    pub profile: Profile,
    pub api_key: Option<String>,
    pub rules: RuleSet,
    pub prompt_context: String,
    pub mode: RulesMode,
    pub retry_delay: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub hypothesis: String,
    pub error: Option<String>,
    pub wer: WerStats,
    pub terms: TermStats,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SampleResult {
    pub name: String,
    pub off: Option<Outcome>,
    pub on: Option<Outcome>,
}

pub async fn run_eval(samples: &[Sample], config: &EvalConfig) -> Result<Vec<SampleResult>, ProviderError> {
    let on_client = OpenAiCompatible::new(config.profile.clone(), config.api_key.clone())?;
    let mut off_profile = config.profile.clone();
    off_profile.send_prompt = false;
    off_profile.send_keywords = false;
    off_profile.apply_rules = false;
    let off_client = OpenAiCompatible::new(off_profile, config.api_key.clone())?;
    let no_rules = RuleSet::empty();
    let terms = config.rules.terms();
    let wants_off = config.mode != RulesMode::On;
    let wants_on = config.mode != RulesMode::Off;

    let mut results = Vec::with_capacity(samples.len());
    for sample in samples {
        let (off, on) = match pipeline::prepare(&sample.recording) {
            Ok(audio) => {
                let off = if wants_off {
                    Some(evaluate(&off_client, &no_rules, "", &audio, sample, terms, config.retry_delay).await)
                } else {
                    None
                };
                let on = if wants_on {
                    Some(
                        evaluate(
                            &on_client,
                            &config.rules,
                            &config.prompt_context,
                            &audio,
                            sample,
                            terms,
                            config.retry_delay,
                        )
                        .await,
                    )
                } else {
                    None
                };
                (off, on)
            }
            Err(err) => {
                let failed = failed(sample, terms, err.to_string());
                (wants_off.then(|| failed.clone()), wants_on.then_some(failed))
            }
        };
        results.push(SampleResult { name: sample.name.clone(), off, on });
    }
    Ok(results)
}

async fn evaluate(
    client: &OpenAiCompatible,
    rules: &RuleSet,
    prompt_context: &str,
    audio: &PreparedAudio,
    sample: &Sample,
    terms: &[String],
    retry_delay: Duration,
) -> Outcome {
    let ctx = PipelineContext { primary: client, fallback: None, rules, prompt_context, retry_delay };
    match pipeline::transcribe(audio, &ctx).await {
        Ok(transcript) => Outcome {
            wer: wer(&sample.reference, &transcript.text),
            terms: term_hits(&sample.reference, &transcript.text, terms),
            latency_ms: transcript.request_ms,
            hypothesis: transcript.text,
            error: None,
        },
        Err(err) => failed(sample, terms, err.to_string()),
    }
}

fn failed(sample: &Sample, terms: &[String], error: String) -> Outcome {
    Outcome {
        hypothesis: String::new(),
        error: Some(error),
        wer: wer(&sample.reference, ""),
        terms: term_hits(&sample.reference, "", terms),
        latency_ms: 0,
    }
}
