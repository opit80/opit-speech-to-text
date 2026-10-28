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
pub const MAX_HISTORY_ENTRIES: u32 = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalConfig {
    pub build: String,
    pub threads: u16,
    pub gpu: bool,
    pub idle_minutes: u16,
    pub preload: bool,
    /// An ASCII absolute folder. Only the app's own child directory is managed.
    pub models_folder: Option<String>,
}

impl Default for LocalConfig {
    fn default() -> Self {
        Self { build: "blas".into(), threads: 0, gpu: false, idle_minutes: 10, preload: false, models_folder: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub media: MediaConfig,
    pub history: HistoryConfig,
    pub learning: crate::learning::LearningConfig,
    pub rules: RulesConfig,
    pub ask: crate::ask::AskConfig,
    pub cleanup: crate::cleanup::CleanupConfig,
    pub translate: crate::voice_modes::TranslateConfig,
    pub app_rules: crate::app_rules::AppRulesConfig,
    pub local: LocalConfig,
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
            media: MediaConfig::default(),
            history: HistoryConfig::default(),
            learning: crate::learning::LearningConfig::default(),
            rules: RulesConfig::default(),
            ask: crate::ask::AskConfig::default(),
            cleanup: crate::cleanup::CleanupConfig::default(),
            translate: crate::voice_modes::TranslateConfig::default(),
            app_rules: crate::app_rules::AppRulesConfig::default(),
            local: LocalConfig::default(),
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaMode {
    #[default]
    Pause,
    Duck,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaConfig {
    pub enabled: bool,
    pub mode: MediaMode,
    pub duck_volume_percent: u8,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self { enabled: true, mode: MediaMode::Pause, duck_volume_percent: 20 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyConfig {
    /// Key names understood by the app's hotkey hook, e.g. `RightCtrl`.
    pub keys: Vec<String>,
    /// Empty disables the optional recording pause shortcut.
    pub pause_keys: Vec<String>,
    pub ask_keys: Vec<String>,
    pub edit_keys: Vec<String>,
    pub translate_keys: Vec<String>,
    pub style_cycle_keys: Vec<String>,
    pub mode: HotkeyMode,
    pub enabled: bool,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            keys: vec!["RightCtrl".into(), "RightShift".into()],
            pause_keys: Vec::new(),
            ask_keys: Vec::new(),
            edit_keys: Vec::new(),
            translate_keys: Vec::new(),
            style_cycle_keys: Vec::new(),
            mode: HotkeyMode::Toggle,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingConfig {
    /// Device name; `None` = system default.
    pub microphone: Option<String>,
    pub max_seconds: u32,
    /// Absolute speech floor in dBFS (20 log10 of the former 0.008 RMS floor).
    pub speech_floor_db: f64,
    /// Required margin above this recording's noise floor (formerly 2x RMS).
    pub noise_margin_db: f64,
    pub min_speech_seconds: f64,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            microphone: None,
            max_seconds: 180,
            speech_floor_db: -41.938200260161125,
            noise_margin_db: 6.020599913279624,
            min_speech_seconds: 0.3,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteMethod {
    #[default]
    CtrlV,
    ShiftInsert,
    CtrlShiftV,
    Type,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteAfter {
    #[default]
    None,
    Enter,
    CtrlEnter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PasteConfig {
    pub method: PasteMethod,
    pub after: PasteAfter,
    /// Opt in to restoring the previous clipboard; otherwise keep the latest dictation.
    pub restore_clipboard: bool,
    pub trailing_space: bool,
}

impl Default for PasteConfig {
    fn default() -> Self {
        Self {
            method: PasteMethod::default(),
            after: PasteAfter::default(),
            restore_clipboard: false,
            trailing_space: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryConfig {
    pub enabled: bool,
    pub save_audio: bool,
    pub audio_retention_days: u32,
    /// Maximum retained rows, including failures; zero disables the row cap.
    pub max_entries: u32,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self { enabled: true, save_audio: false, audio_retention_days: 30, max_entries: 1000 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RulesConfig {
    pub enabled_packs: Vec<String>,
    /// Short context sentence placed at the start of the Whisper prompt.
    pub prompt_context: String,
    /// Spell standalone integers in the transcription profile's language.
    pub numbers_as_words: bool,
}

impl Default for RulesConfig {
    fn default() -> Self {
        Self {
            enabled_packs: vec!["tr-core".into(), "tr-tech".into()],
            prompt_context: String::new(),
            numbers_as_words: false,
        }
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub theme: Theme,
    pub start_in_tray: bool,
    pub autostart: bool,
    pub overlay_position: OverlayPosition,
    pub sound_feedback: bool,
    /// The first-run wizard was finished or skipped.
    pub setup_done: bool,
    /// Ask GitHub for a newer version at start-up and once a day (release builds only).
    pub check_updates: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            start_in_tray: false,
            autostart: true,
            overlay_position: OverlayPosition::RightCenter,
            sound_feedback: true,
            setup_done: false,
            check_updates: true,
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
            // Notepad and PowerShell 5 start the file with a UTF-8 BOM, which JSON does not allow.
            Ok(src) => Self::from_json(src.strip_prefix('\u{FEFF}').unwrap_or(&src)),
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
        self.translate.normalize();
        self.cleanup.normalize_styles();
        self.normalize_shortcuts();
        self.media.duck_volume_percent = self.media.duck_volume_percent.min(100);
        self.history.max_entries = self.history.max_entries.min(MAX_HISTORY_ENTRIES);
        self.cleanup.timeout_seconds = self.cleanup.timeout_seconds.clamp(1, 120);
        self.schema_version = CONFIG_SCHEMA_VERSION;
        self.recording.max_seconds = self.recording.max_seconds.clamp(MIN_RECORDING_SECONDS, MAX_RECORDING_SECONDS);
        let defaults = RecordingConfig::default();
        for (value, default, min, max) in [
            (&mut self.recording.speech_floor_db, defaults.speech_floor_db, -90.0, -6.0),
            (&mut self.recording.noise_margin_db, defaults.noise_margin_db, 0.0, 30.0),
            (&mut self.recording.min_speech_seconds, defaults.min_speech_seconds, 0.03, 5.0),
        ] {
            *value = if value.is_finite() { value.clamp(min, max) } else { default };
        }

        let mut seen = HashSet::new();
        self.profiles.retain(|p| seen.insert(p.id.clone()));
        if self.profiles.is_empty() {
            self.profiles.push(presets::groq());
        }
        if self.profile(&self.active_profile_id).is_none() {
            self.active_profile_id = self.profiles[0].id.clone();
        }
        for profile in &mut self.profiles {
            if profile.provider == crate::provider::ProviderKind::LocalWhisper {
                profile.api_key_ref = None;
                profile.audio_format = crate::audio::encode::AudioFormat::Wav;
                profile.send_keywords = false;
            }
            let valid = profile.fallback_profile_id.as_ref().is_some_and(|f| f != &profile.id && seen.contains(f));
            if !valid {
                profile.fallback_profile_id = None;
            }
        }
        self.app_rules.normalize(
            &self.cleanup.styles.iter().map(|s| s.id.clone()).collect(),
            &self.profiles.iter().map(|p| p.id.clone()).collect(),
        );
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    /// Startup/hand-edited configs keep the first valid shortcut, clearing later conflicts.
    /// Interactive saves validate before normalization so mistakes are reported to the user.
    pub fn normalize_shortcuts(&mut self) {
        if crate::hotkey::parse_combo(&self.hotkey.keys).is_err() {
            self.hotkey.keys = HotkeyConfig::default().keys;
        }
        let mut accepted = vec![self.hotkey.keys.clone()];
        for keys in std::iter::once(&mut self.hotkey.pause_keys)
            .chain(std::iter::once(&mut self.hotkey.ask_keys))
            .chain(std::iter::once(&mut self.hotkey.style_cycle_keys))
            .chain(std::iter::once(&mut self.hotkey.edit_keys))
            .chain(std::iter::once(&mut self.hotkey.translate_keys))
            .chain(self.cleanup.styles.iter_mut().map(|style| &mut style.hotkey))
        {
            if keys.is_empty() {
                continue;
            }
            if accepted.iter().any(|previous| crate::hotkey::validate_shortcuts(previous, keys).is_err()) {
                keys.clear();
            } else {
                accepted.push(keys.clone());
            }
        }
    }

    pub fn shortcuts(&self) -> Vec<&[String]> {
        std::iter::once(self.hotkey.keys.as_slice())
            .chain(std::iter::once(self.hotkey.pause_keys.as_slice()))
            .chain(std::iter::once(self.hotkey.ask_keys.as_slice()))
            .chain(std::iter::once(self.hotkey.style_cycle_keys.as_slice()))
            .chain(std::iter::once(self.hotkey.edit_keys.as_slice()))
            .chain(std::iter::once(self.hotkey.translate_keys.as_slice()))
            .chain(self.cleanup.styles.iter().map(|style| style.hotkey.as_slice()))
            .collect()
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

    #[test]
    fn voice_modes_default_off_normalize_languages_and_validate_all_shortcuts() {
        let old = AppConfig::from_json("{}").unwrap();
        assert!(old.hotkey.edit_keys.is_empty() && old.hotkey.translate_keys.is_empty());
        assert_eq!(old.translate, crate::voice_modes::TranslateConfig::default());
        let mut config = old;
        config.hotkey.edit_keys = vec!["F18".into()];
        config.hotkey.translate_keys = vec!["F19".into()];
        config.translate.target_language = " TR ".into();
        config.translate.model = Some("  custom-model  ".into());
        config.normalize();
        assert_eq!(config.translate.target_language, "tr");
        assert_eq!(config.translate.model.as_deref(), Some("custom-model"));
        assert_eq!(AppConfig::from_json(&config.to_json()).unwrap(), config);
        let mut other = config.shortcuts().iter().map(|keys| keys.to_vec()).collect::<Vec<_>>();
        other.retain(|keys| !keys.is_empty() && *keys != config.hotkey.translate_keys);
        for conflict in
            other.into_iter().chain([vec!["F18".into(), "LeftCtrl".into()], vec!["Escape".into()], vec!["bad".into()]])
        {
            config.hotkey.translate_keys = conflict;
            assert!(crate::hotkey::validate_list(&config.shortcuts()).is_err());
            config.normalize();
            assert!(config.hotkey.translate_keys.is_empty());
        }
        config.translate.target_language = "unknown".into();
        config.translate.model = Some("  ".into());
        config.normalize();
        assert_eq!(config.translate, crate::voice_modes::TranslateConfig::default());
    }

    #[test]
    fn cleanup_styles_load_old_configs_and_normalize_shortcut_conflicts() {
        let old = AppConfig::from_json(r#"{"cleanup":{"enabled":true,"prompt_override":"Legacy"}}"#).unwrap();
        assert_eq!(old.cleanup.active_style_id, "clean");
        assert_eq!(old.cleanup.style("clean").unwrap().prompt, "Legacy");
        assert!(old.cleanup.prompt_override.is_empty());
        assert!(old.hotkey.style_cycle_keys.is_empty());
        assert!(old.cleanup.styles.iter().all(|s| s.in_cycle && s.hotkey.is_empty()));
        assert_eq!(AppConfig::from_json(&old.to_json()).unwrap(), old);
        let mut c = old;
        c.hotkey.keys = vec!["F13".into()];
        c.hotkey.pause_keys = vec!["F14".into()];
        c.hotkey.ask_keys = vec!["F15".into()];
        c.hotkey.style_cycle_keys = vec!["F16".into()];
        c.cleanup.styles[0].hotkey = vec!["F17".into()];
        for conflict in [
            vec!["f13".into()],
            vec!["F14".into(), "LeftCtrl".into()],
            vec!["F15".into()],
            vec!["F16".into()],
            vec!["F17".into()],
            vec!["Escape".into()],
            vec!["Unknown".into()],
        ] {
            c.cleanup.styles[1].hotkey = conflict;
            assert!(crate::hotkey::validate_list(&c.shortcuts()).is_err());
            c.normalize();
            assert!(c.cleanup.styles[1].hotkey.is_empty());
        }
        c.hotkey.style_cycle_keys = vec!["F13".into()];
        c.normalize();
        assert!(c.hotkey.style_cycle_keys.is_empty());
        assert!(crate::hotkey::validate_list(&c.shortcuts()).is_ok());
    }

    #[test]
    fn media_defaults_are_backward_compatible_and_partial_objects_keep_defaults() {
        for json in [r#"{}"#, r#"{"media":{}}"#] {
            let config = AppConfig::from_json(json).unwrap();
            assert_eq!(config.media, MediaConfig { enabled: true, mode: MediaMode::Pause, duck_volume_percent: 20 });
        }
        let config =
            AppConfig::from_json(r#"{"media":{"enabled":false,"mode":"duck","duck_volume_percent":255}}"#).unwrap();
        assert!(!config.media.enabled);
        assert_eq!(config.media.mode, MediaMode::Duck);
        assert_eq!(config.media.duck_volume_percent, 100);
        assert_eq!(AppConfig::from_json(&config.to_json()).unwrap(), config);
        assert_eq!(
            AppConfig::from_json(r#"{"media":{"duck_volume_percent":0}}"#).unwrap().media.duck_volume_percent,
            0
        );
        for json in [
            r#"{"media":{"mode":"toggle"}}"#,
            r#"{"media":{"duck_volume_percent":-1}}"#,
            r#"{"media":{"duck_volume_percent":256}}"#,
        ] {
            assert!(AppConfig::from_json(json).is_err());
        }
    }

    #[test]
    fn history_cap_and_theme_defaults_normalize_and_round_trip() {
        let mut c = AppConfig::from_json(r#"{"history":{},"ui":{}}"#).unwrap();
        assert_eq!(c.history.max_entries, 1000);
        assert_eq!(c.ui.theme, Theme::System);
        for cap in [0, 1, 1000, 100_000] {
            c.history.max_entries = cap;
            c.normalize();
            assert_eq!(c.history.max_entries, cap);
        }
        c.history.max_entries = u32::MAX;
        c.normalize();
        assert_eq!(c.history.max_entries, 100_000);
        for theme in [Theme::System, Theme::Light, Theme::Dark] {
            c.ui.theme = theme;
            assert_eq!(AppConfig::from_json(&c.to_json()).unwrap(), c);
        }
        assert!(AppConfig::from_json(r#"{"ui":{"theme":"unknown"}}"#).is_err());
        assert!(AppConfig::from_json(r#"{"history":{"max_entries":-1}}"#).is_err());
    }

    #[test]
    fn ask_defaults_are_optional_and_existing_config_files_remain_compatible() {
        let config = AppConfig::from_json("{}").unwrap();
        assert!(config.hotkey.ask_keys.is_empty());
        assert_eq!(config.ask, crate::ask::AskConfig::default());
        assert_eq!(config.ask.timeout_seconds, 180);
        assert_eq!(config.ask.continue_minutes, 10);
        let mut config = config;
        config.ask.agent = crate::ask::Agent::Claude;
        config.ask.permission = crate::ask::Permission::Workspace;
        config.hotkey.ask_keys = vec!["F15".into()];
        assert_eq!(AppConfig::from_json(&serde_json::to_string(&config).unwrap()).unwrap(), config);
    }

    #[test]
    fn local_settings_migrate_without_changing_old_cloud_profiles() {
        let old = AppConfig::from_json("{}").unwrap();
        assert_eq!(old.local, LocalConfig::default());
        assert_eq!(old.profiles, vec![presets::groq(), presets::openai()]);
        let mut config = old;
        let mut local = presets::local();
        local.api_key_ref = Some("unwanted".into());
        local.audio_format = crate::audio::encode::AudioFormat::Flac;
        config.profiles.push(local);
        config.profiles[0].fallback_profile_id = Some("local".into());
        config.profiles[2].fallback_profile_id = Some("groq".into());
        config.normalize();
        let local = &config.profiles[2];
        assert_eq!(local.api_key_ref, None);
        assert_eq!(local.audio_format, crate::audio::encode::AudioFormat::Wav);
        assert_eq!(config.fallback_for(local).unwrap().id, "groq");
        assert_eq!(config.fallback_for(&config.profiles[0]).unwrap().id, "local");
        let value = serde_json::to_value(local).unwrap();
        assert_eq!(value["provider"], "local_whisper");
        assert_eq!(AppConfig::from_json(&config.to_json()).unwrap(), config);
    }

    #[test]
    fn cleanup_is_opt_in_and_old_profiles_default_to_applying_it() {
        let config = AppConfig::from_json("{}").unwrap();
        assert!(!config.cleanup.enabled);
        assert_eq!(config.cleanup.base_url, "https://inference.baseten.co/v1");
        assert_eq!(config.cleanup.model, "deepseek-ai/DeepSeek-V4.1-Flash-Fast");
        let mut json = serde_json::to_value(config).unwrap();
        json.as_object_mut().unwrap().remove("cleanup");
        for profile in json["profiles"].as_array_mut().unwrap() {
            profile.as_object_mut().unwrap().remove("apply_cleanup");
        }
        let mut old = AppConfig::from_json(&json.to_string()).unwrap();
        assert!(!old.cleanup.enabled);
        assert!(old.profiles.iter().all(|p| p.apply_cleanup));
        old.cleanup.enabled = true;
        old.cleanup.reasoning_effort = Some("none".into());
        old.profiles[0].apply_cleanup = false;
        assert_eq!(AppConfig::from_json(&old.to_json()).unwrap(), old);
        old.cleanup.timeout_seconds = 0;
        old.normalize();
        assert_eq!(old.cleanup.timeout_seconds, 1);
    }
    use crate::provider::presets;

    #[test]
    fn speech_detection_defaults_load_legacy_configs_and_validate_ranges() {
        let c = AppConfig::from_json(r#"{"recording":{"max_seconds":60}}"#).unwrap();
        assert_eq!(c.recording.speech_floor_db, RecordingConfig::default().speech_floor_db);
        assert_eq!(c.recording.noise_margin_db, RecordingConfig::default().noise_margin_db);
        assert_eq!(c.recording.min_speech_seconds, 0.3);
        let mut c = c;
        c.recording.speech_floor_db = -200.0;
        c.recording.noise_margin_db = -1.0;
        c.recording.min_speech_seconds = 50.0;
        c.normalize();
        assert_eq!(
            (c.recording.speech_floor_db, c.recording.noise_margin_db, c.recording.min_speech_seconds),
            (-90.0, 0.0, 5.0)
        );
        c.recording.speech_floor_db = f64::NAN;
        c.recording.noise_margin_db = f64::INFINITY;
        c.recording.min_speech_seconds = f64::NEG_INFINITY;
        c.normalize();
        assert_eq!(c.recording.speech_floor_db, RecordingConfig::default().speech_floor_db);
        assert_eq!(c.recording.noise_margin_db, RecordingConfig::default().noise_margin_db);
        assert_eq!(c.recording.min_speech_seconds, 0.3);
        assert_eq!(AppConfig::from_json(&c.to_json()).unwrap(), c);
    }
    #[test]
    fn optional_pause_shortcut_is_compatible_with_existing_configs() {
        let old = r#"{"hotkey":{"keys":["F13"],"mode":"push_to_talk","enabled":true}}"#;
        let mut config = AppConfig::from_json(old).unwrap();
        assert!(config.hotkey.pause_keys.is_empty());
        assert_eq!(config.hotkey.keys, ["F13"]);
        config.hotkey.pause_keys = vec!["F14".into()];
        assert_eq!(AppConfig::from_json(&config.to_json()).unwrap(), config);
    }

    #[test]
    fn defaults_match_the_brief() {
        let c = AppConfig::default();
        assert_eq!(c.schema_version, CONFIG_SCHEMA_VERSION);
        assert_eq!(c.active_profile_id, "groq");
        assert_eq!(c.profiles, vec![presets::groq(), presets::openai()]);
        assert_eq!(c.hotkey.keys, ["RightCtrl", "RightShift"]);
        assert_eq!(c.hotkey.mode, HotkeyMode::Toggle);
        assert!(c.hotkey.pause_keys.is_empty());
        assert_eq!(c.recording.max_seconds, 180);
        assert!(!c.paste.restore_clipboard && c.paste.trailing_space);
        assert!(c.history.enabled && !c.history.save_audio);
        assert_eq!(c.history.audio_retention_days, 30);
        assert_eq!(c.rules.enabled_packs, ["tr-core", "tr-tech"]);
        assert!(!c.ui.start_in_tray && c.ui.autostart && c.ui.sound_feedback);
        assert_eq!(c.ui.overlay_position, OverlayPosition::RightCenter);
        assert!(!c.ui.setup_done);
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
        assert!(!c.paste.restore_clipboard);
        assert_eq!(c.profiles.len(), 2);
    }

    #[test]
    fn clipboard_restore_remains_an_explicit_opt_in() {
        let c = AppConfig::from_json(r#"{"paste":{"restore_clipboard":true}}"#).unwrap();
        assert!(c.paste.restore_clipboard);
        assert!(AppConfig::from_json(&c.to_json()).unwrap().paste.restore_clipboard);
    }

    #[test]
    fn paste_methods_and_after_keys_round_trip_with_legacy_defaults() {
        let legacy = AppConfig::from_json(r#"{"paste":{"restore_clipboard":true,"trailing_space":false}}"#).unwrap();
        assert_eq!(legacy.paste.method, PasteMethod::CtrlV);
        assert_eq!(legacy.paste.after, PasteAfter::None);
        assert!(legacy.paste.restore_clipboard);
        assert!(!legacy.paste.trailing_space);
        assert_eq!(AppConfig::from_json("{}").unwrap().paste, PasteConfig::default());
        for (name, method) in [
            ("ctrl_v", PasteMethod::CtrlV),
            ("shift_insert", PasteMethod::ShiftInsert),
            ("ctrl_shift_v", PasteMethod::CtrlShiftV),
            ("type", PasteMethod::Type),
        ] {
            for (after_name, after) in
                [("none", PasteAfter::None), ("enter", PasteAfter::Enter), ("ctrl_enter", PasteAfter::CtrlEnter)]
            {
                let json = serde_json::json!({"paste": {"method": name, "after": after_name}}).to_string();
                let config = AppConfig::from_json(&json).unwrap();
                assert_eq!(config.paste.method, method);
                assert_eq!(config.paste.after, after);
                let saved: Value = serde_json::from_str(&config.to_json()).unwrap();
                assert_eq!(saved["paste"]["method"], name);
                assert_eq!(saved["paste"]["after"], after_name);
            }
        }
        assert!(AppConfig::from_json(r#"{"paste":{"method":"unknown"}}"#).is_err());
        assert!(AppConfig::from_json(r#"{"paste":{"after":"unknown"}}"#).is_err());
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
    fn number_spelling_defaults_off_for_existing_configs_and_round_trips() {
        let old = AppConfig::from_json(r#"{"rules":{"enabled_packs":[],"prompt_context":""}}"#).unwrap();
        assert!(!old.rules.numbers_as_words);
        let mut config = old;
        config.rules.numbers_as_words = true;
        assert!(AppConfig::from_json(&config.to_json()).unwrap().rules.numbers_as_words);
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
    fn a_leading_utf8_bom_is_ignored() {
        // Notepad and PowerShell 5 write one.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "\u{FEFF}{\"paste\":{\"trailing_space\":false}}").unwrap();
        assert!(!AppConfig::load(&path).unwrap().paste.trailing_space);
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

    #[test]
    fn a_config_without_setup_done_loads_as_not_done() {
        let json = r#"{"schema_version":1,"ui":{"start_in_tray":true}}"#;
        let c = AppConfig::from_json(json).unwrap();
        assert!(c.ui.start_in_tray);
        assert!(!c.ui.setup_done);
    }

    #[test]
    fn update_checks_default_on_also_for_older_configs() {
        assert!(AppConfig::default().ui.check_updates);
        let c = AppConfig::from_json(r#"{"schema_version":1,"ui":{"start_in_tray":true}}"#).unwrap();
        assert!(c.ui.check_updates);
    }
}
