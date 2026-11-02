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
async fn http_errors_have_safe_specific_kinds_without_retrying() {
    for (status, body, expected) in [
        (402, r#"{"error":"please check your current payment status."}"#, CleanupError::PaymentRequired),
        (404, r#"{"error":"private transcript and key"}"#, CleanupError::ModelNotFound),
        (
            400,
            r#"{"error":{"message":"API key not valid","details":[{"reason":"API_KEY_INVALID"}]}}"#,
            CleanupError::InvalidKey,
        ),
        (400, r#"{"error":"private transcript and key"}"#, CleanupError::Rejected),
        (429, "private transcript and key", CleanupError::RateLimited),
        (503, "private transcript and key", CleanupError::Server),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;
        let client = CleanupClient::new(
            CleanupConfig { base_url: server.uri(), api_key_ref: None, timeout_seconds: 2, ..Default::default() },
            None,
        )
        .unwrap();
        let stats = cleanup::CleanupStats::default();
        let error = client.clean_with_stats("some input words", cleanup::EN_PROMPT, &stats).await.unwrap_err();
        assert_eq!(error, expected);
        assert!(!error.to_string().contains("private"));
        assert!(!error.to_string().contains("API_KEY_INVALID"));
        assert_eq!(stats.attempts(), 1);
    }
}

#[tokio::test]
async fn stalled_400_body_is_rejected_before_the_hedge_starts() {
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        // Keep every response open, including any hedge, without completing the chunked body.
        let mut connections = Vec::new();
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                headers.push(stream.read_u8().await.unwrap());
            }
            stream
                .write_all(b"HTTP/1.1 400 Bad Request\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nx\r\n")
                .await
                .unwrap();
            connections.push(stream);
        }
    });
    let client = CleanupClient::new(
        CleanupConfig {
            base_url: format!("http://{address}"),
            api_key_ref: None,
            timeout_seconds: 8,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    let stats = cleanup::CleanupStats::default();
    let started = Instant::now();
    let result = client.clean_with_stats("some input words", cleanup::EN_PROMPT, &stats).await;
    let elapsed = started.elapsed();
    server.abort();
    let _ = server.await;

    assert_eq!(result, Err(CleanupError::Rejected));
    assert!(elapsed < Duration::from_secs(2), "400 body inspection took {elapsed:?}");
    assert_eq!(stats.attempts(), 1);
}

#[tokio::test]
async fn invalid_key_marker_is_detected_only_within_the_bounded_prefix() {
    for (body, expected) in [
        (format!("{}API_KEY_INVALID", "x".repeat(64 * 1024 - 15)), CleanupError::InvalidKey),
        (format!("{}API_KEY_INVALID", "x".repeat(64 * 1024 - 5)), CleanupError::Rejected),
        (format!("{}API_KEY_INVALID", "x".repeat(64 * 1024)), CleanupError::Rejected),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;
        let client =
            CleanupClient::new(CleanupConfig { base_url: server.uri(), api_key_ref: None, ..Default::default() }, None)
                .unwrap();
        let error = client.clean("some input words", cleanup::EN_PROMPT).await.unwrap_err();
        assert_eq!(error, expected);
    }
}

#[tokio::test]
async fn null_content_is_empty_and_length_is_distinct_even_with_null_content() {
    for (finish, expected) in [("stop", CleanupError::BadResponse), ("length", CleanupError::Truncated)] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": null, "reasoning_content": "Private internal reasoning"}, "finish_reason": finish}]
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
        assert_eq!(bodies[2]["max_tokens"], 16384);
        assert_eq!(bodies[2]["messages"][0]["content"], "short input");
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
    assert_eq!(client.clean_with_stats("some input text", cleanup::EN_PROMPT, &stats).await.unwrap(), "Edited text.");
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
            (402, Some("private response"), "stop", CleanupError::PaymentRequired),
            (403, Some("private response"), "stop", CleanupError::InvalidKey),
            (404, Some("private response"), "stop", CleanupError::ModelNotFound),
            (408, Some("private response"), "stop", CleanupError::Rejected),
            (429, Some("private response"), "stop", CleanupError::RateLimited),
            (503, Some("private response"), "stop", CleanupError::Server),
            (200, None, "stop", CleanupError::BadResponse),
            (200, None, "length", CleanupError::Truncated),
            (200, Some("some input words"), "content_filter", CleanupError::BadResponse),
            (200, Some("some input words"), "tool_calls", CleanupError::BadResponse),
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
        client.clean_with_stats("some input text", cleanup::EN_PROMPT, &stats),
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
        assert_eq!(client.clean("some input text", cleanup::EN_PROMPT).await.unwrap(), "Edited text.");
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
            client.clean("some input answer", cleanup::EN_PROMPT).await.unwrap(),
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
    assert_eq!(body["messages"][1]["content"], "<transcript>\nios ios için ox_lib aç\n</transcript>");
    let prompt = body["messages"][0]["content"].as_str().unwrap();
    assert!(prompt.starts_with(cleanup::TR_PROMPT));
    assert!(prompt.contains("iOS, ox_lib"));
    assert!(prompt.contains("Software discussion"));
}

#[tokio::test]
async fn failures_keep_rules_applied_text_and_never_expose_server_body() {
    for (status, body, expected) in [
        (401, "private key and transcript".to_string(), CleanupError::InvalidKey),
        (402, "private key and transcript".to_string(), CleanupError::PaymentRequired),
        (429, "private key and transcript".to_string(), CleanupError::RateLimited),
        (500, "private key and transcript".to_string(), CleanupError::Server),
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

#[tokio::test]
async fn transcript_framing_and_echo_stripping_also_cover_custom_instructions() {
    for language in ["en", "tr"] {
        for instructions in 0..4 {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "choices": [{"message": {"content": "```text\n<transcript>\n“Atlas ayarlarını aç.”\n</transcript>\n```"}}]
                })))
                .expect(1).mount(&server).await;
            let mut config = CleanupConfig { base_url: server.uri(), api_key_ref: None, ..Default::default() };
            if instructions == 1 || instructions == 2 {
                config.prompt_override = "My custom editor".into();
            }
            if instructions == 1 {
                config.normalize_styles();
            } else if instructions == 3 {
                config.active_style_id = "custom-editor".into();
                config.styles.push(cleanup::CleanupStyle {
                    id: "custom-editor".into(),
                    prompt: "My custom editor".into(),
                    ..Default::default()
                });
            }
            let system = cleanup::system_prompt(&config, language, &RuleSet::empty(), "");
            if instructions != 0 {
                assert!(system.starts_with("My custom editor\n"));
            }
            let client = CleanupClient::new(config, None).unwrap();
            assert_eq!(client.clean("Atlas ayarlarını aç", &system).await.unwrap(), "Atlas ayarlarını aç.");
            let requests = server.received_requests().await.unwrap();
            let body: serde_json::Value = requests[0].body_json().unwrap();
            let prompt = body["messages"][0]["content"].as_str().unwrap();
            assert!(prompt.contains("<transcript>"));
            assert!(prompt.contains("</transcript>"));
            assert_eq!(body["messages"][1]["content"], "<transcript>\nAtlas ayarlarını aç\n</transcript>");
        }
    }
}

#[tokio::test]
async fn expansion_limits_and_token_budgets_follow_the_style() {
    let input = "Atlas veya Nebula";
    let answer =
        format!("Goal: Atlas\nContext: Nebula\nConstraints: {}\nExpected output: Atlas", "Atlas Nebula ".repeat(90));
    assert!(answer.chars().count() > input.chars().count() * 3 + 120);
    for (id, accepts) in
        [("prompt", true), ("custom-expand", true), ("clean", false), ("formal", false), ("casual", false)]
    {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": answer}, "finish_reason": "stop"}]
            })))
            .expect(2)
            .mount(&server)
            .await;
        let mut config = CleanupConfig {
            base_url: server.uri(),
            api_key_ref: None,
            active_style_id: id.into(),
            reasoning_effort: Some("low".into()),
            ..Default::default()
        };
        if id.starts_with("custom-") {
            config.styles.push(cleanup::CleanupStyle {
                id: id.into(),
                prompt: "Expand the supplied material".into(),
                ..Default::default()
            });
        }
        let system = cleanup::system_prompt(&config, "en", &RuleSet::empty(), "");
        let client = CleanupClient::new(config, None).unwrap();
        let result = client.clean(input, &system).await;
        if accepts {
            assert_eq!(result.unwrap(), answer);
        } else {
            assert_eq!(result, Err(CleanupError::BadResponse));
        }
        // Long inputs must never push the provider's token budget past its ceiling.
        let _ = client.clean(&"Atlas ".repeat(10000), &system).await;
        let requests = server.received_requests().await.unwrap();
        let short: serde_json::Value = requests[0].body_json().unwrap();
        let long: serde_json::Value = requests[1].body_json().unwrap();
        assert_eq!(short["max_tokens"], if accepts { 12288 } else { 6144 });
        assert_eq!(long["max_tokens"], 16384);
    }
}

#[tokio::test]
async fn builtins_reject_unrelated_meta_replies_and_keep_rules_applied_text() {
    for id in ["clean", "formal", "casual", "prompt"] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "Please supply a transcript before continuing."}, "finish_reason": "stop"}]
            }))).expect(1).mount(&server).await;
        let config = CleanupConfig {
            base_url: server.uri(),
            api_key_ref: None,
            active_style_id: id.into(),
            ..Default::default()
        };
        let system = cleanup::system_prompt(&config, "tr", &RuleSet::empty(), "");
        let client = CleanupClient::new(config, None).unwrap();
        let mut t = transcript();
        let before = t.text.clone();
        cleanup::apply(&mut t, Ok(&client), &system, &RuleSet::empty(), true).await;
        assert_eq!(t.text, before);
        assert_eq!(t.cleanup_status, Some(CleanupStatus::Failed));
        assert_eq!(t.cleanup_error, Some(CleanupError::BadResponse));
    }
}

#[tokio::test]
async fn overlap_guard_allows_edits_prompt_restructuring_and_short_or_custom_inputs() {
    for (id, input, answer) in [
        ("clean", "ıı şey yani İstanbul için API kaydı aç", "İSTANBUL için api KAYDI aç."),
        ("formal", "dosyaları bugün bana yolla", "Dosyaları bugün iletmenizi rica ederim."),
        ("prompt", "Atlas veya Nebula", "Goal: Atlas veya Nebula\nExpected output: Atlas veya Nebula"),
        ("clean", "evet tamam", "Anlaşıldı."),
        ("custom-rewrite", "Atlas ayarlarını hemen aç", "Completely different wording is permitted here."),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"choices": [{"message": {"content": answer}}]})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let mut config = CleanupConfig {
            base_url: server.uri(),
            api_key_ref: None,
            active_style_id: id.into(),
            ..Default::default()
        };
        if id.starts_with("custom-") {
            config.styles.push(cleanup::CleanupStyle { id: id.into(), ..Default::default() });
        }
        let system = cleanup::system_prompt(&config, "tr", &RuleSet::empty(), "");
        let client = CleanupClient::new(config, None).unwrap();
        assert_eq!(client.clean(input, &system).await.unwrap(), answer);
    }
}

#[tokio::test]
async fn expansion_keeps_the_absolute_response_guard_and_other_modes_keep_their_messages() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": "Atlas ".repeat(90000)}, "finish_reason": "stop"}]
        })))
        .expect(3)
        .mount(&server)
        .await;
    let client = CleanupClient::new(
        CleanupConfig {
            base_url: server.uri(),
            api_key_ref: None,
            active_style_id: "prompt".into(),
            ..Default::default()
        },
        None,
    )
    .unwrap();
    let messages = [cleanup::ChatMessage { role: "user", content: "Atlas settings".into() }];
    assert_eq!(client.clean("Atlas settings", cleanup::EN_PROMPT).await, Err(CleanupError::BadResponse));
    assert_eq!(client.complete(&messages).await, Err(CleanupError::BadResponse));
    assert_eq!(client.complete_mode(&messages).await, Err(CleanupError::BadResponse));
    let requests = server.received_requests().await.unwrap();
    let ask: serde_json::Value = requests[1].body_json().unwrap();
    let voice: serde_json::Value = requests[2].body_json().unwrap();
    assert_eq!(ask["messages"][0]["content"], "Atlas settings");
    assert_eq!(ask["max_tokens"], 4096);
    assert_eq!(voice["messages"][0]["content"], "Atlas settings");
    assert_eq!(voice["max_tokens"], 16384);
}

async fn review_response(id: &str, input: &str, answer: &str) -> Result<String, CleanupError> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": answer}, "finish_reason": "stop"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let mut config =
        CleanupConfig { base_url: server.uri(), api_key_ref: None, active_style_id: id.into(), ..Default::default() };
    if id.starts_with("custom-") {
        config.styles.push(cleanup::CleanupStyle { id: id.into(), ..Default::default() });
    }
    let system = cleanup::system_prompt(&config, "tr", &RuleSet::empty(), "");
    let client = CleanupClient::new(config, None).unwrap();
    client.clean(input, &system).await
}

#[tokio::test]
async fn review_meta_replies_are_rejected_even_when_they_quote_short_inputs() {
    // Reproduce the reported replies, including the one that retains every input word.
    let cases = [
        (
            "Gerekli olanın hepsini yap.",
            "Düzenlenecek konuşma dökümü paylaşılmadığı için işlem yapılamıyor. Lütfen dökümü gönderin.",
        ),
        (
            "Klikle soruda ne var?",
            "Düzenlenecek konuşma dökümü paylaşılmamış; yalnızca “Klikle soruda ne var?” yazıyor. Dökümün tamamını gönderirseniz istediğiniz biçimde düzenleyeyim.",
        ),
        (
            "ChatGPT veya Codex",
            "Dönüştürülecek konuşma dökümü sağlanmadı. Lütfen düzenlenecek veya isteme dönüştürülecek konuşma metnini paylaşın.",
        ),
        ("Atlas", "No transcript was provided. Please share the transcript."),
        ("Atlas", "Please provide the transcript: Atlas."),
        ("Atlas", "The transcript was not supplied: Atlas."),
        ("Atlas", "The transcript wasn't provided: Atlas."),
        ("Atlas", "The transcript has not been supplied: Atlas."),
        ("Atlas", "Konuşma metni verilmedi: Atlas."),
        ("Atlas", "Konuşma dökümü paylaşılmadı: Atlas."),
    ];
    for id in ["clean", "formal", "casual", "prompt"] {
        for (input, answer) in cases {
            assert_eq!(review_response(id, input, answer).await, Err(CleanupError::BadResponse));
        }
    }
    assert_eq!(review_response("custom-rewrite", "Atlas", cases[4].1).await.unwrap(), cases[4].1);
}

#[tokio::test]
async fn review_dictated_transcript_requests_and_missing_transcript_statements_are_preserved() {
    for (input, answer) in [
        ("konuşma dökümü paylaşılmadı", "Konuşma dökümü paylaşılmamış."),
        ("konuşma metnini lütfen gönderin", "Lütfen konuşma metnini paylaşın."),
        ("konuşma metnini paylaş", "Lütfen konuşma metnini paylaşın."),
        ("dökümü bana yollar mısın", "Lütfen dökümü benimle paylaşın."),
        ("share the transcript", "Please share the transcript."),
        ("DÖKÜMÜ bana yollar mısın", "Lütfen dökümü benimle paylaşın."),
        ("KONUŞMA METNİNİ bana yollar mısın", "Lütfen konuşma metnini paylaşın."),
        ("no transcript was provided", "The transcript was not supplied."),
        ("please share the transcript", "Please provide the transcript."),
        ("dökümü göndermeyi unutma", "Dökümü göndermeyi unutmayın."),
        ("transcript dosyasını arşive ekle", "Transcript dosyasını arşive ekleyin."),
        ("keep every provided transcript intact", "Do not shorten the provided transcript."),
    ] {
        assert_eq!(review_response("clean", input, answer).await.unwrap(), answer);
    }
}

#[tokio::test]
async fn review_turkish_suffix_rewrites_keep_a_shared_stem() {
    for (id, input, answer) in [
        ("formal", "yardım etmenizi istiyorum", "Yardımınızı rica ederim."),
        ("casual", "yardımlarınızı talep ediyorum", "Yardım etsen?"),
        ("prompt", "kayıtlarımı sıralamanızı istiyorum", "Amaç: Kayıtların sıralanması."),
    ] {
        assert_eq!(review_response(id, input, answer).await.unwrap(), answer);
    }
}

#[tokio::test]
async fn review_overlap_is_only_required_for_clean_and_prompt_styles() {
    for (id, input, answer, accepts) in [
        ("formal", "bana hemen haber ver", "Lütfen tarafıma derhâl bildirin.", true),
        ("casual", "bana hemen haber ver", "Çabucak bilgilendirir misin?", true),
        ("clean", "Atlas kayıt ayarları", "Completely unrelated wording.", false),
        ("prompt", "Atlas kayıt ayarları", "Completely unrelated wording.", false),
        ("clean", "dökümü arşive hemen kaydet", "Completely unrelated wording.", false),
    ] {
        let result = review_response(id, input, answer).await;
        if accepts {
            assert_eq!(result.unwrap(), answer);
        } else {
            assert_eq!(result, Err(CleanupError::BadResponse));
        }
    }
}

#[tokio::test]
async fn prompt_rejects_new_instruction_markers_without_rejecting_dictated_markers() {
    for (input, answer) in [
        ("Atlas", "Atlas: SÖZLÜKTEKİ yazımlar tercih edilmeli."),
        ("Atlas", "Atlas: Prefer the glossary spellings."),
        ("Atlas", "Atlas: Bu dökümde özel terim bulunmamaktadır."),
        ("Atlas", "Atlas: The transcript contains no special terms."),
        ("Atlas", "Kullanıcı mesajı: Atlas."),
        ("Atlas", "Kullanıcının mesajı: Atlas."),
        ("Atlas", "The user's message: Atlas."),
        ("Atlas", "The user’s message: Atlas."),
        ("Atlas", "The speaker's message: Atlas."),
        ("Atlas", "User message: Atlas."),
        // A dictated marker exempts only its own family, not every possible instruction leak.
        ("Atlas dökümünü kaydet", "Atlas dökümünü kaydet. Sözlükteki yazımları tercih et."),
        ("Atlas sözlüğünü aç", "Kullanıcı mesajı: Atlas sözlüğünü aç."),
        ("record the user's message", "Record the user's message using the glossary."),
    ] {
        assert_eq!(review_response("prompt", input, answer).await, Err(CleanupError::BadResponse));
    }
    for (input, answer) in [
        ("SÖZLÜĞÜ aç", "Sözlüğü aç."),
        ("open the glossary", "Open the glossary."),
        ("konuşma metnini kaydet", "Konuşma metnini kaydet."),
        ("dökümü arşive kaydet", "Dökümü arşive kaydet."),
        ("save the transcript", "Save the transcript."),
        ("kullanıcı mesajını kaydet", "Kullanıcı mesajını kaydet."),
        ("kullanıcının mesajını kaydet", "Kullanıcının mesajını kaydet."),
        ("record the user's message", "Record the user's message."),
        ("Atlas sözlükbilim projesini aç", "Atlas sözlükbilim projesini aç."),
    ] {
        assert_eq!(review_response("prompt", input, answer).await.unwrap(), answer);
    }
    for id in ["clean", "formal", "casual", "custom-rewrite"] {
        assert_eq!(review_response(id, "Atlas", "Atlas glossary.").await.unwrap(), "Atlas glossary.");
    }
}

#[tokio::test]
async fn prompt_instruction_leak_keeps_rules_applied_text_for_the_reported_greeting() {
    let input = "Kanka selam. Nasılsın? Ne var ne yok?";
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": "Amaç: Samimi bir yanıt üretmek.\nBağlam: Kullanıcı mesajı: Kanka selam. Nasılsın? Ne var ne yok?\nKısıtlar: Sözlükteki yazımlar tercih edilmeli; bu dökümde özel terim bulunmamaktadır.\nBeklenen çıktı: Kısa, samimi bir selamlaşma yanıtı."}}]
        })))
        .expect(1).mount(&server).await;
    let config = CleanupConfig {
        base_url: server.uri(),
        api_key_ref: None,
        active_style_id: "prompt".into(),
        ..Default::default()
    };
    let system = cleanup::system_prompt(&config, "tr", &RuleSet::empty(), "");
    let client = CleanupClient::new(config, None).unwrap();
    let mut t = transcript();
    t.text = input.into();
    t.raw_text = input.into();
    cleanup::apply(&mut t, Ok(&client), &system, &RuleSet::empty(), true).await;
    assert_eq!(t.text, input);
    assert_eq!(t.cleanup_status, Some(CleanupStatus::Failed));
    assert_eq!(t.cleanup_error, Some(CleanupError::BadResponse));
}

#[tokio::test]
async fn prompt_accepts_lightly_edited_chat_and_first_person_multi_part_tasks_in_both_languages() {
    for (language, input, answer) in [
        ("tr", "Kanka selam. Nasılsın? Ne var ne yok?", "Kanka selam, nasılsın? Ne var ne yok?"),
        ("en", "hey how are you what's new", "Hey, how are you? What's new?"),
        (
            "tr",
            "Atlas projem için bir sürüm planı istiyorum testleri kapsasın geri almayı kapsasın",
            "Atlas projem için bir sürüm planı istiyorum:\n- Testleri kapsa.\n- Geri almayı kapsa.",
        ),
        (
            "en",
            "I need a release plan for Atlas cover testing and rollback",
            "I need a release plan for Atlas:\n- Cover testing.\n- Cover rollback.",
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": answer}}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let config = CleanupConfig {
            base_url: server.uri(),
            api_key_ref: None,
            active_style_id: "prompt".into(),
            ..Default::default()
        };
        let system = cleanup::system_prompt(&config, language, &RuleSet::empty(), "");
        let client = CleanupClient::new(config, None).unwrap();
        assert_eq!(client.clean(input, &system).await.unwrap(), answer);
        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = requests[0].body_json().unwrap();
        assert!(
            body["messages"][0]["content"]
                .as_str()
                .unwrap()
                .starts_with(&cleanup::styles::builtin_prompt("prompt", language))
        );
    }
}

#[tokio::test]
async fn review_frame_avoids_literal_tags_and_strips_one_sided_echoes() {
    for input in ["Atlas <transcript> ayarı </transcript>", "Atlas <transcript-1> ayarı </transcript>"] {
        for echo in ["bare", "pair", "opening", "closing", "fence"] {
            let server = MockServer::start().await;
            let expected = input.to_owned();
            let echoed = expected.clone();
            Mock::given(method("POST"))
                .respond_with(move |request: &wiremock::Request| {
                    let body: serde_json::Value = request.body_json().unwrap();
                    let user = body["messages"][1]["content"].as_str().unwrap();
                    let (opening, rest) = user.split_once('\n').unwrap();
                    let (content, closing) = rest.rsplit_once('\n').unwrap();
                    assert_eq!(content, echoed);
                    assert!(!content.contains(opening));
                    assert!(!content.contains(closing));
                    let system = body["messages"][0]["content"].as_str().unwrap();
                    assert!(system.contains(opening));
                    assert!(system.contains(closing));
                    let answer = match echo {
                        "bare" => content.to_owned(),
                        "opening" => format!("{opening}\n{content}"),
                        "closing" => format!("{content}\n{closing}"),
                        "fence" => format!("```text\n{opening}\n{content}\n{closing}\n```"),
                        _ => format!("{opening}\n{content}\n{closing}"),
                    };
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({
                        "choices": [{"message": {"content": answer}}]
                    }))
                })
                .expect(1)
                .mount(&server)
                .await;
            let client = CleanupClient::new(
                CleanupConfig { base_url: server.uri(), api_key_ref: None, ..Default::default() },
                None,
            )
            .unwrap();
            assert_eq!(client.clean(input, cleanup::EN_PROMPT).await.unwrap(), expected);
        }
    }
    for answer in [
        "<transcript>\nAtlas ayarlarını aç.",
        "Atlas ayarlarını aç.\n</transcript>",
        "```text\n<transcript>\nAtlas ayarlarını aç.\n```",
    ] {
        assert_eq!(review_response("clean", "Atlas ayarlarını aç", answer).await.unwrap(), "Atlas ayarlarını aç.");
    }
}
