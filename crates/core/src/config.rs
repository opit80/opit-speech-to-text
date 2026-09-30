//! Application settings (`config.json`), with defaults, validation and schema migration.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::provider::{Profile, presets};

pub const APP_DIR_NAME: &str = "opit-speech-to-text";
pub const CONFIG_SCHEMA_VERSION: u32 = 1;
pub const MIN_RECORDING_SECONDS: u32 = 10;
pub const MAX_RECORDING_SECONDS: u32 = 600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub schema_version: u32,
    /// `en`, `tr`, or `None` to follow the system language.
    pub ui_language: Option<String>,
    pub active_profile_id: String,
    pub profiles: Vec<Profile>,
    pub hotkey: HotkeyConfig,
    pub recording: RecordingConfig,
    pub paste: PasteConfig,
    pub history: HistoryConfig,
    pub rules: RulesConfig,
    pub ui: UiConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            ui_language: None,
            active_profile_id: presets::GROQ_ID.into(),
            profiles: vec![presets::groq(), presets::openai()],
            hotkey: HotkeyConfig::default(),
            recording: RecordingConfig::default(),
            paste: PasteConfig::default(),
            history: HistoryConfig::default(),
            rules: RulesConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyMode {
    Toggle,
    PushToTalk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyConfig {
    /// Key names understood by the app's hotkey hook, e.g. `RightCtrl`.
    pub keys: Vec<String>,
    pub mode: HotkeyMode,
    pub enabled: bool,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self { keys: vec!["RightCtrl".into(), "RightShift".into()], mode: HotkeyMode::Toggle, enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingConfig {
    /// Device name; `None` = system default.
    pub microphone: Option<String>,
    pub max_seconds: u32,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self { microphone: None, max_seconds: 180 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PasteConfig {
    pub restore_clipboard: bool,
    pub trailing_space: bool,
}

impl Default for PasteConfig {
    fn default() -> Self {
        Self { restore_clipboard: true, trailing_space: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryConfig {
    pub enabled: bool,
    pub save_audio: bool,
    pub audio_retention_days: u32,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self { enabled: true, save_audio: false, audio_retention_days: 30 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RulesConfig {
    pub enabled_packs: Vec<String>,
    /// Short context sentence placed at the start of the Whisper prompt.
    pub prompt_context: String,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self { enabled_packs: vec!["tr-core".into(), "tr-tech".into()], prompt_context: String::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayPosition {
    RightCenter,
    TopCenter,
    BottomCenter,
    LeftCenter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub start_in_tray: bool,
    pub autostart: bool,
    pub overlay_position: OverlayPosition,
    pub sound_feedback: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            start_in_tray: false,
            autostart: true,
            overlay_position: OverlayPosition::RightCenter,
            sound_feedback: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("config is not valid: {0}")]
    Json(String),
    #[error("config was written by a newer version (schema {found})")]
    TooNew { found: u32 },
    #[error("config file I/O failed: {0}")]
    Io(String),
}

impl AppConfig {
    pub fn from_json(src: &str) -> Result<Self, ConfigError> {
        let value: Value = serde_json::from_str(src).map_err(|e| ConfigError::Json(e.to_string()))?;
        let value = migrate(value)?;
        let mut config: AppConfig = serde_json::from_value(value).map_err(|e| ConfigError::Json(e.to_string()))?;
        config.normalize();
        Ok(config)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("config always serializes")
    }

    /// Loads `path`; a missing file yields the defaults.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(src) => Self::from_json(&src),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(ConfigError::Io(e.to_string())),
        }
    }

    /// Writes atomically: a temp file next to `path`, then a rename over it.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let io = |e: std::io::Error| ConfigError::Io(e.to_string());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, self.to_json()).map_err(io)?;
        std::fs::rename(&tmp, path).map_err(io)
    }

    /// Repairs values a hand edit or an old version could leave inconsistent.
    pub fn normalize(&mut self) {
        self.schema_version = CONFIG_SCHEMA_VERSION;
        self.recording.max_seconds = self.recording.max_seconds.clamp(MIN_RECORDING_SECONDS, MAX_RECORDING_SECONDS);

        let mut seen = HashSet::new();
        self.profiles.retain(|p| seen.insert(p.id.clone()));
        if self.profiles.is_empty() {
            self.profiles.push(presets::groq());
        }
        if self.profile(&self.active_profile_id).is_none() {
            self.active_profile_id = self.profiles[0].id.clone();
        }
        for profile in &mut self.profiles {
            let valid = profile.fallback_profile_id.as_ref().is_some_and(|f| f != &profile.id && seen.contains(f));
            if !valid {
                profile.fallback_profile_id = None;
            }
        }
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn active_profile(&self) -> Option<&Profile> {
        self.profile(&self.active_profile_id)
    }

    pub fn fallback_for(&self, profile: &Profile) -> Option<&Profile> {
        profile.fallback_profile_id.as_deref().and_then(|id| self.profile(id))
    }
}

fn migrate(mut value: Value) -> Result<Value, ConfigError> {
    let object = value.as_object_mut().ok_or_else(|| ConfigError::Json("config root must be an object".into()))?;
    let version = object.get("schema_version").and_then(Value::as_u64).unwrap_or(0);
    if version > u64::from(CONFIG_SCHEMA_VERSION) {
        return Err(ConfigError::TooNew { found: version as u32 });
    }
    // v0 (unversioned pre-release files) has the same shape as v1.
    object.insert("schema_version".into(), Value::from(CONFIG_SCHEMA_VERSION));
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::presets;

    #[test]
    fn defaults_match_the_brief() {
        let c = AppConfig::default();
        assert_eq!(c.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(c.active_profile_id, "groq");
        assert_eq!(c.profiles, vec![presets::groq(), presets::openai()]);
        assert_eq!(c.hotkey.keys, ["RightCtrl", "RightShift"]);
        assert_eq!(c.hotkey.mode, HotkeyMode::Toggle);
        assert_eq!(c.recording.max_seconds, 180);
        assert!(c.paste.restore_clipboard && c.paste.trailing_space);
        assert!(c.history.enabled && !c.history.save_audio);
        assert_eq!(c.history.audio_retention_days, 30);
        assert_eq!(c.rules.enabled_packs, ["tr-core", "tr-tech"]);
        assert!(!c.ui.start_in_tray && c.ui.autostart && c.ui.sound_feedback);
        assert_eq!(c.ui.overlay_position, OverlayPosition::RightCenter);
    }

    #[test]
    fn json_round_trip() {
        let c = AppConfig::default();
        assert_eq!(AppConfig::from_json(&c.to_json()).unwrap(), c);
    }

    #[test]
    fn partial_json_fills_defaults_and_ignores_unknown_fields() {
        let c = AppConfig::from_json(r#"{"schema_version":1,"paste":{"trailing_space":false},"future":true}"#).unwrap();
        assert!(!c.paste.trailing_space);
        assert!(c.paste.restore_clipboard);
        assert_eq!(c.profiles.len(), 2);
    }

    #[test]
    fn unversioned_files_are_migrated() {
        let c = AppConfig::from_json(r#"{"recording":{"max_seconds":60}}"#).unwrap();
        assert_eq!(c.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(c.recording.max_seconds, 60);
    }

    #[test]
    fn newer_schema_is_rejected() {
        let err = AppConfig::from_json(r#"{"schema_version":99}"#).unwrap_err();
        assert!(matches!(err, ConfigError::TooNew { found: 99 }));
    }

    #[test]
    fn garbage_is_a_json_error() {
        assert!(matches!(AppConfig::from_json("{nope"), Err(ConfigError::Json(_))));
        assert!(matches!(AppConfig::from_json("[1,2]"), Err(ConfigError::Json(_))));
    }

    #[test]
    fn normalize_repairs_invalid_values() {
        let mut c = AppConfig::default();
        c.recording.max_seconds = 5_000;
        c.active_profile_id = "missing".into();
        c.profiles[0].fallback_profile_id = Some("groq".into());
        c.profiles[1].fallback_profile_id = Some("ghost".into());
        c.profiles.push(presets::groq());
        c.normalize();
        assert_eq!(c.recording.max_seconds, MAX_RECORDING_SECONDS);
        assert_eq!(c.active_profile_id, "groq");
        assert_eq!(c.profiles.len(), 2, "duplicate id removed");
        assert!(c.profiles.iter().all(|p| p.fallback_profile_id.is_none()));

        c.recording.max_seconds = 0;
        c.profiles.clear();
        c.normalize();
        assert_eq!(c.recording.max_seconds, MIN_RECORDING_SECONDS);
        assert_eq!(c.profiles, vec![presets::groq()]);
    }

    #[test]
    fn active_and_fallback_lookup() {
        let mut c = AppConfig::default();
        c.profiles[0].fallback_profile_id = Some("openai".into());
        let active = c.active_profile().unwrap();
        assert_eq!(active.id, "groq");
        assert_eq!(c.fallback_for(active).unwrap().id, "openai");
        assert!(c.fallback_for(&c.profiles[1]).is_none());
    }

    #[test]
    fn load_missing_file_gives_defaults_and_save_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.json");
        assert_eq!(AppConfig::load(&path).unwrap(), AppConfig::default());
        let mut c = AppConfig::default();
        c.rules.prompt_context = "FiveM üzerine konuşma.".into();
        c.save(&path).unwrap();
        c.save(&path).unwrap();
        assert_eq!(AppConfig::load(&path).unwrap(), c);
    }
}
