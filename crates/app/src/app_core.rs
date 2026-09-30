//! Everything the invoke commands and the tray do, without Tauri types, so it can be
//! tested with the platform fakes. `commands.rs` is a thin wrapper over this.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use opit_core::config::{AppConfig, ConfigError};
use opit_core::history::{Dictation, HistoryError};
use opit_core::provider::{OpenAiCompatible, Profile, ProviderError};
use opit_core::rules::prompt::{BuiltPrompt, build_prompt};
use opit_core::rules::{PackError, RuleHit, RulePack, RuleSet, RuleWarning};
use serde::Serialize;
use tracing::{info, warn};

use crate::controller::{ControllerHandle, DictationStatus, ErrorKind};
use crate::history_service::HistoryService;
use crate::platform::{
    Autostart, Hotkey, HotkeyError, Microphone, Overlay, OverlayView, SecretError, SecretStore, Tone,
};
use crate::settings::{Settings, SettingsHandle};
use crate::startup::{Paths, StartupNotice};

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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RulesPreview {
    pub text: String,
    pub hits: Vec<RuleHit>,
    pub warnings: Vec<RuleWarning>,
    pub hallucination: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HotkeyState {
    pub paused: bool,
    /// Why the global hook is not working, if it is not.
    pub error: Option<String>,
}

/// Starter content for a personal pack that does not exist yet.
pub const USER_PACK_TEMPLATE: &str = "schema: 1\nid: user\nname: Personal\nlanguage: tr\nterms: []\n";

const STARTUP_NOTICE_HIDE: Duration = Duration::from_secs(6);

pub struct Platform {
    pub secrets: Arc<dyn SecretStore>,
    pub hotkey: Arc<dyn Hotkey>,
    pub mic: Arc<dyn Microphone>,
    pub overlay: Arc<dyn Overlay>,
    pub autostart: Arc<dyn Autostart>,
}

pub struct AppCore {
    pub paths: Paths,
    pub settings: SettingsHandle,
    pub controller: ControllerHandle,
    pub history: Option<Arc<HistoryService>>,
    pub platform: Platform,
    user_pack: Mutex<Option<RulePack>>,
    system_locale: Option<String>,
    hotkey_paused: AtomicBool,
    hotkey_error: Mutex<Option<String>>,
    notices: Mutex<Vec<StartupNotice>>,
    /// Serializes read-modify-write of config.json.
    config_lock: Mutex<()>,
}

impl AppCore {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        paths: Paths,
        settings: SettingsHandle,
        controller: ControllerHandle,
        history: Option<Arc<HistoryService>>,
        platform: Platform,
        user_pack: Option<RulePack>,
        system_locale: Option<String>,
        notices: Vec<StartupNotice>,
    ) -> Self {
        Self {
            paths,
            settings,
            controller,
            history,
            platform,
            user_pack: Mutex::new(user_pack),
            system_locale,
            hotkey_paused: AtomicBool::new(false),
            hotkey_error: Mutex::default(),
            notices: Mutex::new(notices),
            config_lock: Mutex::default(),
        }
    }

    pub fn config(&self) -> AppConfig {
        self.settings.current().config.clone()
    }

    pub fn status(&self) -> DictationStatus {
        self.controller.status()
    }

    /// Notices from start-up (broken config, broken user.yaml). Returned once.
    pub fn take_notices(&self) -> Vec<StartupNotice> {
        std::mem::take(&mut *self.notices.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The one message worth flashing on the overlay right after start-up, if any.
    pub fn startup_overlay(&self) -> Option<OverlayView> {
        let lang = self.settings.current().lang;
        let notices = self.notices.lock().unwrap_or_else(PoisonError::into_inner);
        let text = if notices.iter().any(|n| matches!(n, StartupNotice::ConfigReset { .. })) {
            lang.config_reset()
        } else if notices.iter().any(|n| matches!(n, StartupNotice::UserRulesBroken { .. })) {
            lang.user_rules_broken()
        } else if self.hotkey_state().error.is_some() {
            lang.hotkey_failed()
        } else {
            return None;
        };
        Some(OverlayView {
            tone: Tone::Warning,
            text: text.into(),
            level: None,
            button: None,
            hide_after: Some(STARTUP_NOTICE_HIDE),
        })
    }

    fn rebuild(&self, config: AppConfig) -> Vec<RuleWarning> {
        let user = self.user_pack.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let (settings, warnings) = Settings::build(config, user, self.system_locale.as_deref());
        for warning in &warnings {
            warn!(pack = %warning.pack_id, message = %warning.message, "rule skipped");
        }
        self.settings.replace(settings);
        warnings
    }

    /// Validates, saves and applies a config from the UI; returns what was stored.
    pub fn save_config(&self, mut config: AppConfig) -> Result<AppConfig, CommandError> {
        let _guard = self.config_lock.lock().unwrap_or_else(PoisonError::into_inner);
        config.normalize();
        let old = self.config();
        config.save(&self.paths.config)?;
        self.rebuild(config.clone());
        if old.hotkey != config.hotkey {
            self.apply_hotkey(&config);
        }
        if old.ui.overlay_position != config.ui.overlay_position {
            self.platform.overlay.set_position(config.ui.overlay_position);
        }
        if old.ui.autostart != config.ui.autostart {
            self.apply_autostart(config.ui.autostart);
        }
        info!("config saved");
        Ok(config)
    }

    pub fn set_active_profile(&self, id: &str) -> Result<(), CommandError> {
        let mut config = self.config();
        if config.profile(id).is_none() {
            return Err(CommandError::new("invalid_input", format!("unknown profile {id}")));
        }
        config.active_profile_id = id.to_string();
        self.save_config(config).map(|_| ())
    }

    /// (Re-)installs the global hook for the configured keys.
    pub fn apply_hotkey(&self, config: &AppConfig) {
        let result = self.platform.hotkey.register(&config.hotkey.keys, self.controller.hotkey_sink());
        let error = result.err().map(|err: HotkeyError| {
            warn!(error = %err, "global shortcut unavailable");
            err.to_string()
        });
        *self.hotkey_error.lock().unwrap_or_else(PoisonError::into_inner) = error;
        self.sync_pause(config);
    }

    pub fn apply_autostart(&self, enabled: bool) {
        if let Err(err) = self.platform.autostart.set_enabled(enabled) {
            warn!(error = %err, "could not update autostart");
        }
    }

    fn sync_pause(&self, config: &AppConfig) {
        let paused = self.hotkey_paused.load(Ordering::SeqCst) || !config.hotkey.enabled;
        self.platform.hotkey.set_paused(paused);
    }

    pub fn set_hotkey_paused(&self, paused: bool) {
        self.hotkey_paused.store(paused, Ordering::SeqCst);
        self.sync_pause(&self.config());
    }

    pub fn hotkey_state(&self) -> HotkeyState {
        HotkeyState {
            paused: self.hotkey_paused.load(Ordering::SeqCst),
            error: self.hotkey_error.lock().unwrap_or_else(PoisonError::into_inner).clone(),
        }
    }

    // ----- API keys -----

    pub fn has_api_key(&self, key_ref: &str) -> Result<bool, CommandError> {
        Ok(self.platform.secrets.get(key_ref)?.is_some_and(|k| !k.trim().is_empty()))
    }

    pub fn set_api_key(&self, key_ref: &str, key: &str) -> Result<(), CommandError> {
        let key = key.trim();
        if key_ref.trim().is_empty() || key.is_empty() {
            return Err(CommandError::new("invalid_input", "the key and its name must not be empty"));
        }
        self.platform.secrets.set(key_ref, key)?;
        info!(key_ref, "API key stored");
        Ok(())
    }

    pub fn delete_api_key(&self, key_ref: &str) -> Result<(), CommandError> {
        self.platform.secrets.delete(key_ref)?;
        info!(key_ref, "API key deleted");
        Ok(())
    }

    /// Tests `profile` with `api_key`, or with the stored key when `api_key` is `None`.
    pub async fn test_connection(&self, profile: Profile, api_key: Option<String>) -> Result<(), CommandError> {
        let key = match api_key {
            Some(key) => Some(key),
            None => match &profile.api_key_ref {
                Some(key_ref) => self.platform.secrets.get(key_ref)?,
                None => None,
            },
        };
        let client = OpenAiCompatible::new(profile, key)?;
        Ok(client.test_connection().await?)
    }

    // ----- rules -----

    pub fn user_rules_yaml(&self) -> Result<String, CommandError> {
        match std::fs::read_to_string(&self.paths.user_rules) {
            Ok(yaml) => Ok(yaml),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(USER_PACK_TEMPLATE.to_string()),
            Err(err) => Err(CommandError::new("rules", err.to_string())),
        }
    }

    /// Validates and stores `user.yaml`, then recompiles the rules; returns skipped-rule warnings.
    pub fn save_user_rules(&self, yaml: &str) -> Result<Vec<RuleWarning>, CommandError> {
        let pack = RulePack::from_yaml(yaml)?;
        if let Some(dir) = self.paths.user_rules.parent() {
            std::fs::create_dir_all(dir).map_err(|e| CommandError::new("rules", e.to_string()))?;
        }
        let tmp = self.paths.user_rules.with_extension("yaml.tmp");
        std::fs::write(&tmp, yaml)
            .and_then(|()| std::fs::rename(&tmp, &self.paths.user_rules))
            .map_err(|e| CommandError::new("rules", e.to_string()))?;
        *self.user_pack.lock().unwrap_or_else(PoisonError::into_inner) = Some(pack);
        info!("user rules saved");
        Ok(self.rebuild(self.config()))
    }

    /// Runs `text` through the rules, optionally with an unsaved `user.yaml` draft.
    pub fn rules_preview(&self, text: &str, draft_yaml: Option<&str>) -> Result<RulesPreview, CommandError> {
        let current = self.settings.current();
        let (rules, warnings) = match draft_yaml {
            None => (current.rules.clone(), Vec::new()),
            Some(yaml) => {
                let draft = RulePack::from_yaml(yaml)?;
                let packs = opit_core::rules::builtin::assemble(Some(draft), &current.config.rules.enabled_packs);
                let (rules, warnings) = RuleSet::compile(&packs);
                (Arc::new(rules), warnings)
            }
        };
        let hallucination = rules.is_hallucination(text.trim());
        let (text, hits) = rules.apply_traced(text.trim());
        Ok(RulesPreview { text, hits, warnings, hallucination })
    }

    /// The Whisper prompt the active profile would send right now.
    pub fn prompt_budget(&self) -> BuiltPrompt {
        let current = self.settings.current();
        let language = current.config.active_profile().map_or("tr", |p| p.language.as_str());
        build_prompt(&current.config.rules.prompt_context, current.rules.terms(), language)
    }

    // ----- history -----

    fn history(&self) -> Result<&HistoryService, CommandError> {
        self.history.as_deref().ok_or_else(|| CommandError::new("unavailable", "history is not available"))
    }

    pub fn history_recent(&self, limit: usize, before_id: Option<i64>) -> Result<Vec<Dictation>, CommandError> {
        Ok(self.history()?.store().recent(limit.min(500), before_id)?)
    }

    pub fn history_search(&self, query: &str, limit: usize) -> Result<Vec<Dictation>, CommandError> {
        Ok(self.history()?.store().search(query, limit.min(500))?)
    }

    pub fn history_delete(&self, id: i64) -> Result<(), CommandError> {
        Ok(self.history()?.delete(id)?)
    }

    pub fn history_clear(&self) -> Result<(), CommandError> {
        Ok(self.history()?.clear()?)
    }

    pub fn microphones(&self) -> Vec<String> {
        self.platform.mic.devices()
    }
}

#[cfg(test)]
mod tests {
    use opit_core::config::{HotkeyMode, OverlayPosition};
    use opit_core::history::{HistoryStore, NewDictation};
    use opit_core::pipeline::TranscriptStatus;

    use super::*;
    use crate::controller::{HistorySink, channel};
    use crate::platform::fake::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        core: AppCore,
        hotkey: Arc<FakeHotkey>,
        overlay: Arc<FakeOverlay>,
        autostart: Arc<FakeAutostart>,
        secrets: Arc<FakeSecrets>,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path().join("opit"));
        let settings = SettingsHandle::new(Settings::build(AppConfig::default(), None, Some("en-US")).0);
        let history =
            Arc::new(HistoryService::with_store(HistoryStore::open_in_memory().unwrap(), paths.audio.clone()));
        let (hotkey, overlay) = (Arc::new(FakeHotkey::default()), Arc::new(FakeOverlay::default()));
        let (autostart, secrets) = (Arc::new(FakeAutostart::default()), Arc::new(FakeSecrets::default()));
        let platform = Platform {
            secrets: secrets.clone(),
            hotkey: hotkey.clone(),
            mic: Arc::new(FakeMic::default()),
            overlay: overlay.clone(),
            autostart: autostart.clone(),
        };
        let core = AppCore::new(paths, settings, channel().0, Some(history), platform, None, None, Vec::new());
        Fixture { _dir: dir, core, hotkey, overlay, autostart, secrets }
    }

    #[test]
    fn save_config_normalizes_writes_and_applies_side_effects() {
        let f = fixture();
        let mut config = f.core.config();
        config.recording.max_seconds = 9_999;
        config.hotkey.mode = HotkeyMode::PushToTalk;
        config.hotkey.keys = vec!["F9".into()];
        config.ui.overlay_position = OverlayPosition::TopCenter;
        config.ui.autostart = false;
        *f.autostart.enabled.lock().unwrap() = true;

        let saved = f.core.save_config(config).unwrap();
        assert_eq!(saved.recording.max_seconds, 600);
        assert_eq!(AppConfig::load(&f.core.paths.config).unwrap(), saved);
        assert_eq!(f.core.settings.current().config, saved);
        assert_eq!(*f.hotkey.registered.lock().unwrap(), [vec!["F9".to_string()]]);
        assert_eq!(*f.overlay.positions.lock().unwrap(), [OverlayPosition::TopCenter]);
        assert!(!*f.autostart.enabled.lock().unwrap());
    }

    #[test]
    fn unchanged_hotkey_is_not_reinstalled() {
        let f = fixture();
        let mut config = f.core.config();
        config.paste.trailing_space = false;
        f.core.save_config(config).unwrap();
        assert!(f.hotkey.registered.lock().unwrap().is_empty());
    }

    #[test]
    fn hotkey_failures_and_pause_are_reported() {
        let f = fixture();
        *f.hotkey.fail_register.lock().unwrap() = Some(HotkeyError::UnknownKey("Hyper".into()));
        f.core.apply_hotkey(&f.core.config());
        assert_eq!(f.core.hotkey_state().error.as_deref(), Some("unknown key name: Hyper"));

        f.core.set_hotkey_paused(true);
        assert!(*f.hotkey.paused.lock().unwrap() && f.core.hotkey_state().paused);
        f.core.set_hotkey_paused(false);
        assert!(!*f.hotkey.paused.lock().unwrap());

        let mut config = f.core.config();
        config.hotkey.enabled = false;
        f.core.save_config(config).unwrap();
        assert!(*f.hotkey.paused.lock().unwrap(), "a disabled shortcut stays paused");
    }

    #[test]
    fn tray_profile_switch_is_saved() {
        let f = fixture();
        f.core.set_active_profile("openai").unwrap();
        assert_eq!(AppConfig::load(&f.core.paths.config).unwrap().active_profile_id, "openai");
        assert_eq!(f.core.set_active_profile("nope").unwrap_err().code, "invalid_input");
    }

    #[test]
    fn api_keys_are_trimmed_and_blank_ones_rejected() {
        let f = fixture();
        assert!(!f.core.has_api_key("groq").unwrap());
        f.core.set_api_key("groq", "  sk-live\n").unwrap();
        assert_eq!(f.secrets.map.lock().unwrap()["groq"], "sk-live");
        assert!(f.core.has_api_key("groq").unwrap());
        assert_eq!(f.core.set_api_key("groq", "   ").unwrap_err().code, "invalid_input");
        f.core.delete_api_key("groq").unwrap();
        assert!(!f.core.has_api_key("groq").unwrap());
    }

    #[test]
    fn user_rules_round_trip_and_take_effect() {
        let f = fixture();
        assert_eq!(f.core.user_rules_yaml().unwrap(), USER_PACK_TEMPLATE);
        let yaml = "schema: 1\nid: user\nname: Me\ncorrections:\n  Opit: [opid]\n";
        let warnings = f.core.save_user_rules(yaml).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(f.core.user_rules_yaml().unwrap(), yaml);
        assert_eq!(f.core.settings.current().rules.apply("opid çalışıyor"), "Opit çalışıyor");
    }

    #[test]
    fn invalid_user_rules_are_rejected_and_nothing_is_written() {
        let f = fixture();
        let err = f.core.save_user_rules("schema: 2\nid: user\nname: Me\n").unwrap_err();
        assert_eq!(err.code, "rules");
        assert!(!f.core.paths.user_rules.exists());
    }

    #[test]
    fn preview_uses_a_draft_without_saving_it() {
        let f = fixture();
        let draft = "schema: 1\nid: user\nname: Me\ncorrections:\n  Opit: [opid]\nreplacements:\n  - { from: '(', to: x, regex: true }\n";
        let preview = f.core.rules_preview("opid ve cloud code", Some(draft)).unwrap();
        assert_eq!(preview.text, "Opit ve Claude Code");
        assert_eq!(preview.warnings.len(), 1);
        assert!(preview.hits.len() >= 2);
        assert_eq!(f.core.rules_preview("opid", None).unwrap().text, "opid");
        assert!(f.core.rules_preview("Altyazı M.K.", None).unwrap().hallucination);
    }

    #[test]
    fn prompt_budget_reflects_the_context_sentence() {
        let f = fixture();
        let mut config = f.core.config();
        config.rules.prompt_context = "Yazılım konuşması".into();
        f.core.save_config(config).unwrap();
        let prompt = f.core.prompt_budget();
        assert!(prompt.prompt.unwrap().starts_with("Yazılım konuşması. Geçen terimler:"));
    }

    #[test]
    fn history_commands_work_and_are_capped() {
        let f = fixture();
        let history = f.core.history.clone().unwrap();
        for i in 0..3 {
            let entry = NewDictation {
                created_at_ms: i,
                profile_id: "groq",
                raw_text: "github",
                text: "GitHub",
                status: TranscriptStatus::Ok,
                audio_ms: 1,
                latency_ms: 1,
            };
            history.record(&entry, None).unwrap();
        }
        assert_eq!(f.core.history_recent(2, None).unwrap().len(), 2);
        assert_eq!(f.core.history_search("git", 10).unwrap().len(), 3);
        let first = f.core.history_recent(1, None).unwrap()[0].id;
        f.core.history_delete(first).unwrap();
        assert_eq!(f.core.history_recent(10, None).unwrap().len(), 2);
        f.core.history_clear().unwrap();
        assert!(f.core.history_recent(10, None).unwrap().is_empty());
    }

    #[test]
    fn startup_overlay_picks_the_most_important_notice() {
        let f = fixture();
        assert_eq!(f.core.startup_overlay(), None);
        *f.hotkey.fail_register.lock().unwrap() = Some(HotkeyError::Install("denied".into()));
        f.core.apply_hotkey(&f.core.config());
        assert_eq!(f.core.startup_overlay().unwrap().text, "Shortcut unavailable — use the tray icon");
        f.core.notices.lock().unwrap().push(StartupNotice::UserRulesBroken { reason: "x".into() });
        f.core.notices.lock().unwrap().push(StartupNotice::ConfigReset { backup: None, reason: "y".into() });
        let view = f.core.startup_overlay().unwrap();
        assert_eq!((view.tone, view.text.as_str()), (Tone::Warning, "Settings file was damaged; defaults loaded"));
    }

    #[test]
    fn notices_are_handed_out_once() {
        let f = fixture();
        f.core.notices.lock().unwrap().push(StartupNotice::UserRulesBroken { reason: "x".into() });
        assert_eq!(f.core.take_notices().len(), 1);
        assert!(f.core.take_notices().is_empty());
    }

    #[test]
    fn provider_errors_keep_their_kind_for_the_ui() {
        let err = CommandError::from(ProviderError::Unauthorized(401));
        assert_eq!((err.code, err.kind), ("provider", Some(ErrorKind::InvalidKey)));
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "invalid_key");
    }
}
