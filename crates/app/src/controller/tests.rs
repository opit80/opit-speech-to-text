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
    profile_gates: Mutex<HashMap<String, Arc<Semaphore>>>,
    calls: AtomicUsize,
    warmups: AtomicUsize,
    cleanup: Mutex<Option<Arc<CleanupClient>>>,
}

impl ProviderState {
    fn new() -> Self {
        Self {
            scripts: Mutex::default(),
            missing_keys: Mutex::default(),
            gate: Arc::new(Semaphore::new(1_000)),
            profile_gates: Mutex::default(),
            calls: AtomicUsize::new(0),
            warmups: AtomicUsize::new(0),
            cleanup: Mutex::default(),
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
            })
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

    fn cleanup_client(&self, _config: &CleanupConfig) -> Result<Arc<CleanupClient>, CleanupError> {
        self.0.cleanup.lock().unwrap().clone().ok_or(CleanupError::MissingKey)
    }

    fn warm_up(&self, _client: &ScriptedClient) {
        self.0.warmups.fetch_add(1, SeqCst);
    }
}

// ---------- history + events doubles ----------

#[derive(Debug, Clone, PartialEq)]
struct Saved {
    profile_id: String,
    language: Option<String>,
    cleanup_status: Option<cleanup::CleanupStatus>,
    cleanup_model: Option<String>,
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
            language: entry.language.map(str::to_string),
            cleanup_status: entry.cleanup_status,
            cleanup_model: entry.cleanup_model.map(str::to_string),
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
        row.raw_text = entry.raw_text.into();
        row.text = entry.text.into();
        row.status = entry.status;
        row.has_audio = keep_audio;
        Ok(())
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
async fn cleanup_success_is_saved_and_escape_during_cleanup_never_pastes() {
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
        let mut h = harness(config);
        *h.providers.cleanup.lock().unwrap() = Some(client);
        h.script("groq", vec![Ok("um today today we have a meeting")]);
        h.send(Msg::Toggle);
        h.expect(&[Recording]).await;
        h.send(Msg::Toggle);
        h.expect(&[Transcribing, DictationState::Cleaning]).await;
        if cancel {
            h.hotkey(HotkeyEvent::Escape);
            h.expect(&[Cancelled, Idle]).await;
            tokio::time::sleep(Duration::from_millis(550)).await;
            assert!(h.paster.texts().is_empty());
            assert!(h.history.entries().is_empty());
        } else {
            h.expect(&[Pasting, Idle]).await;
            assert_eq!(h.paster.texts(), ["Today we have a meeting. "]);
            assert_eq!(h.history.entries()[0].cleanup_status, Some(cleanup::CleanupStatus::Ok));
            assert_eq!(h.history.entries()[0].raw_text, "um today today we have a meeting");
        }
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
}

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
    assert!(h.handle.hold_for_update().await.is_none(), "recording");
    h.send(Msg::Toggle);
    h.expect(&[Transcribing]).await;
    assert!(h.handle.hold_for_update().await.is_none(), "transcribing");

    h.providers.gate.add_permits(1_000);
    h.expect(&[Pasting, Idle]).await;
    assert_eq!(h.paster.texts(), ["bitti "], "the refused hold changed nothing");
    assert!(h.handle.hold_for_update().await.is_some());
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
    assert!(h.handle.hold_for_update().await.is_none(), "waiting recordings block updates");
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
    assert!(h.handle.hold_for_update().await.is_some());
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
    h.mic.append_samples(&[0.9; 100]);
    h.mic.emit(CaptureEvent::Level(0.9));
    h.settle().await;
    assert_eq!(h.overlay.last(), frozen);
    tokio::time::advance(Duration::from_secs(30)).await;
    h.settle().await;
    assert_eq!(h.mic.finished.load(SeqCst), 0);
    h.hotkey(HotkeyEvent::Pause);
    h.expect(&[Recording]).await;
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
    assert!(h.handle.hold_for_update().await.is_none());
    h.hotkey(HotkeyEvent::Escape);
    h.settle().await;
    assert_eq!(h.handle.status().waiting, 1, "Escape cannot interrupt a paste in progress");
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    wait_for(&mut h, Idle, 0).await;
    assert_eq!(h.paster.texts(), ["first ", "second "]);
    assert_eq!(h.history.entries().len(), 2);
}
