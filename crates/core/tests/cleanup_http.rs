use opit_core::cleanup::{self, CleanupClient, CleanupConfig, CleanupError, CleanupStatus};
use opit_core::pipeline::{Transcript, TranscriptStatus};
use opit_core::provider::presets;
use opit_core::rules::{RulePack, RuleSet};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn transcript() -> Transcript {
    Transcript {
        raw_text: "ee ios ios için ox_lib aç".into(),
        text: "ios ios için ox_lib aç".into(),
        detected_language: Some("tr".into()),
        language: Some("tr".into()),
        status: TranscriptStatus::Ok,
        profile_id: "test".into(),
        used_fallback: false,
        audio_ms: 1000,
        request_ms: 10,
        hits: vec![],
        cleanup_status: Some(CleanupStatus::Skipped),
        cleanup_model: None,
        cleanup_error: None,
    }
}

#[tokio::test]
async fn request_shape_glossary_and_final_casing() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_millis(25)).set_body_json(
            serde_json::json!({
                "choices": [{"message": {"content": "```text\nios için ox_lib aç.\n```"}, "finish_reason": "stop"}]
            }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let config = CleanupConfig {
        base_url: format!("{}/v1/", server.uri()),
        reasoning_effort: Some("none".into()),
        enabled: true,
        ..CleanupConfig::default()
    };
    let rules =
        RuleSet::compile(
            &[RulePack::from_yaml("schema: 1\nid: user\nname: Personal\nterms: [iOS, ox_lib]\n").unwrap()],
        )
        .0;
    let system = cleanup::system_prompt(&config, "tr", &rules, "Software discussion");
    let client = CleanupClient::new(config.clone(), Some(" test-key ".into())).unwrap();
    let mut t = transcript();
    assert!(cleanup::eligible(&t, &config, &presets::groq()));
    cleanup::apply(&mut t, Ok(&client), &system, &rules, true).await;
    assert_eq!(t.text, "iOS için ox_lib aç.");
    assert_eq!(t.raw_text, "ee ios ios için ox_lib aç");
    assert_eq!(t.cleanup_status, Some(CleanupStatus::Ok));
    assert!(t.request_ms >= 35, "cleanup time contributes to request latency");
    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(body["model"], config.model);
    assert_eq!(body["temperature"], 0);
    assert_eq!(body["reasoning_effort"], "none");
    assert!(body["max_tokens"].as_u64().unwrap() >= 256);
    assert_eq!(body["messages"][1]["content"], "ios ios için ox_lib aç");
    let prompt = body["messages"][0]["content"].as_str().unwrap();
    assert!(prompt.starts_with(cleanup::TR_PROMPT));
    assert!(prompt.contains("iOS, ox_lib"));
    assert!(prompt.contains("Software discussion"));
}

#[tokio::test]
async fn failures_keep_rules_applied_text_and_never_expose_server_body() {
    for (status, body, expected) in [
        (401, "private key and transcript".to_string(), CleanupError::InvalidKey),
        (500, "private key and transcript".to_string(), CleanupError::Rejected),
        (200, "private key and transcript".to_string(), CleanupError::BadResponse),
        (200, serde_json::json!({"choices":[{"message":{"content":" "}}]}).to_string(), CleanupError::BadResponse),
        (
            200,
            serde_json::json!({"choices":[{"message":{"content":"x".repeat(2000)}}]}).to_string(),
            CleanupError::BadResponse,
        ),
        (
            200,
            serde_json::json!({"choices":[{"message":{"content":"partial"},"finish_reason":"length"}]}).to_string(),
            CleanupError::BadResponse,
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;
        let config = CleanupConfig { base_url: server.uri(), api_key_ref: None, ..CleanupConfig::default() };
        let client = CleanupClient::new(config, None).unwrap();
        let mut t = transcript();
        let before = t.text.clone();
        cleanup::apply(&mut t, Ok(&client), cleanup::EN_PROMPT, &RuleSet::empty(), true).await;
        assert_eq!(t.text, before);
        assert_eq!(t.cleanup_status, Some(CleanupStatus::Failed));
        assert_eq!(t.cleanup_error, Some(expected));
        assert!(!t.cleanup_error.unwrap().to_string().contains("private"));
    }
}

#[tokio::test]
async fn timeout_and_omitted_reasoning_and_disabled_guards() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(2)))
        .mount(&server)
        .await;
    let mut config = CleanupConfig {
        base_url: server.uri(),
        api_key_ref: None,
        timeout_seconds: 1,
        enabled: true,
        ..CleanupConfig::default()
    };
    let client = CleanupClient::new(config.clone(), None).unwrap();
    assert_eq!(client.clean("some input words", cleanup::EN_PROMPT).await, Err(CleanupError::Timeout));
    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert!(body.get("reasoning_effort").is_none());
    let mut t = transcript();
    t.text = "two words".into();
    assert!(!cleanup::eligible(&t, &config, &presets::groq()));
    t.text = "three input words".into();
    config.enabled = false;
    assert!(!cleanup::eligible(&t, &config, &presets::groq()));
    config.enabled = true;
    let mut profile = presets::groq();
    profile.apply_cleanup = false;
    assert!(!cleanup::eligible(&t, &config, &profile));
}
