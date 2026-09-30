//! Eval loop against a fake provider.

use std::time::Duration;

use opit_core::audio::Recording;
use opit_core::provider::presets;
use opit_core::rules::{RulePack, RuleSet};
use opit_eval::dataset::Sample;
use opit_eval::report;
use opit_eval::run::{EvalConfig, RulesMode, run_eval};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn tone() -> Recording {
    let samples = (0..16_000).map(|i| 0.3 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 16_000.0).sin()).collect();
    Recording { samples, sample_rate: 16_000, channels: 1 }
}

fn config(server: &MockServer, mode: RulesMode) -> EvalConfig {
    let mut profile = presets::groq();
    profile.base_url = format!("{}/v1", server.uri());
    let pack = RulePack::from_yaml(
        "schema: 1\nid: t\nname: T\nterms: [Claude Code]\ncorrections:\n  Claude Code: [cloud code]\n",
    )
    .unwrap();
    let (rules, _) = RuleSet::compile(&[pack]);
    EvalConfig { profile, api_key: None, rules, prompt_context: String::new(), mode, retry_delay: Duration::ZERO }
}

#[tokio::test]
async fn rules_on_beats_rules_off_on_the_same_audio() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "cloud code'u aç"})))
        .expect(2)
        .mount(&server)
        .await;
    let samples = vec![Sample { name: "a".into(), recording: tone(), reference: "Claude Code'u aç".into() }];

    let results = run_eval(&samples, &config(&server, RulesMode::Both)).await.unwrap();
    let off = results[0].off.as_ref().unwrap();
    let on = results[0].on.as_ref().unwrap();
    assert_eq!((off.wer.errors, on.wer.errors), (1, 0));
    assert_eq!((off.terms.hit, off.terms.expected), (0, 1));
    assert_eq!((on.terms.hit, on.terms.expected), (1, 1));
    assert!(report::render(&results).contains("| a | 25.0% | 0.0% | 0/1 | 1/1 |"));
}

#[tokio::test]
async fn silent_samples_are_reported_without_a_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).expect(0).mount(&server).await;
    let silent = Recording { samples: vec![0.0; 16_000], sample_rate: 16_000, channels: 1 };
    let samples = vec![Sample { name: "s".into(), recording: silent, reference: "bir şey".into() }];

    let results = run_eval(&samples, &config(&server, RulesMode::On)).await.unwrap();
    assert!(results[0].off.is_none());
    assert_eq!(results[0].on.as_ref().unwrap().error.as_deref(), Some("no speech was detected"));
}
