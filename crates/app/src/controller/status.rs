//! What the controller reports to the tray and the UI.

use opit_core::pipeline::PipelineError;
use opit_core::provider::ProviderError;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    MissingKey,
    InvalidKey,
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
    pub const ALL: [ErrorKind; 14] = [
        ErrorKind::MissingKey,
        ErrorKind::InvalidKey,
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
        matches!(self, ErrorKind::MissingKey | ErrorKind::InvalidKey | ErrorKind::Credentials)
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
