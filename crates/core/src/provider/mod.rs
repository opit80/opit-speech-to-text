//! Speech-to-text providers, selected explicitly by the saved profile.

use crate::audio::encode::EncodedAudio;

pub mod deepgram;
pub mod elevenlabs;
pub mod language;
pub mod local;
pub mod openai;
pub mod profile;
pub mod retry;
pub mod streaming;

pub use openai::OpenAiCompatible;
pub use profile::{Profile, ProviderKind, ResponseFormat, presets};

pub enum ProviderClient {
    OpenaiCompatible(OpenAiCompatible),
    Elevenlabs(elevenlabs::ElevenLabs),
    Deepgram(deepgram::Deepgram),
    LocalWhisper(local::LocalWhisper),
}

impl ProviderClient {
    pub fn new(profile: Profile, api_key: Option<String>) -> Result<Self, ProviderError> {
        match profile.provider {
            ProviderKind::OpenaiCompatible => OpenAiCompatible::new(profile, api_key).map(Self::OpenaiCompatible),
            ProviderKind::Elevenlabs => elevenlabs::ElevenLabs::new(profile, api_key).map(Self::Elevenlabs),
            ProviderKind::Deepgram => deepgram::Deepgram::new(profile, api_key).map(Self::Deepgram),
            ProviderKind::LocalWhisper => local::LocalWhisper::new(profile).map(Self::LocalWhisper),
        }
    }

    pub async fn test_connection(&self) -> Result<(), ProviderError> {
        match self {
            Self::OpenaiCompatible(client) => client.test_connection().await,
            Self::Elevenlabs(client) => client.test_connection().await,
            Self::Deepgram(client) => client.test_connection().await,
            Self::LocalWhisper(client) => client.test_connection().await,
        }
    }

    pub async fn warm_up(&self) {
        match self {
            Self::OpenaiCompatible(client) => client.warm_up().await,
            Self::Elevenlabs(client) => client.warm_up().await,
            Self::Deepgram(_) => {}
            Self::LocalWhisper(_) => {}
        }
    }
}

impl Transcriber for ProviderClient {
    fn profile(&self) -> &Profile {
        match self {
            Self::OpenaiCompatible(client) => client.profile(),
            Self::Elevenlabs(client) => client.profile(),
            Self::Deepgram(client) => client.profile(),
            Self::LocalWhisper(client) => client.profile(),
        }
    }

    async fn transcribe(&self, request: &TranscribeRequest<'_>) -> Result<RawTranscript, ProviderError> {
        match self {
            Self::OpenaiCompatible(client) => client.transcribe(request).await,
            Self::Elevenlabs(client) => client.transcribe(request).await,
            Self::Deepgram(client) => client.transcribe(request).await,
            Self::LocalWhisper(client) => client.transcribe(request).await,
        }
    }
}

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
    pub language: Option<String>,
    /// verbose_json segments dropped as probable silence.
    pub dropped_segments: usize,
    pub segments: Vec<crate::files::Segment>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("the API key was rejected (HTTP {0})")]
    Unauthorized(u16),
    #[error("the recording exceeds the provider's upload limit")]
    PayloadTooLarge,
    #[error("the provider is rate limiting requests")]
    RateLimited,
    #[error("the provider is rate limiting requests")]
    RateLimitedAfter(std::time::Duration),
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
            ProviderError::RateLimited
                | ProviderError::RateLimitedAfter(_)
                | ProviderError::Server(500..=599)
                | ProviderError::Network(_)
                | ProviderError::Timeout
        )
    }

    pub fn retry_after(&self) -> Option<std::time::Duration> {
        match self {
            Self::RateLimitedAfter(delay) => Some((*delay).min(std::time::Duration::from_secs(120))),
            _ => None,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized(_) => "invalid_key",
            Self::PayloadTooLarge => "too_large",
            Self::RateLimited | Self::RateLimitedAfter(_) => "rate_limited",
            Self::Server(_) => "server",
            Self::Http { .. } => "rejected",
            Self::Network(_) => "network",
            Self::Timeout => "timeout",
            Self::BadResponse(_) => "bad_response",
        }
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
