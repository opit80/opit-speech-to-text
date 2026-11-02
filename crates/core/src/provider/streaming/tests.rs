use super::*;
use crate::provider::presets;
use tokio::{net::TcpListener, time::timeout};

#[test]
fn explicit_tls_provider_builds_without_global_crypto_configuration() {
    assert!(tls_config().is_ok());
}

#[test]
fn elevenlabs_assembles_revisions_committed_segments_and_safe_errors() {
    let mut assembly = Assembly::default();
    let provider = ProviderKind::Elevenlabs;
    assert!(assembly.parse(provider, r#"{"message_type":"session_started"}"#).unwrap().live.is_none());
    for (kind, text, expected) in [
        ("partial_transcript", "one draft", "one draft"),
        ("partial_transcript", "one", "one"),
        ("committed_transcript", "one", "one"),
        ("partial_transcript", "two", "one two"),
        ("committed_transcript_with_timestamps", "two", "one two"),
    ] {
        let parsed = assembly.parse(provider, &json!({"message_type":kind,"text":text}).to_string()).unwrap();
        assert_eq!(parsed.live.unwrap().text, expected);
        assert_eq!(parsed.final_ack, kind.starts_with("committed"));
    }
    assert_eq!(assembly.transcript().text, "one two");
    for kind in [
        "error",
        "auth_error",
        "quota_exceeded",
        "commit_throttled",
        "unaccepted_terms",
        "rate_limited",
        "queue_overflow",
        "resource_exhausted",
        "session_time_limit_exceeded",
        "input_error",
        "invalid_request",
        "chunk_size_exceeded",
        "insufficient_audio_activity",
        "transcriber_error",
    ] {
        let error = assembly
            .parse(provider, &json!({"message_type":kind,"error":"private words and key"}).to_string())
            .err()
            .unwrap();
        assert!(!error.to_string().contains("private"));
    }
}

#[test]
fn deepgram_only_accumulates_final_results_and_tracks_finalize_ack() {
    let mut assembly = Assembly::default();
    for (text, final_result, ack, expected) in [
        ("draft", false, false, "draft"),
        ("one", true, false, "one"),
        ("draft two", false, false, "one draft two"),
        ("two", true, false, "one two"),
        ("", true, true, "one two"),
    ] {
        let parsed = assembly
            .parse(
                ProviderKind::Deepgram,
                &json!({"type":"Results","is_final":final_result,
            "from_finalize":ack,"channel":{"alternatives":[{"transcript":text}]}})
                .to_string(),
            )
            .unwrap();
        assert_eq!(parsed.live.unwrap().text, expected);
        assert_eq!(parsed.final_ack, ack);
    }
    assert_eq!(assembly.transcript().text, "one two");
    for body in ["invalid private text", r#"{"type":"Results"}"#, r#"{"type":"Error","description":"private"}"#] {
        assert!(!assembly.parse(ProviderKind::Deepgram, body).err().unwrap().to_string().contains("private"));
    }
}

#[test]
fn urls_auth_languages_terms_and_capability_are_explicit() {
    for mut profile in [presets::deepgram(), presets::elevenlabs()] {
        assert!(profile.supports_streaming());
        profile.send_keywords = true;
        let req = request(&profile, Some(" secret "), &["term one".into(), "ğüşi".into()]).unwrap();
        let uri = req.uri().to_string();
        assert!(!uri.contains("secret"));
        assert!(uri.contains("language"));
        if profile.provider == ProviderKind::Deepgram {
            assert_eq!(req.headers()["Authorization"], "Token secret");
            assert!(uri.contains("keyterm=term+one"));
            assert!(uri.contains("model=nova-3"));
        } else {
            assert_eq!(req.headers()["xi-api-key"], "secret");
            assert!(uri.contains("keyterms=term+one"));
            assert!(uri.contains("commit_strategy=manual"));
            assert!(uri.contains("language_code=tr"));
        }
        profile.language = "auto".into();
        let uri = request(&profile, None, &[]).unwrap().uri().to_string();
        assert!(uri.contains(if profile.provider == ProviderKind::Deepgram {
            "language=multi"
        } else {
            "include_language_detection=true"
        }));
    }
    assert!(!presets::groq().supports_streaming());
    assert!(!presets::local().supports_streaming());
}

async fn local_profile(provider: ProviderKind) -> (TcpListener, Profile) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut profile = if provider == ProviderKind::Deepgram { presets::deepgram() } else { presets::elevenlabs() };
    profile.base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    (listener, profile)
}

#[tokio::test]
async fn local_servers_verify_partial_audio_and_finish_handshakes_for_both_protocols() {
    for provider in [ProviderKind::Elevenlabs, ProviderKind::Deepgram] {
        let (listener, profile) = local_profile(provider).await;
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let audio = socket.next().await.unwrap().unwrap();
            if provider == ProviderKind::Elevenlabs {
                let value: Value = serde_json::from_str(audio.to_text().unwrap()).unwrap();
                assert_eq!(value["commit"], false);
                assert!(value.get("previous_text").is_none());
                assert_eq!(STANDARD.decode(value["audio_base_64"].as_str().unwrap()).unwrap(), [1, 0, 255, 255]);
                socket
                    .send(Message::Text(json!({"message_type":"partial_transcript","text":"draft"}).to_string().into()))
                    .await
                    .unwrap();
                let value: Value =
                    serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                assert_eq!(value["commit"], true);
                socket
                    .send(Message::Text(
                        json!({"message_type":"committed_transcript","text":"final"}).to_string().into(),
                    ))
                    .await
                    .unwrap();
            } else {
                assert_eq!(audio.into_data().as_ref(), &[1, 0, 255, 255]);
                socket
                    .send(Message::Text(
                        json!({"type":"Results","is_final":false,"channel":{"alternatives":[{"transcript":"draft"}]}})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .unwrap();
                let keep: Value =
                    serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                assert_eq!(keep["type"], "KeepAlive");
                let finish: Value =
                    serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                assert_eq!(finish["type"], "Finalize");
                socket.send(Message::Text(json!({"type":"Results","is_final":true,"from_finalize":true,"channel":{"alternatives":[{"transcript":"final"}]}}).to_string().into())).await.unwrap();
                let close: Value =
                    serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                assert_eq!(close["type"], "CloseStream");
            }
            let _ = socket.next().await;
        });
        let mut session = WebSocketSession::connect(profile, Some("key".into()), &[]).await.unwrap();
        session.send_audio(&[1, -1]).await.unwrap();
        assert_eq!(session.next_update().await.unwrap().unwrap().text, "draft");
        session.keep_alive().await.unwrap();
        assert_eq!(session.finish().await.unwrap().text, "final");
        timeout(Duration::from_secs(2), server).await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn local_error_frames_abrupt_disconnects_and_handshake_statuses_are_safe() {
    for scenario in ["error", "disconnect", "401", "403", "429"] {
        let (listener, profile) = local_profile(ProviderKind::Deepgram).await;
        let server = tokio::spawn(async move {
            let (mut tcp, _) = listener.accept().await.unwrap();
            if let Ok(code) = scenario.parse::<u16>() {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buffer = [0; 2048];
                let _ = tcp.read(&mut buffer).await;
                tcp.write_all(
                    format!("HTTP/1.1 {code} Refused\r\nRetry-After: 3\r\nContent-Length: 0\r\n\r\n").as_bytes(),
                )
                .await
                .unwrap();
                return;
            }
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            if scenario == "error" {
                socket.send(Message::Text(r#"{"type":"Error","description":"private secret"}"#.into())).await.unwrap();
            }
        });
        let error = match WebSocketSession::connect(profile, Some("private secret".into()), &[]).await {
            Ok(mut session) => session.next_update().await.err().unwrap(),
            Err(error) => error,
        };
        assert!(!error.to_string().contains("private"));
        if scenario == "401" || scenario == "403" {
            assert_eq!(error.code(), "invalid_key");
        }
        if scenario == "429" {
            assert_eq!(error.retry_after().unwrap().as_secs(), 3);
        }
        server.await.unwrap();
    }
}
