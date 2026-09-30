//! Speech-to-text providers. v1 has one adapter: OpenAI-compatible HTTP.

use crate::audio::encode::EncodedAudio;

pub mod openai;
pub mod profile;
pub mod retry;

pub use openai::OpenAiCompatible;
pub use profile::{Profile, ResponseFormat, presets};

pub struct TranscribeRequest<'a> {
    pub audio: &'a EncodedAudio,
    /// Length of the audio; stretches the request timeout.
    pub audio_ms: u64,
    pub prompt: Option<&'a str>,
    pub keywords: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTranscript {
    pub text: String,
    /// verbose_json segments dropped as probable silence.
    pub dropped_segments: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("the API key was rejected (HTTP {0})")]
    Unauthorized(u16),
    #[error("the recording exceeds the provider's upload limit")]
    PayloadTooLarge,
    #[error("the provider is rate limiting requests")]
    RateLimited,
    #[error("the provider had a server error (HTTP {0})")]
    Server(u16),
    #[error("the provider refused the request (HTTP {status}): {message}")]
    Http { status: u16, message: String },
    #[error("network error: {0}")]
    Network(String),
    #[error("the request timed out")]
    Timeout,
    #[error("the provider sent an unreadable response: {0}")]
    BadResponse(String),
}

impl ProviderError {
    /// Worth one retry and, after that, the fallback profile.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ProviderError::RateLimited | ProviderError::Server(_) | ProviderError::Network(_) | ProviderError::Timeout
        )
    }
}

pub trait Transcriber {
    fn profile(&self) -> &Profile;

    fn transcribe(
        &self,
        request: &TranscribeRequest<'_>,
    ) -> impl Future<Output = Result<RawTranscript, ProviderError>> + Send;
}

/// Lets the app keep one client (and its pooled connection) per profile and share it
/// between dictations.
impl<T: Transcriber + Sync> Transcriber for std::sync::Arc<T> {
    fn profile(&self) -> &Profile {
        (**self).profile()
    }

    fn transcribe(
        &self,
        request: &TranscribeRequest<'_>,
    ) -> impl Future<Output = Result<RawTranscript, ProviderError>> + Send {
        (**self).transcribe(request)
    }
}
