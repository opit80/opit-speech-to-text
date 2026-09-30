//! Dictation state machine: Idle → Recording → Transcribing → Pasting → Idle, plus the
//! transient Cancelled and Error states. One actor task owns every piece of dictation
//! state; the hotkey hook, tray, overlay and UI only send it messages, so two dictations
//! can never overlap.

mod status;
#[cfg(test)]
mod tests;

use std::mem;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use opit_core::audio::Recording;
use opit_core::audio::encode::EncodeError;
use opit_core::config::HotkeyMode;
use opit_core::history::NewDictation;
use opit_core::pipeline::{self, PipelineContext, PipelineError, PreparedAudio, Transcript, TranscriptStatus};
use opit_core::provider::{Profile, Transcriber};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use tracing::{info, warn};

pub use status::{DictationState, DictationStatus, ErrorKind};

use crate::i18n::Lang;
use crate::platform::{
    Capture, CaptureEvent, CaptureSink, Hotkey, HotkeyEvent, HotkeySink, Microphone, Overlay, OverlayAction,
    OverlayButton, OverlaySink, OverlayView, PasteOutcome, Paster, SecretError, Sound, Sounds, Tone,
};
use crate::settings::{Settings, SettingsHandle};

const PASTED_HIDE: Duration = Duration::from_millis(1_500);
const INFO_HIDE: Duration = Duration::from_millis(2_500);
const CANCEL_HIDE: Duration = Duration::from_millis(1_000);
const WARNING_HIDE: Duration = Duration::from_secs(5);
const ERROR_HIDE: Duration = Duration::from_secs(5);
const ERROR_WITH_BUTTON_HIDE: Duration = Duration::from_secs(8);

/// Builds provider clients for profiles. The real implementation caches one HTTP client
/// per profile so dictations reuse pooled connections.
pub trait Providers: Send + Sync + 'static {
    type Client: Transcriber + Send + Sync + 'static;

    fn client(&self, profile: &Profile) -> Result<Self::Client, SetupError>;

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
    fn record(&self, entry: &NewDictation<'_>, audio: Option<&PreparedAudio>) -> Result<i64, String>;
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
    /// Tray left click, UI button, or the toggle-mode hotkey: start or stop.
    Toggle,
    Stop,
    Cancel,
    Retry,
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
}

#[derive(Clone)]
pub struct ControllerHandle {
    tx: mpsc::UnboundedSender<Msg>,
    status: Arc<Mutex<DictationStatus>>,
}

pub struct Inbox(mpsc::UnboundedReceiver<Msg>);

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
        session: 0,
        retry: None,
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
    primary: C,
    fallback: Option<C>,
}

struct Job {
    settings: Arc<Settings>,
    /// When the user stopped recording (or pressed "Try again"); latency is measured from here.
    started: Instant,
}

enum Input {
    Raw(Recording),
    Prepared(Arc<PreparedAudio>),
}

enum Phase<C> {
    Idle,
    Recording {
        session: u64,
        capture: Box<dyn Capture>,
        settings: Arc<Settings>,
        clients: Clients<C>,
        limit: AbortHandle,
        label: &'static str,
    },
    /// The microphone was lost; the audio captured so far is being prepared so it can be kept
    /// for "Try again". Brief, so the published state stays Recording until the error.
    Salvaging {
        session: u64,
        settings: Arc<Settings>,
        task: AbortHandle,
    },
    Transcribing {
        session: u64,
        job: Job,
        task: AbortHandle,
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
    phase: Phase<P::Client>,
    session: u64,
    /// Prepared audio of the last failed dictation, for "Try again".
    retry: Option<Arc<PreparedAudio>>,
}

impl<P: Providers> Controller<P> {
    async fn serve(mut self, mut inbox: Inbox) {
        while let Some(msg) = inbox.0.recv().await {
            if let Msg::Shutdown = msg {
                self.cancel(false);
                break;
            }
            self.handle(msg);
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Toggle => self.toggle(),
            Msg::Stop => self.stop(),
            Msg::Cancel => self.cancel(true),
            Msg::Retry | Msg::Overlay(OverlayAction::Retry) => self.retry(),
            Msg::Overlay(OverlayAction::OpenSettings) => self.env.events.open_settings(),
            Msg::Hotkey(event) => self.on_hotkey(event),
            Msg::Capture { session, event } => self.on_capture(session, event),
            Msg::LimitReached { session } => {
                if self.recording_session() == Some(session) {
                    info!(session, "recording limit reached; sending");
                    self.stop();
                }
            }
            Msg::Salvaged { session, result } => self.on_salvaged(session, result),
            Msg::Transcribed { session, audio, result } => self.on_transcribed(session, audio, result),
            Msg::Pasted { session, outcome } => self.on_pasted(session, outcome),
            Msg::Shutdown => {}
        }
    }

    fn recording_session(&self) -> Option<u64> {
        match self.phase {
            Phase::Recording { session, .. } => Some(session),
            _ => None,
        }
    }

    fn toggle(&mut self) {
        match self.phase {
            Phase::Idle => self.start(),
            Phase::Recording { .. } => self.stop(),
            // One dictation at a time: triggers while busy are ignored.
            Phase::Salvaging { .. } | Phase::Transcribing { .. } | Phase::Pasting { .. } => {
                info!("busy; trigger ignored")
            }
        }
    }

    fn on_hotkey(&mut self, event: HotkeyEvent) {
        let mode = self.env.settings.current().config.hotkey.mode;
        let idle = matches!(self.phase, Phase::Idle);
        let recording = matches!(self.phase, Phase::Recording { .. });
        match (event, mode) {
            (HotkeyEvent::Escape, _) => self.cancel(true),
            (HotkeyEvent::ComboDown, HotkeyMode::Toggle) => self.toggle(),
            (HotkeyEvent::LoneCtrl, HotkeyMode::Toggle) if recording => self.stop(),
            (HotkeyEvent::ComboDown, HotkeyMode::PushToTalk) if idle => self.start(),
            (HotkeyEvent::ComboUp, HotkeyMode::PushToTalk) if recording => self.stop(),
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
        Ok(Clients { primary, fallback })
    }

    fn start(&mut self) {
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

        let max = Duration::from_secs(u64::from(settings.config.recording.max_seconds));
        let tx = self.tx.clone();
        let limit = tokio::spawn(async move {
            tokio::time::sleep(max).await;
            let _ = tx.send(Msg::LimitReached { session });
        })
        .abort_handle();

        let label = if started.fell_back { settings.lang.listening_default_mic() } else { settings.lang.listening() };
        self.env.hotkey.set_capture_escape(true);
        self.play(Sound::Start);
        self.env.overlay.show(listening(label, 0.0));
        info!(session, device = %started.device, profile = %settings.config.active_profile_id, "recording started");
        self.phase = Phase::Recording { session, capture: started.capture, settings, clients, limit, label };
        self.set_state(DictationState::Recording);
    }

    fn stop(&mut self) {
        if self.recording_session().is_none() {
            return;
        }
        let Phase::Recording { session, capture, settings, clients, limit, .. } =
            mem::replace(&mut self.phase, Phase::Idle)
        else {
            unreachable!("checked above")
        };
        limit.abort();
        let recording = capture.finish();
        info!(session, audio_ms = recording.duration_ms(), "recording stopped");
        self.begin_transcription(session, settings, clients, Input::Raw(recording));
    }

    fn retry(&mut self) {
        if !matches!(self.phase, Phase::Idle) {
            return;
        }
        let Some(audio) = self.retry.clone() else {
            return;
        };
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
        self.begin_transcription(self.session, settings, clients, Input::Prepared(audio));
    }

    fn begin_transcription(
        &mut self,
        session: u64,
        settings: Arc<Settings>,
        clients: Clients<P::Client>,
        input: Input,
    ) {
        self.env.overlay.show(view(Tone::Busy, settings.lang.transcribing(), None));
        let tx = self.tx.clone();
        let retry_delay = self.env.retry_delay;
        let task_settings = settings.clone();
        let task = tokio::spawn(async move {
            let audio = match input {
                Input::Prepared(audio) => audio,
                Input::Raw(recording) => match prepare_off_thread(recording).await {
                    Ok(audio) => Arc::new(audio),
                    Err(err) => {
                        let _ = tx.send(Msg::Transcribed { session, audio: None, result: Err(err) });
                        return;
                    }
                },
            };
            let ctx = PipelineContext {
                primary: &clients.primary,
                fallback: clients.fallback.as_ref(),
                rules: &task_settings.rules,
                prompt_context: &task_settings.config.rules.prompt_context,
                retry_delay,
            };
            let result = pipeline::transcribe(&audio, &ctx).await;
            let _ = tx.send(Msg::Transcribed { session, audio: Some(audio), result });
        })
        .abort_handle();
        self.phase = Phase::Transcribing { session, job: Job { settings, started: Instant::now() }, task };
        self.set_state(DictationState::Transcribing);
    }

    fn on_capture(&mut self, session: u64, event: CaptureEvent) {
        let Phase::Recording { session: current, label, .. } = self.phase else {
            return;
        };
        if current != session {
            return;
        }
        match event {
            CaptureEvent::Level(level) => self.env.overlay.show(listening(label, level)),
            CaptureEvent::Failed(reason) => {
                warn!(session, %reason, "microphone stopped during recording");
                let Phase::Recording { capture, limit, settings, .. } = mem::replace(&mut self.phase, Phase::Idle)
                else {
                    unreachable!("checked above")
                };
                limit.abort();
                // Keep what was said so far (spec §8): prepare it, but send nothing on our own.
                let recording = capture.finish();
                let tx = self.tx.clone();
                let task = tokio::spawn(async move {
                    let result = prepare_off_thread(recording).await;
                    let _ = tx.send(Msg::Salvaged { session, result });
                })
                .abort_handle();
                self.phase = Phase::Salvaging { session, settings, task };
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
        let Phase::Transcribing { job, .. } = mem::replace(&mut self.phase, Phase::Idle) else {
            unreachable!("checked above")
        };
        self.env.hotkey.set_capture_escape(false);
        let lang = job.settings.lang;
        let transcript = match result {
            Ok(transcript) => transcript,
            Err(PipelineError::TooShort) => {
                info!(session, "recording too short; not sent");
                return self.finish_info(lang.too_short());
            }
            Err(PipelineError::NoSpeech) => {
                info!(session, "no speech detected; not sent");
                return self.finish_info(lang.no_speech());
            }
            Err(err) => {
                warn!(session, error = %err, "transcription failed");
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
            return self.finish_info(lang.empty());
        }

        let paste = &job.settings.config.paste;
        let text = if paste.trailing_space { format!("{} ", transcript.text) } else { transcript.text.clone() };
        let restore = paste.restore_clipboard;
        let paster = self.env.paster.clone();
        let tx = self.tx.clone();
        tokio::task::spawn_blocking(move || {
            let outcome = paster.paste(&text, restore);
            let _ = tx.send(Msg::Pasted { session, outcome });
        });
        self.phase = Phase::Pasting { session, job, transcript, audio };
        self.set_state(DictationState::Pasting);
    }

    fn on_pasted(&mut self, session: u64, outcome: PasteOutcome) {
        if !matches!(self.phase, Phase::Pasting { session: current, .. } if current == session) {
            return;
        }
        let Phase::Pasting { job, transcript, audio, .. } = mem::replace(&mut self.phase, Phase::Idle) else {
            unreachable!("checked above")
        };
        let lang = job.settings.lang;
        let elapsed = job.started.elapsed();
        self.record(&job, &transcript, audio.as_deref());
        match outcome {
            PasteOutcome::Pasted => {
                info!(session, total_ms = elapsed.as_millis() as u64, "pasted");
                self.play(Sound::Done);
                self.env.overlay.show(view(Tone::Success, lang.pasted(elapsed.as_secs_f32()), Some(PASTED_HIDE)));
            }
            PasteOutcome::ClipboardOnly => {
                info!(session, "target window refused input; text left on the clipboard");
                self.play(Sound::Done);
                self.env.overlay.show(view(Tone::Warning, lang.clipboard_only(), Some(WARNING_HIDE)));
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
        self.set_state(DictationState::Idle);
    }

    fn record(&self, job: &Job, transcript: &Transcript, audio: Option<&PreparedAudio>) {
        let history = &job.settings.config.history;
        if !history.enabled {
            return;
        }
        let entry = NewDictation {
            created_at_ms: now_ms(),
            profile_id: &transcript.profile_id,
            raw_text: &transcript.raw_text,
            text: &transcript.text,
            status: transcript.status,
            audio_ms: transcript.audio_ms,
            latency_ms: job.started.elapsed().as_millis() as u64,
        };
        let audio = if history.save_audio { audio } else { None };
        match self.env.history.record(&entry, audio) {
            Ok(id) => self.env.events.history_added(id),
            Err(err) => warn!(error = %err, "history write failed"),
        }
    }

    /// `announce: false` is the quiet variant used on shutdown.
    fn cancel(&mut self, announce: bool) {
        let lang = match mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Idle => return,
            pasting @ Phase::Pasting { .. } => {
                self.phase = pasting; // Ctrl+V is already on its way
                return;
            }
            Phase::Recording { session, capture, limit, settings, .. } => {
                limit.abort();
                drop(capture);
                info!(session, "recording cancelled");
                settings.lang
            }
            Phase::Salvaging { session, settings, task } => {
                task.abort();
                info!(session, "cancelled after the microphone was lost");
                settings.lang
            }
            Phase::Transcribing { session, task, job } => {
                task.abort();
                info!(session, "transcription cancelled");
                job.settings.lang
            }
        };
        self.env.hotkey.set_capture_escape(false);
        if announce {
            self.play(Sound::Cancel);
            self.env.overlay.show(view(Tone::Neutral, lang.cancelled(), Some(CANCEL_HIDE)));
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
        self.env.hotkey.set_capture_escape(false);
        let button = if kind.needs_settings() {
            Some(OverlayButton { action: OverlayAction::OpenSettings, label: lang.open_settings().into() })
        } else if self.retry.is_some() {
            Some(OverlayButton { action: OverlayAction::Retry, label: lang.retry().into() })
        } else {
            None
        };
        let hide_after = if button.is_some() { ERROR_WITH_BUTTON_HIDE } else { ERROR_HIDE };
        self.env.overlay.show(OverlayView {
            tone: Tone::Error,
            text: message.clone(),
            level: None,
            button,
            hide_after: Some(hide_after),
        });
        self.play(Sound::Error);
        self.set_state(DictationState::Error { kind, message });
        self.set_state(DictationState::Idle);
    }

    fn finish_info(&mut self, text: &str) {
        self.env.overlay.show(view(Tone::Neutral, text, Some(INFO_HIDE)));
        self.set_state(DictationState::Idle);
    }

    fn play(&self, sound: Sound) {
        if self.env.settings.current().config.ui.sound_feedback {
            self.env.sounds.play(sound);
        }
    }

    fn set_state(&mut self, state: DictationState) {
        let status = DictationStatus { state, can_retry: self.retry.is_some() };
        *self.status.lock().unwrap_or_else(PoisonError::into_inner) = status.clone();
        self.env.events.status_changed(&status);
    }
}

/// Runs the CPU-heavy prepare step (resample + silence gate) on the blocking pool.
async fn prepare_off_thread(recording: Recording) -> Result<PreparedAudio, PipelineError> {
    tokio::task::spawn_blocking(move || pipeline::prepare(&recording))
        .await
        .unwrap_or_else(|join| Err(PipelineError::Encode(EncodeError(join.to_string()))))
}

fn view(tone: Tone, text: impl Into<String>, hide_after: Option<Duration>) -> OverlayView {
    OverlayView { tone, text: text.into(), level: None, button: None, hide_after }
}

fn listening(label: &str, rms: f32) -> OverlayView {
    OverlayView { tone: Tone::Busy, text: label.into(), level: Some(meter(rms)), button: None, hide_after: None }
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
