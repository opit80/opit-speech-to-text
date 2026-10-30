use super::*;
use opit_core::provider::{ProviderKind, presets};

fn live_config() -> AppConfig {
    let mut config = AppConfig::default();
    config.profiles.push(presets::deepgram());
    config.active_profile_id = "deepgram".into();
    config.recording.live_text = true;
    config.history.save_audio = true;
    config
}

async fn eventually(predicate: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("streaming condition timed out");
}

#[tokio::test]
async fn live_partials_reach_overlay_and_final_uses_normal_rules_history_and_audio() {
    let mut config = live_config();
    config.rules.numbers_as_words = true;
    let mut h = harness(config);
    h.providers.live.result.lock().unwrap().as_mut().unwrap().text = "12 items".into();
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    eventually(|| h.overlay.last().live_text.as_deref() == Some("live draft")).await;
    assert!(h.overlay.last().recording.is_some());
    assert!(h.overlay.last().level.is_some());
    assert!(h.paster.texts().is_empty());
    assert_eq!(h.providers.calls(), 0);
    h.send(Msg::Toggle);
    h.expect(&[Transcribing, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["On iki items "]);
    assert_eq!(h.providers.calls(), 0);
    let entries = h.history.entries();
    assert_eq!(entries[0].raw_text, "12 items");
    assert!(entries[0].has_audio);
    assert_eq!(entries[0].profile_id, "deepgram");
    assert_eq!(h.providers.live.audio.lock().unwrap().len(), 16_000);
    assert_eq!(h.providers.live.finishes.load(SeqCst), 1);
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn live_failures_use_the_recorded_audio_and_existing_retry_and_fallback() {
    for scenario in ["connect", "send", "frame", "finish", "empty", "timeout"] {
        let mut config = live_config();
        config.profiles.last_mut().unwrap().fallback_profile_id = Some("groq".into());
        let mut h = harness(config);
        let failure = ProviderError::Unauthorized(401);
        match scenario {
            "connect" => *h.providers.live.connect_error.lock().unwrap() = Some(failure),
            "send" => *h.providers.live.send_error.lock().unwrap() = Some(failure),
            "frame" => *h.providers.live.updates.lock().unwrap() = [Err(failure)].into(),
            "finish" => *h.providers.live.result.lock().unwrap() = Err(failure),
            "empty" => h.providers.live.result.lock().unwrap().as_mut().unwrap().text.clear(),
            "timeout" => h.providers.live.hang_finish.store(true, SeqCst),
            _ => unreachable!(),
        }
        h.script("deepgram", vec![Err(ProviderError::Server(503)), Err(ProviderError::Server(503))]);
        h.script("groq", vec![Ok("batch recovered")]);
        h.send(Msg::Toggle);
        h.expect(&[Recording]).await;
        h.send(Msg::Toggle);
        h.wait_for(|s| s.state == Idle).await;
        assert_eq!(h.paster.texts(), ["batch recovered "], "{scenario}");
        assert_eq!(h.providers.calls(), 3, "{scenario}");
        assert_eq!(h.history.entries()[0].profile_id, "groq");
        h.send(Msg::Shutdown);
    }
}

#[tokio::test]
async fn disabled_or_unsupported_live_text_never_opens_a_session() {
    for scenario in ["off", "unsupported"] {
        let mut config = live_config();
        if scenario == "off" {
            config.recording.live_text = false;
        } else {
            config.profiles.last_mut().unwrap().provider = ProviderKind::OpenaiCompatible;
        }
        let mut h = harness(config);
        h.script("deepgram", vec![Ok("batch words")]);
        h.send(Msg::Toggle);
        h.expect(&[Recording]).await;
        h.send(Msg::Toggle);
        h.expect(&[Transcribing, Pasting, Idle]).await;
        assert_eq!(h.providers.live.connections.load(SeqCst), 0);
        assert_eq!(h.providers.calls(), 1);
        h.send(Msg::Shutdown);
    }
}

#[tokio::test]
async fn cancel_and_shutdown_drop_the_socket_without_finish_or_batch() {
    for message in [Msg::Cancel, Msg::Shutdown] {
        let mut h = harness(live_config());
        h.send(Msg::Toggle);
        h.expect(&[Recording]).await;
        eventually(|| h.overlay.last().live_text.is_some()).await;
        h.send(message);
        h.wait_for(|s| s.state == Idle).await;
        eventually(|| h.providers.live.dropped.load(SeqCst) == 1).await;
        assert_eq!(h.providers.live.finishes.load(SeqCst), 0);
        assert_eq!(h.providers.calls(), 0);
        assert!(h.paster.texts().is_empty());
        h.send(Msg::Shutdown);
    }
}

#[tokio::test]
async fn speech_gate_rejection_discards_a_stream_final() {
    let mut h = harness(live_config());
    *h.mic.recording.lock().unwrap() = Some(silent_recording());
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.send(Msg::Toggle);
    h.wait_for(|s| s.state == Idle).await;
    assert!(h.paster.texts().is_empty());
    assert!(h.history.entries().is_empty());
    assert_eq!(h.providers.calls(), 0);
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn audio_before_connection_is_buffered_and_pause_does_not_send_audio() {
    let mut h = harness(live_config());
    h.providers.live.connect_gate.forget_permits(1000);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.mic.append_samples(&vec![0.2; 9600]);
    h.handle.control(ControlAction::Pause).await.unwrap();
    h.mic.append_samples(&vec![0.8; 9600]);
    h.providers.live.connect_gate.add_permits(1);
    eventually(|| h.providers.live.audio.lock().unwrap().len() >= 16_000).await;
    let paused_len = h.providers.live.audio.lock().unwrap().len();
    h.mic.append_samples(&vec![0.8; 9600]);
    h.handle.control(ControlAction::Resume).await.unwrap();
    h.mic.append_samples(&vec![0.2; 9600]);
    h.send(Msg::Toggle);
    h.wait_for(|s| s.state == Idle).await;
    assert!(paused_len <= 17_600);
    assert_eq!(h.providers.live.audio.lock().unwrap().len(), 19_200);
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn per_app_profile_decides_streaming_and_stale_session_updates_are_ignored() {
    let mut config = live_config();
    config.active_profile_id = "groq".into();
    config.app_rules.rules.push(opit_core::app_rules::AppRule {
        exe: "code.exe".into(),
        profile_id: Some("deepgram".into()),
        ..Default::default()
    });
    let mut h = harness(config);
    *h.foreground.info.lock().unwrap() =
        Some(crate::platform::ForegroundInfo { exe: "code.exe".into(), title: String::new() });
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    eventually(|| h.overlay.last().live_text.is_some()).await;
    h.send(Msg::LiveText { session: 9999, text: "stale".into() });
    h.handle.control(ControlAction::Pause).await.unwrap();
    assert_ne!(h.overlay.last().live_text.as_deref(), Some("stale"));
    h.send(Msg::Toggle);
    h.wait_for(|s| s.state == Idle).await;
    assert_eq!(h.providers.live.connections.load(SeqCst), 1);
    assert_eq!(h.history.entries()[0].profile_id, "deepgram");
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn queued_stream_finishes_while_an_earlier_batch_job_is_busy() {
    let mut config = live_config();
    config.active_profile_id = "groq".into();
    let mut h = harness(config.clone());
    h.providers.gate.forget_permits(1000);
    h.script("groq", vec![Ok("earlier batch")]);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.send(Msg::Toggle);
    h.expect(&[Transcribing]).await;
    config.active_profile_id = "deepgram".into();
    h.settings.replace(settings(config));
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.send(Msg::Stop);
    eventually(|| h.providers.live.finishes.load(SeqCst) == 1 && h.providers.live.dropped.load(SeqCst) == 1).await;
    h.providers.gate.add_permits(1000);
    h.wait_for(|s| s.state == Idle).await;
    assert_eq!(h.providers.calls(), 1);
    assert_eq!(h.history.entries().iter().map(|e| e.profile_id.as_str()).collect::<Vec<_>>(), ["groq", "deepgram"]);
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn streaming_finals_continue_through_edit_translate_and_cleanup() {
    for mode in ["edit", "translate", "cleanup"] {
        let (_server, mut h) = voice_harness(200, "Final transformed output").await;
        let mut config = h.settings.current().config.clone();
        config.profiles.push(presets::elevenlabs());
        config.active_profile_id = "elevenlabs".into();
        config.recording.live_text = true;
        config.cleanup.enabled = mode == "cleanup";
        h.settings.replace(settings(config));
        let event = if mode == "edit" { HotkeyEvent::Edit } else { HotkeyEvent::Translate };
        if mode == "cleanup" {
            h.send(Msg::Toggle);
            h.expect(&[Recording]).await;
        } else {
            h.hotkey(event.clone());
            h.expect(&[Transcribing, Recording]).await;
        }
        eventually(|| h.overlay.last().live_text.is_some()).await;
        if mode == "cleanup" {
            h.send(Msg::Toggle);
        } else {
            h.hotkey(event);
        }
        h.expect(&[Transcribing, DictationState::Cleaning, Pasting, Idle]).await;
        assert_eq!(h.paster.texts(), ["Final transformed output "]);
        assert_eq!(h.providers.calls(), 0);
        let rows = h.history.entries();
        assert_eq!(rows[0].profile_id, "elevenlabs");
        assert_eq!(rows[0].mode, if mode == "cleanup" { "dictation" } else { mode });
        assert_eq!(rows[0].cleanup_status, Some(cleanup::CleanupStatus::Ok));
        h.send(Msg::Shutdown);
    }
}

#[tokio::test]
async fn paused_deepgram_sessions_keep_alive_without_new_audio() {
    let mut h = harness(live_config());
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    eventually(|| h.overlay.last().live_text.is_some()).await;
    h.handle.control(ControlAction::Pause).await.unwrap();
    let count = h.providers.live.audio.lock().unwrap().len();
    tokio::time::sleep(Duration::from_millis(4100)).await;
    assert!(h.providers.live.keepalives.load(SeqCst) >= 1);
    assert_eq!(h.providers.live.audio.lock().unwrap().len(), count);
    h.send(Msg::Cancel);
    h.wait_for(|s| s.state == Idle).await;
    h.send(Msg::Shutdown);
}
