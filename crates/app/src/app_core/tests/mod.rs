use std::sync::atomic::Ordering;

use opit_core::config::AppConfig;
use opit_core::learning::{Dismissals, LearningItem, LearningTarget, TermChange};
use opit_core::provider::ProviderError;
use opit_core::rules::pack::CorrectionOutcome;

use crate::controller::ErrorKind;
use crate::platform::{CaptureEvent, HotkeyError, HotkeyEvent};
use crate::settings::Settings;

use opit_core::config::{HotkeyMode, OverlayPosition};
use opit_core::history::{HistoryStore, NewDictation};
use opit_core::pipeline::TranscriptStatus;

use super::*;
use crate::controller::{HistorySink, channel};
use crate::platform::MicError;
use crate::platform::fake::*;

struct Fixture {
    _dir: tempfile::TempDir,
    core: AppCore,
    mic: Arc<FakeMic>,
    media: Arc<FakeMedia>,
    hotkey: Arc<FakeHotkey>,
    overlay: Arc<FakeOverlay>,
    autostart: Arc<FakeAutostart>,
    secrets: Arc<FakeSecrets>,
}

fn fixture() -> Fixture {
    fixture_with(tempfile::tempdir().unwrap(), Vec::new())
}

fn learning_pair(heard: &str, corrected: &str) -> TermChange {
    TermChange { heard: heard.into(), corrected: corrected.into() }
}

fn learning_history(core: &AppCore, text: &str) -> i64 {
    learning_history_in_mode(core, text, "dictation")
}

fn learning_history_in_mode(core: &AppCore, text: &str, mode: &str) -> i64 {
    core.history
        .as_ref()
        .unwrap()
        .store()
        .insert(&NewDictation {
            created_at_ms: 42,
            profile_id: "groq",
            raw_text: "original recognition",
            text,
            status: TranscriptStatus::Ok,
            audio_ms: 1000,
            latency_ms: 50,
            error_code: None,
            language: Some("tr"),
            cleanup_status: None,
            cleanup_model: None,
            cleanup_style_id: None,
            source_text: None,
            instruction: None,
            mode,
            agent: None,
        })
        .unwrap()
}

#[test]
fn learning_history_edit_offers_without_saving_rules_and_uses_latest_text() {
    let f = fixture();
    let id = learning_history(&f.core, "use zeta wrong today");
    let update = f.core.history_update_text(id, "use ZetaSuite today").unwrap();
    assert_eq!(update.suggestions, [learning_pair("zeta wrong", "ZetaSuite")]);
    assert_eq!(update.dictation.raw_text, "original recognition");
    assert_eq!(update.dictation.created_at_ms, 42);
    assert!(!f.core.paths.user_rules.exists());
    assert!(!f.core.paths.root.join("learning.json").exists());
    assert!(f.core.history_update_text(id, "use ZetaSuite today").unwrap().suggestions.is_empty());
    let next = f.core.history_update_text(id, "use OmegaSuite today").unwrap();
    assert_eq!(next.suggestions, [learning_pair("ZetaSuite", "OmegaSuite")]);
    assert_eq!(f.core.history_search("OmegaSuite", 10).unwrap()[0].id, id);
}

#[test]
fn learning_history_edits_non_dictation_modes_without_offering_suggestions() {
    let f = fixture();
    for mode in ["translate", "edit", "ask", "unknown"] {
        let id = learning_history_in_mode(&f.core, "use zeta wrong today", mode);
        let mut expected = f.core.history.as_ref().unwrap().store().get(id).unwrap().unwrap();
        expected.text = "use ZetaSuite today".into();
        let updated = f.core.history_update_text(id, &expected.text).unwrap();
        assert!(updated.suggestions.is_empty(), "mode: {mode}");
        assert_eq!(updated.dictation, expected);
        assert_eq!(f.core.history.as_ref().unwrap().store().get(id).unwrap().unwrap(), expected);
    }
    // Unrelated learning preferences must not prevent editing other modes.
    std::fs::create_dir_all(&f.core.paths.root).unwrap();
    std::fs::write(f.core.paths.root.join("learning.json"), "invalid preferences").unwrap();
    let id = learning_history_in_mode(&f.core, "use zeta wrong today", "translate");
    let updated = f.core.history_update_text(id, "use ZetaSuite today").unwrap();
    assert!(updated.suggestions.is_empty());
    assert_eq!(updated.dictation.text, "use ZetaSuite today");
}

#[test]
fn learning_disabled_still_saves_history_and_allows_explicit_apply() {
    let f = fixture();
    f.core
        .update_config(|config| {
            config.learning.enabled = false;
            Ok(())
        })
        .unwrap();
    let id = learning_history(&f.core, "wrongword");
    // Disabled suggestions do not read a broken preferences file.
    let path = f.core.paths.root.join("learning.json");
    std::fs::write(&path, "bad private data").unwrap();
    let updated = f.core.history_update_text(id, "RightWord").unwrap();
    assert!(updated.suggestions.is_empty());
    assert_eq!(updated.dictation.text, "RightWord");
    std::fs::remove_file(&path).unwrap();
    f.core
        .learning_apply(&[LearningItem {
            change: learning_pair("wrongword", "RightWord"),
            target: LearningTarget::Rule,
        }])
        .unwrap();
    assert_eq!(f.core.settings.current().rules.apply("wrongword"), "RightWord");
}

#[test]
fn learning_apply_preserves_header_recompiles_and_persists_confirmed_choices() {
    let f = fixture();
    f.core
        .save_user_rules(
            "# Personal dictionary\n# Keep this context\nschema: 1\nid: user\nname: Personal\nterms: [zetasuite]\n",
        )
        .unwrap();
    let items = [
        LearningItem { change: learning_pair("zeta wrong", "ZetaSuite"), target: LearningTarget::Rule },
        LearningItem { change: learning_pair("omega wrong", "OmegaSuite"), target: LearningTarget::Term },
        LearningItem { change: learning_pair("zetasuite", "ZetaSuite"), target: LearningTarget::Term },
    ];
    assert!(f.core.learning_apply(&items).unwrap().is_empty());
    let yaml = f.core.user_rules_yaml().unwrap();
    assert!(yaml.starts_with("# Personal dictionary\n# Keep this context\n"));
    let pack = RulePack::from_yaml(&yaml).unwrap();
    assert_eq!(pack.corrections["ZetaSuite"], ["zeta wrong"]);
    assert!(!pack.corrections.contains_key("OmegaSuite"));
    assert_eq!(pack.terms, ["ZetaSuite", "OmegaSuite"]);
    assert_eq!(f.core.settings.current().rules.apply("zeta wrong and omegasuite"), "ZetaSuite and OmegaSuite");
    assert!(f.core.prompt_budget().prompt.unwrap().contains("OmegaSuite"));
    f.core.learning_apply(&items).unwrap();
    assert_eq!(f.core.user_rules_yaml().unwrap(), yaml, "retries are idempotent");
    let saved = Dismissals::load(&f.core.paths.root.join("learning.json")).unwrap();
    assert!(saved.contains(&learning_pair("omega wrong", "OmegaSuite")));
    let id = learning_history(&f.core, "omega wrong");
    assert!(f.core.history_update_text(id, "OmegaSuite").unwrap().suggestions.is_empty());
}

#[test]
fn learning_dismiss_survives_restarting_core_and_batch_errors_do_not_write() {
    let f = fixture();
    let pair = learning_pair("zeta wrong", "ZetaSuite");
    f.core.learning_dismiss(std::slice::from_ref(&pair)).unwrap();
    let yaml = f.core.user_rules_yaml().unwrap();
    let before = std::fs::read(f.core.paths.root.join("learning.json")).unwrap();
    let bad = learning_pair("", "Nope");
    assert_eq!(f.core.learning_dismiss(&[pair.clone(), bad.clone()]).unwrap_err().code, "invalid_input");
    assert_eq!(std::fs::read(f.core.paths.root.join("learning.json")).unwrap(), before);
    assert_eq!(
        f.core
            .learning_apply(&[
                LearningItem { change: pair, target: LearningTarget::Rule },
                LearningItem { change: bad, target: LearningTarget::Term },
            ])
            .unwrap_err()
            .code,
        "invalid_input"
    );
    assert_eq!(f.core.user_rules_yaml().unwrap(), yaml);
    let dir = f._dir;
    drop(f.core);
    let reopened = fixture_with(dir, Vec::new());
    let id = learning_history(&reopened.core, "zeta wrong");
    assert!(reopened.core.history_update_text(id, "ZetaSuite").unwrap().suggestions.is_empty());
}

#[test]
fn learning_validates_inputs_and_conflicts_without_disclosing_text() {
    let f = fixture();
    let id = learning_history(&f.core, "private original");
    for text in ["".into(), " ".into(), "private\0data".into(), "x".repeat(MAX_HISTORY_TEXT_BYTES + 1)] {
        assert_eq!(f.core.history_update_text(id, &text).unwrap_err().code, "invalid_input");
    }
    assert_eq!(f.core.history_update_text(-1, "valid").unwrap_err().code, "invalid_input");
    assert_eq!(f.core.history_update_text(i64::MAX, "valid").unwrap_err().code, "unavailable");
    assert_eq!(f.core.learning_apply(&[]).unwrap_err().code, "invalid_input");
    assert_eq!(f.core.learning_dismiss(&vec![learning_pair("a", "b"); 9]).unwrap_err().code, "invalid_input");
    f.core
        .save_user_rules("schema: 1\nid: user\nname: Personal\ncorrections:\n  ExistingPrivate: [privateheard]\n")
        .unwrap();
    let before = f.core.user_rules_yaml().unwrap();
    let error = f
        .core
        .learning_apply(&[LearningItem {
            change: learning_pair("privateheard", "CorrectedPrivate"),
            target: LearningTarget::Rule,
        }])
        .unwrap_err();
    assert_eq!(error.code, "invalid_input");
    assert!(!error.message.contains("Private") && !error.message.contains("privateheard"));
    assert_eq!(f.core.user_rules_yaml().unwrap(), before);
    let store = f.core.history.as_ref().unwrap().store();
    let failed_id = store
        .insert(&NewDictation {
            status: TranscriptStatus::Failed,
            created_at_ms: 1,
            profile_id: "groq",
            raw_text: "",
            text: "",
            audio_ms: 1,
            latency_ms: 1,
            error_code: Some("network"),
            language: None,
            cleanup_status: None,
            cleanup_model: None,
            cleanup_style_id: None,
            source_text: None,
            instruction: None,
            mode: "dictation",
            agent: None,
        })
        .unwrap();
    drop(store);
    assert_eq!(f.core.history_update_text(failed_id, "valid").unwrap_err().code, "invalid_input");
}

#[test]
fn learning_read_failure_keeps_history_and_rules_unchanged() {
    let f = fixture();
    let id = learning_history(&f.core, "private original");
    std::fs::create_dir_all(&f.core.paths.root).unwrap();
    std::fs::write(f.core.paths.root.join("learning.json"), "private malformed data").unwrap();
    let err = f.core.history_update_text(id, "private corrected").unwrap_err();
    assert_eq!(err.code, "learning");
    assert!(!err.message.contains("private"));
    assert_eq!(f.core.history_recent(1, None).unwrap()[0].text, "private original");
    let item = LearningItem { change: learning_pair("wrong", "Right"), target: LearningTarget::Term };
    assert_eq!(f.core.learning_apply(&[item]).unwrap_err().code, "learning");
    assert!(!f.core.paths.user_rules.exists());
}

#[test]
fn learning_reports_partial_disk_failure_and_retry_is_safe() {
    let f = fixture();
    let blocked_tmp = f.core.paths.root.join("learning.json.tmp");
    std::fs::create_dir_all(&blocked_tmp).unwrap();
    let item = LearningItem { change: learning_pair("wrong", "Right"), target: LearningTarget::Term };
    let error = f.core.learning_apply(std::slice::from_ref(&item)).unwrap_err();
    assert_eq!(error.code, "learning");
    assert!(error.message.starts_with("personal rules were saved"));
    assert_eq!(RulePack::from_yaml(&f.core.user_rules_yaml().unwrap()).unwrap().terms, ["Right"]);
    std::fs::remove_dir(&blocked_tmp).unwrap();
    f.core.learning_apply(std::slice::from_ref(&item)).unwrap();
    assert_eq!(RulePack::from_yaml(&f.core.user_rules_yaml().unwrap()).unwrap().terms, ["Right"]);
    assert!(Dismissals::load(&f.core.paths.root.join("learning.json")).unwrap().contains(&item.change));
}

#[test]
fn concurrent_learning_batches_preserve_both_changes() {
    let f = fixture();
    std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            f.core
                .learning_apply(&[LearningItem {
                    change: learning_pair("zeta wrong", "ZetaSuite"),
                    target: LearningTarget::Rule,
                }])
                .unwrap()
        });
        let b = scope.spawn(|| {
            f.core
                .learning_apply(&[LearningItem {
                    change: learning_pair("omega wrong", "OmegaSuite"),
                    target: LearningTarget::Rule,
                }])
                .unwrap()
        });
        a.join().unwrap();
        b.join().unwrap();
    });
    let pack = RulePack::from_yaml(&f.core.user_rules_yaml().unwrap()).unwrap();
    assert_eq!(pack.corrections.len(), 2);
    let saved = Dismissals::load(&f.core.paths.root.join("learning.json")).unwrap();
    assert!(saved.contains(&learning_pair("zeta wrong", "ZetaSuite")));
    assert!(saved.contains(&learning_pair("omega wrong", "OmegaSuite")));
}

#[tokio::test]
async fn cli_dispatcher_uses_fake_services_validates_and_sanitizes_errors() {
    use crate::ipc::dispatch;
    use opit_ipc::{Command, Request};
    let f = fixture();
    let core = Arc::new(f.core);
    let status = dispatch(core.clone(), Request::new(Command::Status)).await.unwrap();
    assert_eq!(status["state"], "idle");
    let profiles = dispatch(core.clone(), Request::new(Command::ProfilesList)).await.unwrap();
    assert!(profiles.as_array().unwrap().iter().any(|p| p["id"] == "groq" && p["active"] == true));
    assert!(!profiles.to_string().contains("api_key"));
    let selected = dispatch(core.clone(), Request::new(Command::ProfileUse { id: "openai".into() })).await.unwrap();
    assert_eq!(selected["active_profile_id"], "openai");
    assert_eq!(AppConfig::load(&core.paths.config).unwrap().active_profile_id, "openai");
    let error =
        dispatch(core.clone(), Request::new(Command::ProfileUse { id: "sensitive input".into() })).await.unwrap_err();
    assert_eq!(error.code, "invalid_input");
    assert!(!error.message.contains("sensitive input"));
    let rows = dispatch(core.clone(), Request::new(Command::HistoryList { limit: 10, search: None })).await.unwrap();
    assert_eq!(rows, serde_json::json!([]));
    let error = dispatch(core.clone(), Request::new(Command::HistoryRetry { id: 999 })).await.unwrap_err();
    assert_eq!(error.code, "unavailable");
    let mut invalid = Request::new(Command::ProfileUse { id: "groq".into() });
    invalid.version += 1;
    assert_eq!(dispatch(core.clone(), invalid).await.unwrap_err().code, "protocol_version");
    assert_eq!(core.config().active_profile_id, "openai");
    assert_eq!(dispatch(core, Request::new(Command::Version)).await.unwrap()["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn cli_file_dispatch_uses_the_shared_engine_and_requires_real_timestamps() {
    use crate::ipc::dispatch;
    use opit_ipc::{Command, Format, Request};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let f = fixture();
    let audio = f._dir.path().join("clip.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&audio, spec).unwrap();
    for i in 0..16_000 {
        writer.write_sample((5000.0 * (i as f32 * 0.08).sin()) as i16).unwrap();
    }
    writer.finalize().unwrap();
    let server = MockServer::start().await;
    let timed = Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "text":"sample words", "segments":[{"start":0.1,"end":0.8,"text":"sample words"}]
        })))
        .mount_as_scoped(&server)
        .await;
    let mut config = f.core.config();
    config.profiles.truncate(1);
    config.profiles[0].base_url = server.uri();
    config.profiles[0].api_key_ref = None;
    config.profiles[0].apply_rules = false;
    f.core.save_config(config).unwrap();
    let core = Arc::new(f.core);
    let request = |format, timestamps| {
        Request::new(Command::Transcribe { path: audio.to_str().unwrap().into(), format, timestamps, cleanup: false })
    };
    let txt = dispatch(core.clone(), request(Format::Txt, false)).await.unwrap();
    assert_eq!(txt["text"], "sample words");
    let srt = dispatch(core.clone(), request(Format::Srt, false)).await.unwrap();
    assert!(srt["text"].as_str().unwrap().contains("00:00:00,100 --> 00:00:00,800"));
    drop(timed);
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"text":"sample words"})))
        .mount(&server)
        .await;
    assert_eq!(dispatch(core.clone(), request(Format::Txt, true)).await.unwrap_err().code, "file_timestamps");
    assert!(core.history_recent(10, None).unwrap().is_empty(), "file jobs do not add dictation history");
}

fn fixture_with(dir: tempfile::TempDir, notices: Vec<StartupNotice>) -> Fixture {
    let paths = Paths::under(dir.path().join("opit"));
    let settings = SettingsHandle::new(Settings::build(AppConfig::default(), None, Some("en-US")).0);
    let history = Arc::new(HistoryService::with_store(HistoryStore::open_in_memory().unwrap(), paths.audio.clone()));
    let (hotkey, overlay) = (Arc::new(FakeHotkey::default()), Arc::new(FakeOverlay::default()));
    let (autostart, secrets) = (Arc::new(FakeAutostart::default()), Arc::new(FakeSecrets::default()));
    let mic = Arc::new(FakeMic::default());
    let media = Arc::new(FakeMedia::default());
    let platform = Platform {
        foreground: Arc::new(FakeForegroundApp::default()),
        media: media.clone(),
        secrets: secrets.clone(),
        hotkey: hotkey.clone(),
        mic: mic.clone(),
        overlay: overlay.clone(),
        autostart: autostart.clone(),
    };
    let core = AppCore::new(paths, settings, channel().0, Some(history), platform, None, None, notices);
    Fixture { _dir: dir, core, mic, media, hotkey, overlay, autostart, secrets }
}

/// A damaged config.json that start-up could not move aside, because another program
/// holds it open without sharing. Returns the fixture and that lock.
fn fixture_with_a_kept_bad_config() -> (Fixture, std::fs::File) {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::under(dir.path().join("opit"));
    std::fs::create_dir_all(&paths.root).unwrap();
    std::fs::write(&paths.config, "{damaged").unwrap();
    let lock = std::fs::OpenOptions::new().read(true).share_mode(0).open(&paths.config).unwrap();
    let loaded = crate::startup::load_config(&paths.config, 1);
    assert!(matches!(loaded.notice, Some(StartupNotice::ConfigReset { backup: None, .. })));
    (fixture_with(dir, loaded.notice.into_iter().collect()), lock)
}

fn bad_config_backups(paths: &Paths) -> Vec<String> {
    std::fs::read_dir(&paths.root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with("config.json.bad-"))
        .map(|path| std::fs::read_to_string(path).unwrap())
        .collect()
}

#[test]
fn a_config_that_could_not_be_moved_aside_is_never_overwritten() {
    let (f, lock) = fixture_with_a_kept_bad_config();
    let mut config = f.core.config();
    config.paste.trailing_space = false;

    let err = f.core.save_config(config.clone()).unwrap_err();
    assert_eq!(err.code, "config");
    assert!(err.message.contains("moved aside"), "{}", err.message);
    drop(lock);
    assert_eq!(std::fs::read_to_string(&f.core.paths.config).unwrap(), "{damaged");
    assert!(f.core.settings.current().config.paste.trailing_space, "a refused save is not applied");

    // Once the file can be moved, saving backs it up first and then writes.
    let saved = f.core.save_config(config).unwrap();
    assert_eq!(bad_config_backups(&f.core.paths), ["{damaged"]);
    assert_eq!(AppConfig::load(&f.core.paths.config).unwrap(), saved);

    // Done once: later saves just save.
    f.core.set_active_profile("openai").unwrap();
    assert_eq!(bad_config_backups(&f.core.paths).len(), 1);
}

#[test]
fn a_kept_bad_config_that_is_gone_no_longer_blocks_saving() {
    let (f, lock) = fixture_with_a_kept_bad_config();
    drop(lock);
    std::fs::remove_file(&f.core.paths.config).unwrap();
    let saved = f.core.save_config(f.core.config()).unwrap();
    assert_eq!(AppConfig::load(&f.core.paths.config).unwrap(), saved);
    assert!(bad_config_backups(&f.core.paths).is_empty());
}

#[test]
fn save_config_normalizes_writes_and_applies_side_effects() {
    let f = fixture();
    let mut config = f.core.config();
    config.recording.max_seconds = 9_999;
    config.hotkey.mode = HotkeyMode::PushToTalk;
    config.hotkey.keys = vec!["F9".into()];
    config.ui.overlay_position = OverlayPosition::TopCenter;
    config.ui.setup_done = true;

    let saved = f.core.save_config(config).unwrap();
    assert_eq!(saved.recording.max_seconds, 600);
    assert_eq!(AppConfig::load(&f.core.paths.config).unwrap(), saved);
    assert_eq!(f.core.settings.current().config, saved);
    assert_eq!(*f.hotkey.registered.lock().unwrap(), [vec!["F9".to_string()]]);
    assert_eq!(*f.overlay.positions.lock().unwrap(), [OverlayPosition::TopCenter]);
    assert!(*f.autostart.enabled.lock().unwrap(), "finishing the wizard applies the default autostart");
}

#[test]
fn autostart_needs_both_the_setting_and_a_finished_wizard() {
    let mut config = AppConfig::default();
    assert!(config.ui.autostart && !config.ui.setup_done, "defaults: on, wizard not done");
    assert!(!autostart_wanted(&config));
    config.ui.setup_done = true;
    assert!(autostart_wanted(&config));
    config.ui.autostart = false;
    assert!(!autostart_wanted(&config));
}

#[test]
fn autostart_waits_until_the_wizard_is_finished_or_skipped() {
    let f = fixture();
    f.core
        .update_config(|c| {
            c.paste.trailing_space = false;
            Ok(())
        })
        .unwrap();
    assert!(!*f.autostart.enabled.lock().unwrap(), "nothing is registered before the wizard ends");

    f.core
        .update_config(|c| {
            c.ui.setup_done = true;
            Ok(())
        })
        .unwrap();
    assert!(*f.autostart.enabled.lock().unwrap(), "finish/skip applies the (default) choice");

    f.core
        .update_config(|c| {
            c.ui.autostart = false;
            Ok(())
        })
        .unwrap();
    assert!(!*f.autostart.enabled.lock().unwrap());
}

#[test]
fn a_wizard_that_turned_autostart_off_never_registers_it() {
    let f = fixture();
    f.core
        .update_config(|c| {
            c.ui.autostart = false;
            Ok(())
        })
        .unwrap();
    f.core
        .update_config(|c| {
            c.ui.setup_done = true;
            Ok(())
        })
        .unwrap();
    assert!(!*f.autostart.enabled.lock().unwrap());
}

#[test]
fn unchanged_hotkey_is_not_reinstalled() {
    let f = fixture();
    let mut config = f.core.config();
    config.paste.trailing_space = false;
    f.core.save_config(config).unwrap();
    assert!(f.hotkey.registered.lock().unwrap().is_empty());
}

#[test]
fn hotkey_failures_and_pause_are_reported() {
    let f = fixture();
    *f.hotkey.fail_register.lock().unwrap() = Some(HotkeyError::UnknownKey("Hyper".into()));
    f.core.apply_hotkey(&f.core.config());
    assert_eq!(f.core.hotkey_state().error.as_deref(), Some("unknown key name: Hyper"));

    f.core.set_hotkey_paused(true);
    assert!(*f.hotkey.paused.lock().unwrap() && f.core.hotkey_state().paused);
    f.core.set_hotkey_paused(false);
    assert!(!*f.hotkey.paused.lock().unwrap());

    let mut config = f.core.config();
    config.hotkey.enabled = false;
    f.core.save_config(config).unwrap();
    assert!(*f.hotkey.paused.lock().unwrap(), "a disabled shortcut stays paused");
}

#[test]
fn mic_test_forwards_levels_and_stops() {
    let f = fixture();
    let media = f.media.clone();
    let (mic, core) = (f.mic.clone(), Arc::new(f.core));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink_seen = seen.clone();
    let device = core.mic_test_start(None, Arc::new(move |event| sink_seen.lock().unwrap().push(event))).unwrap();
    assert!(!device.is_empty());
    mic.emit(CaptureEvent::Level(0.25));
    mic.emit(CaptureEvent::Failed("unplugged".into()));
    assert_eq!(
        *seen.lock().unwrap(),
        [MicTestEvent::Level { value: 0.25 }, MicTestEvent::Failed { message: "unplugged".into() }]
    );
    core.mic_test_stop();
    assert_eq!(mic.dropped.load(Ordering::SeqCst), 1, "stopping drops (cancels) the capture");
    assert!(media.calls.lock().unwrap().is_empty(), "Settings microphone tests never engage media control");
}

#[test]
fn a_second_mic_test_replaces_the_first() {
    let f = fixture();
    let (mic, core) = (f.mic.clone(), Arc::new(f.core));
    core.mic_test_start(None, Arc::new(|_| {})).unwrap();
    core.mic_test_start(Some("Fake Mic"), Arc::new(|_| {})).unwrap();
    assert_eq!(mic.dropped.load(Ordering::SeqCst), 1);
    assert_eq!(mic.starts(), 2);
}

#[test]
fn mic_test_stops_by_itself_after_the_limit() {
    let f = fixture();
    let (mic, core) = (f.mic.clone(), Arc::new(f.core));
    core.mic_test_start_for(None, Arc::new(|_| {}), Duration::from_millis(30)).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(mic.dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn stopping_during_mic_test_start_discards_the_opened_capture() {
    let f = fixture();
    let (mic, core) = (f.mic.clone(), Arc::new(f.core));
    let weak = Arc::downgrade(&core);
    *mic.before_start_returns.lock().unwrap() = Some(Box::new(move || {
        weak.upgrade().unwrap().mic_test_stop();
    }));

    let err = core.mic_test_start(None, Arc::new(|_| {})).unwrap_err();
    assert_eq!(err.code, "unavailable");
    assert_eq!(err.message, "the microphone test was stopped");
    assert_eq!(mic.dropped.load(Ordering::SeqCst), 1);
    assert!(core.mic_test.lock().unwrap().is_none(), "nothing remains running");
}

#[test]
fn a_newer_mic_test_start_wins_when_the_older_start_returns_last() {
    let f = fixture();
    let (mic, core) = (f.mic.clone(), Arc::new(f.core));
    let weak = Arc::downgrade(&core);
    *mic.before_start_returns.lock().unwrap() = Some(Box::new(move || {
        weak.upgrade().unwrap().mic_test_start(None, Arc::new(|_| {})).unwrap();
    }));

    let err = core.mic_test_start(None, Arc::new(|_| {})).unwrap_err();
    assert_eq!(err.code, "unavailable");
    assert_eq!(mic.starts(), 2);
    assert_eq!(mic.dropped.load(Ordering::SeqCst), 1);
    let generation = core.mic_test.lock().unwrap().as_ref().unwrap().0;
    assert_eq!(generation, core.mic_test_generation.load(Ordering::SeqCst));
    core.mic_test_stop();
    assert_eq!(mic.dropped.load(Ordering::SeqCst), 2);
}

#[test]
fn a_replaced_mic_tests_timer_keeps_the_new_capture_running() {
    let f = fixture();
    let (mic, core) = (f.mic.clone(), Arc::new(f.core));
    core.mic_test_start_for(None, Arc::new(|_| {}), Duration::from_millis(30)).unwrap();
    core.mic_test_start_for(None, Arc::new(|_| {}), Duration::from_secs(20)).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(mic.dropped.load(Ordering::SeqCst), 1);
    assert!(core.mic_test.lock().unwrap().is_some());
    core.mic_test_stop();
    assert_eq!(mic.dropped.load(Ordering::SeqCst), 2);
}

#[test]
fn mic_test_errors_carry_the_microphone_kind() {
    let f = fixture();
    *f.mic.fail_start.lock().unwrap() = Some(MicError::NoDevice);
    let core = Arc::new(f.core);
    let err = core.mic_test_start(None, Arc::new(|_| {})).unwrap_err();
    assert_eq!(err.kind, Some(ErrorKind::Microphone));
}

#[test]
fn hotkey_capture_pauses_the_hook_and_expires() {
    let f = fixture();
    let (hotkey, core) = (f.hotkey.clone(), Arc::new(f.core));
    core.set_hotkey_capture_for(true, Duration::from_millis(30));
    assert!(*hotkey.paused.lock().unwrap());
    assert!(!core.hotkey_state().paused, "capture is not the user's pause");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!*hotkey.paused.lock().unwrap(), "capture expired");
}

#[test]
fn ending_capture_keeps_a_user_pause() {
    let f = fixture();
    let (hotkey, core) = (f.hotkey.clone(), Arc::new(f.core));
    core.set_hotkey_paused(true);
    core.set_hotkey_capture(true);
    core.set_hotkey_capture(false);
    assert!(*hotkey.paused.lock().unwrap());
}

#[test]
fn an_ended_hotkey_captures_timer_keeps_the_restarted_capture_paused() {
    let f = fixture();
    let (hotkey, core) = (f.hotkey.clone(), Arc::new(f.core));
    core.set_hotkey_capture_for(true, Duration::from_millis(30));
    core.set_hotkey_capture(false);
    core.set_hotkey_capture_for(true, Duration::from_secs(30));
    std::thread::sleep(Duration::from_millis(300));
    assert!(*hotkey.paused.lock().unwrap());
    assert!(core.hotkey_capturing.load(Ordering::SeqCst));
    assert!(!core.hotkey_state().paused, "capture is not the user's pause");
    core.set_hotkey_capture(false);
    assert!(!*hotkey.paused.lock().unwrap());
}

#[test]
fn validate_hotkey_uses_the_hook_key_names() {
    assert!(AppCore::validate_hotkey(&["RightCtrl".into(), "F13".into()]).is_ok());
    assert_eq!(AppCore::validate_hotkey(&[]).unwrap_err().code, "invalid_input");
    assert_eq!(AppCore::validate_hotkey(&["Banana".into()]).unwrap_err().code, "invalid_input");
}

#[test]
fn pause_shortcut_is_applied_and_collisions_cannot_change_saved_config() {
    let f = fixture();
    let mut config = f.core.config();
    config.hotkey.pause_keys = vec!["F14".into()];
    let saved = f.core.save_config(config).unwrap();
    assert!(f.hotkey.shortcuts.lock().unwrap().contains(&(HotkeyEvent::Pause, vec!["F14".into()])));
    let before = std::fs::read(&f.core.paths.config).unwrap();
    let mut invalid = saved.clone();
    invalid.hotkey.pause_keys = invalid.hotkey.keys.clone();
    assert_eq!(f.core.save_config(invalid).unwrap_err().code, "invalid_input");
    assert_eq!(f.core.config(), saved);
    assert_eq!(std::fs::read(&f.core.paths.config).unwrap(), before);
}

#[test]
fn set_active_profile_saves_and_returns_the_config() {
    let f = fixture();
    let saved = f.core.set_active_profile("openai").unwrap();
    assert_eq!(saved.active_profile_id, "openai");
    assert_eq!(f.core.config().active_profile_id, "openai");
    let on_disk = AppConfig::load(&f.core.paths.config).unwrap();
    assert_eq!(on_disk.active_profile_id, "openai");
}

#[test]
fn style_switches_persist_without_losing_other_settings_and_reject_unknown_ids() {
    let f = fixture();
    f.core
        .update_config(|c| {
            c.paste.trailing_space = false;
            Ok(())
        })
        .unwrap();
    let saved = f.core.set_active_style("formal").unwrap();
    assert_eq!(saved.cleanup.active_style_id, "formal");
    assert!(!saved.paste.trailing_space);
    assert_eq!(AppConfig::load(&f.core.paths.config).unwrap(), saved);
    assert_eq!(f.core.set_active_style("missing").unwrap_err().code, "invalid_input");
    assert_eq!(f.core.config(), saved);
    assert_eq!(AppConfig::load(&f.core.paths.config).unwrap(), saved);
}

#[test]
fn style_shortcut_edits_register_and_reject_conflicts_before_writing() {
    let f = fixture();
    let mut config = f.core.config();
    config.hotkey.edit_keys = vec!["F18".into()];
    config.hotkey.translate_keys = vec!["F19".into()];
    config.hotkey.style_cycle_keys = vec!["F16".into()];
    config.cleanup.styles.iter_mut().find(|s| s.id == "formal").unwrap().hotkey = vec!["F17".into()];
    let saved = f.core.save_config(config).unwrap();
    let shortcuts = f.hotkey.shortcuts.lock().unwrap().clone();
    assert!(shortcuts.contains(&(HotkeyEvent::Edit, vec!["F18".into()])));
    assert!(shortcuts.contains(&(HotkeyEvent::Translate, vec!["F19".into()])));
    assert!(shortcuts.contains(&(HotkeyEvent::StyleCycle, vec!["F16".into()])));
    assert!(shortcuts.contains(&(HotkeyEvent::Style("formal".into()), vec!["F17".into()])));
    for target in ["dictation", "pause", "ask", "style_cycle", "edit", "translate", "style:clean"] {
        assert_eq!(f.core.validate_shortcut(vec!["F17".into()], target, None).unwrap_err().code, "invalid_input");
    }
    assert!(f.core.validate_shortcut(vec!["F17".into()], "style:formal", None).is_ok());
    assert!(f.core.validate_shortcut(vec![], "style:formal", None).is_ok());
    assert!(f.core.validate_shortcut(vec![], "dictation", None).is_err());
    assert!(f.core.validate_shortcut(vec!["F18".into()], "style:unknown", None).is_err());
    let mut conflict = saved.clone();
    conflict.cleanup.styles[0].hotkey = vec!["F16".into()];
    assert_eq!(f.core.save_config(conflict).unwrap_err().code, "invalid_input");
    assert_eq!(AppConfig::load(&f.core.paths.config).unwrap(), saved);
    let mut removed = saved.clone();
    removed.hotkey.style_cycle_keys.clear();
    f.core.save_config(removed).unwrap();
    assert!(f.core.validate_shortcut(vec!["F16".into()], "style:clean", None).is_ok());
    assert!(f.core.validate_shortcut(vec!["F16".into()], "style:clean", Some(saved)).is_err());
}

#[test]
fn editing_style_metadata_does_not_reregister_held_shortcuts() {
    let f = fixture();
    let mut config = f.core.config();
    config.hotkey.mode = opit_core::config::HotkeyMode::PushToTalk;
    config.hotkey.keys = vec!["F13".into()];
    config.hotkey.edit_keys = vec!["F14".into()];
    config.hotkey.translate_keys = vec!["F15".into()];
    config.cleanup.styles[0].hotkey = vec!["F16".into()];
    f.core.save_config(config.clone()).unwrap();
    let registered = f.hotkey.registered.lock().unwrap().len();
    // Any of these recording shortcuts may be held while Settings saves a style.
    for key in ["F13", "F14", "F15"] {
        let mut held =
            crate::platform::keys::KeyTracker::new(crate::platform::keys::parse_combo(&[key.into()]).unwrap());
        let vk = crate::platform::keys::vk_for(key).unwrap();
        assert_eq!(held.on_key(vk, true), Some(HotkeyEvent::ComboDown));
        config.cleanup.styles[0].name = format!("Name for {key}");
        config.cleanup.styles[0].prompt = format!("Prompt for {key}");
        config.cleanup.styles[0].model = Some("edited-model".into());
        config.cleanup.styles[0].in_cycle = false;
        f.core.save_config(config.clone()).unwrap();
        assert_eq!(f.hotkey.registered.lock().unwrap().len(), registered, "{key}");
        assert_eq!(held.on_key(vk, false), Some(HotkeyEvent::ComboUp));
    }
    config.cleanup.styles.reverse();
    config.hotkey.keys = vec![" f13 ".into()];
    f.core.save_config(config.clone()).unwrap();
    assert_eq!(f.hotkey.registered.lock().unwrap().len(), registered);
    config.cleanup.styles.iter_mut().find(|s| s.id == "raw").unwrap().hotkey = vec!["F17".into()];
    f.core.save_config(config).unwrap();
    assert_eq!(f.hotkey.registered.lock().unwrap().len(), registered + 1);
}

#[test]
fn testing_raw_style_does_not_read_credentials_or_create_a_client() {
    let f = fixture();
    let mut config = f.core.config().cleanup;
    config.active_style_id = "raw".into();
    let (client, _, _) = f.core.cleanup_test_client(config).unwrap();
    assert!(client.is_none());
}

#[test]
fn set_active_profile_rejects_an_unknown_id_without_saving() {
    let f = fixture();
    let err = f.core.set_active_profile("nope").unwrap_err();
    assert_eq!(err.code, "invalid_input");
    assert!(!f.core.paths.config.exists());
}

#[test]
fn update_config_applies_the_change_under_the_lock() {
    let f = fixture();
    let saved = f.core.update_config(|c| {
        c.paste.trailing_space = false;
        Ok(())
    });
    assert!(!saved.unwrap().paste.trailing_space);
    assert!(!f.core.config().paste.trailing_space);
}

#[test]
fn tray_profile_switch_is_saved() {
    let f = fixture();
    f.core.set_active_profile("openai").unwrap();
    assert_eq!(AppConfig::load(&f.core.paths.config).unwrap().active_profile_id, "openai");
    assert_eq!(f.core.set_active_profile("nope").unwrap_err().code, "invalid_input");
}

#[test]
fn api_keys_are_trimmed_and_blank_ones_rejected() {
    let f = fixture();
    assert!(!f.core.has_api_key("groq").unwrap());
    f.core.set_api_key("groq", "  sk-live\n").unwrap();
    assert_eq!(f.secrets.map.lock().unwrap()["groq"], "sk-live");
    assert!(f.core.has_api_key("groq").unwrap());
    assert_eq!(f.core.set_api_key("groq", "   ").unwrap_err().code, "invalid_input");
    f.core.delete_api_key("groq").unwrap();
    assert!(!f.core.has_api_key("groq").unwrap());
}

const USER_YAML: &str = "# my header\n\nschema: 1\nid: user\nname: Me\ncorrections:\n  Claude Code: [cloud code]\n";

#[test]
fn number_preview_follows_saved_config_and_profile_language() {
    let f = fixture();
    assert_eq!(f.core.rules_preview("12 kişi", None).unwrap().text, "12 kişi");
    let mut config = f.core.config();
    config.rules.numbers_as_words = true;
    f.core.save_config(config).unwrap();
    let preview = f.core.rules_preview("12 kişi", None).unwrap();
    assert_eq!(preview.text, "On iki kişi");
    assert_eq!(preview.hits[0].rule.kind, opit_core::rules::RuleKind::Numbers);
}

#[test]
fn render_user_rules_keeps_the_header_and_validates() {
    let pack = AppCore::parse_user_rules(USER_YAML).unwrap();
    let yaml = AppCore::render_user_rules(&pack, USER_YAML).unwrap();
    assert!(yaml.starts_with("# my header\n"));
    assert_eq!(AppCore::parse_user_rules(&yaml).unwrap(), pack);
    let mut bad = pack.clone();
    bad.id = "Not Valid".into();
    assert_eq!(AppCore::render_user_rules(&bad, USER_YAML).unwrap_err().code, "rules");
}

#[test]
fn correction_draft_adds_to_the_saved_file_without_saving() {
    let f = fixture();
    std::fs::create_dir_all(f.core.paths.user_rules.parent().unwrap()).unwrap();
    std::fs::write(&f.core.paths.user_rules, USER_YAML).unwrap();
    let draft = f.core.correction_draft("Claude Code", "klod kod").unwrap();
    assert_eq!(draft.outcome, CorrectionOutcome::AddedVariant);
    assert!(draft.yaml.starts_with("# my header\n"));
    assert!(draft.yaml.contains("klod kod"));
    assert_eq!(std::fs::read_to_string(&f.core.paths.user_rules).unwrap(), USER_YAML);
    let preview = f.core.rules_preview("klod kod açtım", Some(&draft.yaml)).unwrap();
    assert!(preview.text.starts_with("Claude Code"));
}

#[test]
fn correction_draft_works_without_a_user_file_and_rejects_bad_input() {
    let f = fixture();
    let draft = f.core.correction_draft("Opit", "opet").unwrap();
    assert!(draft.yaml.contains("Opit"));
    assert_eq!(f.core.correction_draft("", "x").unwrap_err().code, "invalid_input");
}

#[test]
fn history_audio_reads_only_files_in_the_audio_folder() {
    let f = fixture();
    let history = f.core.history.as_ref().unwrap();
    let entry = |text: &'static str| NewDictation {
        source_text: None,
        instruction: None,
        mode: "dictation",
        agent: None,
        created_at_ms: 1,
        profile_id: "groq",
        raw_text: text,
        text,
        status: TranscriptStatus::Ok,
        audio_ms: 1000,
        latency_ms: 500,
        error_code: None,
        language: None,
        cleanup_status: None,
        cleanup_model: None,
        cleanup_style_id: None,
    };
    // Row with an audio file inside paths.audio.
    let inside = history.store().insert(&entry("a")).unwrap();
    std::fs::create_dir_all(&f.core.paths.audio).unwrap();
    let wav = f.core.paths.audio.join(format!("{inside}.wav"));
    std::fs::write(&wav, b"RIFF1234").unwrap();
    history.store().set_audio_path(inside, &wav.display().to_string()).unwrap();
    assert_eq!(f.core.history_audio(inside).unwrap(), b"RIFF1234");
    // Row pointing outside the audio folder.
    let outside = history.store().insert(&entry("b")).unwrap();
    let elsewhere = f.core.paths.root.join("config.json");
    std::fs::create_dir_all(&f.core.paths.root).unwrap();
    std::fs::write(&elsewhere, "{}").unwrap();
    history.store().set_audio_path(outside, &elsewhere.display().to_string()).unwrap();
    assert_eq!(f.core.history_audio(outside).unwrap_err().code, "invalid_input");
    let traversal = f.core.paths.audio.join("..").join("config.json");
    history.store().set_audio_path(outside, &traversal.display().to_string()).unwrap();
    assert_eq!(f.core.history_audio(outside).unwrap_err().code, "invalid_input");
    let mut failed = entry("");
    failed.status = TranscriptStatus::Failed;
    failed.error_code = Some("network");
    let retry = history.store().insert(&failed).unwrap();
    history.store().set_audio_path(retry, &traversal.display().to_string()).unwrap();
    assert_eq!(f.core.history_retry_audio(retry).unwrap_err().code, "invalid_input");
    // Row without audio, and a missing row.
    let none = history.store().insert(&entry("c")).unwrap();
    assert_eq!(f.core.history_audio(none).unwrap_err().code, "unavailable");
    assert_eq!(f.core.history_audio(9999).unwrap_err().code, "unavailable");
}

#[test]
fn failed_history_wav_loads_after_reopen_and_requires_history_enabled() {
    let mut f = fixture();
    let db = f.core.paths.root.join("retry.db");
    let history = Arc::new(HistoryService::open(&db, f.core.paths.audio.clone()).unwrap());
    let entry = NewDictation {
        source_text: None,
        instruction: None,
        mode: "dictation",
        agent: None,
        created_at_ms: 123,
        profile_id: "old",
        raw_text: "",
        text: "",
        status: TranscriptStatus::Failed,
        audio_ms: 1000,
        latency_ms: 100,
        error_code: Some("timeout"),
        language: None,
        cleanup_status: None,
        cleanup_model: None,
        cleanup_style_id: None,
    };
    let audio = opit_core::pipeline::PreparedAudio { samples: vec![0.1; 16_000], audio_ms: 1000, speech_ms: 1000 };
    let id = history.record(&entry, Some(&audio), 1000).unwrap();
    drop(history);
    f.core.history = Some(Arc::new(HistoryService::open(&db, f.core.paths.audio.clone()).unwrap()));
    let loaded = f.core.history_retry_audio(id).unwrap();
    assert_eq!((loaded.samples.len(), loaded.audio_ms), (16_000, 1000));
    let history = f.core.history.as_ref().unwrap();
    for mode in ["edit", "translate"] {
        let mode_entry = NewDictation { mode, ..entry };
        let mode_id = history.record(&mode_entry, Some(&audio), 1000).unwrap();
        assert_eq!(f.core.history_retry_audio(mode_id).unwrap_err().code, "invalid_input");
    }
    let claim = history.claim_retry(id).unwrap();
    assert!(history.claim_retry(id).is_none());
    drop(claim);
    assert!(history.claim_retry(id).is_some());
    let mut config = f.core.config();
    config.history.enabled = false;
    f.core.settings.replace(Settings::build(config, None, None).0);
    assert_eq!(f.core.history_retry_audio(id).unwrap_err().code, "unavailable");
}

#[test]
fn user_rules_round_trip_and_take_effect() {
    let f = fixture();
    assert_eq!(f.core.user_rules_yaml().unwrap(), USER_PACK_TEMPLATE);
    let yaml = "schema: 1\nid: user\nname: Me\ncorrections:\n  Opit: [opid]\n";
    let warnings = f.core.save_user_rules(yaml).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(f.core.user_rules_yaml().unwrap(), yaml);
    assert_eq!(f.core.settings.current().rules.apply("opid çalışıyor"), "Opit çalışıyor");
}

#[test]
fn invalid_user_rules_are_rejected_and_nothing_is_written() {
    let f = fixture();
    let err = f.core.save_user_rules("schema: 2\nid: user\nname: Me\n").unwrap_err();
    assert_eq!(err.code, "rules");
    assert!(!f.core.paths.user_rules.exists());
}

#[test]
fn preview_uses_a_draft_without_saving_it() {
    let f = fixture();
    let draft = "schema: 1\nid: user\nname: Me\ncorrections:\n  Opit: [opid]\nreplacements:\n  - { from: '(', to: x, regex: true }\n";
    let preview = f.core.rules_preview("opid ve cloud code", Some(draft)).unwrap();
    assert_eq!(preview.text, "Opit ve Claude Code");
    assert_eq!(preview.warnings.len(), 1);
    assert!(preview.hits.len() >= 2);
    assert_eq!(f.core.rules_preview("opid", None).unwrap().text, "opid");
    assert!(f.core.rules_preview("Altyazı M.K.", None).unwrap().hallucination);
}

#[test]
fn prompt_budget_reflects_the_context_sentence() {
    let f = fixture();
    let mut config = f.core.config();
    config.rules.prompt_context = "Yazılım konuşması".into();
    f.core.save_config(config).unwrap();
    let prompt = f.core.prompt_budget();
    assert!(prompt.prompt.unwrap().starts_with("Yazılım konuşması. Geçen terimler:"));
}

#[test]
fn history_commands_work_and_are_capped() {
    let f = fixture();
    let history = f.core.history.clone().unwrap();
    for i in 0..3 {
        let entry = NewDictation {
            source_text: None,
            instruction: None,
            mode: "dictation",
            agent: None,
            created_at_ms: i,
            profile_id: "groq",
            raw_text: "github",
            text: "GitHub",
            status: TranscriptStatus::Ok,
            audio_ms: 1,
            latency_ms: 1,
            error_code: None,
            language: None,
            cleanup_status: None,
            cleanup_model: None,
            cleanup_style_id: None,
        };
        history.record(&entry, None, 1000).unwrap();
    }
    assert_eq!(f.core.history_recent(2, None).unwrap().len(), 2);
    assert_eq!(f.core.history_search("git", 10).unwrap().len(), 3);
    let first = f.core.history_recent(1, None).unwrap()[0].id;
    f.core.history_delete(first).unwrap();
    assert_eq!(f.core.history_recent(10, None).unwrap().len(), 2);
    f.core.history_clear().unwrap();
    assert!(f.core.history_recent(10, None).unwrap().is_empty());
}

#[test]
fn startup_overlay_picks_the_most_important_notice() {
    let f = fixture();
    assert_eq!(f.core.startup_overlay(), None);
    *f.hotkey.fail_register.lock().unwrap() = Some(HotkeyError::Install("denied".into()));
    f.core.apply_hotkey(&f.core.config());
    assert_eq!(f.core.startup_overlay().unwrap().text, "Shortcut unavailable — use the tray icon");
    f.core.notices.lock().unwrap().push(StartupNotice::UserRulesBroken { reason: "x".into() });
    f.core.notices.lock().unwrap().push(StartupNotice::ConfigReset { backup: None, reason: "y".into() });
    let view = f.core.startup_overlay().unwrap();
    assert_eq!((view.tone, view.text.as_str()), (Tone::Warning, "Settings file was damaged; defaults loaded"));
}

#[test]
fn notices_are_handed_out_once() {
    let f = fixture();
    f.core.notices.lock().unwrap().push(StartupNotice::UserRulesBroken { reason: "x".into() });
    assert_eq!(f.core.take_notices().len(), 1);
    assert!(f.core.take_notices().is_empty());
}

#[test]
fn cleanup_errors_keep_safe_kinds_for_all_consumers() {
    use opit_core::cleanup::CleanupError;
    for (error, kind, wire) in [
        (CleanupError::MissingKey, ErrorKind::MissingKey, "missing_key"),
        (CleanupError::InvalidKey, ErrorKind::InvalidKey, "invalid_key"),
        (CleanupError::PaymentRequired, ErrorKind::PaymentRequired, "payment_required"),
        (CleanupError::RateLimited, ErrorKind::RateLimited, "rate_limited"),
        (CleanupError::Server, ErrorKind::Server, "server"),
        (CleanupError::Network, ErrorKind::Network, "network"),
        (CleanupError::Timeout, ErrorKind::Timeout, "timeout"),
        (CleanupError::Rejected, ErrorKind::Rejected, "rejected"),
        (CleanupError::BadResponse, ErrorKind::BadResponse, "bad_response"),
        (CleanupError::Truncated, ErrorKind::BadResponse, "bad_response"),
    ] {
        assert_eq!(ErrorKind::from(error), kind);
        let command = CommandError::from(error);
        assert_eq!((command.code, command.kind), ("cleanup", Some(kind)));
        assert_eq!(serde_json::to_value(command).unwrap()["kind"], wire);
    }
}

#[test]
fn provider_errors_keep_their_kind_for_the_ui() {
    let err = CommandError::from(ProviderError::Unauthorized(401));
    assert_eq!((err.code, err.kind), ("provider", Some(ErrorKind::InvalidKey)));
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(json["kind"], "invalid_key");
}
