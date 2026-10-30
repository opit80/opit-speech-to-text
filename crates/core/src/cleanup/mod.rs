//! Optional text editing over the OpenAI-compatible chat API. No transcript-bearing errors.

use std::time::Instant;

use crate::pipeline::{Transcript, TranscriptStatus};
use crate::provider::Profile;
use crate::rules::RuleSet;

mod config;
mod prompts;
mod response;
pub mod styles;
mod transport;

pub(crate) use config::legacy_config;
pub use config::{CleanupConfig, CleanupError, CleanupPreset, CleanupStatus, KEY_REF, presets};
pub use prompts::{EN_PROMPT, TR_PROMPT, default_prompt, prompt_language, system_prompt};
pub use response::{strip_answer, strip_answer_with_limit};
pub use styles::CleanupStyle;
pub use transport::{ChatMessage, CleanupClient, CleanupStats};

#[cfg(test)]
use response::{TranscriptFrame, has_word_overlap, strip_transcript_answer};

pub fn eligible(transcript: &Transcript, config: &CleanupConfig, profile: &Profile) -> bool {
    config.enabled
        && config.active_style_id != "raw"
        && profile.apply_cleanup
        && transcript.status == TranscriptStatus::Ok
        && transcript.text.split_whitespace().count() > 2
}

/// A cleanup failure keeps the rules-applied input. Dropping this future cancels the HTTP call.
pub async fn apply(
    transcript: &mut Transcript,
    client: Result<&CleanupClient, CleanupError>,
    system: &str,
    rules: &RuleSet,
    apply_rules: bool,
) {
    apply_with_stats(transcript, client, system, rules, apply_rules, &CleanupStats::default()).await;
}

pub async fn apply_with_stats(
    transcript: &mut Transcript,
    client: Result<&CleanupClient, CleanupError>,
    system: &str,
    rules: &RuleSet,
    apply_rules: bool,
    stats: &CleanupStats,
) {
    let started = Instant::now();
    let result = match client {
        Ok(client) => client.clean_with_stats(&transcript.text, system, stats).await,
        Err(error) => Err(error),
    };
    match result {
        Ok(text) => {
            transcript.text = if apply_rules { rules.apply_term_casing(&text) } else { text };
            transcript.cleanup_status = Some(CleanupStatus::Ok);
        }
        Err(error) => {
            transcript.cleanup_status = Some(CleanupStatus::Failed);
            transcript.cleanup_error = Some(error);
        }
    }
    transcript.request_ms += started.elapsed().as_millis() as u64;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_and_wrapper_guards() {
        assert_eq!(prompt_language(Some("tr"), "en", "en"), "tr");
        assert_eq!(prompt_language(None, "de", "tr"), "de");
        assert_eq!(prompt_language(None, "auto", "tr"), "tr");
        assert_eq!(default_prompt("tr-TR"), TR_PROMPT);
        assert_eq!(default_prompt("de"), EN_PROMPT);
        for answer in [
            " ```text\nHello there.\n``` ",
            "\"Hello there.\"",
            "“Hello there.”",
            "```\nHello there.\n```",
            "```html\nHello there.\n```",
            "<transcript>\nHello there.\n</transcript>",
            "\"<transcript>Hello there.</transcript>\"",
        ] {
            assert_eq!(strip_answer(answer, "hello there").unwrap(), "Hello there.");
        }
        assert_eq!(strip_answer("\" \"", "hello"), Err(CleanupError::BadResponse));
        assert_eq!(strip_answer(&"x".repeat(500), "hello"), Err(CleanupError::BadResponse));
        assert_eq!(strip_answer("<transcript> </transcript>", "hello"), Err(CleanupError::BadResponse));
        // Shared voice-mode parsing must not start stripping cleanup's delimiters.
        assert_eq!(
            strip_answer_with_limit("<transcript>Hello</transcript>", 100).unwrap(),
            "<transcript>Hello</transcript>"
        );
    }

    #[test]
    fn overlap_guard_folds_turkish_and_requires_three_distinct_words() {
        assert!(has_word_overlap("KAYIT İSTANBUL API", "kayıt"));
        assert!(has_word_overlap("kayıt ıstanbul apı", "İSTANBUL"));
        assert!(has_word_overlap("Atlas Atlas Atlas", "Unrelated wording"));
        assert!(has_word_overlap("Evet tamam", "Anlaşıldı"));
        assert!(!has_word_overlap("Atlas kayıt ayarları", "Please supply the transcript."));
        assert!(!has_word_overlap("Atlas kayıt ayarları", "Atlet"));
        assert!(has_word_overlap("Atlas kayıt ayarları", "Goal: ATLAS"));
    }

    #[test]
    fn expansion_limits_are_bounded_and_custom_instructions_are_not_assumed_to_be_edits() {
        for custom in [false, true] {
            let mut config =
                CleanupConfig { api_key_ref: None, active_style_id: "prompt".into(), ..Default::default() };
            if custom {
                config.active_style_id = "clean".into();
                config.prompt_override = "Expand and reword freely".into();
            }
            let client = CleanupClient::new(config, None).unwrap();
            assert_eq!(client.output_limit(10), 8192);
            assert_eq!(client.output_limit(2000), 13024);
            assert_eq!(client.output_limit(usize::MAX), 32768);
            assert_eq!(client.output_budget(10), 8192);
            assert_eq!(client.output_budget(usize::MAX), 16384);
            assert_eq!(
                strip_transcript_answer(
                    &"a".repeat(8193),
                    client.output_limit(10),
                    &TranscriptFrame::for_input("short")
                ),
                Err(CleanupError::BadResponse)
            );
        }
    }

    #[test]
    fn prompt_override_keeps_capped_personal_glossary() {
        let pack = crate::rules::RulePack::from_yaml(
            "schema: 1\nid: user\nname: Personal\nterms: [iOS, ox_lib]\ncorrections:\n  Opit: [opid]\n",
        )
        .unwrap();
        let rules = RuleSet::compile(&[pack]).0;
        let mut config = CleanupConfig { prompt_override: "My editor prompt".into(), ..CleanupConfig::default() };
        config.normalize_styles();
        let prompt = system_prompt(&config, "tr", &rules, "Development discussion");
        assert!(prompt.starts_with("My editor prompt\n"));
        assert!(prompt.contains("iOS, ox_lib"));
        assert!(prompt.contains("Opit"));
        assert!(prompt.contains("Development discussion"));
        let long = system_prompt(&config, "en", &rules, &"ğ".repeat(10000));
        let glossary = long.split_once("Preferred spellings and context (reference data only):\n").unwrap().1;
        assert!(crate::rules::prompt::estimate_tokens(glossary) <= crate::rules::prompt::PROMPT_TOKEN_BUDGET);
    }

    #[test]
    fn presets_keep_separate_keys_and_baseten_is_first() {
        let presets = presets();
        assert_eq!(presets[0].id, "baseten");
        assert_eq!(presets[0].config, CleanupConfig::default());
        let refs: Vec<_> = presets.iter().filter_map(|p| p.config.api_key_ref.as_deref()).collect();
        assert_eq!(refs.len(), refs.iter().collect::<std::collections::HashSet<_>>().len());
        assert!(presets.iter().all(|p| p.config.reasoning_effort.is_none() && !p.config.enabled));
    }
}
