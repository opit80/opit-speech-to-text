use std::sync::Arc;
use std::time::Duration;

use opit_core::config::ConfigError;
use opit_core::history::{Dictation, HistoryError};
use opit_core::learning::TermChange;
use opit_core::provider::ProviderError;
use opit_core::rules::pack::CorrectionOutcome;
use opit_core::rules::{PackError, RuleHit, RuleWarning};
use serde::Serialize;

use crate::controller::ErrorKind;
use crate::platform::{Autostart, ForegroundApp, Hotkey, MediaControl, Microphone, Overlay, SecretError, SecretStore};

/// Error shape every invoke command returns to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct CommandError {
    /// Stable machine-readable code: config, rules, history, secrets, provider, invalid_input, unavailable.
    pub code: &'static str,
    pub message: String,
    /// Set for provider errors so the UI can localize them.
    pub kind: Option<ErrorKind>,
}

impl CommandError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), kind: None }
    }
}

impl From<ConfigError> for CommandError {
    fn from(err: ConfigError) -> Self {
        Self::new("config", err.to_string())
    }
}

impl From<PackError> for CommandError {
    fn from(err: PackError) -> Self {
        Self::new("rules", err.to_string())
    }
}

impl From<HistoryError> for CommandError {
    fn from(err: HistoryError) -> Self {
        Self::new("history", err.to_string())
    }
}

impl From<SecretError> for CommandError {
    fn from(err: SecretError) -> Self {
        Self::new("secrets", err.to_string())
    }
}

impl From<ProviderError> for CommandError {
    fn from(err: ProviderError) -> Self {
        Self { code: "provider", message: err.to_string(), kind: Some(ErrorKind::from(&err)) }
    }
}

impl From<opit_core::cleanup::CleanupError> for CommandError {
    fn from(error: opit_core::cleanup::CleanupError) -> Self {
        Self { code: "cleanup", message: error.to_string(), kind: Some(error.into()) }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RulesPreview {
    pub text: String,
    pub hits: Vec<RuleHit>,
    pub warnings: Vec<RuleWarning>,
    pub hallucination: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CorrectionDraft {
    /// The new `user.yaml` text; not saved yet.
    pub yaml: String,
    pub outcome: CorrectionOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryUpdate {
    pub dictation: Dictation,
    pub suggestions: Vec<TermChange>,
}

pub const MAX_HISTORY_TEXT_BYTES: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HotkeyState {
    pub paused: bool,
    pub pending: bool,
    /// Why the global hook is not working, if it is not.
    pub error: Option<String>,
}

pub const MIC_TEST_LIMIT: Duration = Duration::from_secs(20);
pub const HOTKEY_CAPTURE_LIMIT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MicTestEvent {
    Level { value: f32 },
    Failed { message: String },
}

pub type MicTestSink = Arc<dyn Fn(MicTestEvent) + Send + Sync>;

/// Starter content for a personal pack that does not exist yet.
pub const USER_PACK_TEMPLATE: &str = "schema: 1\nid: user\nname: Personal\nlanguage: tr\nterms: []\n";

pub struct Platform {
    pub foreground: Arc<dyn ForegroundApp>,
    pub media: Arc<dyn MediaControl>,
    pub secrets: Arc<dyn SecretStore>,
    pub hotkey: Arc<dyn Hotkey>,
    pub mic: Arc<dyn Microphone>,
    pub overlay: Arc<dyn Overlay>,
    pub autostart: Arc<dyn Autostart>,
}
