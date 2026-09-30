//! Provider profiles and built-in presets.

use serde::{Deserialize, Serialize};

use crate::audio::encode::AudioFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseFormat {
    VerboseJson,
    Json,
}

impl ResponseFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ResponseFormat::VerboseJson => "verbose_json",
            ResponseFormat::Json => "json",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    /// Credential Manager entry that holds the API key; `None` means no key (local servers).
    #[serde(default)]
    pub api_key_ref: Option<String>,
    /// ISO-639-1 transcription language, or `auto`.
    #[serde(default = "default_language")]
    pub language: String,
    pub audio_format: AudioFormat,
    pub response_format: ResponseFormat,
    pub send_prompt: bool,
    pub send_keywords: bool,
    /// Apply the client-side rule layer; off when the server applies its own rules.
    pub apply_rules: bool,
    #[serde(default)]
    pub fallback_profile_id: Option<String>,
}

impl Profile {
    pub fn is_insecure(&self) -> bool {
        self.base_url.trim_start().to_ascii_lowercase().starts_with("http://")
    }
}

fn default_language() -> String {
    "tr".to_string()
}

pub mod presets {
    use super::{AudioFormat, Profile, ResponseFormat};

    pub const GROQ_ID: &str = "groq";
    pub const OPENAI_ID: &str = "openai";

    pub fn groq() -> Profile {
        Profile {
            id: GROQ_ID.into(),
            name: "Groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            model: "whisper-large-v3".into(),
            api_key_ref: Some(GROQ_ID.into()),
            language: "tr".into(),
            audio_format: AudioFormat::Flac,
            response_format: ResponseFormat::VerboseJson,
            send_prompt: true,
            send_keywords: false,
            apply_rules: true,
            fallback_profile_id: None,
        }
    }

    pub fn openai() -> Profile {
        Profile {
            id: OPENAI_ID.into(),
            name: "OpenAI".into(),
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-transcribe".into(),
            api_key_ref: Some(OPENAI_ID.into()),
            language: "tr".into(),
            audio_format: AudioFormat::Wav,
            response_format: ResponseFormat::Json,
            send_prompt: true,
            send_keywords: true,
            apply_rules: true,
            fallback_profile_id: None,
        }
    }

    /// A user-defined OpenAI-compatible server. Plain JSON is the safe default;
    /// the user can switch to verbose_json if the server supports it.
    pub fn custom(id: &str, name: &str, base_url: &str, model: &str) -> Profile {
        Profile {
            id: id.into(),
            name: name.into(),
            base_url: base_url.into(),
            model: model.into(),
            api_key_ref: Some(id.into()),
            language: "tr".into(),
            audio_format: AudioFormat::Wav,
            response_format: ResponseFormat::Json,
            send_prompt: true,
            send_keywords: false,
            apply_rules: true,
            fallback_profile_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groq_preset_matches_the_brief() {
        let p = presets::groq();
        assert_eq!(p.id, "groq");
        assert_eq!(p.base_url, "https://api.groq.com/openai/v1");
        assert_eq!(p.model, "whisper-large-v3");
        assert_eq!(p.audio_format, AudioFormat::Flac);
        assert_eq!(p.response_format, ResponseFormat::VerboseJson);
        assert!(p.send_prompt && !p.send_keywords && p.apply_rules);
        assert_eq!(p.api_key_ref.as_deref(), Some("groq"));
        assert_eq!(p.language, "tr");
    }

    #[test]
    fn openai_preset_sends_keywords_and_plain_json() {
        let p = presets::openai();
        assert_eq!(p.base_url, "https://api.openai.com/v1");
        assert_eq!(p.model, "gpt-transcribe");
        assert_eq!(p.audio_format, AudioFormat::Wav);
        assert_eq!(p.response_format, ResponseFormat::Json);
        assert!(p.send_prompt && p.send_keywords);
    }

    #[test]
    fn custom_preset_is_conservative() {
        let p = presets::custom("gpu", "GPU box", "http://10.0.0.2:8888/v1", "large-v3");
        assert_eq!(p.audio_format, AudioFormat::Wav);
        assert_eq!(p.response_format, ResponseFormat::Json);
        assert!(p.send_prompt && !p.send_keywords && p.apply_rules);
        assert!(p.is_insecure());
        assert!(!presets::groq().is_insecure());
    }

    #[test]
    fn serializes_with_snake_case_enums() {
        let json = serde_json::to_value(presets::groq()).unwrap();
        assert_eq!(json["audio_format"], "flac");
        assert_eq!(json["response_format"], "verbose_json");
        let back: Profile = serde_json::from_value(json).unwrap();
        assert_eq!(back, presets::groq());
    }
}
