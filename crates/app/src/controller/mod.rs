//! Opit dictation: capture independently of a bounded FIFO. Transcription and paste
//! run serially, including clipboard restore. Waiting items continue after Escape.

mod delivery;
mod handle;
mod live;
mod recording;
#[cfg(target_os = "linux")]
mod session;
mod status;
#[cfg(target_os = "linux")]
pub use session::{PendingResults, ShortcutTest};
#[cfg(test)]
mod tests;
mod transcription;

use std::collections::VecDeque;
use std::mem;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use opit_core::audio::Recording;
use opit_core::audio::encode::EncodeError;
use opit_core::cleanup::{self, CleanupClient, CleanupConfig, CleanupError, CleanupStyle};
use opit_core::config::{HotkeyMode, PasteConfig};
use opit_core::history::NewDictation;
use opit_core::pipeline::{self, PipelineContext, PipelineError, PreparedAudio, Transcript, TranscriptStatus};
use opit_core::provider::retry::RetryWait;
use opit_core::provider::{Profile, Transcriber};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::AbortHandle;
use tracing::{info, warn};

pub use handle::{ControlAction, ControllerHandle, HoldRefusal, Inbox, UpdateHold, WorkGuard, channel};
pub use status::{DictationState, DictationStatus, ErrorKind, meter};

use status::view;
use transcription::prepare_off_thread;

use crate::i18n::Lang;
use crate::platform::{
    Capture, CaptureEvent, CaptureSink, ForegroundApp, Hotkey, HotkeyEvent, HotkeySink, MediaControl, MediaGuard,
    Microphone, Overlay, OverlayAction, OverlaySink, PasteOutcome, PasteRequest, Paster, SecretError, SelectionError,
    Sound, Sounds, Started, Tone,
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

    fn connect_live(
        &self,
        _profile: Profile,
        _terms: Vec<String>,
    ) -> impl Future<
        Output = Result<Box<dyn opit_core::provider::streaming::LiveSession>, opit_core::provider::ProviderError>,
    > + Send {
        async { Err(opit_core::provider::ProviderError::Network("streaming unavailable".into())) }
    }

    fn ask_service(&self) -> Option<Arc<crate::ask::AskService>> {
        None
    }

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
    #[cfg(target_os = "linux")]
    fn pending_results_changed(&self) {}
    /// Called on a blocking worker by the ordered style writer, independently of controller input.
    fn save_style(&self, id: &str) -> Option<CleanupStyle>;
}

pub struct Env<P: Providers> {
    pub foreground: Arc<dyn ForegroundApp>,
    pub settings: SettingsHandle,
    pub providers: Arc<P>,
    pub mic: Arc<dyn Microphone>,
    pub media: Arc<dyn MediaControl>,
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
    LiveText {
        session: u64,
        text: String,
    },
    VoiceReady(u64),
    StyleChanged(CleanupStyle),
    StyleSaved {
        generation: u64,
        style: Option<CleanupStyle>,
    },
    Ask {
        text: Option<String>,
        reply: Option<oneshot::Sender<Result<String, ErrorKind>>>,
    },
    TestAsk {
        config: Box<opit_core::ask::AskConfig>,
        reply: oneshot::Sender<Result<String, ErrorKind>>,
    },
    NewConversation,
    BeginAsk {
        session: u64,
        reply: oneshot::Sender<bool>,
    },
    AskFailed {
        session: u64,
        kind: ErrorKind,
    },
    Asking {
        session: u64,
    },
    Control {
        action: ControlAction,
        reply: oneshot::Sender<Result<DictationStatus, ErrorKind>>,
    },
    Record {
        seconds: u32,
        reply: oneshot::Sender<Result<String, ErrorKind>>,
    },
    /// Tray left click or UI button: start or stop, keeping stopped items clipboard-only.
    Toggle,
    #[cfg(target_os = "linux")]
    ShortcutTest {
        active: bool,
        reply: oneshot::Sender<Result<ShortcutTest, ErrorKind>>,
    },
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
        /// Rules-applied text, retained until the controller accepts cleanup.
        cleanup_input: Option<String>,
    },
    RetrySaved {
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

/// A caller always receives a terminal answer, including cancellation, shutdown,
/// microphone loss or a job removed from the queue.
struct TextReply(Option<oneshot::Sender<Result<String, ErrorKind>>>);

impl TextReply {
    fn finish(mut self, result: Result<String, ErrorKind>) {
        if let Some(reply) = self.0.take() {
            let _ = reply.send(result);
        }
    }
}

impl Drop for TextReply {
    fn drop(&mut self) {
        if let Some(reply) = self.0.take() {
            let _ = reply.send(Err(ErrorKind::Cancelled));
        }
    }
}

/// The actor loop. Spawn it on the app's tokio runtime; it ends after `Msg::Shutdown`.
pub fn run<P: Providers>(
    handle: &ControllerHandle,
    inbox: Inbox,
    env: Env<P>,
) -> impl Future<Output = ()> + Send + 'static {
    let (style_writer, style_requests) = mpsc::unbounded_channel();
    let controller = Controller {
        env,
        tx: handle.tx.clone(),
        status: handle.status.clone(),
        phase: Phase::Idle,
        recording: None,
        preparing: None,
        waiting: VecDeque::new(),
        session: 0,
        retry: None,
        update_holds: 0,
        style_writer,
        style_generation: 0,
        pending_style: None,
        style_fallback: None,
        #[cfg(target_os = "linux")]
        session_state: handle.session_state.clone(),
    };
    controller.serve(inbox, style_requests)
}

struct Clients<C> {
    primary: C,
    fallback: Option<C>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VoiceMode {
    Edit,
    Translate,
}

impl VoiceMode {
    fn name(self) -> &'static str {
        match self {
            Self::Edit => "edit",
            Self::Translate => "translate",
        }
    }
}

// Never Debug: the selection, source and instruction are private user content.
#[derive(Clone)]
struct VoiceContext {
    mode: VoiceMode,
    selection: Option<String>,
    source_text: Option<String>,
    instruction: Option<String>,
    client: Arc<CleanupClient>,
}

struct VoicePrepared<C> {
    voice: VoiceContext,
    clients: Clients<C>,
    started: Started,
}

struct Preparing<C> {
    session: u64,
    mode: VoiceMode,
    settings: Arc<Settings>,
    cancelled: Arc<AtomicBool>,
    microphone_failed: bool,
    task: tokio::task::JoinHandle<Result<VoicePrepared<C>, VoiceStartError>>,
}

enum VoiceStartError {
    Connection,
    Selection(SelectionError),
    Setup(SetupError),
    Microphone,
}

struct Job {
    voice: Option<VoiceContext>,
    ask: bool,
    ask_test: bool,
    text_reply: Option<TextReply>,
    settings: Arc<Settings>,
    paste: PasteConfig,
    cleanup_style: CleanupStyle,
    /// When the user stopped recording (or pressed "Try again"); latency is measured from here.
    started: Instant,
    auto_paste: bool,
    history_id: Option<i64>,
    reply: Option<oneshot::Sender<Result<(), ErrorKind>>>,
}

enum Input {
    Text(String),
    Raw(Recording, Option<live::LiveResult>),
    Prepared(Arc<PreparedAudio>),
}

struct ActiveRecording<C> {
    live: Option<live::LiveRecording>,
    live_text: String,
    voice: Option<VoiceContext>,
    ask: bool,
    text_reply: Option<TextReply>,
    session: u64,
    capture: Box<dyn Capture>,
    media: MediaGuard,
    settings: Arc<Settings>,
    paste: PasteConfig,
    cleanup_style: CleanupStyle,
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
    clients: Option<Clients<C>>,
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
        asking: bool,
    },
    Pasting {
        session: u64,
        job: Job,
        transcript: Transcript,
        audio: Option<Arc<PreparedAudio>>,
    },
}

struct StyleWrite {
    generation: u64,
    id: String,
}

struct PendingStyle {
    generation: u64,
    style: CleanupStyle,
    recording_session: Option<u64>,
}

struct Controller<P: Providers> {
    #[cfg(target_os = "linux")]
    session_state: Arc<Mutex<session::SessionState>>,
    env: Env<P>,
    tx: mpsc::UnboundedSender<Msg>,
    status: Arc<Mutex<DictationStatus>>,
    phase: Phase,
    recording: Option<ActiveRecording<P::Client>>,
    preparing: Option<Preparing<P::Client>>,
    waiting: VecDeque<Pending<P::Client>>,
    session: u64,
    /// Prepared audio of the last failed dictation, for "Try again".
    retry: Option<Arc<PreparedAudio>>,
    /// Live [`UpdateHold`]s; no dictation starts while there is one.
    update_holds: usize,
    style_writer: mpsc::UnboundedSender<StyleWrite>,
    style_generation: u64,
    pending_style: Option<PendingStyle>,
    style_fallback: Option<CleanupStyle>,
}

impl<P: Providers> Controller<P> {
    async fn serve(mut self, mut inbox: Inbox, mut style_requests: mpsc::UnboundedReceiver<StyleWrite>) {
        let events = self.env.events.clone();
        let tx = self.tx.clone();
        // One writer preserves press order and drains queued writes even after actor shutdown.
        // Spawn only once the controller future is polled, since Tauri setup has no Tokio context.
        tokio::spawn(async move {
            while let Some(request) = style_requests.recv().await {
                let events = events.clone();
                let style = tokio::task::spawn_blocking(move || events.save_style(&request.id)).await.ok().flatten();
                let _ = tx.send(Msg::StyleSaved { generation: request.generation, style });
            }
        });
        while let Some(msg) = inbox.0.recv().await {
            if let Msg::Shutdown = msg {
                if let Some(preparing) = self.preparing.take() {
                    preparing.cancelled.store(true, Ordering::Relaxed);
                }
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
            #[cfg(target_os = "linux")]
            if let Msg::Hotkey(ref event) = msg
                && self
                    .session_state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .probe_input(matches!(event, HotkeyEvent::ComboDown))
            {
                continue;
            }
            match msg {
                Msg::VoiceReady(session) => self.voice_ready(session).await,
                Msg::Hotkey(HotkeyEvent::StyleCycle) => self.select_style(None),
                Msg::Hotkey(HotkeyEvent::Style(id)) => self.select_style(Some(id)),
                msg => self.handle(msg),
            }
            self.pump();
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            #[cfg(target_os = "linux")]
            Msg::ShortcutTest { active, reply } => {
                if active
                    && (self.recording.is_some()
                        || self.preparing.is_some()
                        || !self.waiting.is_empty()
                        || !matches!(self.phase, Phase::Idle)
                        || self.update_holds != 0)
                {
                    let _ = reply.send(Err(ErrorKind::Rejected));
                } else {
                    let test =
                        self.session_state.lock().unwrap_or_else(PoisonError::into_inner).set_shortcut_test(active);
                    let _ = reply.send(Ok(test));
                }
            }
            Msg::LiveText { session, text } => {
                if let Some(recording) = self.recording.as_mut().filter(|r| r.session == session && !r.paused) {
                    recording.live_text = live::tail(&text);
                    self.show_recording();
                }
            }
            Msg::VoiceReady(_) => {} // handled by the async actor loop
            Msg::StyleChanged(style) => {
                if self.pending_style.is_some() {
                    // A tray/UI save may overtake a queued hotkey write under config_lock.
                    // Append its choice so older queued writes cannot become the final config.
                    self.queue_style(style);
                } else {
                    self.style_changed(style);
                }
            }
            Msg::StyleSaved { generation, style } => self.style_saved(generation, style),
            Msg::TestAsk { config, reply } => {
                let before = self.session;
                self.ask(Some(opit_core::ask::TEST_REQUEST.into()), Some(reply));
                if self.session != before
                    && let Some(pending) = self.waiting.back_mut()
                {
                    pending.job.ask_test = true;
                    let mut settings = Settings {
                        config: pending.job.settings.config.clone(),
                        rules: pending.job.settings.rules.clone(),
                        lang: pending.job.settings.lang,
                    };
                    settings.config.ask = *config;
                    settings.config.history.enabled = false;
                    pending.job.settings = Arc::new(settings);
                }
            }
            Msg::NewConversation => {
                if let Some(service) = self.env.providers.ask_service() {
                    service.reset();
                }
            }
            Msg::Ask { text, reply } => self.ask(text, reply),
            Msg::AskFailed { session, kind } => {
                if matches!(self.phase, Phase::Transcribing { session: current, .. } if current == session) {
                    let Phase::Transcribing { mut job, .. } = mem::replace(&mut self.phase, Phase::Idle) else {
                        unreachable!()
                    };
                    if let Some(reply) = job.text_reply.take() {
                        reply.finish(Err(kind));
                    }
                    self.fail(kind, job.settings.lang);
                }
            }
            Msg::BeginAsk { session, reply } => {
                let active = matches!(self.phase, Phase::Transcribing { session: current, .. } if current == session);
                if active {
                    self.handle(Msg::Asking { session });
                }
                let _ = reply.send(active);
            }
            Msg::Asking { session } => {
                if let Phase::Transcribing { session: active, asking, .. } = &mut self.phase
                    && *active == session
                {
                    *asking = true;
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        let _ = tx.send(Msg::Asking { session });
                    });
                    self.show_progress();
                    self.set_state(DictationState::Asking);
                }
            }
            Msg::Control { action, reply } => {
                #[cfg(target_os = "linux")]
                if matches!(action, ControlAction::Toggle | ControlAction::Shortcut(_))
                    && self.session_state.lock().unwrap_or_else(PoisonError::into_inner).probe_input(matches!(
                        action,
                        ControlAction::Toggle | ControlAction::Shortcut(HotkeyEvent::ComboDown)
                    ))
                {
                    let _ = reply.send(Ok(self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()));
                    return;
                }
                let result = self.control(action);
                self.pump();
                let status = self.status.lock().unwrap_or_else(PoisonError::into_inner).clone();
                let _ = reply.send(result.map(|()| status));
            }
            Msg::Record { seconds, reply } => {
                let reply = TextReply(Some(reply));
                if !(1..=300).contains(&seconds) || self.recording.is_some() || self.update_holds != 0 {
                    reply.finish(Err(ErrorKind::Rejected));
                } else {
                    self.start();
                    if let Some(r) = self.recording.as_mut() {
                        r.text_reply = Some(reply);
                        let mut config = r.settings.config.clone();
                        config.recording.max_seconds = seconds;
                        r.settings =
                            Arc::new(Settings { config, rules: r.settings.rules.clone(), lang: r.settings.lang });
                        r.limit.abort();
                        r.remaining = Duration::from_secs(u64::from(seconds));
                        let session = r.session;
                        let remaining = r.remaining;
                        let limit = self.arm_limit(session, remaining);
                        self.recording.as_mut().unwrap().limit = limit;
                        self.show_recording();
                    } else {
                        reply.finish(Err(ErrorKind::Rejected));
                    }
                }
            }
            Msg::Toggle => self.toggle(false),
            Msg::Stop => self.stop(false),
            Msg::PauseResume => self.pause_resume(),
            Msg::Cancel => self.cancel(true),
            #[cfg(target_os = "linux")]
            Msg::Overlay(OverlayAction::Cancel) => self.cancel(true),
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
            Msg::Transcribed { session, audio, result, cleanup_input } => {
                self.on_transcribed(session, audio, result, cleanup_input)
            }
            Msg::RetrySaved { session, audio, result } => self.finish_transcription(session, audio, result),
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
        self.preparing.is_none()
            && self.recording.is_none()
            && self.waiting.is_empty()
            && matches!(self.phase, Phase::Idle)
    }

    fn control(&mut self, action: ControlAction) -> Result<(), ErrorKind> {
        match action {
            #[cfg(target_os = "linux")]
            ControlAction::ShortcutProbe => {
                // A late child process is harmless after probe cancellation or window close.
                self.session_state.lock().unwrap_or_else(PoisonError::into_inner).probe_input(true);
            }
            #[cfg(target_os = "linux")]
            ControlAction::Shortcut(event) => match event {
                HotkeyEvent::StyleCycle => self.select_style(None),
                HotkeyEvent::Style(id) => self.select_style(Some(id)),
                event => self.on_hotkey(event),
            },
            ControlAction::Start if self.recording.is_some() => return Err(ErrorKind::Rejected),
            ControlAction::Start => {
                self.start();
                if self.recording.is_none() {
                    return Err(ErrorKind::Rejected);
                }
            }
            ControlAction::Toggle => {
                if self.recording.is_some() {
                    self.stop(false);
                } else {
                    self.start();
                    if self.recording.is_none() {
                        return Err(ErrorKind::Rejected);
                    }
                }
            }
            ControlAction::Stop if self.recording.is_none() => return Err(ErrorKind::Rejected),
            ControlAction::Stop => self.stop(false),
            ControlAction::Cancel => self.cancel(true),
            ControlAction::Pause | ControlAction::Resume => {
                let r = self.recording.as_ref().ok_or(ErrorKind::Rejected)?;
                if r.paused != matches!(action, ControlAction::Pause) {
                    self.pause_resume();
                }
            }
            ControlAction::Retry => {
                if !self.is_idle() || self.retry.is_none() || self.update_holds != 0 {
                    return Err(ErrorKind::Rejected);
                }
                self.retry();
                if matches!(self.phase, Phase::Idle) {
                    return Err(ErrorKind::Rejected);
                }
            }
        }
        Ok(())
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

    fn pump(&mut self) {
        if matches!(self.phase, Phase::Idle)
            && let Some(p) = self.waiting.pop_front()
        {
            if p.salvage {
                let Input::Raw(recording, _) = p.input else { unreachable!() };
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
        let state = if self.preparing.is_some() {
            DictationState::Transcribing
        } else if self.recording.is_some() {
            DictationState::Recording
        } else {
            match self.phase {
                Phase::Idle => DictationState::Idle,
                Phase::Salvaging { .. } => DictationState::Transcribing,
                Phase::Transcribing { asking: true, .. } => DictationState::Asking,
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
            self.preparing.is_some()
                || self.recording.is_some()
                || matches!(self.phase, Phase::Salvaging { .. } | Phase::Transcribing { .. }),
        );
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
