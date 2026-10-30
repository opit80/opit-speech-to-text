use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use opit_core::config::{AppConfig, HotkeyMode, PasteAfter, PasteMethod};
use opit_core::history::NewDictation;
use opit_core::pipeline::{PreparedAudio, TranscriptStatus};
use opit_core::provider::{Profile, ProviderError, RawTranscript, TranscribeRequest, Transcriber};
use tokio::sync::{Semaphore, mpsc};

use super::transcription::{CleanupLog, wait_for_retry};
use super::*;
use crate::platform::fake::*;
use crate::platform::{HotkeyEvent, MicError, OverlayAction, PasteOutcome, Sound, Tone};
use crate::settings::{Settings, SettingsHandle};

use DictationState::{Cancelled, Idle, Pasting, Recording, Transcribing};

#[test]
fn cleanup_log_records_one_safe_metadata_line_for_every_outcome() {
    #[derive(Clone)]
    struct Writer(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    for (outcome, error, kind) in [
        ("ok", None, "None"),
        ("skipped", None, "Cancelled"),
        ("failed", Some(CleanupError::Timeout), "Timeout"),
        ("failed", Some(CleanupError::PaymentRequired), "PaymentRequired"),
        ("failed", Some(CleanupError::RateLimited), "RateLimited"),
        ("failed", Some(CleanupError::Server), "Server"),
        ("failed", Some(CleanupError::Truncated), "Truncated"),
    ] {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let writer = Writer(bytes.clone());
        let subscriber =
            tracing_subscriber::fmt().without_time().with_ansi(false).with_writer(move || writer.clone()).finish();
        tracing::subscriber::with_default(subscriber, || {
            let mut log = CleanupLog::new(42, "test-model".into());
            if outcome != "skipped" {
                log.finished(error);
            }
        });
        let text = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
        assert_eq!(text.lines().count(), 1);
        for field in ["session=42", "attempts=0", "elapsed_ms=", "model=test-model", kind, outcome] {
            assert!(text.contains(field), "missing {field}");
        }
    }
}

// ---------- scripted providers ----------
mod live_tests;

#[tokio::test]
async fn app_rule_overrides_are_private_start_snapshots_with_the_selected_profiles_fallback() {
    use crate::platform::ForegroundInfo;
    use opit_core::app_rules::AppRule;
    let mut config = AppConfig::default();
    config.profiles[1].fallback_profile_id = Some("groq".into());
    config.app_rules.rules.push(AppRule {
        id: "editor".into(),
        exe: "code.exe".into(),
        title_contains: "İŞ IŞIK".into(),
        style_id: Some("raw".into()),
        profile_id: Some("openai".into()),
        paste_method: Some(PasteMethod::Type),
        paste_after: Some(PasteAfter::CtrlEnter),
        ..AppRule::default()
    });
    let mut h = harness(config.clone());
    *h.foreground.info.lock().unwrap() =
        Some(ForegroundInfo { exe: "CODE.EXE".into(), title: "iş ışık - editor".into() });
    h.script("openai", vec![Err(ProviderError::Server(503)), Err(ProviderError::Server(503))]);
    h.script("groq", vec![Ok("fallback words")]);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    assert_eq!(*h.foreground.calls.lock().unwrap(), [true]);
    assert!(h.overlay.last().text.contains("Raw"));
    assert_eq!(h.settings.current().config, config, "global profile/style/paste must stay unchanged");
    config.app_rules.rules.clear();
    config.cleanup.active_style_id = "casual".into();
    config.paste.method = PasteMethod::ShiftInsert;
    h.settings.replace(settings(config));
    *h.foreground.info.lock().unwrap() = None;
    h.send(Msg::Toggle);
    h.expect(&[Transcribing, Pasting, Idle]).await;
    let requests = h.paster.requests.lock().unwrap();
    assert_eq!(requests[0].method, PasteMethod::Type);
    assert_eq!(requests[0].after, PasteAfter::CtrlEnter);
    let entries = h.history.entries();
    assert_eq!(entries[0].profile_id, "groq", "the overridden profile's fallback is used");
    assert_eq!(entries[0].cleanup_style_id.as_deref(), Some("raw"));
    assert!(h.providers.scripts.lock().unwrap()["openai"].is_empty(), "overridden primary was tried first");
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn app_rule_profile_and_style_overrides_survive_queueing_and_focus_changes() {
    use crate::platform::ForegroundInfo;
    use opit_core::app_rules::AppRule;
    let mut config = AppConfig::default();
    config.app_rules.rules.push(AppRule {
        exe: "code.exe".into(),
        style_id: Some("formal".into()),
        profile_id: Some("openai".into()),
        paste_method: Some(PasteMethod::ShiftInsert),
        paste_after: Some(PasteAfter::Enter),
        ..AppRule::default()
    });
    let mut h = harness(config);
    h.providers.gate.forget_permits(1_000);
    h.script("openai", vec![Ok("first application")]);
    h.script("groq", vec![Ok("second application")]);
    *h.foreground.info.lock().unwrap() = Some(ForegroundInfo { exe: "code.exe".into(), title: String::new() });
    h.handle.control(ControlAction::Start).await.unwrap();
    h.expect(&[Recording]).await;
    h.send(Msg::Toggle);
    h.expect(&[Transcribing]).await;
    *h.foreground.info.lock().unwrap() = Some(ForegroundInfo { exe: "other.exe".into(), title: String::new() });
    h.handle.control(ControlAction::Start).await.unwrap();
    h.expect(&[Recording]).await;
    h.send(Msg::Toggle);
    h.providers.gate.add_permits(10);
    h.wait_for(|s| s.state == Idle).await;
    let entries = h.history.entries();
    assert_eq!(entries.iter().map(|e| e.profile_id.as_str()).collect::<Vec<_>>(), ["openai", "groq"]);
    assert_eq!(
        entries.iter().map(|e| e.cleanup_style_id.as_deref()).collect::<Vec<_>>(),
        [Some("formal"), Some("clean")]
    );
    let requests = h.paster.requests.lock().unwrap();
    assert_eq!((requests[0].method, requests[0].after), (PasteMethod::ShiftInsert, PasteAfter::Enter));
    assert_eq!((requests[1].method, requests[1].after), (PasteMethod::CtrlV, PasteAfter::None));
    assert_eq!(*h.foreground.calls.lock().unwrap(), [false, false]);
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn app_rules_keep_globals_when_disabled_unmatched_unreadable_or_missing_references() {
    use crate::platform::ForegroundInfo;
    use opit_core::app_rules::AppRule;
    for scenario in ["disabled", "rule_disabled", "unmatched", "unreadable", "missing_refs", "empty"] {
        let mut config = AppConfig::default();
        config.app_rules.enabled = scenario != "disabled";
        config.paste.after = PasteAfter::Enter;
        if scenario != "empty" {
            config.app_rules.rules.push(AppRule {
                id: "editor".into(),
                enabled: scenario != "rule_disabled",
                exe: "code.exe".into(),
                style_id: Some(if scenario == "missing_refs" { "deleted" } else { "raw" }.into()),
                profile_id: Some(if scenario == "missing_refs" { "deleted" } else { "openai" }.into()),
                ..AppRule::default()
            });
        }
        let mut h = harness(config.clone());
        if scenario != "unreadable" {
            *h.foreground.info.lock().unwrap() = Some(ForegroundInfo {
                exe: if scenario == "unmatched" { "other.exe" } else { "code.exe" }.into(),
                title: "private caption".into(),
            });
        }
        h.script("groq", vec![Ok("global words")]);
        h.send(Msg::Toggle);
        h.expect(&[Recording]).await;
        assert_eq!(
            *h.foreground.calls.lock().unwrap(),
            if ["disabled", "rule_disabled", "empty"].contains(&scenario) { vec![] } else { vec![false] }
        );
        h.send(Msg::Toggle);
        h.expect(&[Transcribing, Pasting, Idle]).await;
        let entries = h.history.entries();
        assert_eq!(entries[0].profile_id, "groq", "{scenario}");
        assert_eq!(entries[0].cleanup_style_id.as_deref(), Some("clean"), "{scenario}");
        let requests = h.paster.requests.lock().unwrap();
        assert_eq!(requests[0].method, config.paste.method);
        assert_eq!(requests[0].after, PasteAfter::Enter);
        assert_eq!(h.settings.current().config, config);
        h.send(Msg::Shutdown);
    }
}

#[tokio::test]
async fn cleanup_style_snapshot_survives_unrelated_config_changes_and_raw_skips_clients() {
    for raw in [false, true] {
        let mut config = AppConfig::default();
        config.cleanup.enabled = true;
        config.cleanup.active_style_id = if raw { "raw" } else { "formal" }.into();
        config.cleanup.styles.iter_mut().find(|s| s.id == "formal").unwrap().model = Some("formal-model".into());
        let mut h = harness(config.clone());
        h.script("groq", vec![Ok("three clear words")]);
        h.send(Msg::Toggle);
        h.expect(&[Recording]).await;
        config.cleanup.active_style_id = "clean".into();
        config.cleanup.model = "changed-model".into();
        config.cleanup.styles.iter_mut().find(|s| s.id == "formal").unwrap().model = Some("changed-override".into());
        h.settings.replace(settings(config));
        h.send(Msg::Stop);
        h.wait_for(|s| s.state == Idle).await;
        let saved = h.history.entries();
        assert_eq!(saved[0].cleanup_style_id.as_deref(), Some(if raw { "raw" } else { "formal" }));
        if raw {
            assert_eq!(saved[0].cleanup_status, Some(cleanup::CleanupStatus::Skipped));
            assert_eq!(saved[0].cleanup_model, None);
            assert!(h.providers.cleanup_configs.lock().unwrap().is_empty());
        } else {
            assert_eq!(saved[0].cleanup_model.as_deref(), Some("formal-model"));
            assert_eq!(h.providers.cleanup_configs.lock().unwrap()[0].model, "formal-model");
        }
        h.send(Msg::Shutdown);
    }
}

#[tokio::test]
async fn style_hotkeys_persist_in_order_and_switch_the_recording_before_stop() {
    let mut h = harness(AppConfig::default());
    h.hotkey(HotkeyEvent::StyleCycle);
    h.handle.control(ControlAction::Start).await.unwrap();
    h.expect(&[Recording]).await;
    assert!(h.overlay.last().text.contains("Formal"));
    h.hotkey(HotkeyEvent::Style("prompt".into()));
    h.hotkey(HotkeyEvent::StyleCycle);
    // The recording uses the latest choice without waiting for persistence.
    h.script("groq", vec![Ok("three clear words")]);
    h.send(Msg::Stop);
    h.wait_for(|s| s.state == Idle).await;
    h.wait_for_style_saves(3).await;
    assert_eq!(*h.events.style_saves.lock().unwrap(), ["formal", "prompt", "raw"]);
    assert_eq!(h.history.entries()[0].cleanup_style_id.as_deref(), Some("raw"));
    assert_eq!(h.settings.current().config.cleanup.active_style_id, "raw");
    h.hotkey(HotkeyEvent::Style("clean".into()));
    // A control round-trip waits for the immediate idle feedback, not its disk write.
    h.handle.control(ControlAction::Cancel).await.unwrap();
    assert!(
        h.overlay
            .views
            .lock()
            .unwrap()
            .iter()
            .any(|v| v.text == "Clean" && v.hide_after == Some(Duration::from_millis(1500)))
    );
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn blocked_style_save_does_not_delay_lifecycle_or_media_release() {
    for end in ["stop", "escape", "shutdown", "limit"] {
        let mut h = harness(AppConfig::default());
        h.providers.gate.forget_permits(1_000);
        h.send(Msg::Toggle);
        h.expect(&[Recording]).await;
        let blocked = h.block_style_saves();
        h.hotkey(HotkeyEvent::Style("formal".into()));
        tokio::time::timeout(Duration::from_secs(2), blocked.0.entered.acquire()).await.unwrap().unwrap().forget();
        match end {
            "stop" => {
                tokio::time::timeout(Duration::from_secs(1), h.handle.control(ControlAction::Stop))
                    .await
                    .unwrap()
                    .unwrap();
            }
            "escape" => {
                h.hotkey(HotkeyEvent::Escape);
                let _ =
                    tokio::time::timeout(Duration::from_secs(1), h.handle.control(ControlAction::Pause)).await.unwrap();
            }
            "shutdown" => {
                h.hotkey(HotkeyEvent::Style("prompt".into()));
                h.hotkey(HotkeyEvent::StyleCycle);
                h.send(Msg::Shutdown);
                tokio::time::timeout(Duration::from_secs(1), h.wait_for(|s| s.state == Idle)).await.unwrap();
            }
            "limit" => {
                let session = h.overlay.last().recording.unwrap().session;
                tokio::time::pause();
                tokio::time::advance(Duration::from_secs(181)).await;
                h.send(Msg::LimitReached { session });
                let _ =
                    tokio::time::timeout(Duration::from_secs(1), h.handle.control(ControlAction::Pause)).await.unwrap();
                tokio::time::resume();
            }
            _ => unreachable!(),
        }
        let calls = h.media.calls.lock().unwrap().clone();
        assert_eq!(calls, [Some((opit_core::config::MediaMode::Duck, 50)), None], "{end}");
        assert!(h.events.style_saves.lock().unwrap().is_empty());
        drop(blocked);
        h.wait_for_style_saves(if end == "shutdown" { 3 } else { 1 }).await;
        if end == "shutdown" {
            assert_eq!(*h.events.style_saves.lock().unwrap(), ["formal", "prompt", "raw"]);
        }
        h.providers.gate.add_permits(10);
        h.send(Msg::Shutdown);
    }
}

#[tokio::test]
async fn queued_style_switches_apply_immediately_and_persist_in_order_after_stop() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("three clear words")]);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    let blocked = h.block_style_saves();
    h.hotkey(HotkeyEvent::StyleCycle);
    tokio::time::timeout(Duration::from_secs(2), blocked.0.entered.acquire()).await.unwrap().unwrap().forget();
    h.hotkey(HotkeyEvent::Style("prompt".into()));
    h.hotkey(HotkeyEvent::StyleCycle);
    tokio::time::timeout(Duration::from_secs(1), h.handle.control(ControlAction::Stop)).await.unwrap().unwrap();
    h.wait_for(|s| s.state == Idle).await;
    assert_eq!(h.history.entries()[0].cleanup_style_id.as_deref(), Some("raw"));
    assert!(h.events.style_saves.lock().unwrap().is_empty());
    drop(blocked);
    h.wait_for_style_saves(3).await;
    assert_eq!(*h.events.style_saves.lock().unwrap(), ["formal", "prompt", "raw"]);
    assert_eq!(h.settings.current().config.cleanup.active_style_id, "raw");
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn pending_style_is_used_at_next_start_and_old_completion_keeps_its_snapshot() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("three clear words")]);
    let blocked = h.block_style_saves();
    h.hotkey(HotkeyEvent::Style("formal".into()));
    tokio::time::timeout(Duration::from_secs(2), blocked.0.entered.acquire()).await.unwrap().unwrap().forget();
    h.hotkey(HotkeyEvent::StyleCycle);
    h.handle.control(ControlAction::Start).await.unwrap();
    h.expect(&[Recording]).await;
    assert!(h.overlay.last().text.contains("Casual"));
    // A newer explicit selection must not be replaced by an older save completion.
    let style = h.settings.current().config.cleanup.style("prompt").unwrap().clone();
    h.send(Msg::StyleChanged(style));
    h.handle.control(ControlAction::Pause).await.unwrap();
    drop(blocked);
    h.wait_for_style_saves(3).await;
    assert_eq!(*h.events.style_saves.lock().unwrap(), ["formal", "casual", "prompt"]);
    assert_eq!(h.settings.current().config.cleanup.active_style_id, "prompt");
    h.handle.control(ControlAction::Stop).await.unwrap();
    h.wait_for(|s| s.state == Idle).await;
    assert_eq!(h.history.entries()[0].cleanup_style_id.as_deref(), Some("prompt"));
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn failed_latest_style_save_restores_last_confirmed_style_for_the_same_recording() {
    let mut h = harness(AppConfig::default());
    h.events.style_failures.lock().unwrap().insert("prompt".into());
    h.script("groq", vec![Ok("three clear words")]);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    let blocked = h.block_style_saves();
    h.hotkey(HotkeyEvent::Style("formal".into()));
    tokio::time::timeout(Duration::from_secs(2), blocked.0.entered.acquire()).await.unwrap().unwrap().forget();
    h.hotkey(HotkeyEvent::Style("prompt".into()));
    h.handle.control(ControlAction::Pause).await.unwrap();
    assert!(h.overlay.last().text.contains("AI prompt"));
    drop(blocked);
    tokio::time::timeout(Duration::from_secs(2), async {
        while !h.overlay.last().text.contains("Formal") {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    h.handle.control(ControlAction::Stop).await.unwrap();
    h.wait_for(|s| s.state == Idle).await;
    assert_eq!(h.history.entries()[0].cleanup_style_id.as_deref(), Some("formal"));
    assert_eq!(h.settings.current().config.cleanup.active_style_id, "formal");
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn cycle_advances_from_the_recording_style_snapshot() {
    let mut config = AppConfig::default();
    config.cleanup.active_style_id = "formal".into();
    let mut h = harness(config.clone());
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    config.cleanup.active_style_id = "clean".into();
    h.settings.replace(settings(config));
    h.hotkey(HotkeyEvent::StyleCycle);
    h.handle.control(ControlAction::Pause).await.unwrap();
    h.wait_for_style_saves(1).await;
    assert_eq!(h.settings.current().config.cleanup.active_style_id, "casual");
    assert!(h.overlay.last().text.contains("Casual"));
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn saving_style_metadata_while_holding_each_recording_shortcut_keeps_the_stop_release() {
    use crate::app_core::{AppCore, Platform};
    use crate::platform::keys::{KeyTracker, track_shortcuts};
    for vk in [0x7C, 0x7D, 0x7E] {
        let (server, mut h) = voice_harness(200, "Edited output.").await;
        if vk == 0x7C {
            server.reset().await;
        }
        let mut config = h.settings.current().config.clone();
        config.hotkey.mode = HotkeyMode::PushToTalk;
        config.hotkey.keys = vec!["F13".into()];
        config.hotkey.edit_keys = vec!["F14".into()];
        config.hotkey.translate_keys = vec!["F15".into()];
        h.settings.replace(settings(config.clone()));
        let scratch = tempfile::tempdir().unwrap();
        let core = Arc::new(AppCore::new(
            crate::startup::Paths::under(scratch.path().to_owned()),
            h.settings.clone(),
            h.handle.clone(),
            None,
            Platform {
                foreground: h.foreground.clone(),
                media: h.media.clone(),
                secrets: Arc::new(FakeSecrets::default()),
                hotkey: h.hotkey.clone(),
                mic: h.mic.clone(),
                overlay: h.overlay.clone(),
                autostart: Arc::new(FakeAutostart::default()),
            },
            None,
            Some("en-US".into()),
            Vec::new(),
        ));
        let install = core.clone();
        tokio::task::spawn_blocking(move || install.apply_hotkey(&install.config())).await.unwrap();
        let registered = h.hotkey.registered.lock().unwrap().len();
        let mut main = KeyTracker::new(vec![0x7C]);
        let mut auxiliary = vec![
            (HotkeyEvent::Edit, KeyTracker::new(vec![0x7D])),
            (HotkeyEvent::Translate, KeyTracker::new(vec![0x7E])),
        ];
        h.script("groq", vec![Ok("three clear words")]);
        for event in track_shortcuts(&mut main, &mut auxiliary, vk, true, |_| true) {
            h.hotkey(event);
        }
        h.wait_for(|status| status.state == Recording).await;
        config.cleanup.styles[0].name = "Renamed style".into();
        config.cleanup.styles[0].prompt = "Updated prompt".into();
        tokio::task::spawn_blocking(move || core.save_config(config)).await.unwrap().unwrap();
        assert_eq!(h.hotkey.registered.lock().unwrap().len(), registered);
        for event in track_shortcuts(&mut main, &mut auxiliary, vk, false, |_| true) {
            h.hotkey(event);
        }
        h.wait_for(|status| status.state == Idle).await;
        assert_eq!(h.mic.finished.load(SeqCst), 1);
        assert_eq!(h.media.calls.lock().unwrap().last(), Some(&None));
        assert_eq!(
            h.history.entries()[0].mode,
            match vk {
                0x7C => "dictation",
                0x7D => "edit",
                _ => "translate",
            }
        );
        h.send(Msg::Shutdown);
    }
}

#[tokio::test]
async fn explicit_style_notification_updates_recording_but_keeps_queued_snapshots() {
    let mut h = harness(AppConfig::default());
    h.providers.gate.forget_permits(1_000);
    h.script("groq", vec![Ok("first recording"), Ok("second recording")]);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.send(Msg::Stop);
    h.expect(&[Transcribing]).await;
    h.handle.control(ControlAction::Start).await.unwrap();
    h.expect(&[Recording]).await;
    let style = h.settings.current().config.cleanup.style("casual").unwrap().clone();
    h.send(Msg::StyleChanged(style));
    h.handle.control(ControlAction::Pause).await.unwrap();
    assert!(h.overlay.last().text.contains("Casual"));
    assert!(h.overlay.last().recording.is_some());
    h.send(Msg::Stop);
    h.providers.gate.add_permits(10);
    h.wait_for(|s| s.state == Idle).await;
    let entries = h.history.entries();
    assert_eq!(
        entries.iter().map(|e| e.cleanup_style_id.as_deref()).collect::<Vec<_>>(),
        [Some("clean"), Some("casual")]
    );
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn media_releases_on_every_recording_end_before_transcription_finishes() {
    use opit_core::config::MediaMode;
    for mode in [MediaMode::Pause, MediaMode::Duck] {
        for end in ["stop", "escape", "error", "limit", "shutdown"] {
            let mut config = AppConfig::default();
            config.media.mode = mode;
            config.media.duck_volume_percent = 35;
            let mut h = harness(config);
            h.script("groq", vec![Ok("sample")]);
            h.providers.gate.forget_permits(1_000);
            h.send(Msg::Toggle);
            h.expect(&[Recording]).await;
            assert_eq!(*h.media.calls.lock().unwrap(), [Some((mode, 35))]);
            match end {
                "stop" => h.send(Msg::Stop),
                "escape" => h.hotkey(HotkeyEvent::Escape),
                "error" => h.mic.emit(CaptureEvent::Failed("unplugged".into())),
                "limit" => {
                    tokio::time::pause();
                    tokio::time::advance(Duration::from_secs(181)).await;
                }
                "shutdown" => h.send(Msg::Shutdown),
                _ => unreachable!(),
            }
            h.settle().await;
            assert_eq!(*h.media.calls.lock().unwrap(), [Some((mode, 35)), None], "{end}");
            if end == "stop" || end == "limit" {
                assert!(h.paster.texts().is_empty(), "media must release while provider is blocked");
            }
            h.send(Msg::Shutdown);
            h.settle().await;
            assert_eq!(h.media.calls.lock().unwrap().len(), 2, "double release is harmless");
            if end == "limit" {
                tokio::time::resume();
            }
        }
    }
}

#[tokio::test]
async fn media_is_disabled_or_not_engaged_if_microphone_cannot_start() {
    for disabled in [true, false] {
        let mut config = AppConfig::default();
        config.media.enabled = !disabled;
        let h = harness(config);
        if !disabled {
            *h.mic.fail_start.lock().unwrap() = Some(MicError::NoDevice);
        }
        h.send(Msg::Toggle);
        h.settle().await;
        h.send(Msg::Shutdown);
        h.settle().await;
        assert!(h.media.calls.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn media_stays_engaged_during_user_pause_and_uses_start_settings() {
    use opit_core::config::MediaMode;
    let config = AppConfig::default();
    let h = harness(config.clone());
    h.handle.control(ControlAction::Start).await.unwrap();
    let mut changed = config;
    changed.media.enabled = false;
    h.settings.replace(settings(changed));
    h.handle.control(ControlAction::Pause).await.unwrap();
    h.handle.control(ControlAction::Pause).await.unwrap();
    h.handle.control(ControlAction::Resume).await.unwrap();
    assert_eq!(*h.media.calls.lock().unwrap(), [Some((MediaMode::Duck, 50))]);
    h.handle.control(ControlAction::Cancel).await.unwrap();
    assert_eq!(*h.media.calls.lock().unwrap(), [Some((MediaMode::Duck, 50)), None]);
}

#[tokio::test]
async fn media_engages_for_microphone_ask_but_not_text_ask() {
    let mut h = harness(AppConfig::default());
    h.send(Msg::Ask { text: None, reply: None });
    h.expect(&[Recording]).await;
    assert_eq!(h.media.calls.lock().unwrap().len(), 1);
    h.handle.control(ControlAction::Cancel).await.unwrap();
    assert_eq!(h.media.calls.lock().unwrap().len(), 2);
    h.send(Msg::Ask { text: Some("request".into()), reply: None });
    h.settle().await;
    assert_eq!(h.media.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn media_releases_when_microphone_ask_fails() {
    let mut h = harness(AppConfig::default());
    h.send(Msg::Ask { text: None, reply: None });
    h.expect(&[Recording]).await;
    h.mic.emit(CaptureEvent::Failed("unplugged".into()));
    h.expect_error(ErrorKind::Microphone).await;
    assert_eq!(h.media.calls.lock().unwrap().len(), 2);
    assert_eq!(h.mic.dropped.load(SeqCst), 1);
}

#[tokio::test]
async fn paste_method_and_after_are_snapshotted_at_start_and_type_leaves_clipboard_alone() {
    for method in [PasteMethod::CtrlV, PasteMethod::ShiftInsert, PasteMethod::CtrlShiftV, PasteMethod::Type] {
        let mut config = AppConfig::default();
        config.paste.method = method;
        config.paste.after = PasteAfter::CtrlEnter;
        config.paste.restore_clipboard = true;
        let mut h = harness(config.clone());
        h.script("groq", vec![Ok("sample")]);
        h.hotkey(HotkeyEvent::ComboDown);
        h.expect(&[Recording]).await;
        config.paste = PasteConfig::default();
        h.settings.replace(settings(config));
        h.hotkey(HotkeyEvent::ComboDown);
        h.expect(&[Transcribing, Pasting, Idle]).await;
        assert_eq!(
            *h.paster.requests.lock().unwrap(),
            [PasteRequest { method, after: PasteAfter::CtrlEnter, restore: true, auto_paste: true }]
        );
        assert_eq!(*h.paster.after_keys.lock().unwrap(), [PasteAfter::CtrlEnter]);
        if method == PasteMethod::Type {
            assert_eq!(*h.paster.typed.lock().unwrap(), ["sample "]);
            assert!(h.paster.clipboard_writes.lock().unwrap().is_empty());
        } else {
            assert!(h.paster.typed.lock().unwrap().is_empty());
            assert_eq!(*h.paster.clipboard_writes.lock().unwrap(), ["sample "]);
        }
        h.send(Msg::Shutdown);
    }
}

#[tokio::test]
async fn after_keys_only_follow_successful_inserts_for_every_method() {
    for method in [PasteMethod::CtrlV, PasteMethod::ShiftInsert, PasteMethod::CtrlShiftV, PasteMethod::Type] {
        for after in [PasteAfter::None, PasteAfter::Enter, PasteAfter::CtrlEnter] {
            for outcome in
                [PasteOutcome::Pasted, PasteOutcome::ClipboardOnly, PasteOutcome::Failed("input failed".into())]
            {
                let mut config = AppConfig::default();
                config.paste.method = method;
                config.paste.after = after;
                let mut h = harness(config);
                *h.paster.outcome.lock().unwrap() = outcome.clone();
                h.script("groq", vec![Ok("sample")]);
                h.hotkey(HotkeyEvent::ComboDown);
                h.expect(&[Recording]).await;
                h.hotkey(HotkeyEvent::ComboDown);
                h.expect(&[Transcribing, Pasting]).await;
                if matches!(outcome, PasteOutcome::Failed(_)) {
                    h.expect_error(ErrorKind::Paste).await;
                } else {
                    h.expect(&[Idle]).await;
                }
                let expected =
                    if outcome == PasteOutcome::Pasted && after != PasteAfter::None { vec![after] } else { vec![] };
                assert_eq!(*h.paster.after_keys.lock().unwrap(), expected);
                if method == PasteMethod::Type && outcome == PasteOutcome::ClipboardOnly {
                    assert!(h.paster.typed.lock().unwrap().is_empty());
                    assert_eq!(*h.paster.clipboard_writes.lock().unwrap(), ["sample "]);
                }
                h.send(Msg::Shutdown);
            }
        }
    }
}

#[tokio::test]
async fn type_with_auto_paste_disabled_copies_without_typing_or_after_key() {
    let mut config = AppConfig::default();
    config.paste.method = PasteMethod::Type;
    config.paste.after = PasteAfter::Enter;
    config.paste.trailing_space = false;
    let mut h = harness(config);
    h.script("groq", vec![Ok("sample")]);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.send(Msg::Stop);
    h.expect(&[Transcribing, Pasting, Idle]).await;
    assert!(h.paster.typed.lock().unwrap().is_empty());
    assert!(h.paster.after_keys.lock().unwrap().is_empty());
    assert_eq!(*h.paster.clipboard_writes.lock().unwrap(), ["sample"]);
}

#[tokio::test]
async fn queued_jobs_keep_their_own_paste_settings_and_order() {
    let mut config = AppConfig::default();
    config.paste.method = PasteMethod::ShiftInsert;
    config.paste.after = PasteAfter::Enter;
    let mut h = harness(config.clone());
    h.script("groq", vec![Ok("first"), Ok("second")]);
    h.providers.gate.forget_permits(1_000);
    h.hotkey(HotkeyEvent::ComboDown);
    h.hotkey(HotkeyEvent::ComboDown);
    h.expect(&[Recording, Transcribing]).await;
    config.paste.method = PasteMethod::Type;
    config.paste.after = PasteAfter::CtrlEnter;
    h.settings.replace(settings(config.clone()));
    h.hotkey(HotkeyEvent::ComboDown);
    h.expect(&[Recording]).await;
    h.hotkey(HotkeyEvent::ComboDown);
    wait_for(&mut h, Transcribing, 1).await;
    config.paste = PasteConfig::default();
    h.settings.replace(settings(config));
    h.providers.gate.add_permits(1_000);
    wait_for(&mut h, Idle, 0).await;
    assert_eq!(h.paster.texts(), ["first ", "second "]);
    assert_eq!(
        *h.paster.requests.lock().unwrap(),
        [
            PasteRequest {
                method: PasteMethod::ShiftInsert,
                after: PasteAfter::Enter,
                restore: false,
                auto_paste: true
            },
            PasteRequest { method: PasteMethod::Type, after: PasteAfter::CtrlEnter, restore: false, auto_paste: true },
        ]
    );
    assert_eq!(*h.paster.clipboard_writes.lock().unwrap(), ["first "]);
    assert_eq!(*h.paster.typed.lock().unwrap(), ["second "]);
    assert_eq!(*h.paster.after_keys.lock().unwrap(), [PasteAfter::Enter, PasteAfter::CtrlEnter]);
}

#[tokio::test]
async fn cli_controls_are_atomic_and_pause_resume_are_idempotent() {
    let h = harness(AppConfig::default());
    assert!(h.handle.control(ControlAction::Stop).await.is_err());
    assert!(h.handle.control(ControlAction::Retry).await.is_err());
    assert!(h.handle.control(ControlAction::Pause).await.is_err());
    assert_eq!(h.handle.control(ControlAction::Start).await.unwrap().state, Recording);
    assert!(h.handle.control(ControlAction::Start).await.is_err());
    for _ in 0..2 {
        assert!(h.handle.control(ControlAction::Pause).await.unwrap().paused);
    }
    for _ in 0..2 {
        assert!(!h.handle.control(ControlAction::Resume).await.unwrap().paused);
    }
    assert_eq!(h.handle.control(ControlAction::Cancel).await.unwrap().state, Idle);
}

#[tokio::test]
async fn cli_timed_record_returns_its_own_text_without_touching_clipboard() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("ordinary"), Ok("CLI sample")]);
    h.providers.gate.forget_permits(1_000);
    h.send(Msg::Toggle);
    h.send(Msg::Stop);
    h.expect(&[Recording, Transcribing]).await;
    let handle = h.handle.clone();
    let result = tokio::spawn(async move { handle.record(1).await });
    h.expect(&[Recording]).await;
    assert!(h.handle.record(1).await.is_err(), "cannot replace an existing clip");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    h.providers.gate.add_permits(1_000);
    assert_eq!(result.await.unwrap().unwrap(), "CLI sample");
    assert_eq!(h.media.calls.lock().unwrap().len(), 4, "each microphone recording engages and releases media");
    assert_eq!(h.paster.texts(), ["ordinary "]);
    assert_eq!(h.history.entries().len(), 2);
}

#[tokio::test]
async fn cli_record_always_replies_on_cancel_and_provider_failure() {
    let mut h = harness(AppConfig::default());
    let handle = h.handle.clone();
    let result = tokio::spawn(async move { handle.record(5).await });
    h.expect(&[Recording]).await;
    h.handle.control(ControlAction::Cancel).await.unwrap();
    assert_eq!(result.await.unwrap(), Err(ErrorKind::Cancelled));
    h.script("groq", vec![Err(ProviderError::Unauthorized(401))]);
    let handle = h.handle.clone();
    let result = tokio::spawn(async move { handle.record(5).await });
    h.expect(&[Cancelled, Idle, Recording]).await;
    h.handle.control(ControlAction::Stop).await.unwrap();
    assert_eq!(result.await.unwrap(), Err(ErrorKind::InvalidKey));
    assert!(h.paster.texts().is_empty());
}

#[tokio::test]
async fn cli_completion_preserves_a_new_recordings_overlay() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("CLI sample")]);
    h.providers.gate.forget_permits(1_000);
    let handle = h.handle.clone();
    let result = tokio::spawn(async move { handle.record(5).await });
    h.expect(&[Recording]).await;
    h.handle.control(ControlAction::Stop).await.unwrap();
    h.expect(&[Transcribing]).await;
    h.handle.control(ControlAction::Start).await.unwrap();
    h.expect(&[Recording]).await;
    h.providers.gate.add_permits(1_000);
    assert_eq!(result.await.unwrap().unwrap(), "CLI sample");
    assert_eq!(h.handle.status().state, Recording);
    assert!(h.overlay.last().recording.is_some(), "the new recording must remain visible");
    h.handle.control(ControlAction::Cancel).await.unwrap();
    assert!(h.paster.texts().is_empty());
}

struct ProviderState {
    live: Arc<FakeStreaming>,
    scripts: Mutex<HashMap<String, VecDeque<Result<String, ProviderError>>>>,
    missing_keys: Mutex<HashSet<String>>,
    gate: Arc<Semaphore>,
    profile_gates: Mutex<HashMap<String, Arc<Semaphore>>>,
    calls: AtomicUsize,
    warmups: AtomicUsize,
    cleanup: Mutex<Option<Arc<CleanupClient>>>,
    cleanup_configs: Mutex<Vec<CleanupConfig>>,
    ask: Arc<crate::ask::AskService>,
}

impl ProviderState {
    fn new() -> Self {
        Self {
            live: Arc::default(),
            scripts: Mutex::default(),
            missing_keys: Mutex::default(),
            gate: Arc::new(Semaphore::new(1_000)),
            profile_gates: Mutex::default(),
            calls: AtomicUsize::new(0),
            warmups: AtomicUsize::new(0),
            cleanup: Mutex::default(),
            cleanup_configs: Mutex::default(),
            ask: Arc::new(crate::ask::AskService::new(Arc::new(FakeSecrets::default()))),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(SeqCst)
    }
}

struct TestProviders(Arc<ProviderState>);

struct ScriptedClient {
    profile: Profile,
    state: Arc<ProviderState>,
}

impl Transcriber for ScriptedClient {
    fn profile(&self) -> &Profile {
        &self.profile
    }

    fn transcribe(
        &self,
        _request: &TranscribeRequest<'_>,
    ) -> impl Future<Output = Result<RawTranscript, ProviderError>> + Send {
        let state = self.state.clone();
        let id = self.profile.id.clone();
        async move {
            let _permit = state.gate.acquire().await.unwrap();
            state.calls.fetch_add(1, SeqCst);
            let gate = state.profile_gates.lock().unwrap().get(&id).cloned();
            let _profile_permit = match &gate {
                Some(gate) => Some(gate.acquire().await.unwrap()),
                None => None,
            };
            let next = state.scripts.lock().unwrap().get_mut(&id).and_then(VecDeque::pop_front);
            next.unwrap_or_else(|| panic!("unexpected request to {id}")).map(|text| RawTranscript {
                text,
                language: None,
                dropped_segments: 0,
                segments: Vec::new(),
            })
        }
    }
}

impl Providers for TestProviders {
    type Client = ScriptedClient;
    async fn connect_live(
        &self,
        _profile: Profile,
        _terms: Vec<String>,
    ) -> Result<Box<dyn opit_core::provider::streaming::LiveSession>, ProviderError> {
        self.0.live.connections.fetch_add(1, SeqCst);
        let _permit = self.0.live.connect_gate.acquire().await.unwrap();
        if let Some(error) = self.0.live.connect_error.lock().unwrap().clone() {
            return Err(error);
        }
        Ok(Box::new(FakeLiveSession(self.0.live.clone())))
    }
    fn ask_service(&self) -> Option<Arc<crate::ask::AskService>> {
        Some(self.0.ask.clone())
    }

    fn client(&self, profile: &Profile) -> Result<ScriptedClient, SetupError> {
        if self.0.missing_keys.lock().unwrap().contains(&profile.id) {
            return Err(SetupError::MissingKey(profile.name.clone()));
        }
        Ok(ScriptedClient { profile: profile.clone(), state: self.0.clone() })
    }

    fn cleanup_client(&self, config: &CleanupConfig) -> Result<Arc<CleanupClient>, CleanupError> {
        self.0.cleanup_configs.lock().unwrap().push(config.clone());
        if self.0.cleanup.lock().unwrap().is_some() {
            CleanupClient::new(config.clone(), None).map(Arc::new)
        } else {
            Err(CleanupError::MissingKey)
        }
    }

    fn warm_up(&self, _client: &ScriptedClient) {
        self.0.warmups.fetch_add(1, SeqCst);
    }
}

// ---------- history + events doubles ----------

#[derive(Debug, Clone, PartialEq)]
struct Saved {
    source_text: Option<String>,
    instruction: Option<String>,
    mode: String,
    agent: Option<String>,
    profile_id: String,
    language: Option<String>,
    cleanup_status: Option<cleanup::CleanupStatus>,
    cleanup_model: Option<String>,
    cleanup_style_id: Option<String>,
    error_code: Option<String>,
    raw_text: String,
    text: String,
    status: TranscriptStatus,
    has_audio: bool,
}

#[derive(Default)]
struct FakeHistory {
    saved: Mutex<Vec<Saved>>,
}

impl FakeHistory {
    fn entries(&self) -> Vec<Saved> {
        self.saved.lock().unwrap().clone()
    }
}

impl HistorySink for FakeHistory {
    fn record(
        &self,
        entry: &NewDictation<'_>,
        audio: Option<&PreparedAudio>,
        _max_entries: u32,
    ) -> Result<i64, String> {
        let mut saved = self.saved.lock().unwrap();
        saved.push(Saved {
            source_text: entry.source_text.map(str::to_owned),
            instruction: entry.instruction.map(str::to_owned),
            mode: entry.mode.into(),
            agent: entry.agent.map(str::to_owned),
            profile_id: entry.profile_id.into(),
            language: entry.language.map(str::to_string),
            cleanup_status: entry.cleanup_status,
            cleanup_model: entry.cleanup_model.map(str::to_string),
            cleanup_style_id: entry.cleanup_style_id.map(str::to_string),
            error_code: entry.error_code.map(str::to_string),
            raw_text: entry.raw_text.into(),
            text: entry.text.into(),
            status: entry.status,
            has_audio: audio.is_some(),
        });
        Ok(saved.len() as i64)
    }

    fn replace(&self, id: i64, entry: &NewDictation<'_>, keep_audio: bool) -> Result<(), String> {
        let mut saved = self.saved.lock().unwrap();
        let row = saved.get_mut((id - 1) as usize).ok_or("missing history row")?;
        row.profile_id = entry.profile_id.into();
        row.language = entry.language.map(str::to_string);
        row.cleanup_status = entry.cleanup_status;
        row.cleanup_model = entry.cleanup_model.map(str::to_string);
        row.cleanup_style_id = entry.cleanup_style_id.map(str::to_string);
        row.error_code = entry.error_code.map(str::to_string);
        row.raw_text = entry.raw_text.into();
        row.text = entry.text.into();
        row.status = entry.status;
        row.has_audio = keep_audio;
        Ok(())
    }
}

struct StyleSaveGate {
    released: Mutex<bool>,
    wake: Condvar,
    entered: Semaphore,
}

/// Always release a blocked worker, including when an assertion fails.
struct BlockedStyleSave(Arc<StyleSaveGate>);
impl Drop for BlockedStyleSave {
    fn drop(&mut self) {
        *self.0.released.lock().unwrap() = true;
        self.0.wake.notify_all();
    }
}

struct FakeEvents {
    style_gate: Mutex<Option<Arc<StyleSaveGate>>>,
    style_failures: Mutex<HashSet<String>>,
    settings: SettingsHandle,
    style_saves: Mutex<Vec<String>>,
    tx: mpsc::UnboundedSender<DictationStatus>,
    history: Mutex<Vec<i64>>,
    settings_opened: AtomicUsize,
}

impl UiEvents for FakeEvents {
    fn save_style(&self, id: &str) -> Option<CleanupStyle> {
        let gate = self.style_gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.entered.add_permits(1);
            let mut released = gate.released.lock().unwrap();
            while !*released {
                released = gate.wake.wait(released).unwrap();
            }
        }
        if self.style_failures.lock().unwrap().contains(id) {
            return None;
        }
        let mut config = self.settings.current().config.clone();
        let style = config.cleanup.style(id)?.clone();
        config.cleanup.active_style_id = id.into();
        self.settings.replace(settings(config));
        self.style_saves.lock().unwrap().push(id.into());
        Some(style)
    }
    fn status_changed(&self, status: &DictationStatus) {
        let _ = self.tx.send(status.clone());
    }

    fn history_added(&self, id: i64) {
        self.history.lock().unwrap().push(id);
    }

    fn open_settings(&self) {
        self.settings_opened.fetch_add(1, SeqCst);
    }
}

// ---------- harness ----------

struct Harness {
    foreground: Arc<FakeForegroundApp>,
    handle: ControllerHandle,
    states: mpsc::UnboundedReceiver<DictationStatus>,
    settings: SettingsHandle,
    providers: Arc<ProviderState>,
    mic: Arc<FakeMic>,
    media: Arc<FakeMedia>,
    hotkey: Arc<FakeHotkey>,
    paster: Arc<FakePaster>,
    overlay: Arc<FakeOverlay>,
    sounds: Arc<FakeSounds>,
    history: Arc<FakeHistory>,
    events: Arc<FakeEvents>,
}

fn settings(config: AppConfig) -> Settings {
    Settings::build(config, None, Some("en-US")).0
}

#[tokio::test]
async fn cleanup_failure_pastes_rules_applied_text_and_warns() {
    let mut config = AppConfig::default();
    config.cleanup.enabled = true;
    let mut h = harness(config.clone());
    let pack = opit_core::rules::RulePack::from_yaml("schema: 1\nid: user\nname: Personal\nterms: [Opit]\n").unwrap();
    h.settings.replace(Settings::build(config, Some(pack), Some("en-US")).0);
    h.script("groq", vec![Ok("opit bugün açılıyor")]);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.send(Msg::Toggle);
    h.expect(&[Transcribing, DictationState::Cleaning, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["Opit bugün açılıyor "]);
    let saved = h.history.entries();
    assert_eq!(saved[0].cleanup_status, Some(cleanup::CleanupStatus::Failed));
    assert_eq!(saved[0].cleanup_model.as_deref(), Some("deepseek-ai/DeepSeek-V4.1-Flash-Fast"));
    assert_eq!(saved[0].raw_text, "opit bugün açılıyor");
    assert_eq!(h.overlay.last().tone, Tone::Warning);
    assert!(h.overlay.last().text.contains("Cleanup failed"));
    assert!(!h.overlay.last().text.contains("opit bugün"));
}

#[tokio::test]
async fn cleanup_success_is_saved_and_escape_during_cleanup_pastes_rules_applied_input() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    for cancel in [false, true] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(if cancel { 500 } else { 10 }))
                    .set_body_json(serde_json::json!({"choices":[{"message":{"content":"Today we have a meeting."}}]})),
            )
            .mount(&server)
            .await;
        let mut config = AppConfig::default();
        config.cleanup.enabled = true;
        config.cleanup.base_url = server.uri();
        config.cleanup.api_key_ref = None;
        let client = Arc::new(CleanupClient::new(config.cleanup.clone(), None).unwrap());
        let mut h = harness(config.clone());
        let pack =
            opit_core::rules::RulePack::from_yaml("schema: 1\nid: user\nname: Personal\nterms: [Today]\n").unwrap();
        h.settings.replace(Settings::build(config, Some(pack), Some("en-US")).0);
        *h.providers.cleanup.lock().unwrap() = Some(client);
        h.script("groq", vec![Ok("um today today we have a meeting")]);
        h.hotkey(HotkeyEvent::ComboDown);
        h.expect(&[Recording]).await;
        h.hotkey(HotkeyEvent::ComboDown);
        h.expect(&[Transcribing, DictationState::Cleaning]).await;
        assert!(h.overlay.last().text.contains("Esc to skip"));
        if cancel {
            h.hotkey(HotkeyEvent::Escape);
            h.expect(&[Transcribing, Pasting, Idle]).await;
            assert_eq!(h.paster.texts(), ["um Today Today we have a meeting "]);
            assert_eq!(h.history.entries()[0].cleanup_status, Some(cleanup::CleanupStatus::Skipped));
            assert_eq!(*h.paster.policies.lock().unwrap(), [true]);
            assert!(h.overlay.last().tone != Tone::Warning);
            // A later Escape still cancels an ordinary recording.
            h.send(Msg::Toggle);
            h.expect(&[Recording]).await;
            h.hotkey(HotkeyEvent::Escape);
            h.expect(&[Cancelled, Idle]).await;
        } else {
            h.expect(&[Pasting, Idle]).await;
            assert_eq!(h.paster.texts(), ["Today we have a meeting. "]);
            assert_eq!(h.history.entries()[0].cleanup_status, Some(cleanup::CleanupStatus::Ok));
            assert_eq!(h.history.entries()[0].raw_text, "um today today we have a meeting");
        }
        h.send(Msg::Shutdown);
    }
}

#[tokio::test]
async fn escape_after_cleanup_completion_overrides_success_and_failure_before_delivery() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    for failed in [false, true] {
        for retry in [false, true] {
            let server = MockServer::start().await;
            let response = if failed {
                ResponseTemplate::new(500)
            } else {
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "choices": [{"message": {"content": "Today we have a meeting."}, "finish_reason": "stop"}]
                }))
            };
            Mock::given(method("POST")).respond_with(response).expect(1).mount(&server).await;
            let mut config = AppConfig::default();
            config.cleanup.enabled = true;
            config.cleanup.base_url = server.uri();
            config.cleanup.api_key_ref = None;
            let client = Arc::new(CleanupClient::new(config.cleanup.clone(), None).unwrap());
            let gate = Arc::new(TranscriptionDeliveryGate { completed: Semaphore::new(0), deliver: Semaphore::new(0) });
            let mut h = harness_with_delivery_gate(config.clone(), Some(gate.clone()));
            let pack =
                opit_core::rules::RulePack::from_yaml("schema: 1\nid: user\nname: Personal\nterms: [Today]\n").unwrap();
            h.settings.replace(Settings::build(config, Some(pack), Some("en-US")).0);
            *h.providers.cleanup.lock().unwrap() = Some(client);
            if retry {
                // Seed a failed row using the production failure-persistence path.
                h.script("groq", vec![Err(ProviderError::Unauthorized(401))]);
                h.send(Msg::Toggle);
                h.expect(&[Recording]).await;
                h.send(Msg::Toggle);
                h.expect(&[Transcribing]).await;
                gate.wait_until_completed().await;
                gate.deliver.add_permits(1);
                h.expect_error(ErrorKind::InvalidKey).await;
                assert_eq!(h.history.entries().len(), 1);
            }
            h.script("groq", vec![Ok("um today today we have a meeting")]);
            let reply = if retry {
                let (reply, received) = oneshot::channel();
                let audio = Arc::new(PreparedAudio { samples: vec![0.1; 16_000], audio_ms: 1000, speech_ms: 1000 });
                h.send(Msg::RetryHistory { id: 1, audio, reply });
                Some(received)
            } else {
                h.hotkey(HotkeyEvent::ComboDown);
                h.expect(&[Recording]).await;
                h.hotkey(HotkeyEvent::ComboDown);
                None
            };
            h.expect(&[Transcribing, DictationState::Cleaning]).await;
            gate.wait_until_completed().await;
            // The result is complete and held outside the controller inbox.
            // Escape must win even though the worker cannot observe it anymore.
            h.hotkey(HotkeyEvent::Escape);
            h.expect(&[Transcribing]).await;
            gate.deliver.add_permits(1);
            h.expect(&[Pasting, Idle]).await;
            if let Some(reply) = reply {
                assert!(reply.await.unwrap().is_ok());
            }
            assert_eq!(h.paster.texts(), ["um Today Today we have a meeting "]);
            let entries = h.history.entries();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].text, "um Today Today we have a meeting");
            assert_eq!(entries[0].raw_text, "um today today we have a meeting");
            assert_eq!(entries[0].cleanup_status, Some(cleanup::CleanupStatus::Skipped));
            if retry {
                assert_eq!(h.overlay.last().text, Lang::En.clipboard_only());
            } else {
                assert_ne!(h.overlay.last().tone, Tone::Warning);
            }
            h.send(Msg::Shutdown);
        }
    }
}

#[tokio::test]
async fn second_escape_after_skipping_cleanup_cancels_the_pending_dictation() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(10)))
        .mount(&server)
        .await;
    let mut config = AppConfig::default();
    config.cleanup.enabled = true;
    config.cleanup.base_url = server.uri();
    config.cleanup.api_key_ref = None;
    let client = Arc::new(CleanupClient::new(config.cleanup.clone(), None).unwrap());
    let mut h = harness(config);
    *h.providers.cleanup.lock().unwrap() = Some(client);
    h.script("groq", vec![Ok("three input words")]);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.send(Msg::Toggle);
    h.expect(&[Transcribing, DictationState::Cleaning]).await;
    // Both messages are queued before the worker can publish the skipped result.
    h.hotkey(HotkeyEvent::Escape);
    h.hotkey(HotkeyEvent::Escape);
    h.expect(&[Transcribing, Cancelled, Idle]).await;
    h.settle().await;
    assert!(h.paster.texts().is_empty());
    assert!(h.history.entries().is_empty());
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn escape_during_voice_modes_or_ask_cleanup_still_cancels_fully() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    for event in [HotkeyEvent::Edit, HotkeyEvent::Translate, HotkeyEvent::Ask] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(10)))
            .mount(&server)
            .await;
        let mut config = AppConfig::default();
        config.cleanup.enabled = true;
        config.ask.cleanup_request = true;
        config.cleanup.base_url = server.uri();
        config.cleanup.api_key_ref = None;
        let client = Arc::new(CleanupClient::new(config.cleanup.clone(), None).unwrap());
        let mut h = harness(config);
        *h.providers.cleanup.lock().unwrap() = Some(client);
        h.script("groq", vec![Ok("three input words")]);
        h.hotkey(event.clone());
        h.wait_for(|s| s.state == Recording).await;
        h.hotkey(event);
        h.expect(&[Transcribing, DictationState::Cleaning]).await;
        assert!(!h.overlay.last().text.contains("Esc to skip"));
        h.hotkey(HotkeyEvent::Escape);
        h.expect(&[Cancelled, Idle]).await;
        h.settle().await;
        assert!(h.paster.texts().is_empty());
        assert!(h.history.entries().is_empty());
        h.send(Msg::Shutdown);
    }
}

#[tokio::test]
async fn disabled_cleanup_is_skipped_on_retry_and_by_profile() {
    for global in [false, true] {
        let mut config = AppConfig::default();
        config.cleanup.enabled = global;
        config.profiles[0].apply_cleanup = !global;
        let mut h = harness(config);
        h.script("groq", vec![Err(ProviderError::Unauthorized(401)), Ok("three clear words")]);
        h.send(Msg::Toggle);
        h.expect(&[Recording]).await;
        h.send(Msg::Toggle);
        h.expect(&[Transcribing]).await;
        h.expect_error(ErrorKind::InvalidKey).await;
        h.send(Msg::Retry);
        h.expect(&[Transcribing, Pasting, Idle]).await;
        assert_eq!(h.history.entries().last().unwrap().cleanup_status, Some(cleanup::CleanupStatus::Skipped));
        assert_eq!(h.paster.texts(), ["three clear words "]);
    }
}

fn harness(config: AppConfig) -> Harness {
    harness_with_delivery_gate(config, None)
}

struct TranscriptionDeliveryGate {
    completed: Semaphore,
    deliver: Semaphore,
}

impl TranscriptionDeliveryGate {
    async fn wait_until_completed(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.completed.acquire())
            .await
            .expect("transcription did not complete")
            .unwrap()
            .forget();
    }
}

fn harness_with_delivery_gate(config: AppConfig, gate: Option<Arc<TranscriptionDeliveryGate>>) -> Harness {
    let (tx, states) = mpsc::unbounded_channel();
    let providers = Arc::new(ProviderState::new());
    let settings = SettingsHandle::new(settings(config));
    let events = Arc::new(FakeEvents {
        style_gate: Mutex::default(),
        style_failures: Mutex::default(),
        settings: settings.clone(),
        style_saves: Mutex::default(),
        tx,
        history: Mutex::default(),
        settings_opened: AtomicUsize::new(0),
    });
    let (mic, hotkey, paster) = (Arc::<FakeMic>::default(), Arc::<FakeHotkey>::default(), Arc::<FakePaster>::default());
    let (overlay, sounds, history) =
        (Arc::<FakeOverlay>::default(), Arc::<FakeSounds>::default(), Arc::<FakeHistory>::default());
    let (handle, inbox) = channel();
    let inbox = if let Some(gate) = gate {
        let (forward, forwarded) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut inbox = inbox.0;
            while let Some(msg) = inbox.recv().await {
                if matches!(msg, Msg::Transcribed { .. }) {
                    let gate = gate.clone();
                    let forward = forward.clone();
                    tokio::spawn(async move {
                        gate.completed.add_permits(1);
                        gate.deliver.acquire().await.unwrap().forget();
                        let _ = forward.send(msg);
                    });
                } else if forward.send(msg).is_err() {
                    break;
                }
            }
        });
        Inbox(forwarded)
    } else {
        inbox
    };
    let media = Arc::new(FakeMedia::default());
    let foreground = Arc::new(FakeForegroundApp::default());
    tokio::spawn(run(
        &handle,
        inbox,
        Env {
            foreground: foreground.clone(),
            settings: settings.clone(),
            providers: Arc::new(TestProviders(providers.clone())),
            mic: mic.clone(),
            media: media.clone(),
            hotkey: hotkey.clone(),
            paster: paster.clone(),
            overlay: overlay.clone(),
            sounds: sounds.clone(),
            history: history.clone(),
            events: events.clone(),
            retry_delay: Duration::ZERO,
        },
    ));
    Harness {
        foreground,
        handle,
        states,
        settings,
        providers,
        mic,
        media,
        hotkey,
        paster,
        overlay,
        sounds,
        history,
        events,
    }
}

impl Harness {
    fn block_style_saves(&self) -> BlockedStyleSave {
        let gate =
            Arc::new(StyleSaveGate { released: Mutex::new(false), wake: Condvar::new(), entered: Semaphore::new(0) });
        *self.events.style_gate.lock().unwrap() = Some(gate.clone());
        BlockedStyleSave(gate)
    }

    async fn wait_for_style_saves(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while self.events.style_saves.lock().unwrap().len() < count {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("style persistence did not complete");
    }
    async fn wait_for(&mut self, predicate: impl Fn(&DictationStatus) -> bool) -> DictationStatus {
        loop {
            let status = tokio::time::timeout(Duration::from_secs(5), self.states.recv())
                .await
                .expect("status timed out")
                .expect("controller stopped");
            if predicate(&status) {
                return status;
            }
        }
    }
    fn script(&self, profile: &str, replies: Vec<Result<&str, ProviderError>>) {
        let replies = replies.into_iter().map(|r| r.map(str::to_string)).collect();
        self.providers.scripts.lock().unwrap().insert(profile.into(), replies);
    }

    fn send(&self, msg: Msg) {
        self.handle.send(msg);
    }

    fn hotkey(&self, event: HotkeyEvent) {
        self.send(Msg::Hotkey(event));
    }

    async fn next(&mut self) -> DictationState {
        let mut was_waiting = false;
        loop {
            let status = tokio::time::timeout(Duration::from_secs(5), self.states.recv())
                .await
                .expect("timed out waiting for a state change")
                .expect("controller stopped");
            if status.state == Transcribing && status.retry_wait.is_some() {
                was_waiting = true;
                continue;
            }
            if status.state == Transcribing && was_waiting {
                was_waiting = false;
                continue;
            }
            return status.state;
        }
    }

    async fn expect(&mut self, expected: &[DictationState]) {
        for want in expected {
            assert_eq!(&self.next().await, want);
        }
    }

    async fn expect_error(&mut self, kind: ErrorKind) {
        match self.next().await {
            DictationState::Error { kind: got, message } => {
                assert_eq!(got, kind);
                assert!(!message.is_empty());
            }
            other => panic!("expected an error, got {other:?}"),
        }
        self.expect(&[Idle]).await;
    }

    /// Lets the actor drain its queue (for "nothing happened" assertions).
    async fn settle(&self) {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }
}

fn with_fallback() -> AppConfig {
    let mut config = AppConfig::default();
    config.profiles[0].fallback_profile_id = Some("openai".into());
    config
}

// ---------- tests ----------

#[tokio::test(start_paused = true)]
async fn even_the_quick_retry_wait_publishes_safe_progress_and_wakes_on_cancel() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (cancel, cancelled) = watch::channel(false);
    let progress = RetryWait { kind: "timeout", seconds: 0, attempt: 1, total: 5 };
    let waiting = tokio::spawn(wait_for_retry(tx, 1, cancelled, progress, Duration::from_millis(500)));
    let Some(Msg::RetryProgress { progress: Some(progress), .. }) = rx.recv().await else { panic!("missing progress") };
    assert_eq!(progress.seconds, 1);
    let now = tokio::time::Instant::now();
    cancel.send(true).unwrap();
    assert!(!waiting.await.unwrap());
    assert_eq!(tokio::time::Instant::now(), now);
}

#[tokio::test(start_paused = true)]
async fn cancelling_a_patient_wait_keeps_audio_and_advances_the_fifo() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Err(ProviderError::RateLimited), Err(ProviderError::RateLimited), Ok("second")]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    let status = h.wait_for(|s| s.retry_wait.is_some()).await;
    assert_eq!(status.retry_wait.unwrap(), RetryWait { kind: "rate_limited", seconds: 10, attempt: 2, total: 5 });
    assert!(h.overlay.last().text.contains("retrying in 10 s (2/5)"));
    h.send(Msg::Toggle);
    h.wait_for(|s| s.state == Recording).await;
    h.send(Msg::Stop);
    h.wait_for(|s| s.waiting == 1).await;
    assert_eq!(h.providers.calls(), 2);
    h.hotkey(HotkeyEvent::Escape);
    h.wait_for(|s| matches!(s.state, DictationState::Error { kind: ErrorKind::Cancelled, .. })).await;
    h.wait_for(|s| s.state == Idle).await;
    let saved = h.history.entries();
    assert_eq!(saved.iter().map(|s| s.status).collect::<Vec<_>>(), [TranscriptStatus::Failed, TranscriptStatus::Ok]);
    assert!(saved[0].has_audio);
    assert!(saved[0].text.is_empty() && saved[0].raw_text.is_empty());
    assert_eq!(h.paster.texts(), ["second "]);
    assert_eq!(h.providers.calls(), 3);
}

#[tokio::test(start_paused = true)]
async fn a_patient_wait_keeps_queued_pastes_in_order() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Err(ProviderError::Timeout), Err(ProviderError::Timeout), Ok("first"), Ok("second")]);
    h.hotkey(HotkeyEvent::ComboDown);
    h.hotkey(HotkeyEvent::ComboDown);
    h.wait_for(|s| s.retry_wait.is_some()).await;
    h.hotkey(HotkeyEvent::ComboDown);
    h.wait_for(|s| s.state == Recording).await;
    h.hotkey(HotkeyEvent::ComboDown);
    h.wait_for(|s| s.waiting == 1).await;
    assert!(h.paster.texts().is_empty());
    h.wait_for(|s| s.state == Idle).await;
    assert_eq!(h.paster.texts(), ["first ", "second "]);
    assert_eq!(*h.paster.policies.lock().unwrap(), [true, true]);
}

#[tokio::test(start_paused = true)]
async fn cancelling_a_wait_with_history_disabled_keeps_only_memory() {
    let mut config = AppConfig::default();
    config.history.enabled = false;
    let mut h = harness(config);
    h.script("groq", vec![Err(ProviderError::Timeout), Err(ProviderError::Timeout)]);
    h.send(Msg::Toggle);
    h.send(Msg::Stop);
    h.wait_for(|s| s.retry_wait.is_some()).await;
    h.send(Msg::Cancel);
    h.wait_for(|s| s.state == Idle).await;
    assert!(h.history.entries().is_empty());
    assert!(h.handle.status().can_retry);
    assert_eq!(h.providers.calls(), 2);
}

#[tokio::test]
async fn history_retry_replaces_the_row_and_copies_once_without_pasting_or_restoring() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Err(ProviderError::Unauthorized(401))]);
    h.script("openai", vec![Ok("12 items")]);
    h.send(Msg::Toggle);
    h.send(Msg::Stop);
    h.wait_for(|s| s.state == Idle).await;
    let mut config = h.settings.current().config.clone();
    config.paste.restore_clipboard = true;
    config.paste.method = PasteMethod::Type;
    config.paste.after = PasteAfter::CtrlEnter;
    config.active_profile_id = "openai".into();
    config.profiles.iter_mut().find(|p| p.id == "openai").unwrap().language = "en".into();
    config.rules.numbers_as_words = true;
    h.settings.replace(settings(config));
    let audio = PreparedAudio { samples: vec![0.1; 16_000], audio_ms: 1000, speech_ms: 1000 };
    h.providers.gate.forget_permits(1_000);
    let (reply, answer) = oneshot::channel();
    h.send(Msg::RetryHistory { id: 1, audio: Arc::new(audio.clone()), reply });
    h.wait_for(|s| s.state == Transcribing).await;
    let (reply, duplicate) = oneshot::channel();
    h.send(Msg::RetryHistory { id: 1, audio: Arc::new(audio), reply });
    assert_eq!(duplicate.await.unwrap(), Err(ErrorKind::Rejected));
    h.providers.gate.add_permits(1);
    assert_eq!(answer.await.unwrap(), Ok(()));
    h.wait_for(|s| s.state == Idle).await;
    assert_eq!(h.history.entries().len(), 1);
    assert_eq!(h.history.entries()[0].text, "Twelve items");
    assert_eq!(h.history.entries()[0].profile_id, "openai");
    assert!(!h.history.entries()[0].has_audio);
    assert_eq!(*h.paster.pasted.lock().unwrap(), [("Twelve items ".into(), false)]);
    assert_eq!(*h.paster.policies.lock().unwrap(), [false]);
    assert_eq!(*h.paster.clipboard_writes.lock().unwrap(), ["Twelve items "]);
    assert!(h.paster.typed.lock().unwrap().is_empty());
    assert!(h.paster.after_keys.lock().unwrap().is_empty());
}

#[tokio::test]
async fn toggle_records_transcribes_pastes_and_saves() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("cloud code'u aç")]);

    h.hotkey(HotkeyEvent::ComboDown);
    h.expect(&[Recording]).await;
    assert!(h.hotkey.escape_captured(), "Esc cancels while recording");
    assert_eq!(h.overlay.last().level, Some(0.0));
    h.hotkey(HotkeyEvent::ComboDown);
    h.expect(&[Transcribing, Pasting, Idle]).await;

    assert_eq!(h.paster.texts(), ["Claude Code'u aç "]);
    assert!(!h.paster.pasted.lock().unwrap()[0].1, "the latest dictation stays on the clipboard by default");
    let saved = h.history.entries();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].raw_text, "cloud code'u aç");
    assert_eq!(saved[0].text, "Claude Code'u aç");
    assert_eq!(
        (saved[0].profile_id.as_str(), saved[0].status, saved[0].has_audio),
        ("groq", TranscriptStatus::Ok, false)
    );
    assert_eq!(*h.events.history.lock().unwrap(), [1]);
    assert_eq!(*h.sounds.played.lock().unwrap(), [Sound::Start, Sound::Done]);
    let done = h.overlay.last();
    assert_eq!(done.tone, Tone::Success);
    assert!(done.text.starts_with("Pasted ("), "{}", done.text);
    assert!(!h.hotkey.escape_captured());
    assert_eq!((h.mic.finished.load(SeqCst), h.providers.warmups.load(SeqCst)), (1, 1));
    assert_eq!(h.handle.status(), DictationStatus::default());
}

#[tokio::test]
async fn consecutive_dictations_write_the_latest_text_without_restoring_the_clipboard() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("ilk dikte"), Ok("son dikte")]);

    for _ in 0..2 {
        h.send(Msg::Toggle);
        h.expect(&[Recording]).await;
        h.send(Msg::Toggle);
        h.expect(&[Transcribing, Pasting, Idle]).await;
    }

    assert_eq!(*h.paster.pasted.lock().unwrap(), [("ilk dikte ".into(), false), ("son dikte ".into(), false)]);
}

#[tokio::test]
async fn toggle_mode_starts_on_the_combo_and_stops_on_a_lone_ctrl() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("merhaba")]);
    h.hotkey(HotkeyEvent::ComboDown);
    h.expect(&[Recording]).await;
    h.hotkey(HotkeyEvent::ComboUp);
    h.settle().await;
    assert_eq!(h.handle.status().state, Recording, "releasing the combo does not stop in toggle mode");
    h.hotkey(HotkeyEvent::LoneCtrl);
    h.expect(&[Transcribing, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["merhaba "]);
}

#[tokio::test]
async fn push_to_talk_records_while_the_combo_is_held() {
    let mut config = AppConfig::default();
    config.hotkey.mode = HotkeyMode::PushToTalk;
    let mut h = harness(config);
    h.script("groq", vec![Ok("bas konuş")]);
    h.hotkey(HotkeyEvent::ComboDown);
    h.expect(&[Recording]).await;
    h.hotkey(HotkeyEvent::LoneCtrl);
    h.settle().await;
    assert_eq!(h.handle.status().state, Recording, "lone Ctrl only stops in toggle mode");
    h.hotkey(HotkeyEvent::ComboUp);
    h.expect(&[Transcribing, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["bas konuş "]);
}

#[tokio::test]
async fn triggers_while_busy_start_a_second_recording() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("tek")]);
    h.providers.gate.forget_permits(1_000);

    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing]).await;
    h.send(Msg::Toggle);
    h.send(Msg::Retry);
    h.expect(&[Recording]).await;
    assert_eq!(h.mic.starts(), 2);
    h.hotkey(HotkeyEvent::Escape);
    h.expect(&[Transcribing]).await;

    h.providers.gate.add_permits(1_000);
    h.expect(&[Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["tek "]);
}

#[tokio::test]
async fn escape_cancels_a_recording() {
    let mut h = harness(AppConfig::default());
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.hotkey(HotkeyEvent::Escape);
    h.expect(&[Cancelled, Idle]).await;

    assert_eq!((h.mic.finished.load(SeqCst), h.mic.dropped.load(SeqCst)), (0, 1));
    assert_eq!(h.providers.calls(), 0);
    assert!(h.history.entries().is_empty());
    assert_eq!(*h.sounds.played.lock().unwrap(), [Sound::Start, Sound::Cancel]);
    assert!(!h.hotkey.escape_captured());
}

#[tokio::test]
async fn escape_cancels_a_transcription() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("geç kalan")]);
    h.providers.gate.forget_permits(1_000);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing]).await;
    assert!(h.hotkey.escape_captured(), "Esc still cancels while transcribing");
    h.hotkey(HotkeyEvent::Escape);
    h.expect(&[Cancelled, Idle]).await;

    h.providers.gate.add_permits(1_000);
    tokio::time::sleep(Duration::from_millis(50)).await;
    h.settle().await;
    assert!(h.paster.texts().is_empty());
    assert!(h.history.entries().is_empty());
    assert_eq!(h.handle.status(), DictationStatus::default());
}

#[tokio::test]
async fn microphone_failure_during_recording_is_an_error() {
    let mut h = harness(AppConfig::default());
    *h.mic.recording.lock().unwrap() = Some(silent_recording());
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.mic.emit(CaptureEvent::Level(0.2));
    h.mic.emit(CaptureEvent::Failed("device unplugged".into()));
    h.expect(&[Transcribing]).await;
    h.expect_error(ErrorKind::Microphone).await;

    assert_eq!(h.providers.calls(), 0);
    assert_eq!(h.mic.finished.load(SeqCst) + h.mic.dropped.load(SeqCst), 1, "the capture is released");
    assert!(!h.handle.status().can_retry, "nothing worth keeping was captured");
    assert_eq!(*h.sounds.played.lock().unwrap(), [Sound::Start, Sound::Error]);
    let error = h.overlay.last();
    assert_eq!((error.tone, error.button.is_none()), (Tone::Error, true));
    assert!(h.overlay.views.lock().unwrap().iter().any(|v| v.level.is_some_and(|l| l > 0.0)), "level meter moved");
}

#[tokio::test]
async fn a_lost_microphone_keeps_the_speech_for_try_again() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("kaybolmayan cümle")]);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.mic.emit(CaptureEvent::Failed("device unplugged".into()));
    h.expect(&[Transcribing]).await;
    h.expect_error(ErrorKind::Microphone).await;

    assert!(h.handle.status().can_retry);
    assert_eq!(h.mic.finished.load(SeqCst), 1, "the capture is finished, not dropped");
    assert_eq!(h.providers.calls(), 0, "nothing is sent until the user asks");
    assert!(h.paster.texts().is_empty());
    let error = h.overlay.last();
    assert_eq!((error.tone, error.button.map(|b| b.action)), (Tone::Error, Some(OverlayAction::Retry)));
    assert!(!h.hotkey.escape_captured());

    h.send(Msg::Retry);
    h.expect(&[Transcribing, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["kaybolmayan cümle "]);
    assert_eq!(h.mic.starts(), 1, "the kept audio is sent, nothing is re-recorded");
    assert_eq!(h.providers.calls(), 1);
    assert!(!h.handle.status().can_retry);
}

#[tokio::test]
async fn a_microphone_that_cannot_open_never_enters_recording() {
    let mut h = harness(AppConfig::default());
    *h.mic.fail_start.lock().unwrap() = Some(MicError::NoDevice);
    h.send(Msg::Toggle);
    h.expect_error(ErrorKind::Microphone).await;
    assert!(!h.hotkey.escape_captured());
}

#[tokio::test]
async fn a_missing_api_key_blocks_recording_and_offers_settings() {
    let mut h = harness(AppConfig::default());
    h.providers.missing_keys.lock().unwrap().insert("groq".into());
    h.send(Msg::Toggle);
    h.expect_error(ErrorKind::MissingKey).await;
    assert_eq!(h.mic.starts(), 0, "no point recording without a key");
    assert_eq!(h.overlay.last().button.map(|b| b.action), Some(OverlayAction::OpenSettings));

    h.send(Msg::Overlay(OverlayAction::OpenSettings));
    h.settle().await;
    assert_eq!(h.events.settings_opened.load(SeqCst), 1);
}

#[tokio::test]
async fn silence_is_not_sent() {
    let mut h = harness(AppConfig::default());
    *h.mic.recording.lock().unwrap() = Some(silent_recording());
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing, Idle]).await;
    assert_eq!(h.providers.calls(), 0);
    assert_eq!(h.overlay.last().text, "No speech detected (-120 dB)");
    assert!(h.history.entries().is_empty());
}

#[tokio::test]
async fn calibration_is_snapshotted_at_recording_start_and_language_reaches_history() {
    let mut config = AppConfig::default();
    config.recording.speech_floor_db = -65.0;
    let mut h = harness(config);
    h.script("groq", vec![Ok("quiet speech")]);
    let mut quiet = speech_recording();
    quiet.samples.iter_mut().for_each(|s| *s *= 0.01);
    *h.mic.recording.lock().unwrap() = Some(quiet);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.settings.replace(settings(AppConfig::default()));
    h.send(Msg::Stop);
    h.expect(&[Transcribing, Pasting, Idle]).await;
    assert_eq!(h.providers.calls(), 1);
    assert_eq!(h.history.entries()[0].language.as_deref(), Some("tr"));
}

#[tokio::test(start_paused = true)]
async fn a_provider_failure_keeps_the_audio_for_try_again() {
    let mut h = harness(AppConfig::default());
    let mut script = vec![Err(ProviderError::Server(500)); 6];
    script.push(Ok("tekrar"));
    h.script("groq", script);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing]).await;
    h.expect_error(ErrorKind::Server).await;
    assert!(h.handle.status().can_retry);
    assert_eq!(h.overlay.last().button.map(|b| b.action), Some(OverlayAction::Retry));
    assert_eq!(h.history.entries()[0].status, TranscriptStatus::Failed);
    assert!(h.history.entries()[0].has_audio);

    h.send(Msg::Overlay(OverlayAction::Retry));
    h.expect(&[Transcribing, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["tekrar "]);
    assert_eq!(h.mic.starts(), 1, "the kept audio is resent, nothing is re-recorded");
    assert_eq!(h.providers.calls(), 7);
    assert!(!h.handle.status().can_retry);
    assert_eq!(h.history.entries().len(), 2);
}

#[tokio::test]
async fn the_fallback_profile_answers_after_the_primary_fails() {
    let mut h = harness(with_fallback());
    h.script("groq", vec![Err(ProviderError::RateLimited), Err(ProviderError::RateLimited)]);
    h.script("openai", vec![Ok("yedek")]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["yedek "]);
    assert_eq!(h.history.entries()[0].profile_id, "openai");
}

#[tokio::test]
async fn a_rejected_key_points_to_settings_but_keeps_the_audio() {
    let mut h = harness(with_fallback());
    h.script("groq", vec![Err(ProviderError::Unauthorized(401))]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing]).await;
    h.expect_error(ErrorKind::InvalidKey).await;
    assert_eq!(h.providers.calls(), 1, "401 is neither retried nor sent to the fallback");
    assert!(h.handle.status().can_retry);
    assert_eq!(h.overlay.last().button.map(|b| b.action), Some(OverlayAction::OpenSettings));
}

#[tokio::test(start_paused = true)]
async fn the_recording_limit_sends_automatically() {
    let mut config = AppConfig::default();
    config.recording.max_seconds = 10;
    let mut h = harness(config);
    h.script("groq", vec![Ok("uzun kayıt")]);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    tokio::time::advance(Duration::from_secs(10)).await;
    h.expect(&[Transcribing, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["uzun kayıt "]);
}

#[tokio::test]
async fn empty_output_is_saved_but_not_pasted() {
    let mut config = AppConfig::default();
    config.ui.sound_feedback = false;
    let mut h = harness(config);
    h.script("groq", vec![Ok("   ")]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing, Idle]).await;
    assert!(h.paster.texts().is_empty());
    assert_eq!(h.history.entries()[0].status, TranscriptStatus::Empty);
    assert_eq!(h.overlay.last().text, "No text came back");
    assert!(h.sounds.played.lock().unwrap().is_empty(), "sound feedback is off");
}

#[tokio::test]
async fn a_refused_paste_leaves_the_text_on_the_clipboard() {
    let mut h = harness(AppConfig::default());
    *h.paster.outcome.lock().unwrap() = PasteOutcome::ClipboardOnly;
    h.script("groq", vec![Ok("yönetici penceresi")]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing, Pasting, Idle]).await;
    let hint = h.overlay.last();
    assert_eq!(hint.tone, Tone::Warning);
    assert!(hint.text.contains("Ctrl+V"), "{}", hint.text);
    assert_eq!(h.history.entries().len(), 1);
}

#[tokio::test]
async fn a_failed_paste_is_an_error_but_the_text_is_saved() {
    let mut h = harness(AppConfig::default());
    *h.paster.outcome.lock().unwrap() = PasteOutcome::Failed("clipboard busy".into());
    h.script("groq", vec![Ok("kaybolmasın")]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing, Pasting]).await;
    h.expect_error(ErrorKind::Paste).await;
    assert_eq!(h.history.entries()[0].text, "kaybolmasın");
}

#[tokio::test]
async fn a_failed_paste_does_not_mention_history_when_it_is_disabled() {
    for language in ["en", "tr"] {
        let mut config = AppConfig { ui_language: Some(language.into()), ..AppConfig::default() };
        config.history.enabled = false;
        let mut h = harness(config);
        *h.paster.outcome.lock().unwrap() = PasteOutcome::Failed("clipboard busy".into());
        h.script("groq", vec![Ok("kaybolmasın")]);
        h.send(Msg::Toggle);
        h.send(Msg::Toggle);
        h.expect(&[Recording, Transcribing, Pasting]).await;
        match h.next().await {
            DictationState::Error { kind, message } => {
                assert_eq!(kind, ErrorKind::Paste);
                assert!(!message.is_empty());
                assert!(!message.contains("History") && !message.contains("Geçmiş"), "{message}");
            }
            other => panic!("expected a paste error, got {other:?}"),
        }
        h.expect(&[Idle]).await;
        assert!(h.history.entries().is_empty());
        assert!(!h.overlay.last().text.contains("History") && !h.overlay.last().text.contains("Geçmiş"));
    }
}

#[tokio::test]
async fn a_failed_paste_uses_the_sessions_history_setting() {
    for history_on in [false, true] {
        let mut config = AppConfig::default();
        config.history.enabled = history_on;
        let mut h = harness(config.clone());
        *h.paster.outcome.lock().unwrap() = PasteOutcome::Failed("clipboard busy".into());
        h.script("groq", vec![Ok("kaybolmasın")]);
        h.send(Msg::Toggle);
        h.expect(&[Recording]).await;
        config.history.enabled = !history_on;
        h.settings.replace(settings(config));
        h.send(Msg::Toggle);
        h.expect(&[Transcribing, Pasting]).await;
        match h.next().await {
            DictationState::Error { kind, message } => {
                assert_eq!(kind, ErrorKind::Paste);
                assert_eq!(message.contains("History"), history_on, "{message}");
            }
            other => panic!("expected a paste error, got {other:?}"),
        }
        h.expect(&[Idle]).await;
        assert_eq!(h.history.entries().len(), usize::from(history_on));
    }
}

#[tokio::test]
async fn history_follows_the_live_config() {
    let mut config = AppConfig::default();
    config.history.enabled = false;
    let mut h = harness(config.clone());
    h.script("groq", vec![Ok("bir"), Ok("iki")]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing, Pasting, Idle]).await;
    assert!(h.history.entries().is_empty());

    config.history.enabled = true;
    config.history.save_audio = true;
    h.settings.replace(settings(config));
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing, Pasting, Idle]).await;
    let saved = h.history.entries();
    assert_eq!((saved.len(), saved[0].text.as_str(), saved[0].has_audio), (1, "iki", true));
}

#[tokio::test]
async fn pending_update_reserves_file_work_and_cancellation_releases_it() {
    let (handle, _inbox) = channel();
    let mut hold = Box::pin(handle.hold_for_update());
    std::future::poll_fn(|cx| {
        assert!(hold.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    assert!(handle.begin_file_work().is_none());
    drop(hold);
    assert!(handle.begin_file_work().is_some());
}

#[tokio::test]
async fn file_work_and_updates_exclude_each_other_in_both_orders() {
    let h = harness(AppConfig::default());
    let ui = h.handle.begin_file_work().unwrap();
    let ipc = h.handle.begin_file_work().unwrap();
    assert_eq!(h.handle.hold_for_update().await.err(), Some(HoldRefusal::FileWork));
    drop(ui);
    assert_eq!(h.handle.hold_for_update().await.err(), Some(HoldRefusal::FileWork));
    drop(ipc);
    let update = h.handle.hold_for_update().await.unwrap();
    assert!(h.handle.begin_file_work().is_none());
    let second = h.handle.hold_for_update().await.unwrap();
    drop(update);
    assert!(h.handle.begin_file_work().is_none());
    drop(second);
    assert!(h.handle.begin_file_work().is_some());
}

#[tokio::test]
async fn an_update_hold_refuses_new_dictations_until_every_hold_is_released() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Err(ProviderError::BadResponse("invalid response".into())), Ok("sonra")]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing]).await;
    h.expect_error(ErrorKind::BadResponse).await;
    assert!(h.handle.status().can_retry);

    let first = h.handle.hold_for_update().await.expect("idle: the update may install");
    let second = h.handle.hold_for_update().await.expect("holds stack");
    h.send(Msg::Toggle);
    h.hotkey(HotkeyEvent::ComboDown);
    h.send(Msg::Retry);
    h.send(Msg::Overlay(OverlayAction::Retry));
    h.settle().await;
    assert_eq!(h.mic.starts(), 1, "no recording while an update installs");
    assert_eq!(h.providers.calls(), 1, "no Try again while an update installs");
    assert_eq!(h.handle.status().state, Idle);
    assert_eq!(h.overlay.last().text, "Installing an update…");

    drop(first);
    h.send(Msg::Toggle);
    h.settle().await;
    assert_eq!(h.mic.starts(), 1, "still held by the second install attempt");

    drop(second);
    h.send(Msg::Retry);
    h.expect(&[Transcribing, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["sonra "]);
}

#[tokio::test]
async fn no_update_hold_while_a_dictation_is_in_flight() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("bitti")]);
    h.providers.gate.forget_permits(1_000);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    assert_eq!(h.handle.hold_for_update().await.err(), Some(HoldRefusal::Dictation), "recording");
    h.send(Msg::Toggle);
    h.expect(&[Transcribing]).await;
    assert!(h.handle.hold_for_update().await.is_err(), "transcribing");

    h.providers.gate.add_permits(1_000);
    h.expect(&[Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["bitti "], "the refused hold changed nothing");
    assert!(h.handle.hold_for_update().await.is_ok());
}

#[tokio::test]
async fn shutdown_releases_the_microphone_quietly() {
    let mut h = harness(AppConfig::default());
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.send(Msg::Shutdown);
    h.expect(&[Idle]).await;
    assert_eq!(h.mic.dropped.load(SeqCst), 1);
    assert_eq!(h.overlay.hides.load(SeqCst), 1);
    assert_eq!(*h.sounds.played.lock().unwrap(), [Sound::Start]);
}

#[tokio::test]
async fn transcript_text_never_reaches_the_logs() {
    let logs = captured_logs();
    let mut h = harness(with_fallback());
    h.script("groq", vec![Err(ProviderError::Server(503)), Err(ProviderError::Server(503))]);
    h.script("openai", vec![Ok("çok gizli cümle")]);
    h.hotkey(HotkeyEvent::ComboDown);
    h.hotkey(HotkeyEvent::ComboDown);
    h.expect(&[Recording, Transcribing, Pasting, Idle]).await;

    let logs = String::from_utf8_lossy(&logs.lock().unwrap()).into_owned();
    assert!(logs.contains("transcribed") && logs.contains("pasted"), "{logs}");
    assert!(!logs.contains("gizli"), "transcript leaked into the logs:\n{logs}");
}

/// Every log line of this test binary. A process-wide subscriber is used because a
/// thread-local `set_default` misses events when other tests run in parallel
/// (tracing caches callsite interest globally).
fn captured_logs() -> Arc<Mutex<Vec<u8>>> {
    static LOGS: std::sync::OnceLock<Arc<Mutex<Vec<u8>>>> = std::sync::OnceLock::new();
    LOGS.get_or_init(|| {
        let logs = Arc::new(Mutex::new(Vec::new()));
        let writer = logs.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || CaptureWriter(writer.clone()))
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("only this test installs a subscriber");
        logs
    })
    .clone()
}

struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_level_meter_uses_a_decibel_scale() {
    assert_eq!(meter(0.0), 0.0);
    assert_eq!(meter(f32::NAN), 0.0);
    assert_eq!(meter(1.0), 1.0);
    assert!((meter(0.1) - 2.0 / 3.0).abs() < 1e-4);
    assert_eq!(meter(0.000_1), 0.0);
}

async fn wait_for(h: &mut Harness, state: DictationState, waiting: usize) {
    loop {
        let status = tokio::time::timeout(Duration::from_secs(5), h.states.recv()).await.unwrap().unwrap();
        if status.state == state && status.waiting == waiting {
            return;
        }
    }
}

#[tokio::test]
async fn bounded_fifo_preserves_profiles_paste_policy_and_history_order() {
    let mut config = AppConfig::default();
    let mut h = harness(config.clone());
    h.script("groq", vec![Ok("first")]);
    h.script("openai", vec![Ok("second"), Ok("third"), Ok("fourth")]);
    let first_gate = Arc::new(Semaphore::new(0));
    h.providers.profile_gates.lock().unwrap().insert("groq".into(), first_gate.clone());
    h.hotkey(HotkeyEvent::ComboDown);
    h.hotkey(HotkeyEvent::LoneCtrl);
    h.expect(&[Recording, Transcribing]).await;
    config.active_profile_id = "openai".into();
    h.settings.replace(settings(config.clone()));
    for waiting in 1..=3 {
        h.send(Msg::Toggle);
        h.expect(&[Recording]).await;
        h.send(Msg::Stop);
        wait_for(&mut h, Transcribing, waiting).await;
    }
    assert!(h.handle.hold_for_update().await.is_err(), "waiting recordings block updates");
    h.send(Msg::Toggle);
    h.send(Msg::Retry);
    h.settle().await;
    assert_eq!(h.mic.starts(), 4, "one processing plus at most three waiting");
    assert!(h.overlay.last().text.contains("Queue full"));
    tokio::time::timeout(Duration::from_secs(5), async {
        while h.providers.calls() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(h.providers.calls(), 1, "a ready later request cannot overtake a blocked first request");
    config.active_profile_id = "groq".into();
    config.paste.trailing_space = false;
    config.history.enabled = false;
    h.settings.replace(settings(config));
    first_gate.add_permits(1);
    wait_for(&mut h, Idle, 0).await;
    assert_eq!(h.paster.texts(), ["first ", "second ", "third ", "fourth "]);
    assert_eq!(*h.paster.policies.lock().unwrap(), [true, false, false, false]);
    assert_eq!(
        h.history.entries().iter().map(|r| r.profile_id.clone()).collect::<Vec<_>>(),
        ["groq", "openai", "openai", "openai"]
    );
    assert_eq!(*h.events.history.lock().unwrap(), [1, 2, 3, 4]);
    assert!(h.handle.hold_for_update().await.is_ok());
}

#[tokio::test]
async fn escape_prefers_recording_then_cancels_inflight_and_keeps_waiting_items() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("waiting")]);
    h.providers.gate.forget_permits(1_000);
    h.send(Msg::Toggle);
    h.send(Msg::Stop);
    h.expect(&[Recording, Transcribing]).await;
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.hotkey(HotkeyEvent::Escape);
    h.expect(&[Transcribing]).await;
    assert_eq!(h.mic.dropped.load(SeqCst), 1);
    assert!(h.hotkey.escape_captured());
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.send(Msg::Stop);
    wait_for(&mut h, Transcribing, 1).await;
    h.hotkey(HotkeyEvent::Escape);
    wait_for(&mut h, Transcribing, 0).await;
    h.providers.gate.add_permits(1_000);
    wait_for(&mut h, Idle, 0).await;
    assert_eq!(h.paster.texts(), ["waiting "]);
    assert_eq!(h.history.entries().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn pause_discards_samples_freezes_meter_and_excludes_paused_time_from_limit() {
    let mut config = AppConfig::default();
    config.recording.max_seconds = 10;
    let mut h = harness(config);
    h.script("groq", vec![Ok("resumed")]);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.mic.append_samples(&[0.1, 0.2]);
    tokio::time::advance(Duration::from_secs(4)).await;
    h.send(Msg::PauseResume);
    h.expect(&[Recording]).await;
    assert!(h.handle.status().paused);
    let frozen = h.overlay.last();
    let timing = frozen.recording.as_ref().unwrap();
    assert!(timing.paused);
    assert_eq!(timing.elapsed, Duration::from_secs(4));
    assert_eq!(timing.elapsed_at(timing.updated_at + Duration::from_secs(30)), Duration::from_secs(4));
    h.mic.append_samples(&[0.9; 100]);
    h.mic.emit(CaptureEvent::Level(0.9));
    h.settle().await;
    assert_eq!(h.overlay.last(), frozen);
    tokio::time::advance(Duration::from_secs(30)).await;
    h.settle().await;
    assert_eq!(h.mic.finished.load(SeqCst), 0);
    h.hotkey(HotkeyEvent::Pause);
    h.expect(&[Recording]).await;
    let resumed = h.overlay.last().recording.unwrap();
    assert!(!resumed.paused);
    assert_eq!(resumed.session, timing.session);
    assert_eq!(resumed.elapsed, Duration::from_secs(4));
    h.mic.append_samples(&[0.3]);
    tokio::time::advance(Duration::from_secs(5)).await;
    h.settle().await;
    assert_eq!(h.mic.finished.load(SeqCst), 0);
    tokio::time::advance(Duration::from_secs(1)).await;
    h.expect(&[Transcribing, Pasting, Idle]).await;
    let captured = h.mic.captured.lock().unwrap();
    let samples = &captured[0].samples;
    assert_eq!(samples.len(), speech_recording().samples.len() + 3);
    assert_eq!(&samples[samples.len() - 3..], [0.1, 0.2, 0.3]);
}

#[tokio::test]
async fn paused_recordings_can_be_stopped_or_cancelled() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("stopped while paused")]);
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.send(Msg::PauseResume);
    h.expect(&[Recording]).await;
    h.send(Msg::Stop);
    h.expect(&[Transcribing, Pasting, Idle]).await;
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.send(Msg::PauseResume);
    h.expect(&[Recording]).await;
    h.hotkey(HotkeyEvent::Escape);
    h.expect(&[Cancelled, Idle]).await;
    assert_eq!(h.mic.finished.load(SeqCst), 1);
    assert_eq!(h.mic.dropped.load(SeqCst), 1);
}

#[tokio::test]
async fn provider_worker_panic_does_not_wedge_the_controller() {
    let mut h = harness(AppConfig::default());
    // An unscripted provider panics. The actor must still accept the next recording.
    h.send(Msg::Toggle);
    h.send(Msg::Stop);
    h.expect(&[Recording, Transcribing]).await;
    h.expect_error(ErrorKind::Audio).await;
    assert_eq!(h.media.calls.lock().unwrap().len(), 2, "media was released before the provider panicked");
    h.script("groq", vec![Ok("recovered")]);
    h.send(Msg::Toggle);
    h.send(Msg::Stop);
    h.expect(&[Recording, Transcribing, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["recovered "]);
}

#[tokio::test]
async fn push_to_talk_starts_while_transcribing_and_snapshots_stop_focus() {
    let mut config = AppConfig::default();
    config.hotkey.mode = HotkeyMode::PushToTalk;
    let mut h = harness(config);
    h.script("groq", vec![Ok("first"), Ok("second")]);
    h.providers.gate.forget_permits(1_000);
    h.hotkey(HotkeyEvent::ComboDown);
    h.hotkey(HotkeyEvent::ComboUp);
    h.expect(&[Recording, Transcribing]).await;
    h.hotkey(HotkeyEvent::ComboDown);
    h.expect(&[Recording]).await;
    *h.paster.allow_auto.lock().unwrap() = Some(false);
    h.hotkey(HotkeyEvent::ComboUp);
    wait_for(&mut h, Transcribing, 1).await;
    *h.paster.allow_auto.lock().unwrap() = Some(true);
    h.providers.gate.add_permits(1_000);
    wait_for(&mut h, Idle, 0).await;
    assert_eq!(*h.paster.policies.lock().unwrap(), [true, false]);
    assert_eq!(h.paster.texts(), ["first ", "second "]);
}

#[tokio::test]
async fn recording_continues_while_paste_and_restore_are_blocked() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("first"), Ok("second")]);
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    *h.paster.gate.lock().unwrap() = Some(gate.clone());
    h.send(Msg::Toggle);
    h.send(Msg::Stop);
    h.expect(&[Recording, Transcribing, Pasting]).await;
    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    h.mic.emit(CaptureEvent::Level(0.3));
    h.settle().await;
    assert!(h.overlay.last().level.is_some());
    h.send(Msg::Stop);
    wait_for(&mut h, Pasting, 1).await;
    assert_eq!(h.providers.calls(), 1, "the next job waits for paste and restore to return");
    assert!(h.handle.hold_for_update().await.is_err());
    h.hotkey(HotkeyEvent::Escape);
    h.settle().await;
    assert_eq!(h.handle.status().waiting, 1, "Escape cannot interrupt a paste in progress");
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    wait_for(&mut h, Idle, 0).await;
    assert_eq!(h.paster.texts(), ["first ", "second "]);
    assert_eq!(h.history.entries().len(), 2);
}

#[tokio::test]
async fn ask_text_and_voice_share_the_queue_and_save_request_answer_and_agent() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"choices":[{"message":{"content":"agent answer"},"finish_reason":"stop"}]}),
        ))
        .mount(&server)
        .await;
    for (voice, output) in [
        (false, opit_core::ask::Output::Copy),
        (true, opit_core::ask::Output::Copy),
        (false, opit_core::ask::Output::Paste),
        (true, opit_core::ask::Output::Paste),
    ] {
        let mut config = AppConfig::default();
        config.ask.base_url = server.uri();
        config.ask.api_key_ref = None;
        config.ask.output = output;
        config.paste.restore_clipboard = true;
        config.paste.method = PasteMethod::Type;
        config.paste.after = PasteAfter::Enter;
        let mut h = harness(config);
        let handle = h.handle.clone();
        h.script("groq", vec![Ok("spoken request")]);
        let result =
            tokio::spawn(async move { handle.ask(if voice { None } else { Some("typed request".into()) }).await });
        if voice {
            h.wait_for(|s| s.state == Recording).await;
            h.hotkey(HotkeyEvent::ComboDown);
        }
        h.wait_for(|s| s.state == DictationState::Asking).await;
        assert_eq!(result.await.unwrap().unwrap(), "agent answer");
        h.wait_for(|s| s.state == Idle).await;
        assert_eq!(h.paster.texts(), ["agent answer"]);
        let inserts = output == opit_core::ask::Output::Paste;
        assert_eq!(
            *h.paster.requests.lock().unwrap(),
            [PasteRequest {
                method: PasteMethod::Type,
                after: PasteAfter::Enter,
                restore: inserts,
                auto_paste: inserts,
            }]
        );
        if inserts {
            assert_eq!(*h.paster.typed.lock().unwrap(), ["agent answer"]);
            assert_eq!(*h.paster.after_keys.lock().unwrap(), [PasteAfter::Enter]);
            assert!(h.paster.clipboard_writes.lock().unwrap().is_empty());
        } else {
            assert_eq!(*h.paster.clipboard_writes.lock().unwrap(), ["agent answer"]);
            assert!(h.paster.typed.lock().unwrap().is_empty());
            assert!(h.paster.after_keys.lock().unwrap().is_empty());
        }
        assert_eq!(h.mic.starts(), usize::from(voice));
        let saved = h.history.entries();
        assert_eq!(saved[0].mode, "ask");
        assert_eq!(saved[0].agent.as_deref(), Some("OpenAI-compatible chat"));
        assert_eq!(saved[0].raw_text, if voice { "spoken request" } else { "typed request" });
        assert_eq!(saved[0].text, "agent answer");
        h.send(Msg::Shutdown);
    }
}

#[tokio::test]
async fn escape_during_ask_cancels_without_paste_or_dictation_retry() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(10)))
        .mount(&server)
        .await;
    let mut config = AppConfig::default();
    config.ask.base_url = server.uri();
    config.ask.api_key_ref = None;
    let mut h = harness(config);
    let handle = h.handle.clone();
    let result = tokio::spawn(async move { handle.ask(Some("request".into())).await });
    h.wait_for(|s| s.state == DictationState::Asking).await;
    h.hotkey(HotkeyEvent::Escape);
    assert_eq!(result.await.unwrap(), Err(ErrorKind::Cancelled));
    h.wait_for(|s| s.state == Idle).await;
    assert!(h.paster.texts().is_empty());
    assert!(!h.handle.status().can_retry);
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn ask_test_uses_fixed_request_without_paste_history_or_conversation_changes() {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"choices":[{"message":{"content":"OK"},"finish_reason":"stop"}]})),
        )
        .mount(&server)
        .await;
    let mut config = AppConfig::default();
    config.ask.base_url = server.uri();
    config.ask.api_key_ref = None;
    let mut h = harness(config.clone());
    assert_eq!(h.handle.test_ask(config.ask).await.unwrap(), "OK");
    h.wait_for(|s| s.state == Idle).await;
    assert!(h.paster.texts().is_empty());
    assert!(h.history.entries().is_empty());
    assert_eq!(h.mic.starts(), 0);
    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = requests[0].body_json().unwrap();
    assert_eq!(body["messages"][1]["content"], opit_core::ask::TEST_REQUEST);
    h.handle.ask(Some("next request".into())).await.unwrap();
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[1].body_json::<serde_json::Value>().unwrap()["messages"].as_array().unwrap().len(), 2);
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn ask_hotkey_stops_and_microphone_failure_never_becomes_plain_dictation_retry() {
    let mut h = harness(AppConfig::default());
    h.hotkey(HotkeyEvent::Ask);
    h.wait_for(|s| s.state == Recording).await;
    h.mic.emit(CaptureEvent::Failed("fake mic lost".into()));
    h.wait_for(|s| matches!(s.state, DictationState::Error { kind: ErrorKind::Microphone, .. })).await;
    h.wait_for(|s| s.state == Idle).await;
    assert!(!h.handle.status().can_retry);
    assert!(h.paster.texts().is_empty());
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn ask_request_cleanup_is_separate_from_dictation_cleanup_and_keeps_original_history() {
    use wiremock::matchers::{body_partial_json, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_partial_json(serde_json::json!({"model":"cleanup-test"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"choices":[{"message":{"content":"cleaned request"},"finish_reason":"stop"}]}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(body_partial_json(serde_json::json!({"model":"ask-test"})))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"choices":[{"message":{"content":"answer"},"finish_reason":"stop"}]}),
            ),
        )
        .mount(&server)
        .await;
    let mut config = AppConfig::default();
    config.cleanup.enabled = false;
    config.cleanup.base_url = server.uri();
    config.cleanup.api_key_ref = None;
    config.cleanup.model = "cleanup-test".into();
    config.profiles[0].apply_cleanup = false;
    config.ask.base_url = server.uri();
    config.ask.api_key_ref = None;
    config.ask.chat_model = "ask-test".into();
    config.ask.cleanup_request = true;
    let cleanup = Arc::new(CleanupClient::new(config.cleanup.clone(), None).unwrap());
    let mut h = harness(config);
    *h.providers.cleanup.lock().unwrap() = Some(cleanup);
    h.script("groq", vec![Ok("original spoken request")]);
    let handle = h.handle.clone();
    let result = tokio::spawn(async move { handle.ask(None).await });
    h.wait_for(|s| s.state == Recording).await;
    h.send(Msg::Stop);
    assert_eq!(result.await.unwrap().unwrap(), "answer");
    h.wait_for(|s| s.state == Idle).await;
    let saved = h.history.entries();
    assert_eq!((saved[0].raw_text.as_str(), saved[0].text.as_str()), ("original spoken request", "answer"));
    assert_eq!(saved[0].cleanup_status, Some(cleanup::CleanupStatus::Ok));
    let requests = server.received_requests().await.unwrap();
    let request = requests
        .iter()
        .find(|r| r.body_json::<serde_json::Value>().ok().is_some_and(|v| v["model"] == "ask-test"))
        .unwrap();
    assert_eq!(request.body_json::<serde_json::Value>().unwrap()["messages"][1]["content"], "cleaned request");
    h.send(Msg::Shutdown);
}

async fn voice_harness(status: u16, answer: &str) -> (wiremock::MockServer, Harness) {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(status).set_body_json(serde_json::json!({
            "choices":[{"message":{"content": answer}, "finish_reason":"stop"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let mut config = AppConfig::default();
    config.cleanup.enabled = false;
    config.cleanup.base_url = server.uri();
    config.cleanup.api_key_ref = None;
    config.cleanup.model = "connection-model".into();
    config.cleanup.reasoning_effort = Some("none".into());
    config.cleanup.active_style_id = "formal".into();
    let style = config.cleanup.styles.iter_mut().find(|s| s.id == "formal").unwrap();
    style.model = Some("ignored-style-model".into());
    style.prompt = "STYLE MUST NOT APPLY".into();
    let client = Arc::new(CleanupClient::new(config.cleanup.clone(), None).unwrap());
    let h = harness(config);
    *h.providers.cleanup.lock().unwrap() = Some(client);
    (server, h)
}

#[tokio::test]
async fn voice_edit_with_or_without_selection_uses_connection_and_saves_originals() {
    for selection in [Some("Original selected text"), None] {
        let (server, mut h) = voice_harness(200, "Requested result").await;
        *h.paster.selection.lock().unwrap() = selection.map(str::to_string);
        h.script("groq", vec![Ok("make this polite please")]);
        h.hotkey(HotkeyEvent::Edit);
        h.expect(&[Transcribing, Recording]).await;
        assert_eq!(h.overlay.last().text, "Edit");
        // Changes while speaking cannot replace the connection, paste policy or selection.
        let mut changed = h.settings.current().config.clone();
        changed.cleanup.model = "later-model".into();
        changed.paste.method = opit_core::config::PasteMethod::Type;
        h.settings.replace(settings(changed));
        *h.paster.selection.lock().unwrap() = Some("later selection".into());
        h.hotkey(HotkeyEvent::Edit);
        h.expect(&[Transcribing, DictationState::Cleaning, Pasting, Idle]).await;
        assert_eq!(h.paster.texts(), ["Requested result "]);
        assert_eq!(h.paster.requests.lock().unwrap()[0].method, opit_core::config::PasteMethod::CtrlV);
        let rows = h.history.entries();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].mode, "edit");
        assert_eq!(rows[0].source_text.as_deref(), selection);
        assert_eq!(rows[0].instruction.as_deref(), Some("make this polite please"));
        assert_eq!(rows[0].text, "Requested result");
        assert_eq!(rows[0].cleanup_model.as_deref(), Some("connection-model"));
        assert_eq!(rows[0].cleanup_style_id, None);
        assert!(!rows[0].has_audio);
        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = requests[0].body_json().unwrap();
        assert_eq!(body["model"], "connection-model");
        assert_eq!(body["reasoning_effort"], "none");
        assert!(!body["messages"][0]["content"].as_str().unwrap().contains("STYLE MUST NOT APPLY"));
        let user: serde_json::Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(user["selection"], serde_json::json!(selection));
        assert_eq!(user["instruction"], "make this polite please");
        assert_eq!(*h.media.calls.lock().unwrap(), [Some((opit_core::config::MediaMode::Duck, 50)), None]);
    }
}

#[tokio::test]
async fn voice_llm_failure_never_pastes_and_retains_failed_edit_or_translation() {
    for (event, mode, status, kind) in [
        (HotkeyEvent::Edit, "edit", 402, ErrorKind::PaymentRequired),
        (HotkeyEvent::Translate, "translate", 402, ErrorKind::PaymentRequired),
        (HotkeyEvent::Edit, "edit", 429, ErrorKind::RateLimited),
        (HotkeyEvent::Translate, "translate", 429, ErrorKind::RateLimited),
        (HotkeyEvent::Edit, "edit", 500, ErrorKind::Server),
        (HotkeyEvent::Translate, "translate", 503, ErrorKind::Server),
    ] {
        let (_server, mut h) = voice_harness(status, "private provider error").await;
        *h.paster.selection.lock().unwrap() = Some("private selected text".into());
        h.script("groq", vec![Ok("private spoken instruction")]);
        h.hotkey(event.clone());
        h.expect(&[Transcribing, Recording]).await;
        h.hotkey(event);
        h.expect(&[Transcribing, DictationState::Cleaning]).await;
        h.expect_error(kind).await;
        assert!(h.paster.texts().is_empty());
        assert!(!h.handle.status().can_retry);
        assert_eq!(h.overlay.last().tone, Tone::Error);
        assert!(!h.overlay.last().text.contains("private"));
        let rows = h.history.entries();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].mode, mode);
        assert_eq!(rows[0].status, TranscriptStatus::Failed);
        assert_eq!(rows[0].error_code.as_deref(), serde_json::to_value(kind).unwrap().as_str());
        assert!(rows[0].text.is_empty());
        if mode == "edit" {
            assert_eq!(rows[0].source_text.as_deref(), Some("private selected text"));
            assert_eq!(rows[0].instruction.as_deref(), Some("private spoken instruction"));
        } else {
            assert_eq!(rows[0].source_text.as_deref(), Some("private spoken instruction"));
            assert_eq!(rows[0].instruction, None);
        }
    }
}

#[tokio::test]
async fn voice_translation_uses_rules_target_and_model_without_style_or_selection() {
    let (server, mut h) = voice_harness(200, "Hallo Opit").await;
    let mut config = h.settings.current().config.clone();
    config.cleanup.enabled = true;
    config.translate.target_language = "de".into();
    config.translate.model = Some("translation-model".into());
    config.paste.method = opit_core::config::PasteMethod::ShiftInsert;
    config.history.save_audio = true;
    let pack = opit_core::rules::RulePack::from_yaml("schema: 1\nid: user\nname: Personal\nterms: [Opit]\n").unwrap();
    h.settings.replace(Settings::build(config, Some(pack), Some("en-US")).0);
    *h.paster.selection_error.lock().unwrap() = Some(crate::platform::SelectionError::Unavailable);
    h.script("groq", vec![Ok("hello opit today")]);
    h.hotkey(HotkeyEvent::Translate);
    h.expect(&[Transcribing, Recording]).await;
    assert_eq!(h.overlay.last().text, "Translate");
    h.hotkey(HotkeyEvent::Translate);
    h.expect(&[Transcribing, DictationState::Cleaning, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["Hallo Opit "]);
    let rows = h.history.entries();
    assert_eq!(rows[0].mode, "translate");
    assert_eq!(rows[0].source_text.as_deref(), Some("hello Opit today"));
    assert_eq!(rows[0].instruction, None);
    assert!(rows[0].has_audio);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let body: serde_json::Value = requests[0].body_json().unwrap();
    assert_eq!(body["model"], "translation-model");
    assert!(body["messages"][0]["content"].as_str().unwrap().contains("German"));
    assert!(body["messages"][1]["content"].as_str().unwrap().contains("hello Opit today"));
    assert_eq!(h.paster.requests.lock().unwrap()[0].method, opit_core::config::PasteMethod::ShiftInsert);
}

#[tokio::test]
async fn voice_shortcuts_require_connection_and_safe_selection_before_opening_microphone() {
    for selection_error in
        [None, Some(crate::platform::SelectionError::Unavailable), Some(crate::platform::SelectionError::TooLong)]
    {
        let mut config = AppConfig::default();
        config.cleanup.api_key_ref = None;
        let client = Arc::new(CleanupClient::new(config.cleanup.clone(), None).unwrap());
        let mut h = harness(config);
        if selection_error.is_some() {
            *h.providers.cleanup.lock().unwrap() = Some(client);
        }
        *h.paster.selection_error.lock().unwrap() = selection_error;
        h.hotkey(HotkeyEvent::Edit);
        h.expect(&[Transcribing]).await;
        h.expect_error(if selection_error.is_none() { ErrorKind::MissingKey } else { ErrorKind::Rejected }).await;
        assert_eq!(h.mic.starts(), 0);
        assert!(h.paster.texts().is_empty());
    }
}

#[tokio::test]
async fn unavailable_selection_shows_hint_without_recording_or_generating() {
    let mut config = AppConfig::default();
    config.cleanup.api_key_ref = None;
    config.paste.restore_clipboard = false;
    let client = Arc::new(CleanupClient::new(config.cleanup.clone(), None).unwrap());
    let mut h = harness(config);
    *h.providers.cleanup.lock().unwrap() = Some(client);
    // Both an incomplete clipboard backup and a failed advertised-text read use this error.
    *h.paster.selection.lock().unwrap() = Some("private selected text".into());
    *h.paster.selection_error.lock().unwrap() = Some(SelectionError::Unavailable);
    h.hotkey(HotkeyEvent::Edit);
    h.expect(&[Transcribing]).await;
    h.expect_error(ErrorKind::Rejected).await;
    assert_eq!(h.overlay.last().text, Lang::En.selection_failed());
    assert_eq!(h.mic.starts(), 0);
    assert_eq!(h.providers.calls(), 0);
    assert!(h.paster.texts().is_empty());
    assert!(h.history.entries().is_empty());
    h.send(Msg::Shutdown);
}

#[tokio::test]
async fn voice_push_to_talk_releases_only_its_own_mode_and_can_pause() {
    let (_server, mut h) = voice_harness(200, "result").await;
    let mut config = h.settings.current().config.clone();
    config.hotkey.mode = opit_core::config::HotkeyMode::PushToTalk;
    h.settings.replace(settings(config));
    h.script("groq", vec![Ok("write something please")]);
    h.hotkey(HotkeyEvent::Edit);
    h.expect(&[Transcribing, Recording]).await;
    h.hotkey(HotkeyEvent::Pause);
    h.wait_for(|s| s.paused).await;
    h.hotkey(HotkeyEvent::TranslateUp);
    h.hotkey(HotkeyEvent::ComboUp);
    h.hotkey(HotkeyEvent::Pause);
    h.wait_for(|s| !s.paused).await;
    h.hotkey(HotkeyEvent::EditUp);
    h.expect(&[Transcribing, DictationState::Cleaning, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["result "]);
}

#[tokio::test]
async fn voice_modes_keep_per_app_profile_fallback_and_paste_but_ignore_style() {
    for event in [HotkeyEvent::Edit, HotkeyEvent::Translate] {
        let (_server, mut h) = voice_harness(200, "Output").await;
        let mut config = h.settings.current().config.clone();
        config.profiles[1].fallback_profile_id = Some("groq".into());
        config.app_rules.rules.push(opit_core::app_rules::AppRule {
            exe: "editor.exe".into(),
            profile_id: Some("openai".into()),
            style_id: Some("raw".into()),
            paste_method: Some(PasteMethod::Type),
            paste_after: Some(PasteAfter::CtrlEnter),
            ..Default::default()
        });
        h.settings.replace(settings(config));
        *h.foreground.info.lock().unwrap() =
            Some(crate::platform::ForegroundInfo { exe: "editor.exe".into(), title: String::new() });
        h.script("openai", vec![Err(ProviderError::Server(503)), Err(ProviderError::Server(503))]);
        h.script("groq", vec![Ok("write output please")]);
        h.hotkey(event.clone());
        h.expect(&[Transcribing, Recording]).await;
        assert!(!h.overlay.last().text.contains("Raw"));
        // A style shortcut changes the global preference, without changing this mode.
        h.hotkey(HotkeyEvent::Style("formal".into()));
        h.hotkey(event);
        h.expect(&[Transcribing, DictationState::Cleaning, Pasting, Idle]).await;
        let rows = h.history.entries();
        assert_eq!(rows[0].profile_id, "groq");
        assert_eq!(rows[0].cleanup_style_id, None);
        assert_eq!(rows[0].cleanup_model.as_deref(), Some("connection-model"));
        let request = h.paster.requests.lock().unwrap()[0];
        assert_eq!(request.method, PasteMethod::Type);
        assert_eq!(request.after, PasteAfter::CtrlEnter);
    }
}

#[tokio::test]
async fn voice_transcription_failure_never_becomes_a_plain_dictation_retry() {
    let mut config = AppConfig::default();
    config.cleanup.api_key_ref = None;
    let client = Arc::new(CleanupClient::new(config.cleanup.clone(), None).unwrap());
    let mut h = harness(config);
    *h.providers.cleanup.lock().unwrap() = Some(client);
    *h.paster.selection.lock().unwrap() = Some("Keep original".into());
    h.script("groq", vec![Err(ProviderError::Unauthorized(401))]);
    h.hotkey(HotkeyEvent::Edit);
    h.expect(&[Transcribing, Recording]).await;
    h.hotkey(HotkeyEvent::Edit);
    h.expect(&[Transcribing]).await;
    h.expect_error(ErrorKind::InvalidKey).await;
    assert!(!h.handle.status().can_retry);
    assert!(h.paster.texts().is_empty());
    let rows = h.history.entries();
    assert_eq!(rows[0].mode, "edit");
    assert_eq!(rows[0].source_text.as_deref(), Some("Keep original"));
    assert_eq!(rows[0].status, TranscriptStatus::Failed);
}

#[tokio::test]
async fn voice_microphone_failure_during_preparation_is_not_lost() {
    let mut config = AppConfig::default();
    config.cleanup.api_key_ref = None;
    let client = Arc::new(CleanupClient::new(config.cleanup.clone(), None).unwrap());
    let mut h = harness(config);
    *h.providers.cleanup.lock().unwrap() = Some(client);
    let mic = h.mic.clone();
    *h.mic.before_start_returns.lock().unwrap() =
        Some(Box::new(move || mic.emit(CaptureEvent::Failed("device lost".into()))));
    h.hotkey(HotkeyEvent::Edit);
    h.expect(&[Transcribing]).await;
    h.expect_error(ErrorKind::Microphone).await;
    assert_eq!(h.mic.dropped.load(SeqCst), 1);
    assert!(h.paster.texts().is_empty());
}

#[tokio::test]
async fn voice_preparation_cancel_drops_capture_and_does_not_paste() {
    let mut config = AppConfig::default();
    config.cleanup.api_key_ref = None;
    let client = Arc::new(CleanupClient::new(config.cleanup.clone(), None).unwrap());
    let mut h = harness(config);
    *h.providers.cleanup.lock().unwrap() = Some(client);
    let handle = h.handle.clone();
    *h.mic.before_start_returns.lock().unwrap() = Some(Box::new(move || handle.send(Msg::Cancel)));
    h.hotkey(HotkeyEvent::Edit);
    h.expect(&[Transcribing, Cancelled, Idle]).await;
    assert_eq!(h.mic.dropped.load(SeqCst), 1);
    assert!(h.paster.texts().is_empty());
    assert!(h.history.entries().is_empty());
}

#[tokio::test]
async fn voice_preparation_shutdown_drops_capture_without_activating_recording() {
    let mut config = AppConfig::default();
    config.cleanup.api_key_ref = None;
    let client = Arc::new(CleanupClient::new(config.cleanup.clone(), None).unwrap());
    let mut h = harness(config);
    *h.providers.cleanup.lock().unwrap() = Some(client);
    let handle = h.handle.clone();
    *h.mic.before_start_returns.lock().unwrap() = Some(Box::new(move || handle.send(Msg::Shutdown)));
    h.hotkey(HotkeyEvent::Edit);
    h.expect(&[Transcribing, Idle]).await;
    tokio::time::timeout(Duration::from_secs(1), async {
        while h.mic.dropped.load(SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(h.paster.texts().is_empty());
    assert!(h.history.entries().is_empty());
}
