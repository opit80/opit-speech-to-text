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
async fn null_content_is_empty_and_length_is_distinct_even_with_null_content() {
    for (finish, expected) in [("stop", CleanupError::BadResponse), ("length", CleanupError::Truncated)] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": null}, "finish_reason": finish}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let config = CleanupConfig { base_url: server.uri(), api_key_ref: None, ..CleanupConfig::default() };
        let client = CleanupClient::new(config, None).unwrap();
        assert_eq!(client.clean("some input words", cleanup::EN_PROMPT).await, Err(expected));
    }
}

#[tokio::test]
async fn cleanup_and_voice_mode_token_budgets_have_reasoning_headroom_and_a_cap() {
    for effort in [None, Some("low"), Some(" ")] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "Edited text."}, "finish_reason": "stop"}]
            })))
            .expect(3)
            .mount(&server)
            .await;
        let config = CleanupConfig {
            base_url: server.uri(),
            api_key_ref: None,
            reasoning_effort: effort.map(str::to_string),
            ..CleanupConfig::default()
        };
        let client = CleanupClient::new(config, None).unwrap();
        client.clean("short input", cleanup::EN_PROMPT).await.unwrap();
        client.clean(&"a".repeat(20000), cleanup::EN_PROMPT).await.unwrap();
        client.complete_mode(&[cleanup::ChatMessage { role: "user", content: "short input".into() }]).await.unwrap();
        let requests = server.received_requests().await.unwrap();
        let bodies: Vec<serde_json::Value> = requests.iter().map(|r| r.body_json().unwrap()).collect();
        assert_eq!(bodies[0]["max_tokens"], if effort == Some("low") { 6144 } else { 2048 });
        assert_eq!(bodies[1]["max_tokens"], 16384);
        assert!(bodies[2]["max_tokens"].as_u64().unwrap() >= 2048);
    }
}

#[tokio::test]
async fn stalled_first_attempt_is_recovered_by_one_hedge_within_the_total_budget() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    let server = MockServer::start().await;
    let count = AtomicUsize::new(0);
    Mock::given(method("POST"))
        .respond_with(move |_: &wiremock::Request| {
            let delay = if count.fetch_add(1, Ordering::SeqCst) == 0 { 6000 } else { 20 };
            ResponseTemplate::new(200).set_delay(Duration::from_millis(delay)).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "Edited text."}, "finish_reason": "stop"}]
            }))
        })
        .expect(2)
        .mount(&server)
        .await;
    let config =
        CleanupConfig { base_url: server.uri(), api_key_ref: None, timeout_seconds: 5, ..CleanupConfig::default() };
    let client = CleanupClient::new(config, None).unwrap();
    let started = Instant::now();
    let stats = cleanup::CleanupStats::default();
    assert_eq!(client.clean_with_stats("some input words", cleanup::EN_PROMPT, &stats).await.unwrap(), "Edited text.");
    assert_eq!(stats.attempts(), 2);
    assert!(started.elapsed() >= Duration::from_millis(2500));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn both_stalled_attempts_share_one_total_deadline() {
    use std::time::{Duration, Instant};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(10)))
        .expect(2)
        .mount(&server)
        .await;
    let config =
        CleanupConfig { base_url: server.uri(), api_key_ref: None, timeout_seconds: 5, ..CleanupConfig::default() };
    let client = CleanupClient::new(config, None).unwrap();
    let started = Instant::now();
    let stats = cleanup::CleanupStats::default();
    assert_eq!(
        client.clean_with_stats("some input words", cleanup::EN_PROMPT, &stats).await,
        Err(CleanupError::Timeout)
    );
    assert_eq!(stats.attempts(), 2);
    // Allow scheduler jitter, but never a second full timeout budget.
    assert!(started.elapsed() >= Duration::from_secs(5));
    assert!(started.elapsed() < Duration::from_millis(5300));
}

#[tokio::test]
async fn client_errors_are_never_retried() {
    for status in [400, 401, 403, 408, 429] {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(status)).expect(1).mount(&server).await;
        let config = CleanupConfig { base_url: server.uri(), api_key_ref: None, ..CleanupConfig::default() };
        let client = CleanupClient::new(config, None).unwrap();
        assert!(client.clean("some input words", cleanup::EN_PROMPT).await.is_err());
    }
}

#[tokio::test]
async fn terminal_errors_do_not_hedge_for_any_shared_transport_caller() {
    for mode in 0..3 {
        for (status, content, finish, expected) in [
            (400, Some("private response"), "stop", CleanupError::Rejected),
            (401, Some("private response"), "stop", CleanupError::InvalidKey),
            (403, Some("private response"), "stop", CleanupError::InvalidKey),
            (408, Some("private response"), "stop", CleanupError::Rejected),
            (429, Some("private response"), "stop", CleanupError::Rejected),
            (200, None, "stop", CleanupError::BadResponse),
            (200, None, "length", CleanupError::Truncated),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status).set_body_json(serde_json::json!({
                    "choices": [{"message": {"content": content}, "finish_reason": finish}]
                })))
                .expect(1)
                .mount(&server)
                .await;
            let client = CleanupClient::new(
                CleanupConfig {
                    base_url: server.uri(),
                    api_key_ref: None,
                    timeout_seconds: 2,
                    ..CleanupConfig::default()
                },
                None,
            )
            .unwrap();
            let stats = cleanup::CleanupStats::default();
            let messages = [cleanup::ChatMessage { role: "user", content: "some input words".into() }];
            let result = match mode {
                0 => client.clean_with_stats("some input words", cleanup::EN_PROMPT, &stats).await,
                1 => client.complete_mode_with_stats(&messages, &stats).await,
                _ => client.complete(&messages).await,
            };
            assert_eq!(result, Err(expected));
            if mode != 2 {
                assert_eq!(stats.attempts(), 1);
            }
        }
    }
}

#[tokio::test]
async fn connection_errors_retry_once_without_resetting_the_budget() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let config = CleanupConfig {
        base_url: format!("http://{address}"),
        api_key_ref: None,
        timeout_seconds: 5,
        ..CleanupConfig::default()
    };
    let client = CleanupClient::new(config, None).unwrap();
    let stats = cleanup::CleanupStats::default();
    let started = std::time::Instant::now();
    let error = client.clean_with_stats("some input words", cleanup::EN_PROMPT, &stats).await.unwrap_err();
    assert!(matches!(error, CleanupError::Network | CleanupError::Timeout));
    assert_eq!(stats.attempts(), 2);
    assert!(started.elapsed() < std::time::Duration::from_millis(5300));
}

#[tokio::test]
async fn equally_slow_valid_responses_succeed_for_cleanup_voice_modes_and_ask() {
    use std::time::{Duration, Instant};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5)).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": "Edited text."}, "finish_reason": "stop"}]
        })))
        .expect(6)
        .mount(&server)
        .await;
    let config =
        CleanupConfig { base_url: server.uri(), api_key_ref: None, timeout_seconds: 8, ..CleanupConfig::default() };
    let client = CleanupClient::new(config, None).unwrap();
    let stats = cleanup::CleanupStats::default();
    let messages = [cleanup::ChatMessage { role: "user", content: "some input words".into() }];
    let started = Instant::now();
    let (cleaned, generated, answer) = tokio::join!(
        client.clean_with_stats("some input words", cleanup::EN_PROMPT, &stats),
        client.complete_mode(&messages),
        client.complete(&messages),
    );
    for result in [cleaned, generated, answer] {
        assert_eq!(result.unwrap(), "Edited text.");
    }
    assert_eq!(stats.attempts(), 2);
    assert!(started.elapsed() >= Duration::from_secs(5));
    assert!(started.elapsed() < Duration::from_secs(8));
}

#[tokio::test]
async fn hedge_race_keeps_the_pending_success_when_either_attempt_fails() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    // The first succeeds after the hedge fails; then the hedge succeeds after
    // the first fails. Both failures happen after the second request starts.
    for first_fails in [false, true] {
        let server = MockServer::start().await;
        let count = AtomicUsize::new(0);
        Mock::given(method("POST"))
            .respond_with(move |_: &wiremock::Request| {
                let first = count.fetch_add(1, Ordering::SeqCst) == 0;
                let fails = first == first_fails;
                let delay_ms = match (first, fails) {
                    (true, true) => 1100,
                    (true, false) => 1500,
                    (false, true) => 20,
                    (false, false) => 500,
                };
                let response = if fails {
                    ResponseTemplate::new(500)
                } else {
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({
                        "choices": [{"message": {"content": "Edited text."}, "finish_reason": "stop"}]
                    }))
                };
                response.set_delay(Duration::from_millis(delay_ms))
            })
            .expect(2)
            .mount(&server)
            .await;
        let client = CleanupClient::new(
            CleanupConfig { base_url: server.uri(), api_key_ref: None, timeout_seconds: 2, ..CleanupConfig::default() },
            None,
        )
        .unwrap();
        let started = Instant::now();
        assert_eq!(client.clean("some input words", cleanup::EN_PROMPT).await.unwrap(), "Edited text.");
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

#[tokio::test]
async fn first_success_wins_in_either_order() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    for first_wins in [false, true] {
        let server = MockServer::start().await;
        let count = AtomicUsize::new(0);
        Mock::given(method("POST"))
            .respond_with(move |_: &wiremock::Request| {
                let first = count.fetch_add(1, Ordering::SeqCst) == 0;
                let delay_ms = match (first, first_wins) {
                    (true, true) => 1300,
                    (true, false) => 5000,
                    (false, true) => 1000,
                    (false, false) => 20,
                };
                ResponseTemplate::new(200).set_delay(Duration::from_millis(delay_ms)).set_body_json(serde_json::json!({
                    "choices": [{"message": {"content": if first { "First answer." } else { "Hedged answer." }}, "finish_reason": "stop"}]
                }))
            })
            .expect(2)
            .mount(&server)
            .await;
        let client = CleanupClient::new(
            CleanupConfig { base_url: server.uri(), api_key_ref: None, timeout_seconds: 2, ..CleanupConfig::default() },
            None,
        )
        .unwrap();
        let started = Instant::now();
        assert_eq!(
            client.clean("some input words", cleanup::EN_PROMPT).await.unwrap(),
            if first_wins { "First answer." } else { "Hedged answer." }
        );
        assert!(started.elapsed() < Duration::from_millis(1800));
    }
}

#[tokio::test]
async fn stalled_attempt_counts_and_deadlines_at_timeout_boundaries() {
    use std::time::{Duration, Instant};
    for (seconds, attempts) in [(1, 1), (2, 2), (3, 2), (8, 2)] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(20)))
            .expect(attempts)
            .mount(&server)
            .await;
        let client = CleanupClient::new(
            CleanupConfig {
                base_url: server.uri(),
                api_key_ref: None,
                timeout_seconds: seconds,
                ..CleanupConfig::default()
            },
            None,
        )
        .unwrap();
        let stats = cleanup::CleanupStats::default();
        let started = Instant::now();
        assert_eq!(
            client.clean_with_stats("some input words", cleanup::EN_PROMPT, &stats).await,
            Err(CleanupError::Timeout)
        );
        assert_eq!(u64::from(stats.attempts()), attempts);
        let budget = Duration::from_secs(u64::from(seconds));
        assert!(started.elapsed() >= budget);
        assert!(started.elapsed() < budget + Duration::from_millis(500));
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
    assert!(body["max_tokens"].as_u64().unwrap() >= 6144);
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
            CleanupError::Truncated,
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
