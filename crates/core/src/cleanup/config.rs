use serde::{Deserialize, Serialize};

use super::{CleanupStyle, styles};

pub const KEY_REF: &str = "cleanup";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CleanupConfig {
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    pub api_key_ref: Option<String>,
    pub prompt_override: String,
    pub styles: Vec<CleanupStyle>,
    pub active_style_id: String,
    #[serde(default = "legacy_timeout_seconds")]
    pub timeout_seconds: u32,
    pub reasoning_effort: Option<String>,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: "https://inference.baseten.co/v1".into(),
            model: "deepseek-ai/DeepSeek-V4.1-Flash-Fast".into(),
            api_key_ref: Some(KEY_REF.into()),
            prompt_override: String::new(),
            styles: styles::builtin_styles(),
            active_style_id: "clean".into(),
            timeout_seconds: 8,
            reasoning_effort: None,
        }
    }
}

fn legacy_timeout_seconds() -> u32 {
    15
}

/// Preserve the effective timeout when loading configs written before this field existed.
pub(crate) fn legacy_config() -> CleanupConfig {
    CleanupConfig { timeout_seconds: legacy_timeout_seconds(), ..CleanupConfig::default() }
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub key_url: &'static str,
    pub config: CleanupConfig,
}

pub fn presets() -> Vec<CleanupPreset> {
    [
        (
            "baseten",
            "Baseten",
            "https://inference.baseten.co/v1",
            "deepseek-ai/DeepSeek-V4.1-Flash-Fast",
            "https://app.baseten.co/settings/api_keys",
        ),
        (
            "openrouter",
            "OpenRouter",
            "https://openrouter.ai/api/v1",
            "google/gemini-2.5-flash-lite",
            "https://openrouter.ai/settings/keys",
        ),
        (
            "google",
            "Google AI Studio",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "gemini-3.5-flash-lite",
            "https://aistudio.google.com/apikey",
        ),
        (
            "deepseek",
            "DeepSeek",
            "https://api.deepseek.com/v1",
            "deepseek-chat",
            "https://platform.deepseek.com/api_keys",
        ),
        ("openai", "OpenAI", "https://api.openai.com/v1", "gpt-4.1-mini", "https://platform.openai.com/api-keys"),
        ("groq", "Groq", "https://api.groq.com/openai/v1", "llama-3.1-8b-instant", "https://console.groq.com/keys"),
        ("local", "Local", "http://127.0.0.1:11434/v1", "qwen3:4b", ""),
        ("custom", "Custom", "", "", ""),
    ]
    .into_iter()
    .map(|(id, name, url, model, key_url)| CleanupPreset {
        id,
        name,
        key_url,
        config: CleanupConfig {
            base_url: url.into(),
            model: model.into(),
            api_key_ref: match id {
                "local" => None,
                "baseten" => Some(KEY_REF.into()),
                _ => Some(format!("cleanup-{id}")),
            },
            ..CleanupConfig::default()
        },
    })
    .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupStatus {
    Ok,
    Failed,
    Skipped,
}

impl CleanupStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ok" => Some(Self::Ok),
            "failed" => Some(Self::Failed),
            "skipped" => Some(Self::Skipped),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CleanupError {
    #[error("cleanup key is unavailable")]
    MissingKey,
    #[error("cleanup key was rejected")]
    InvalidKey,
    #[error("the provider account has a billing or credit problem")]
    PaymentRequired,
    #[error("cleanup model was not found or is unavailable; choose another model")]
    ModelNotFound,
    #[error("cleanup provider is rate limiting")]
    RateLimited,
    #[error("cleanup provider server error")]
    Server,
    #[error("cleanup network request failed")]
    Network,
    #[error("cleanup timed out")]
    Timeout,
    #[error("cleanup request was rejected")]
    Rejected,
    #[error("cleanup returned an invalid answer")]
    BadResponse,
    #[error("the model ran out of answer tokens")]
    Truncated,
}
