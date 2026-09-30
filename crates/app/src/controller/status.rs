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
}

impl ErrorKind {
    pub const ALL: [ErrorKind; 13] = [
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
            ProviderError::RateLimited => ErrorKind::RateLimited,
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
            PipelineError::Encode(_) | PipelineError::TooShort | PipelineError::NoSpeech => ErrorKind::Audio,
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
    /// Transient: emitted once, followed by `Idle`.
    Cancelled,
    /// Transient: emitted once, followed by `Idle`.
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
}

impl Default for DictationStatus {
    fn default() -> Self {
        Self { state: DictationState::Idle, can_retry: false }
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
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json, serde_json::json!({"state":"error","kind":"network","message":"Ağ hatası","can_retry":true}));
        let idle = serde_json::to_value(DictationStatus::default()).unwrap();
        assert_eq!(idle, serde_json::json!({"state":"idle","can_retry":false}));
    }
}
