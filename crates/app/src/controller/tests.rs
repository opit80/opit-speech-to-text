use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use opit_core::config::{AppConfig, HotkeyMode};
use opit_core::history::NewDictation;
use opit_core::pipeline::{PreparedAudio, TranscriptStatus};
use opit_core::provider::{Profile, ProviderError, RawTranscript, TranscribeRequest, Transcriber};
use tokio::sync::{Semaphore, mpsc};

use super::*;
use crate::platform::fake::*;
use crate::platform::{HotkeyEvent, MicError, OverlayAction, PasteOutcome, Sound, Tone};
use crate::settings::{Settings, SettingsHandle};

use DictationState::{Cancelled, Idle, Pasting, Recording, Transcribing};

// ---------- scripted providers ----------

struct ProviderState {
    scripts: Mutex<HashMap<String, VecDeque<Result<String, ProviderError>>>>,
    missing_keys: Mutex<HashSet<String>>,
    gate: Arc<Semaphore>,
    calls: AtomicUsize,
    warmups: AtomicUsize,
}

impl ProviderState {
    fn new() -> Self {
        Self {
            scripts: Mutex::default(),
            missing_keys: Mutex::default(),
            gate: Arc::new(Semaphore::new(1_000)),
            calls: AtomicUsize::new(0),
            warmups: AtomicUsize::new(0),
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
            let next = state.scripts.lock().unwrap().get_mut(&id).and_then(VecDeque::pop_front);
            next.unwrap_or_else(|| panic!("unexpected request to {id}"))
                .map(|text| RawTranscript { text, dropped_segments: 0 })
        }
    }
}

impl Providers for TestProviders {
    type Client = ScriptedClient;

    fn client(&self, profile: &Profile) -> Result<ScriptedClient, SetupError> {
        if self.0.missing_keys.lock().unwrap().contains(&profile.id) {
            return Err(SetupError::MissingKey(profile.name.clone()));
        }
        Ok(ScriptedClient { profile: profile.clone(), state: self.0.clone() })
    }

    fn warm_up(&self, _client: &ScriptedClient) {
        self.0.warmups.fetch_add(1, SeqCst);
    }
}

// ---------- history + events doubles ----------

#[derive(Debug, Clone, PartialEq)]
struct Saved {
    profile_id: String,
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
    fn record(&self, entry: &NewDictation<'_>, audio: Option<&PreparedAudio>) -> Result<i64, String> {
        let mut saved = self.saved.lock().unwrap();
        saved.push(Saved {
            profile_id: entry.profile_id.into(),
            raw_text: entry.raw_text.into(),
            text: entry.text.into(),
            status: entry.status,
            has_audio: audio.is_some(),
        });
        Ok(saved.len() as i64)
    }
}

struct FakeEvents {
    tx: mpsc::UnboundedSender<DictationStatus>,
    history: Mutex<Vec<i64>>,
    settings_opened: AtomicUsize,
}

impl UiEvents for FakeEvents {
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
    handle: ControllerHandle,
    states: mpsc::UnboundedReceiver<DictationStatus>,
    settings: SettingsHandle,
    providers: Arc<ProviderState>,
    mic: Arc<FakeMic>,
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

fn harness(config: AppConfig) -> Harness {
    let (tx, states) = mpsc::unbounded_channel();
    let events = Arc::new(FakeEvents { tx, history: Mutex::default(), settings_opened: AtomicUsize::new(0) });
    let providers = Arc::new(ProviderState::new());
    let settings = SettingsHandle::new(settings(config));
    let (mic, hotkey, paster) = (Arc::<FakeMic>::default(), Arc::<FakeHotkey>::default(), Arc::<FakePaster>::default());
    let (overlay, sounds, history) =
        (Arc::<FakeOverlay>::default(), Arc::<FakeSounds>::default(), Arc::<FakeHistory>::default());
    let (handle, inbox) = channel();
    tokio::spawn(run(
        &handle,
        inbox,
        Env {
            settings: settings.clone(),
            providers: TestProviders(providers.clone()),
            mic: mic.clone(),
            hotkey: hotkey.clone(),
            paster: paster.clone(),
            overlay: overlay.clone(),
            sounds: sounds.clone(),
            history: history.clone(),
            events: events.clone(),
            retry_delay: Duration::ZERO,
        },
    ));
    Harness { handle, states, settings, providers, mic, hotkey, paster, overlay, sounds, history, events }
}

impl Harness {
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
        let status = tokio::time::timeout(Duration::from_secs(5), self.states.recv())
            .await
            .expect("timed out waiting for a state change")
            .expect("controller stopped");
        status.state
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

#[tokio::test]
async fn toggle_records_transcribes_pastes_and_saves() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("cloud code'u aç")]);

    h.send(Msg::Toggle);
    h.expect(&[Recording]).await;
    assert!(h.hotkey.escape_captured(), "Esc cancels while recording");
    assert_eq!(h.overlay.last().level, Some(0.0));
    h.send(Msg::Toggle);
    h.expect(&[Transcribing, Pasting, Idle]).await;

    assert_eq!(h.paster.texts(), ["Claude Code'u aç "]);
    assert!(h.paster.pasted.lock().unwrap()[0].1, "clipboard restore follows the config");
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
async fn triggers_while_busy_are_ignored() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Ok("tek")]);
    h.providers.gate.forget_permits(1_000);

    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing]).await;
    h.send(Msg::Toggle);
    h.hotkey(HotkeyEvent::ComboDown);
    h.send(Msg::Retry);
    h.settle().await;
    assert_eq!(h.mic.starts(), 1, "no second recording while transcribing");
    assert_eq!(h.handle.status().state, Transcribing);

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
    assert_eq!(h.overlay.last().text, "No speech detected");
    assert!(h.history.entries().is_empty());
}

#[tokio::test]
async fn a_provider_failure_keeps_the_audio_for_try_again() {
    let mut h = harness(AppConfig::default());
    h.script("groq", vec![Err(ProviderError::Server(500)), Err(ProviderError::Server(500)), Ok("tekrar")]);
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
    h.expect(&[Recording, Transcribing]).await;
    h.expect_error(ErrorKind::Server).await;
    assert!(h.handle.status().can_retry);
    assert_eq!(h.overlay.last().button.map(|b| b.action), Some(OverlayAction::Retry));
    assert!(h.history.entries().is_empty());

    h.send(Msg::Overlay(OverlayAction::Retry));
    h.expect(&[Transcribing, Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["tekrar "]);
    assert_eq!(h.mic.starts(), 1, "the kept audio is resent, nothing is re-recorded");
    assert_eq!(h.providers.calls(), 3);
    assert!(!h.handle.status().can_retry);
    assert_eq!(h.history.entries().len(), 1);
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
    h.send(Msg::Toggle);
    h.send(Msg::Toggle);
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
