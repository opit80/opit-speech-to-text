//! What the controller reports to the tray and the UI.

use super::{Controller, ERROR_HIDE, ERROR_WITH_BUTTON_HIDE, INFO_HIDE, Phase, Providers};
use crate::i18n::Lang;
use crate::platform::{OverlayAction, OverlayButton, OverlayView, RecordingTiming, Sound, Tone};
use std::sync::PoisonError;
use std::time::{Duration, Instant};

use opit_core::pipeline::PipelineError;
use opit_core::provider::ProviderError;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    MissingKey,
    InvalidKey,
    PaymentRequired,
    ModelNotFound,
    TooLarge,
    RateLimited,
    Server,
    Network,
    Timeout,
    BadResponse,
    Rejected,
    Audio,
    Microphone,
    Credentials,
    Paste,
    Cancelled,
}

impl ErrorKind {
    pub const ALL: [ErrorKind; 16] = [
        ErrorKind::MissingKey,
        ErrorKind::InvalidKey,
        ErrorKind::PaymentRequired,
        ErrorKind::ModelNotFound,
        ErrorKind::TooLarge,
        ErrorKind::RateLimited,
        ErrorKind::Server,
        ErrorKind::Network,
        ErrorKind::Timeout,
        ErrorKind::BadResponse,
        ErrorKind::Rejected,
        ErrorKind::Audio,
        ErrorKind::Microphone,
        ErrorKind::Credentials,
        ErrorKind::Paste,
        ErrorKind::Cancelled,
    ];

    /// Errors the user fixes in Settings rather than by trying again.
    pub fn needs_settings(self) -> bool {
        matches!(
            self,
            ErrorKind::MissingKey | ErrorKind::InvalidKey | ErrorKind::ModelNotFound | ErrorKind::Credentials
        )
    }
}

impl From<opit_core::cleanup::CleanupError> for ErrorKind {
    fn from(error: opit_core::cleanup::CleanupError) -> Self {
        use opit_core::cleanup::CleanupError::*;
        match error {
            MissingKey => Self::MissingKey,
            InvalidKey => Self::InvalidKey,
            PaymentRequired => Self::PaymentRequired,
            ModelNotFound => Self::ModelNotFound,
            RateLimited => Self::RateLimited,
            Server => Self::Server,
            Network => Self::Network,
            Timeout => Self::Timeout,
            Rejected => Self::Rejected,
            BadResponse | Truncated => Self::BadResponse,
        }
    }
}

impl From<&ProviderError> for ErrorKind {
    fn from(err: &ProviderError) -> Self {
        match err {
            ProviderError::Unauthorized(_) => ErrorKind::InvalidKey,
            ProviderError::PayloadTooLarge => ErrorKind::TooLarge,
            ProviderError::RateLimited | ProviderError::RateLimitedAfter(_) => ErrorKind::RateLimited,
            ProviderError::Server(_) => ErrorKind::Server,
            ProviderError::Http { .. } => ErrorKind::Rejected,
            ProviderError::Network(_) => ErrorKind::Network,
            ProviderError::Timeout => ErrorKind::Timeout,
            ProviderError::BadResponse(_) => ErrorKind::BadResponse,
        }
    }
}

impl From<&PipelineError> for ErrorKind {
    fn from(err: &PipelineError) -> Self {
        match err {
            PipelineError::Provider(e) => e.into(),
            PipelineError::RetryCancelled => ErrorKind::Cancelled,
            PipelineError::Encode(_) | PipelineError::TooShort | PipelineError::NoSpeech { .. } => ErrorKind::Audio,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DictationState {
    Idle,
    Recording,
    Transcribing,
    Cleaning,
    Asking,
    Pasting,
    /// Transient: emitted once, followed by the current active state or `Idle`.
    Cancelled,
    /// Transient: emitted once, followed by the current active state or `Idle`.
    Error {
        kind: ErrorKind,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DictationStatus {
    #[serde(flatten)]
    pub state: DictationState,
    /// Audio of the last failed dictation is kept; "Try again" resends it.
    pub can_retry: bool,
    /// Finished recordings waiting behind the current processing item.
    pub waiting: usize,
    pub paused: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_wait: Option<opit_core::provider::retry::RetryWait>,
}

impl Default for DictationStatus {
    fn default() -> Self {
        Self { state: DictationState::Idle, can_retry: false, waiting: 0, paused: false, retry_wait: None }
    }
}

impl<P: Providers> Controller<P> {
    pub(super) fn show_progress(&self) {
        let lang = self.env.settings.current().lang;
        let text = match &self.phase {
            Phase::Transcribing { asking: true, job, .. } => {
                job.settings.lang.asking(job.settings.config.ask.agent.name(), job.started.elapsed().as_secs())
            }
            Phase::Transcribing { cleaning: true, job, .. } if job.voice.is_none() && !job.ask => {
                lang.cleaning_skippable().into()
            }
            Phase::Transcribing { cleaning: true, .. } => lang.cleaning().into(),
            Phase::Transcribing { progress: Some(progress), .. } => lang.retry_wait(*progress),
            _ => lang.transcribing().into(),
        };
        let view = view(Tone::Busy, text, None);
        #[cfg(target_os = "linux")]
        let view = OverlayView {
            button: Some(OverlayButton { action: OverlayAction::Cancel, label: lang.cancel_action().into() }),
            ..view
        };
        self.show(view);
    }

    pub(super) fn fail(&mut self, kind: ErrorKind, lang: Lang) {
        self.fail_with_message(kind, lang, lang.error(kind).to_string());
    }

    pub(super) fn fail_with_message(&mut self, kind: ErrorKind, lang: Lang, message: String) {
        let button = if kind.needs_settings() {
            Some(OverlayButton { action: OverlayAction::OpenSettings, label: lang.open_settings().into() })
        } else if self.retry.is_some() {
            Some(OverlayButton { action: OverlayAction::Retry, label: lang.retry().into() })
        } else {
            None
        };
        let hide_after = if button.is_some() { ERROR_WITH_BUTTON_HIDE } else { ERROR_HIDE };
        self.show(OverlayView {
            live_text: None,
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

    pub(super) fn finish_info(&mut self, text: &str) {
        self.show(view(Tone::Neutral, text, Some(INFO_HIDE)));
        self.set_state(DictationState::Idle);
    }

    pub(super) fn play(&self, sound: Sound) {
        if self.env.settings.current().config.ui.sound_feedback {
            self.env.sounds.play(sound);
        }
    }

    pub(super) fn show_recording(&self) {
        if let Some(r) = &self.recording {
            let label = if r.paused { r.settings.lang.recording_paused() } else { r.label };
            let max = Duration::from_secs(u64::from(r.settings.config.recording.max_seconds));
            let remaining = if r.paused { r.remaining } else { r.remaining.saturating_sub(r.resumed_at.elapsed()) };
            let label = if r.voice.is_some() {
                if r.paused { format!("{} · {}", r.label, label) } else { label.to_string() }
            } else {
                format!("{} - {}", label, r.settings.lang.style_name(&r.cleanup_style))
            };
            let mut view = listening(&r.settings.lang.with_waiting(&label, self.waiting.len()), r.level);
            if !r.live_text.is_empty() {
                view.live_text = Some(r.live_text.clone());
            }
            view.recording = Some(RecordingTiming {
                session: r.session,
                elapsed: max.saturating_sub(remaining),
                updated_at: Instant::now(),
                paused: r.paused,
            });
            #[cfg(target_os = "linux")]
            {
                view.button = Some(OverlayButton {
                    action: OverlayAction::Cancel,
                    label: r.settings.lang.cancel_action().into(),
                });
            }
            self.env.overlay.show(view);
        }
    }

    pub(super) fn show(&self, mut view: OverlayView) {
        if self.recording.is_some() {
            self.show_recording();
        } else {
            view.text = self.env.settings.current().lang.with_waiting(&view.text, self.waiting.len());
            self.env.overlay.show(view);
        }
    }

    pub(super) fn set_state(&mut self, state: DictationState) {
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

pub(super) fn view(tone: Tone, text: impl Into<String>, hide_after: Option<Duration>) -> OverlayView {
    OverlayView { live_text: None, tone, text: text.into(), level: None, recording: None, button: None, hide_after }
}

fn listening(label: &str, rms: f32) -> OverlayView {
    OverlayView {
        live_text: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_errors_map_to_kinds() {
        assert_eq!(ErrorKind::from(&ProviderError::Unauthorized(401)), ErrorKind::InvalidKey);
        assert_eq!(ErrorKind::from(&ProviderError::Http { status: 400, message: "x".into() }), ErrorKind::Rejected);
        let pipeline = PipelineError::Provider(ProviderError::Timeout);
        assert_eq!(ErrorKind::from(&pipeline), ErrorKind::Timeout);
        assert!(ErrorKind::InvalidKey.needs_settings() && !ErrorKind::Server.needs_settings());
        assert_eq!(ErrorKind::from(opit_core::cleanup::CleanupError::ModelNotFound), ErrorKind::ModelNotFound);
        assert!(ErrorKind::ModelNotFound.needs_settings());
    }

    #[test]
    fn status_serializes_flat_for_the_ui() {
        let status = DictationStatus {
            state: DictationState::Error { kind: ErrorKind::Network, message: "Ağ hatası".into() },
            can_retry: true,
            waiting: 2,
            paused: false,
            retry_wait: None,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"state":"error","kind":"network","message":"Ağ hatası","can_retry":true,"waiting":2,"paused":false})
        );
        let idle = serde_json::to_value(DictationStatus::default()).unwrap();
        assert_eq!(idle, serde_json::json!({"state":"idle","can_retry":false,"waiting":0,"paused":false}));
    }
}
