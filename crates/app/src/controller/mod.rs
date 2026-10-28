//! Opit dictation: capture independently of a bounded FIFO. Transcription and paste
//! run serially, including clipboard restore. Waiting items continue after Escape.

mod status;
#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::mem;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use opit_core::audio::Recording;
use opit_core::audio::encode::EncodeError;
use opit_core::cleanup::{self, CleanupClient, CleanupConfig, CleanupError};
use opit_core::config::HotkeyMode;
use opit_core::history::NewDictation;
use opit_core::pipeline::{self, PipelineContext, PipelineError, PreparedAudio, Transcript, TranscriptStatus};
use opit_core::provider::retry::RetryWait;
use opit_core::provider::{Profile, Transcriber};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::AbortHandle;
use tracing::{info, warn};

pub use status::{DictationState, DictationStatus, ErrorKind};

use crate::i18n::Lang;
use crate::platform::{
    Capture, CaptureEvent, CaptureSink, Hotkey, HotkeyEvent, HotkeySink, Microphone, Overlay, OverlayAction,
    OverlayButton, OverlaySink, OverlayView, PasteOutcome, Paster, RecordingTiming, SecretError, Sound, Sounds, Tone,
};
use crate::settings::{Settings, SettingsHandle};

const PASTED_HIDE: Duration = Duration::from_millis(1_500);
const INFO_HIDE: Duration = Duration::from_millis(2_500);
const CANCEL_HIDE: Duration = Duration::from_millis(1_000);
const WARNING_HIDE: Duration = Duration::from_secs(5);
const ERROR_HIDE: Duration = Duration::from_secs(5);
const ERROR_WITH_BUTTON_HIDE: Duration = Duration::from_secs(8);
const MAX_WAITING: usize = 3;

/// Builds provider clients for profiles. The real implementation caches one HTTP client
/// per profile so dictations reuse pooled connections.
pub trait Providers: Send + Sync + 'static {
    type Client: Transcriber + Send + Sync + 'static;

    fn client(&self, profile: &Profile) -> Result<Self::Client, SetupError>;

    fn cleanup_client(&self, _config: &CleanupConfig) -> Result<Arc<CleanupClient>, CleanupError> {
        Err(CleanupError::MissingKey)
    }

    /// Best-effort connection warm-up while the user is still speaking.
    fn warm_up(&self, client: &Self::Client);
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SetupError {
    #[error("no API key is stored for profile {0}")]
    MissingKey(String),
    #[error(transparent)]
    Secret(#[from] SecretError),
    #[error("the provider client could not be created: {0}")]
    Client(String),
}

impl SetupError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            SetupError::MissingKey(_) => ErrorKind::MissingKey,
            SetupError::Secret(_) => ErrorKind::Credentials,
            SetupError::Client(_) => ErrorKind::Network,
        }
    }
}

pub trait HistorySink: Send + Sync {
    /// Stores one dictation (and its 16 kHz audio when given); returns the row id.
    fn record(&self, entry: &NewDictation<'_>, audio: Option<&PreparedAudio>, max_entries: u32) -> Result<i64, String>;
    fn replace(&self, id: i64, entry: &NewDictation<'_>, keep_audio: bool) -> Result<(), String>;
}

/// Notifications for the tray and the web UI.
pub trait UiEvents: Send + Sync {
    fn status_changed(&self, status: &DictationStatus);
    fn history_added(&self, id: i64);
    fn open_settings(&self);
}

pub struct Env<P: Providers> {
    pub settings: SettingsHandle,
    pub providers: P,
    pub mic: Arc<dyn Microphone>,
    pub hotkey: Arc<dyn Hotkey>,
    pub paster: Arc<dyn Paster>,
    pub overlay: Arc<dyn Overlay>,
    pub sounds: Arc<dyn Sounds>,
    pub history: Arc<dyn HistorySink>,
    pub events: Arc<dyn UiEvents>,
    /// Wait before the one retry of a failed request (500 ms in the app, zero in tests).
    pub retry_delay: Duration,
}

/// Controller input. Deliberately not `Debug`: `Transcribed` carries transcript text,
/// which must never reach a log line.
pub enum Msg {
    /// Tray left click or UI button: start or stop, keeping stopped items clipboard-only.
    Toggle,
    Stop,
    PauseResume,
    Cancel,
    Retry,
    RetryHistory {
        id: i64,
        audio: Arc<PreparedAudio>,
        reply: oneshot::Sender<Result<(), ErrorKind>>,
    },
    RetryProgress {
        session: u64,
        progress: Option<RetryWait>,
    },
    HistoryStored(i64),
    Cleaning {
        session: u64,
    },
    Hotkey(HotkeyEvent),
    Overlay(OverlayAction),
    Shutdown,
    Capture {
        session: u64,
        event: CaptureEvent,
    },
    LimitReached {
        session: u64,
    },
    /// The microphone was lost mid-recording; what the prepare step made of the audio so far.
    Salvaged {
        session: u64,
        result: Result<PreparedAudio, PipelineError>,
    },
    Transcribed {
        session: u64,
        audio: Option<Arc<PreparedAudio>>,
        result: Result<Transcript, PipelineError>,
    },
    Pasted {
        session: u64,
        outcome: PasteOutcome,
    },
    /// An update wants to install (the installer closes the app). Answers true, and refuses new
    /// dictations until released, only while idle. See [`ControllerHandle::hold_for_update`].
    HoldForUpdate(oneshot::Sender<bool>),
    ReleaseUpdateHold,
}

#[derive(Clone)]
pub struct ControllerHandle {
    tx: mpsc::UnboundedSender<Msg>,
    status: Arc<Mutex<DictationStatus>>,
}

pub struct Inbox(mpsc::UnboundedReceiver<Msg>);

/// While alive, new dictations and "Try again" are refused; dropping it lifts the hold.
pub struct UpdateHold {
    tx: mpsc::UnboundedSender<Msg>,
}

impl Drop for UpdateHold {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::ReleaseUpdateHold);
    }
}

/// Creates the handle first so the overlay and hotkey sinks can exist before the
/// controller that consumes them.
pub fn channel() -> (ControllerHandle, Inbox) {
    let (tx, rx) = mpsc::unbounded_channel();
    (ControllerHandle { tx, status: Arc::default() }, Inbox(rx))
}

/// The actor loop. Spawn it on the app's tokio runtime; it ends after `Msg::Shutdown`.
pub fn run<P: Providers>(
    handle: &ControllerHandle,
    inbox: Inbox,
    env: Env<P>,
) -> impl Future<Output = ()> + Send + 'static {
    let controller = Controller {
        env,
        tx: handle.tx.clone(),
        status: handle.status.clone(),
        phase: Phase::Idle,
        recording: None,
        waiting: VecDeque::new(),
        session: 0,
        retry: None,
        update_holds: 0,
    };
    controller.serve(inbox)
}

impl ControllerHandle {
    /// Never blocks; messages sent after shutdown are dropped.
    pub fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
    }

    pub fn status(&self) -> DictationStatus {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub async fn retry_history(&self, id: i64, audio: PreparedAudio) -> Result<(), ErrorKind> {
        let (reply, result) = oneshot::channel();
        self.send(Msg::RetryHistory { id, audio: Arc::new(audio), reply });
        result.await.unwrap_or(Err(ErrorKind::Cancelled))
    }

    /// Stops new dictations so an update can install. `None` while a dictation is recording,
    /// waiting, transcribing or pasting (or after shutdown). The controller answers in order with its
    /// other messages, so no dictation can slip in between the answer and the hold.
    pub async fn hold_for_update(&self) -> Option<UpdateHold> {
        let (reply, answer) = oneshot::channel();
        self.send(Msg::HoldForUpdate(reply));
        answer.await.unwrap_or(false).then(|| UpdateHold { tx: self.tx.clone() })
    }

    pub fn hotkey_sink(&self) -> HotkeySink {
        let tx = self.tx.clone();
        Arc::new(move |event| {
            let _ = tx.send(Msg::Hotkey(event));
        })
    }

    pub fn overlay_sink(&self) -> OverlaySink {
        let tx = self.tx.clone();
        Arc::new(move |action| {
            let _ = tx.send(Msg::Overlay(action));
        })
    }
}

struct Clients<C> {
    cleanup: Option<Result<Arc<CleanupClient>, CleanupError>>,
    primary: C,
    fallback: Option<C>,
}

struct Job {
    settings: Arc<Settings>,
    /// When the user stopped recording (or pressed "Try again"); latency is measured from here.
    started: Instant,
    auto_paste: bool,
    history_id: Option<i64>,
    reply: Option<oneshot::Sender<Result<(), ErrorKind>>>,
}

enum Input {
    Raw(Recording),
    Prepared(Arc<PreparedAudio>),
}

struct ActiveRecording<C> {
    session: u64,
    capture: Box<dyn Capture>,
    settings: Arc<Settings>,
    clients: Clients<C>,
    limit: AbortHandle,
    remaining: Duration,
    resumed_at: tokio::time::Instant,
    paused: bool,
    level: f32,
    label: &'static str,
}

struct Pending<C> {
    session: u64,
    job: Job,
    clients: Clients<C>,
    input: Input,
    salvage: bool,
}

enum Phase {
    Idle,
    /// The microphone was lost; the audio captured so far is being prepared so it can be kept
    /// for "Try again". Published as Transcribing unless another recording is active.
    Salvaging {
        session: u64,
        settings: Arc<Settings>,
        task: AbortHandle,
    },
    Transcribing {
        session: u64,
        job: Job,
        task: AbortHandle,
        cancel: watch::Sender<bool>,
        progress: Option<RetryWait>,
        cleaning: bool,
    },
    Pasting {
        session: u64,
        job: Job,
        transcript: Transcript,
        audio: Option<Arc<PreparedAudio>>,
    },
}

struct Controller<P: Providers> {
    env: Env<P>,
    tx: mpsc::UnboundedSender<Msg>,
    status: Arc<Mutex<DictationStatus>>,
    phase: Phase,
    recording: Option<ActiveRecording<P::Client>>,
    waiting: VecDeque<Pending<P::Client>>,
    session: u64,
    /// Prepared audio of the last failed dictation, for "Try again".
    retry: Option<Arc<PreparedAudio>>,
    /// Live [`UpdateHold`]s; no dictation starts while there is one.
    update_holds: usize,
}

impl<P: Providers> Controller<P> {
    async fn serve(mut self, mut inbox: Inbox) {
        while let Some(msg) = inbox.0.recv().await {
            if let Msg::Shutdown = msg {
                if let Some(r) = self.recording.take() {
                    r.limit.abort();
                }
                self.waiting.clear();
                self.cancel(false);
                self.env.overlay.hide();
                self.env.hotkey.set_capture_escape(false);
                self.set_state(DictationState::Idle);
                break;
            }
            self.handle(msg);
            self.pump();
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Toggle => self.toggle(false),
            Msg::Stop => self.stop(false),
            Msg::PauseResume => self.pause_resume(),
            Msg::Cancel => self.cancel(true),
            Msg::Retry | Msg::Overlay(OverlayAction::Retry) => self.retry(),
            Msg::RetryHistory { id, audio, reply } => self.retry_history(id, audio, reply),
            Msg::Cleaning { session } => {
                if let Phase::Transcribing { session: active, cleaning, .. } = &mut self.phase
                    && *active == session
                {
                    *cleaning = true;
                    self.show_progress();
                    self.set_state(DictationState::Cleaning);
                }
            }
            Msg::HistoryStored(id) => self.env.events.history_added(id),
            Msg::RetryProgress { session, progress } => {
                if let Phase::Transcribing { session: active, progress: current, .. } = &mut self.phase
                    && *active == session
                {
                    *current = progress;
                    self.show_progress();
                    self.set_state(DictationState::Transcribing);
                }
            }
            Msg::Overlay(OverlayAction::OpenSettings) => self.env.events.open_settings(),
            Msg::Hotkey(event) => self.on_hotkey(event),
            Msg::Capture { session, event } => self.on_capture(session, event),
            Msg::LimitReached { session } => {
                if self
                    .recording
                    .as_ref()
                    .is_some_and(|r| r.session == session && !r.paused && r.resumed_at.elapsed() >= r.remaining)
                {
                    info!(session, "recording limit reached; sending");
                    self.stop(true);
                }
            }
            Msg::Salvaged { session, result } => self.on_salvaged(session, result),
            Msg::Transcribed { session, audio, result } => self.on_transcribed(session, audio, result),
            Msg::Pasted { session, outcome } => self.on_pasted(session, outcome),
            Msg::HoldForUpdate(reply) => {
                let idle = self.is_idle();
                // A hold whose caller is already gone would never be released.
                if reply.send(idle).is_ok() && idle {
                    self.update_holds += 1;
                }
            }
            Msg::ReleaseUpdateHold => self.update_holds = self.update_holds.saturating_sub(1),
            Msg::Shutdown => {}
        }
    }

    fn is_idle(&self) -> bool {
        self.recording.is_none() && self.waiting.is_empty() && matches!(self.phase, Phase::Idle)
    }

    fn toggle(&mut self, auto_paste: bool) {
        if self.recording.is_some() {
            self.stop(auto_paste);
        } else {
            self.start();
        }
    }

    fn on_hotkey(&mut self, event: HotkeyEvent) {
        let mode = self
            .recording
            .as_ref()
            .map_or_else(|| self.env.settings.current().config.hotkey.mode, |r| r.settings.config.hotkey.mode);
        let recording = self.recording.is_some();
        match (event, mode) {
            (HotkeyEvent::Escape, _) => self.cancel(true),
            (HotkeyEvent::Pause, _) => self.pause_resume(),
            (HotkeyEvent::ComboDown, HotkeyMode::Toggle) => self.toggle(true),
            (HotkeyEvent::LoneCtrl, HotkeyMode::Toggle) if recording => self.stop(true),
            (HotkeyEvent::ComboDown, HotkeyMode::PushToTalk) if !recording => self.start(),
            (HotkeyEvent::ComboUp, HotkeyMode::PushToTalk) if recording => self.stop(true),
            _ => {}
        }
    }

    fn clients(&self, settings: &Settings) -> Result<Clients<P::Client>, SetupError> {
        let config = &settings.config;
        let profile = config.active_profile().ok_or_else(|| SetupError::Client("no active profile".into()))?;
        let primary = self.env.providers.client(profile)?;
        let fallback = config.fallback_for(profile).and_then(|p| match self.env.providers.client(p) {
            Ok(client) => Some(client),
            Err(err) => {
                warn!(profile = %p.id, error = %err, "fallback profile unavailable");
                None
            }
        });
        let uses_cleanup = profile.apply_cleanup || config.fallback_for(profile).is_some_and(|p| p.apply_cleanup);
        let cleanup =
            (config.cleanup.enabled && uses_cleanup).then(|| self.env.providers.cleanup_client(&config.cleanup));
        Ok(Clients { primary, fallback, cleanup })
    }

    /// True (and says so on the overlay) while an update is installing.
    fn held_for_update(&self) -> bool {
        if self.update_holds == 0 {
            return false;
        }
        info!("an update is installing; dictation not started");
        let lang = self.env.settings.current().lang;
        self.show(view(Tone::Neutral, lang.update_installing(), Some(INFO_HIDE)));
        true
    }

    fn start(&mut self) {
        if self.held_for_update() {
            return;
        }
        if self.waiting.len() >= MAX_WAITING {
            let lang = self.env.settings.current().lang;
            self.env.overlay.show(view(Tone::Warning, lang.queue_full(), Some(INFO_HIDE)));
            return;
        }
        let settings = self.env.settings.current();
        let clients = match self.clients(&settings) {
            Ok(clients) => clients,
            Err(err) => {
                warn!(error = %err, "cannot start dictation");
                return self.fail(err.kind(), settings.lang);
            }
        };
        self.retry = None;
        self.session += 1;
        let session = self.session;
        let tx = self.tx.clone();
        let sink: CaptureSink = Arc::new(move |event| {
            let _ = tx.send(Msg::Capture { session, event });
        });
        let started = match self.env.mic.start(settings.config.recording.microphone.as_deref(), sink) {
            Ok(started) => started,
            Err(err) => {
                warn!(error = %err, "microphone failed to start");
                return self.fail(ErrorKind::Microphone, settings.lang);
            }
        };
        if started.fell_back {
            warn!(device = %started.device, "configured microphone unavailable; using the default device");
        }
        self.env.providers.warm_up(&clients.primary);
        if let Some(Ok(client)) = &clients.cleanup {
            let client = client.clone();
            tokio::spawn(async move {
                client.warm_up().await;
            });
        }

        let max = Duration::from_secs(u64::from(settings.config.recording.max_seconds));
        let limit = self.arm_limit(session, max);

        let label = if started.fell_back { settings.lang.listening_default_mic() } else { settings.lang.listening() };
        self.env.hotkey.set_capture_escape(true);
        self.play(Sound::Start);
        info!(session, device = %started.device, profile = %settings.config.active_profile_id, "recording started");
        self.recording = Some(ActiveRecording {
            session,
            capture: started.capture,
            settings,
            clients,
            limit,
            label,
            remaining: max,
            resumed_at: tokio::time::Instant::now(),
            paused: false,
            level: 0.0,
        });
        self.show_recording();
        self.set_state(DictationState::Recording);
    }

    fn stop(&mut self, auto_paste: bool) {
        let Some(r) = self.recording.take() else { return };
        r.limit.abort();
        let auto_paste = auto_paste && self.env.paster.can_auto_paste();
        let recording = r.capture.finish();
        self.waiting.push_back(Pending {
            session: r.session,
            job: Job { settings: r.settings, started: Instant::now(), auto_paste, history_id: None, reply: None },
            clients: r.clients,
            input: Input::Raw(recording),
            salvage: false,
        });
    }

    fn retry(&mut self) {
        if !self.is_idle() {
            return;
        }
        let Some(audio) = self.retry.clone() else {
            return;
        };
        if self.held_for_update() {
            return;
        }
        let settings = self.env.settings.current();
        let clients = match self.clients(&settings) {
            Ok(clients) => clients,
            Err(err) => {
                warn!(error = %err, "cannot retry");
                return self.fail(err.kind(), settings.lang);
            }
        };
        self.session += 1;
        info!(session = self.session, "retrying the last dictation");
        self.env.hotkey.set_capture_escape(true);
        let job = Job {
            settings,
            started: Instant::now(),
            auto_paste: self.env.paster.can_auto_paste(),
            history_id: None,
            reply: None,
        };
        self.begin_transcription(self.session, job, clients, Input::Prepared(audio));
    }

    fn begin_transcription(&mut self, session: u64, job: Job, clients: Clients<P::Client>, input: Input) {
        let settings = job.settings.clone();
        self.show(view(Tone::Busy, settings.lang.transcribing(), None));
        let tx = self.tx.clone();
        let retry_delay = self.env.retry_delay;
        let task_settings = settings.clone();
        let worker_tx = tx.clone();
        let history = self.env.history.clone();
        let history_id = job.history_id;
        let started = job.started;
        let (cancel, cancel_rx) = watch::channel(false);
        let worker = tokio::spawn(async move {
            let tx = worker_tx;
            let audio = match input {
                Input::Prepared(audio) => audio,
                Input::Raw(recording) => {
                    match prepare_off_thread(recording, task_settings.config.recording.clone()).await {
                        Ok(audio) => Arc::new(audio),
                        Err(err) => {
                            let _ = tx.send(Msg::Transcribed { session, audio: None, result: Err(err) });
                            return;
                        }
                    }
                }
            };
            let ctx = PipelineContext {
                primary: &clients.primary,
                fallback: clients.fallback.as_ref(),
                rules: &task_settings.rules,
                prompt_context: &task_settings.config.rules.prompt_context,
                retry_delay,
                numbers_as_words: task_settings.config.rules.numbers_as_words,
            };
            let mut result = pipeline::transcribe_with_wait(&audio, &ctx, |progress, delay| {
                wait_for_retry(tx.clone(), session, cancel_rx.clone(), progress, delay)
            })
            .await;
            if let Ok(transcript) = &mut result {
                let profile = task_settings
                    .config
                    .profiles
                    .iter()
                    .find(|p| p.id == transcript.profile_id)
                    .unwrap_or_else(|| clients.primary.profile());
                let config = &task_settings.config.cleanup;
                if cleanup::eligible(transcript, config, profile) {
                    let _ = tx.send(Msg::Cleaning { session });
                    let ui_language = if task_settings.lang == Lang::Tr { "tr" } else { "en" };
                    let language = cleanup::prompt_language(
                        transcript.detected_language.as_deref(),
                        &profile.language,
                        ui_language,
                    );
                    let system = cleanup::system_prompt(
                        config,
                        language,
                        &task_settings.rules,
                        &task_settings.config.rules.prompt_context,
                    );
                    transcript.cleanup_model = Some(config.model.clone());
                    let client = match &clients.cleanup {
                        Some(Ok(client)) => Ok(client.as_ref()),
                        Some(Err(error)) => Err(*error),
                        None => Err(CleanupError::MissingKey),
                    };
                    cleanup::apply(transcript, client, &system, &task_settings.rules, profile.apply_rules).await;
                }
            }
            if task_settings.config.history.enabled && (result.is_err() || history_id.is_some()) {
                let saved = save_transcription(
                    history,
                    task_settings,
                    history_id,
                    audio.clone(),
                    result.clone(),
                    started.elapsed(),
                )
                .await;
                match saved {
                    Ok(id) => {
                        let _ = tx.send(Msg::HistoryStored(id));
                    }
                    Err(_) => {
                        warn!("history write failed");
                        if history_id.is_some() {
                            result = Err(PipelineError::Encode(EncodeError("history retry could not be saved".into())));
                        }
                    }
                }
            }
            let _ = tx.send(Msg::Transcribed { session, audio: Some(audio), result });
        });
        let task = worker.abort_handle();
        tokio::spawn(async move {
            if worker.await.is_err() {
                let result = Err(PipelineError::Encode(EncodeError("transcription worker stopped".into())));
                let _ = tx.send(Msg::Transcribed { session, audio: None, result });
            }
        });
        self.phase = Phase::Transcribing { session, job, task, cancel, progress: None, cleaning: false };
        self.set_state(DictationState::Transcribing);
    }

    fn retry_history(&mut self, id: i64, audio: Arc<PreparedAudio>, reply: oneshot::Sender<Result<(), ErrorKind>>) {
        let duplicate = matches!(&self.phase, Phase::Transcribing { job, .. } | Phase::Pasting { job, .. } if job.history_id == Some(id))
            || self.waiting.iter().any(|p| p.job.history_id == Some(id));
        let settings = self.env.settings.current();
        // A live recording already owns one future queue slot.
        let occupied = self.waiting.len() + usize::from(self.recording.is_some());
        if duplicate || occupied >= MAX_WAITING || self.update_holds != 0 || !settings.config.history.enabled {
            let _ = reply.send(Err(ErrorKind::Rejected));
            return;
        }
        let clients = match self.clients(&settings) {
            Ok(clients) => clients,
            Err(error) => {
                let _ = reply.send(Err(error.kind()));
                return;
            }
        };
        self.session += 1;
        self.waiting.push_back(Pending {
            session: self.session,
            job: Job { settings, started: Instant::now(), auto_paste: false, history_id: Some(id), reply: Some(reply) },
            clients,
            input: Input::Prepared(audio),
            salvage: false,
        });
    }

    fn show_progress(&self) {
        let lang = self.env.settings.current().lang;
        let text = match &self.phase {
            Phase::Transcribing { cleaning: true, .. } => lang.cleaning().into(),
            Phase::Transcribing { progress: Some(progress), .. } => lang.retry_wait(*progress),
            _ => lang.transcribing().into(),
        };
        self.show(view(Tone::Busy, text, None));
    }

    fn on_capture(&mut self, session: u64, event: CaptureEvent) {
        let Some(r) = self.recording.as_mut().filter(|r| r.session == session) else { return };
        match event {
            CaptureEvent::Level(level) => {
                if !r.paused {
                    r.level = level;
                    self.show_recording();
                }
            }
            CaptureEvent::Failed(reason) => {
                warn!(session, %reason, "microphone stopped during recording");
                let r = self.recording.take().unwrap();
                r.limit.abort();
                // Keep what was said so far (spec §8): prepare it, but send nothing on our own.
                let recording = r.capture.finish();
                self.waiting.push_back(Pending {
                    session,
                    job: Job {
                        settings: r.settings,
                        started: Instant::now(),
                        auto_paste: false,
                        history_id: None,
                        reply: None,
                    },
                    clients: r.clients,
                    input: Input::Raw(recording),
                    salvage: true,
                });
            }
        }
    }

    fn on_salvaged(&mut self, session: u64, result: Result<PreparedAudio, PipelineError>) {
        if !matches!(self.phase, Phase::Salvaging { session: current, .. } if current == session) {
            return; // cancelled meanwhile
        }
        let Phase::Salvaging { settings, .. } = mem::replace(&mut self.phase, Phase::Idle) else {
            unreachable!("checked above")
        };
        match result {
            Ok(audio) => {
                info!(session, audio_ms = audio.audio_ms, "audio before the microphone loss kept for Try again");
                self.retry = Some(Arc::new(audio));
            }
            Err(err) => info!(session, reason = %err, "nothing worth keeping before the microphone loss"),
        }
        self.fail(ErrorKind::Microphone, settings.lang);
    }

    fn on_transcribed(
        &mut self,
        session: u64,
        audio: Option<Arc<PreparedAudio>>,
        result: Result<Transcript, PipelineError>,
    ) {
        if !matches!(self.phase, Phase::Transcribing { session: current, .. } if current == session) {
            return; // cancelled meanwhile
        }
        let Phase::Transcribing { mut job, .. } = mem::replace(&mut self.phase, Phase::Idle) else {
            unreachable!("checked above")
        };
        let lang = job.settings.lang;
        let transcript = match result {
            Ok(transcript) => transcript,
            Err(PipelineError::TooShort) => {
                info!(session, "recording too short; not sent");
                return self.finish_info(lang.too_short());
            }
            Err(PipelineError::NoSpeech { levels }) => {
                info!(session, "no speech detected; not sent");
                return self.finish_info(&lang.no_speech(levels.speech_db));
            }
            Err(err) => {
                warn!(session, kind = ?ErrorKind::from(&err), "transcription failed");
                if let Some(reply) = job.reply.take() {
                    let _ = reply.send(Err(ErrorKind::from(&err)));
                }
                self.retry = audio;
                return self.fail(ErrorKind::from(&err), lang);
            }
        };
        self.retry = None;
        info!(
            session,
            status = transcript.status.as_str(),
            profile = %transcript.profile_id,
            fallback = transcript.used_fallback,
            request_ms = transcript.request_ms,
            chars = transcript.text.chars().count(),
            rule_hits = transcript.hits.len(),
            "transcribed"
        );
        if transcript.status != TranscriptStatus::Ok || transcript.text.is_empty() {
            self.record(&job, &transcript, audio.as_deref());
            if let Some(reply) = job.reply.take() {
                let _ = reply.send(Ok(()));
            }
            return self.finish_info(lang.empty());
        }

        let paste = &job.settings.config.paste;
        let text = if paste.trailing_space { format!("{} ", transcript.text) } else { transcript.text.clone() };
        let restore = job.history_id.is_none() && paste.restore_clipboard;
        let auto_paste = job.auto_paste;
        let paster = self.env.paster.clone();
        let tx = self.tx.clone();
        tokio::task::spawn_blocking(move || {
            let outcome = paster.paste(&text, restore, auto_paste);
            let _ = tx.send(Msg::Pasted { session, outcome });
        });
        self.phase = Phase::Pasting { session, job, transcript, audio };
        self.set_state(DictationState::Pasting);
    }

    fn on_pasted(&mut self, session: u64, outcome: PasteOutcome) {
        if !matches!(self.phase, Phase::Pasting { session: current, .. } if current == session) {
            return;
        }
        let Phase::Pasting { mut job, transcript, audio, .. } = mem::replace(&mut self.phase, Phase::Idle) else {
            unreachable!("checked above")
        };
        let lang = job.settings.lang;
        let elapsed = job.started.elapsed();
        self.record(&job, &transcript, audio.as_deref());
        if let Some(reply) = job.reply.take() {
            let _ = reply.send(if matches!(outcome, PasteOutcome::Failed(_)) { Err(ErrorKind::Paste) } else { Ok(()) });
        }
        match outcome {
            PasteOutcome::Pasted => {
                info!(session, total_ms = elapsed.as_millis() as u64, "pasted");
                self.play(Sound::Done);
                self.show(view(Tone::Success, lang.pasted(elapsed.as_secs_f32()), Some(PASTED_HIDE)));
            }
            PasteOutcome::ClipboardOnly => {
                info!(session, "text left on the clipboard");
                self.play(Sound::Done);
                self.show(view(Tone::Warning, lang.clipboard_only(), Some(WARNING_HIDE)));
            }
            PasteOutcome::Failed(reason) => {
                warn!(session, %reason, "paste failed");
                return self.fail_with_message(
                    ErrorKind::Paste,
                    lang,
                    lang.paste_failed(job.settings.config.history.enabled).to_string(),
                );
            }
        }
        if let Some(error) = transcript.cleanup_error {
            let message = lang.cleanup_failed(error, matches!(outcome, PasteOutcome::ClipboardOnly));
            self.show(view(Tone::Warning, message, Some(WARNING_HIDE)));
        }
        self.set_state(DictationState::Idle);
    }

    fn record(&self, job: &Job, transcript: &Transcript, audio: Option<&PreparedAudio>) {
        let history = &job.settings.config.history;
        if !history.enabled || job.history_id.is_some() {
            return;
        }
        let entry = NewDictation {
            created_at_ms: now_ms(),
            profile_id: &transcript.profile_id,
            language: transcript.language.as_deref(),
            cleanup_status: transcript.cleanup_status,
            cleanup_model: transcript.cleanup_model.as_deref(),
            raw_text: &transcript.raw_text,
            text: &transcript.text,
            status: transcript.status,
            audio_ms: transcript.audio_ms,
            latency_ms: job.started.elapsed().as_millis() as u64,
            error_code: None,
        };
        let audio = if history.save_audio { audio } else { None };
        match self.env.history.record(&entry, audio, history.max_entries) {
            Ok(id) => self.env.events.history_added(id),
            Err(err) => warn!(error = %err, "history write failed"),
        }
    }

    /// `announce: false` is the quiet variant used on shutdown.
    fn cancel(&mut self, announce: bool) {
        if let Some(r) = self.recording.take() {
            r.limit.abort();
            drop(r.capture);
            if announce {
                self.play(Sound::Cancel);
            }
            if matches!(self.phase, Phase::Idle) && self.waiting.is_empty() {
                if announce {
                    self.show(view(Tone::Neutral, r.settings.lang.cancelled(), Some(CANCEL_HIDE)));
                    self.set_state(DictationState::Cancelled);
                    self.set_state(DictationState::Idle);
                }
            } else {
                self.show(view(Tone::Busy, r.settings.lang.transcribing(), None));
            }
            return;
        }
        let lang = match mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Idle => return,
            pasting @ Phase::Pasting { .. } => {
                self.phase = pasting; // Ctrl+V is already on its way
                return;
            }
            Phase::Salvaging { session, settings, task } => {
                task.abort();
                info!(session, "cancelled after the microphone was lost");
                settings.lang
            }
            Phase::Transcribing { session, task, job, cancel, progress, cleaning } => {
                if progress.is_some() {
                    let _ = cancel.send(true);
                    self.phase = Phase::Transcribing { session, task, job, cancel, progress, cleaning };
                    return;
                }
                task.abort();
                info!(session, "transcription cancelled");
                job.settings.lang
            }
        };
        if announce {
            self.play(Sound::Cancel);
            self.show(view(Tone::Neutral, lang.cancelled(), Some(CANCEL_HIDE)));
            self.set_state(DictationState::Cancelled);
        } else {
            self.env.overlay.hide();
        }
        self.set_state(DictationState::Idle);
    }

    fn fail(&mut self, kind: ErrorKind, lang: Lang) {
        self.fail_with_message(kind, lang, lang.error(kind).to_string());
    }

    fn fail_with_message(&mut self, kind: ErrorKind, lang: Lang, message: String) {
        let button = if kind.needs_settings() {
            Some(OverlayButton { action: OverlayAction::OpenSettings, label: lang.open_settings().into() })
        } else if self.retry.is_some() {
            Some(OverlayButton { action: OverlayAction::Retry, label: lang.retry().into() })
        } else {
            None
        };
        let hide_after = if button.is_some() { ERROR_WITH_BUTTON_HIDE } else { ERROR_HIDE };
        self.show(OverlayView {
            tone: Tone::Error,
            text: message.clone(),
            level: None,
            recording: None,
            button,
            hide_after: Some(hide_after),
        });
        self.play(Sound::Error);
        self.set_state(DictationState::Error { kind, message });
        self.set_state(DictationState::Idle);
    }

    fn finish_info(&mut self, text: &str) {
        self.show(view(Tone::Neutral, text, Some(INFO_HIDE)));
        self.set_state(DictationState::Idle);
    }

    fn play(&self, sound: Sound) {
        if self.env.settings.current().config.ui.sound_feedback {
            self.env.sounds.play(sound);
        }
    }

    fn arm_limit(&self, session: u64, remaining: Duration) -> AbortHandle {
        let tx = self.tx.clone();
        let deadline = tokio::time::Instant::now() + remaining;
        tokio::spawn(async move {
            tokio::time::sleep_until(deadline).await;
            let _ = tx.send(Msg::LimitReached { session });
        })
        .abort_handle()
    }

    fn pause_resume(&mut self) {
        let Some(r) = self.recording.as_mut() else { return };
        r.limit.abort();
        if !r.paused {
            r.remaining = r.remaining.saturating_sub(r.resumed_at.elapsed());
        }
        r.paused = !r.paused;
        r.capture.set_paused(r.paused);
        r.resumed_at = tokio::time::Instant::now();
        if !r.paused {
            let (session, remaining) = (r.session, r.remaining);
            let limit = self.arm_limit(session, remaining);
            self.recording.as_mut().unwrap().limit = limit;
        }
        self.show_recording();
        self.set_state(DictationState::Recording);
    }

    fn pump(&mut self) {
        if matches!(self.phase, Phase::Idle)
            && let Some(p) = self.waiting.pop_front()
        {
            if p.salvage {
                let Input::Raw(recording) = p.input else { unreachable!() };
                let session = p.session;
                let tx = self.tx.clone();
                let recording_config = p.job.settings.config.recording.clone();
                let task = tokio::spawn(async move {
                    let result = prepare_off_thread(recording, recording_config).await;
                    let _ = tx.send(Msg::Salvaged { session, result });
                })
                .abort_handle();
                self.phase = Phase::Salvaging { session, settings: p.job.settings, task };
            } else {
                self.begin_transcription(p.session, p.job, p.clients, p.input);
            }
        }
        let state = if self.recording.is_some() {
            DictationState::Recording
        } else {
            match self.phase {
                Phase::Idle => DictationState::Idle,
                Phase::Salvaging { .. } => DictationState::Transcribing,
                Phase::Transcribing { cleaning: true, .. } => DictationState::Cleaning,
                Phase::Transcribing { .. } => DictationState::Transcribing,
                Phase::Pasting { .. } => DictationState::Pasting,
            }
        };
        let status = self.status.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if status.state != state
            || status.waiting != self.waiting.len()
            || status.can_retry != (self.is_idle() && self.retry.is_some())
        {
            if self.recording.is_some() {
                self.show_recording();
            } else if !matches!(self.phase, Phase::Idle) {
                self.show_progress();
            }
            self.set_state(state);
        }
        self.env.hotkey.set_capture_escape(
            self.recording.is_some() || matches!(self.phase, Phase::Salvaging { .. } | Phase::Transcribing { .. }),
        );
    }

    fn show_recording(&self) {
        if let Some(r) = &self.recording {
            let label = if r.paused { r.settings.lang.recording_paused() } else { r.label };
            let max = Duration::from_secs(u64::from(r.settings.config.recording.max_seconds));
            let remaining = if r.paused { r.remaining } else { r.remaining.saturating_sub(r.resumed_at.elapsed()) };
            let mut view = listening(&r.settings.lang.with_waiting(label, self.waiting.len()), r.level);
            view.recording = Some(RecordingTiming {
                session: r.session,
                elapsed: max.saturating_sub(remaining),
                updated_at: Instant::now(),
                paused: r.paused,
            });
            self.env.overlay.show(view);
        }
    }

    fn show(&self, mut view: OverlayView) {
        if self.recording.is_some() {
            self.show_recording();
        } else {
            view.text = self.env.settings.current().lang.with_waiting(&view.text, self.waiting.len());
            self.env.overlay.show(view);
        }
    }

    fn set_state(&mut self, state: DictationState) {
        let state = if self.recording.is_some() { DictationState::Recording } else { state };
        if state == DictationState::Idle && !self.waiting.is_empty() {
            return;
        }
        let status = DictationStatus {
            state,
            can_retry: self.is_idle() && self.retry.is_some(),
            waiting: self.waiting.len(),
            paused: self.recording.as_ref().is_some_and(|r| r.paused),
            retry_wait: match &self.phase {
                Phase::Transcribing { progress, .. } => *progress,
                _ => None,
            },
        };
        if *self.status.lock().unwrap_or_else(PoisonError::into_inner) == status {
            return;
        }
        *self.status.lock().unwrap_or_else(PoisonError::into_inner) = status.clone();
        self.env.events.status_changed(&status);
    }
}

/// Publishes only safe metadata. The cancellation signal also wakes a sleeping wait.
async fn wait_for_retry(
    tx: mpsc::UnboundedSender<Msg>,
    session: u64,
    mut cancelled: watch::Receiver<bool>,
    progress: RetryWait,
    delay: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + delay;
    loop {
        if *cancelled.borrow() {
            return false;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        if !delay.is_zero() {
            let mut progress = progress;
            progress.seconds = remaining.as_millis().div_ceil(1000) as u64;
            let _ = tx.send(Msg::RetryProgress { session, progress: Some(progress) });
        }
        tokio::select! {
            _ = cancelled.changed() => { return false; }
            _ = tokio::time::sleep(remaining.min(Duration::from_secs(1))) => {}
        }
    }
    if !delay.is_zero() {
        let _ = tx.send(Msg::RetryProgress { session, progress: None });
    }
    true
}

/// Failure audio ignores save_audio, but obeys history.enabled at the worker call site.
/// A History retry replaces its existing row before any clipboard operation.
async fn save_transcription(
    history: Arc<dyn HistorySink>,
    settings: Arc<Settings>,
    history_id: Option<i64>,
    audio: Arc<PreparedAudio>,
    result: Result<Transcript, PipelineError>,
    elapsed: Duration,
) -> Result<i64, String> {
    tokio::task::spawn_blocking(move || {
        let failure_code = match &result {
            Err(PipelineError::Provider(error)) => Some(error.code()),
            Err(PipelineError::RetryCancelled) => Some("cancelled"),
            Err(_) => Some("audio"),
            Ok(_) => None,
        };
        let transcript = result.ok();
        let entry = NewDictation {
            created_at_ms: now_ms(),
            profile_id: transcript.as_ref().map_or(settings.config.active_profile_id.as_str(), |t| &t.profile_id),
            raw_text: transcript.as_ref().map_or("", |t| &t.raw_text),
            text: transcript.as_ref().map_or("", |t| &t.text),
            status: transcript.as_ref().map_or(TranscriptStatus::Failed, |t| t.status),
            audio_ms: audio.audio_ms,
            latency_ms: elapsed.as_millis() as u64,
            error_code: failure_code,
            language: transcript.as_ref().and_then(|t| t.language.as_deref()),
            cleanup_status: transcript.as_ref().and_then(|t| t.cleanup_status),
            cleanup_model: transcript.as_ref().and_then(|t| t.cleanup_model.as_deref()),
        };
        match history_id {
            Some(id) => {
                history.replace(id, &entry, failure_code.is_some() || settings.config.history.save_audio).map(|()| id)
            }
            None => history.record(&entry, Some(&audio), settings.config.history.max_entries),
        }
    })
    .await
    .map_err(|_| "history worker stopped".to_string())?
}

/// Runs the CPU-heavy prepare step (resample + silence gate) on the blocking pool.
async fn prepare_off_thread(
    recording: Recording,
    config: opit_core::config::RecordingConfig,
) -> Result<PreparedAudio, PipelineError> {
    tokio::task::spawn_blocking(move || pipeline::prepare_with_config(&recording, &config))
        .await
        .unwrap_or_else(|join| Err(PipelineError::Encode(EncodeError(join.to_string()))))
}

fn view(tone: Tone, text: impl Into<String>, hide_after: Option<Duration>) -> OverlayView {
    OverlayView { tone, text: text.into(), level: None, recording: None, button: None, hide_after }
}

fn listening(label: &str, rms: f32) -> OverlayView {
    OverlayView {
        tone: Tone::Busy,
        text: label.into(),
        level: Some(meter(rms)),
        recording: None,
        button: None,
        hide_after: None,
    }
}

/// Maps microphone RMS to a 0..1 bar on a -60..0 dB scale, so normal speech (≈ -20 dB)
/// fills about two thirds of the bar instead of a sliver.
pub fn meter(rms: f32) -> f32 {
    if !rms.is_finite() || rms <= 0.0 {
        return 0.0;
    }
    ((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
