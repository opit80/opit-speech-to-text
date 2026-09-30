//! Full pipeline against a fake OpenAI-compatible server.

use std::time::Duration;

use opit_core::audio::Recording;
use opit_core::pipeline::{self, PipelineContext, TranscriptStatus};
use opit_core::provider::{OpenAiCompatible, presets};
use opit_core::rules::RuleSet;
use opit_core::rules::builtin::assemble;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn speech() -> Recording {
    let samples = (0..48_000)
        .flat_map(|i| {
            let s = 0.3 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 48_000.0).sin();
            [s, s]
        })
        .collect();
    Recording { samples, sample_rate: 48_000, channels: 2 }
}

/// Compile-time check: the app spawns this future on tokio.
fn assert_send<F: Send>(future: F) -> F {
    future
}

#[tokio::test]
async fn groq_style_round_trip_with_rules() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/openai/v1/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "text": "cloud code'u aç",
            "segments": [{"text": "cloud code'u aç", "no_speech_prob": 0.01, "avg_logprob": -0.1}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let mut profile = presets::groq();
    profile.base_url = format!("{}/openai/v1", server.uri());
    let client = OpenAiCompatible::new(profile, Some("gsk-test".into())).unwrap();
    let (rules, _) = RuleSet::compile(&assemble(None, &["tr-core".to_string(), "tr-tech".to_string()]));
    let ctx = PipelineContext {
        primary: &client,
        fallback: None,
        rules: &rules,
        prompt_context: "Yazılım konuşması",
        retry_delay: Duration::ZERO,
    };

    let transcript = assert_send(pipeline::run(&speech(), &ctx)).await.unwrap();
    assert_eq!(transcript.text, "Claude Code'u aç");
    assert_eq!(transcript.status, TranscriptStatus::Ok);
    assert_eq!(transcript.profile_id, "groq");

    let body = server.received_requests().await.unwrap().remove(0).body;
    assert!(body.windows(4).any(|w| w == b"fLaC"), "the Groq profile uploads FLAC");
}

#[tokio::test]
async fn http_fallback_after_two_server_errors() {
    let primary_server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(503)).expect(2).mount(&primary_server).await;
    let fallback_server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": "yedekten geldi"})))
        .expect(1)
        .mount(&fallback_server)
        .await;

    let mut primary_profile = presets::groq();
    primary_profile.base_url = format!("{}/v1", primary_server.uri());
    let fallback_profile = presets::custom("gpu", "GPU", &format!("{}/v1", fallback_server.uri()), "large-v3");
    let primary = OpenAiCompatible::new(primary_profile, None).unwrap();
    let fallback = OpenAiCompatible::new(fallback_profile, None).unwrap();
    let rules = RuleSet::empty();
    let ctx = PipelineContext {
        primary: &primary,
        fallback: Some(&fallback),
        rules: &rules,
        prompt_context: "",
        retry_delay: Duration::ZERO,
    };

    let transcript = pipeline::run(&speech(), &ctx).await.unwrap();
    assert!(transcript.used_fallback);
    assert_eq!(transcript.profile_id, "gpu");
    assert_eq!(transcript.text, "yedekten geldi");
}
