//! Everything the invoke commands and the tray do, without Tauri types, so it can be
//! tested with the platform fakes. `commands.rs` is a thin wrapper over this.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use opit_core::config::{AppConfig, ConfigError};
use opit_core::history::{Dictation, HistoryError};
use opit_core::provider::{OpenAiCompatible, Profile, ProviderError};
use opit_core::rules::pack::{CorrectionOutcome, header_comments};
use opit_core::rules::prompt::{BuiltPrompt, build_prompt};
use opit_core::rules::{PackError, RuleHit, RulePack, RuleSet, RuleWarning};
use serde::Serialize;
use tracing::{info, warn};

use crate::controller::{ControllerHandle, DictationState, DictationStatus, ErrorKind};
use crate::history_service::HistoryService;
use crate::platform::{
    Autostart, Capture, CaptureEvent, CaptureSink, Hotkey, HotkeyError, Microphone, Overlay, OverlayView, SecretError,
    SecretStore, Tone,
};
use crate::settings::{Settings, SettingsHandle};
use crate::startup::{self, Paths, StartupNotice};

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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CorrectionDraft {
    /// The new `user.yaml` text; not saved yet.
    pub yaml: String,
    pub outcome: CorrectionOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HotkeyState {
    pub paused: bool,
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
    /// The running microphone test and its generation.
    mic_test: Mutex<Option<(u64, Box<dyn Capture>)>>,
    mic_test_generation: AtomicU64,
    /// The UI is recording a new shortcut; the hook is paused meanwhile.
    hotkey_capturing: AtomicBool,
    hotkey_capture_generation: AtomicU64,
    notices: Mutex<Vec<StartupNotice>>,
    /// Serializes read-modify-write of config.json.
    config_lock: Mutex<()>,
    /// Start-up loaded defaults but could not move the damaged config.json aside (it may be
    /// fine, just locked). While set, saving must not overwrite that file.
    bad_config_kept: AtomicBool,
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
        let bad_config_kept = notices.iter().any(|n| matches!(n, StartupNotice::ConfigReset { backup: None, .. }));
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
            mic_test: Mutex::default(),
            mic_test_generation: AtomicU64::new(0),
            hotkey_capturing: AtomicBool::new(false),
            hotkey_capture_generation: AtomicU64::new(0),
            notices: Mutex::new(notices),
            config_lock: Mutex::default(),
            bad_config_kept: AtomicBool::new(bad_config_kept),
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
    pub fn save_config(&self, config: AppConfig) -> Result<AppConfig, CommandError> {
        let _guard = self.config_lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.save_locked(config)
    }

    /// Read-modify-write of the current config, all under `config_lock`, so a concurrent
    /// save from the tray or the UI cannot be lost.
    pub fn update_config(
        &self,
        change: impl FnOnce(&mut AppConfig) -> Result<(), CommandError>,
    ) -> Result<AppConfig, CommandError> {
        let _guard = self.config_lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut config = self.config();
        change(&mut config)?;
        self.save_locked(config)
    }

    /// Caller holds `config_lock`.
    fn save_locked(&self, mut config: AppConfig) -> Result<AppConfig, CommandError> {
        config.normalize();
        if self.bad_config_kept.load(Ordering::SeqCst) {
            self.move_bad_config_aside()?;
        }
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

    /// Retries the start-up move-aside of the damaged config.json; refuses while it still fails.
    /// A file that is gone meanwhile has nothing left to protect.
    fn move_bad_config_aside(&self) -> Result<(), CommandError> {
        match startup::move_config_aside(&self.paths.config, crate::now_ms()) {
            Ok(_) => info!("damaged config.json moved aside"),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => info!("damaged config.json is gone"),
            Err(err) => {
                warn!(error = %err, "damaged config.json still cannot be moved aside; not saving");
                return Err(CommandError::new(
                    "config",
                    "the damaged config.json could not be moved aside, so it was not overwritten; \
                     close any program that uses it and try again",
                ));
            }
        }
        self.bad_config_kept.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn set_active_profile(&self, id: &str) -> Result<AppConfig, CommandError> {
        self.update_config(|config| {
            if config.profile(id).is_none() {
                return Err(CommandError::new("invalid_input", format!("unknown profile {id}")));
            }
            config.active_profile_id = id.to_string();
            Ok(())
        })
    }

    pub fn system_locale(&self) -> Option<&str> {
        self.system_locale.as_deref()
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
        let paused = self.hotkey_paused.load(Ordering::SeqCst)
            || self.hotkey_capturing.load(Ordering::SeqCst)
            || !config.hotkey.enabled;
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

    // ----- microphone test (wizard / settings level meter) -----

    /// Opens `device` and streams its level to `sink` until `mic_test_stop`, a new test, or
    /// MIC_TEST_LIMIT. Refused while a dictation is running. Returns the device opened.
    pub fn mic_test_start(self: &Arc<Self>, device: Option<&str>, sink: MicTestSink) -> Result<String, CommandError> {
        self.mic_test_start_for(device, sink, MIC_TEST_LIMIT)
    }

    pub(crate) fn mic_test_start_for(
        self: &Arc<Self>,
        device: Option<&str>,
        sink: MicTestSink,
        limit: Duration,
    ) -> Result<String, CommandError> {
        if self.status().state != DictationState::Idle {
            return Err(CommandError::new("unavailable", "a dictation is running"));
        }
        self.mic_test_stop();
        let generation = self.mic_test_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let capture_sink: CaptureSink = Arc::new(move |event| match event {
            CaptureEvent::Level(value) => sink(MicTestEvent::Level { value }),
            CaptureEvent::Failed(message) => sink(MicTestEvent::Failed { message }),
        });
        let started = self.platform.mic.start(device, capture_sink).map_err(|e| CommandError {
            code: "unavailable",
            message: e.to_string(),
            kind: Some(ErrorKind::Microphone),
        })?;
        *self.mic_test.lock().unwrap_or_else(PoisonError::into_inner) = Some((generation, started.capture));
        let core = Arc::downgrade(self);
        std::thread::spawn(move || {
            std::thread::sleep(limit);
            if let Some(core) = core.upgrade() {
                core.stop_mic_test_if(generation);
            }
        });
        Ok(started.device)
    }

    /// Stops the test; dropping the capture cancels it (nothing is kept).
    pub fn mic_test_stop(&self) {
        let taken = self.mic_test.lock().unwrap_or_else(PoisonError::into_inner).take();
        drop(taken);
    }

    fn stop_mic_test_if(&self, generation: u64) {
        let mut slot = self.mic_test.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.as_ref().is_some_and(|(g, _)| *g == generation) {
            let taken = slot.take();
            drop(slot);
            drop(taken);
        }
    }

    // ----- shortcut capture -----

    /// While active, the global hook delivers nothing, so pressing the current shortcut in
    /// the capture box does not start a dictation. Expires after HOTKEY_CAPTURE_LIMIT even
    /// if the window that asked for it is gone.
    pub fn set_hotkey_capture(self: &Arc<Self>, active: bool) {
        self.set_hotkey_capture_for(active, HOTKEY_CAPTURE_LIMIT);
    }

    pub(crate) fn set_hotkey_capture_for(self: &Arc<Self>, active: bool, limit: Duration) {
        let generation = self.hotkey_capture_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.hotkey_capturing.store(active, Ordering::SeqCst);
        self.sync_pause(&self.config());
        if active {
            let core = Arc::downgrade(self);
            std::thread::spawn(move || {
                std::thread::sleep(limit);
                if let Some(core) = core.upgrade()
                    && core.hotkey_capture_generation.load(Ordering::SeqCst) == generation
                {
                    core.hotkey_capturing.store(false, Ordering::SeqCst);
                    core.sync_pause(&core.config());
                }
            });
        }
    }

    pub fn validate_hotkey(keys: &[String]) -> Result<(), CommandError> {
        crate::platform::keys::parse_combo(keys)
            .map(|_| ())
            .map_err(|e| CommandError::new("invalid_input", e.to_string()))
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

    pub fn parse_user_rules(yaml: &str) -> Result<RulePack, CommandError> {
        Ok(RulePack::from_yaml(yaml)?)
    }

    /// YAML for `pack`, keeping the leading comment block of `previous_yaml`. The result is
    /// parsed back so an invalid pack (bad id, schema) is refused here, not on save.
    pub fn render_user_rules(pack: &RulePack, previous_yaml: &str) -> Result<String, CommandError> {
        let yaml = pack.to_yaml_with_header(&header_comments(previous_yaml))?;
        RulePack::from_yaml(&yaml)?;
        Ok(yaml)
    }

    pub fn correction_draft(&self, canonical: &str, variant: &str) -> Result<CorrectionDraft, CommandError> {
        let current = self.user_rules_yaml()?;
        let mut pack = RulePack::from_yaml(&current)?;
        let outcome =
            pack.add_correction(canonical, variant).map_err(|e| CommandError::new("invalid_input", e.to_string()))?;
        Ok(CorrectionDraft { yaml: Self::render_user_rules(&pack, &current)?, outcome })
    }

    pub fn user_rules_yaml(&self) -> Result<String, CommandError> {
        match std::fs::read_to_string(&self.paths.user_rules) {
            Ok(yaml) => Ok(yaml),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(USER_PACK_TEMPLATE.to_string()),
            Err(err) => Err(CommandError::new("rules", err.to_string())),
        }
    }

    /// Validates and stores `user.yaml`, then recompiles the rules; returns skipped-rule warnings.
    pub fn save_user_rules(&self, yaml: &str) -> Result<Vec<RuleWarning>, CommandError> {
        // Async commands may save rules and config concurrently; serialize their settings rebuilds.
        let _guard = self.config_lock.lock().unwrap_or_else(PoisonError::into_inner);
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

    /// The WAV bytes of a dictation's saved audio. Only files inside the app's audio folder
    /// are read, whatever path the database holds.
    pub fn history_audio(&self, id: i64) -> Result<Vec<u8>, CommandError> {
        let unavailable = || CommandError::new("unavailable", "this dictation has no saved audio");
        let row = self.history()?.store().get(id)?.ok_or_else(unavailable)?;
        let path = std::path::PathBuf::from(row.audio_path.ok_or_else(unavailable)?);
        let dir = self.paths.audio.canonicalize().map_err(|_| unavailable())?;
        let file = path.canonicalize().map_err(|_| unavailable())?;
        if !file.starts_with(&dir) {
            return Err(CommandError::new("invalid_input", "the audio file is outside the audio folder"));
        }
        std::fs::read(&file).map_err(|e| CommandError::new("history", e.to_string()))
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
    use crate::platform::MicError;
    use crate::platform::fake::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        core: AppCore,
        mic: Arc<FakeMic>,
        hotkey: Arc<FakeHotkey>,
        overlay: Arc<FakeOverlay>,
        autostart: Arc<FakeAutostart>,
        secrets: Arc<FakeSecrets>,
    }

    fn fixture() -> Fixture {
        fixture_with(tempfile::tempdir().unwrap(), Vec::new())
    }

    fn fixture_with(dir: tempfile::TempDir, notices: Vec<StartupNotice>) -> Fixture {
        let paths = Paths::under(dir.path().join("opit"));
        let settings = SettingsHandle::new(Settings::build(AppConfig::default(), None, Some("en-US")).0);
        let history =
            Arc::new(HistoryService::with_store(HistoryStore::open_in_memory().unwrap(), paths.audio.clone()));
        let (hotkey, overlay) = (Arc::new(FakeHotkey::default()), Arc::new(FakeOverlay::default()));
        let (autostart, secrets) = (Arc::new(FakeAutostart::default()), Arc::new(FakeSecrets::default()));
        let mic = Arc::new(FakeMic::default());
        let platform = Platform {
            secrets: secrets.clone(),
            hotkey: hotkey.clone(),
            mic: mic.clone(),
            overlay: overlay.clone(),
            autostart: autostart.clone(),
        };
        let core = AppCore::new(paths, settings, channel().0, Some(history), platform, None, None, notices);
        Fixture { _dir: dir, core, mic, hotkey, overlay, autostart, secrets }
    }

    /// A damaged config.json that start-up could not move aside, because another program
    /// holds it open without sharing. Returns the fixture and that lock.
    fn fixture_with_a_kept_bad_config() -> (Fixture, std::fs::File) {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::under(dir.path().join("opit"));
        std::fs::create_dir_all(&paths.root).unwrap();
        std::fs::write(&paths.config, "{damaged").unwrap();
        let lock = std::fs::OpenOptions::new().read(true).share_mode(0).open(&paths.config).unwrap();
        let loaded = crate::startup::load_config(&paths.config, 1);
        assert!(matches!(loaded.notice, Some(StartupNotice::ConfigReset { backup: None, .. })));
        (fixture_with(dir, loaded.notice.into_iter().collect()), lock)
    }

    fn bad_config_backups(paths: &Paths) -> Vec<String> {
        std::fs::read_dir(&paths.root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with("config.json.bad-"))
            .map(|path| std::fs::read_to_string(path).unwrap())
            .collect()
    }

    #[test]
    fn a_config_that_could_not_be_moved_aside_is_never_overwritten() {
        let (f, lock) = fixture_with_a_kept_bad_config();
        let mut config = f.core.config();
        config.paste.trailing_space = false;

        let err = f.core.save_config(config.clone()).unwrap_err();
        assert_eq!(err.code, "config");
        assert!(err.message.contains("moved aside"), "{}", err.message);
        drop(lock);
        assert_eq!(std::fs::read_to_string(&f.core.paths.config).unwrap(), "{damaged");
        assert!(f.core.settings.current().config.paste.trailing_space, "a refused save is not applied");

        // Once the file can be moved, saving backs it up first and then writes.
        let saved = f.core.save_config(config).unwrap();
        assert_eq!(bad_config_backups(&f.core.paths), ["{damaged"]);
        assert_eq!(AppConfig::load(&f.core.paths.config).unwrap(), saved);

        // Done once: later saves just save.
        f.core.set_active_profile("openai").unwrap();
        assert_eq!(bad_config_backups(&f.core.paths).len(), 1);
    }

    #[test]
    fn a_kept_bad_config_that_is_gone_no_longer_blocks_saving() {
        let (f, lock) = fixture_with_a_kept_bad_config();
        drop(lock);
        std::fs::remove_file(&f.core.paths.config).unwrap();
        let saved = f.core.save_config(f.core.config()).unwrap();
        assert_eq!(AppConfig::load(&f.core.paths.config).unwrap(), saved);
        assert!(bad_config_backups(&f.core.paths).is_empty());
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
    fn mic_test_forwards_levels_and_stops() {
        let f = fixture();
        let (mic, core) = (f.mic.clone(), Arc::new(f.core));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink_seen = seen.clone();
        let device = core.mic_test_start(None, Arc::new(move |event| sink_seen.lock().unwrap().push(event))).unwrap();
        assert!(!device.is_empty());
        mic.emit(CaptureEvent::Level(0.25));
        mic.emit(CaptureEvent::Failed("unplugged".into()));
        assert_eq!(
            *seen.lock().unwrap(),
            [MicTestEvent::Level { value: 0.25 }, MicTestEvent::Failed { message: "unplugged".into() }]
        );
        core.mic_test_stop();
        assert_eq!(mic.dropped.load(Ordering::SeqCst), 1, "stopping drops (cancels) the capture");
    }

    #[test]
    fn a_second_mic_test_replaces_the_first() {
        let f = fixture();
        let (mic, core) = (f.mic.clone(), Arc::new(f.core));
        core.mic_test_start(None, Arc::new(|_| {})).unwrap();
        core.mic_test_start(Some("Fake Mic"), Arc::new(|_| {})).unwrap();
        assert_eq!(mic.dropped.load(Ordering::SeqCst), 1);
        assert_eq!(mic.starts(), 2);
    }

    #[test]
    fn mic_test_stops_by_itself_after_the_limit() {
        let f = fixture();
        let (mic, core) = (f.mic.clone(), Arc::new(f.core));
        core.mic_test_start_for(None, Arc::new(|_| {}), Duration::from_millis(30)).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(mic.dropped.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn mic_test_errors_carry_the_microphone_kind() {
        let f = fixture();
        *f.mic.fail_start.lock().unwrap() = Some(MicError::NoDevice);
        let core = Arc::new(f.core);
        let err = core.mic_test_start(None, Arc::new(|_| {})).unwrap_err();
        assert_eq!(err.kind, Some(ErrorKind::Microphone));
    }

    #[test]
    fn hotkey_capture_pauses_the_hook_and_expires() {
        let f = fixture();
        let (hotkey, core) = (f.hotkey.clone(), Arc::new(f.core));
        core.set_hotkey_capture_for(true, Duration::from_millis(30));
        assert!(*hotkey.paused.lock().unwrap());
        assert!(!core.hotkey_state().paused, "capture is not the user's pause");
        std::thread::sleep(Duration::from_millis(300));
        assert!(!*hotkey.paused.lock().unwrap(), "capture expired");
    }

    #[test]
    fn ending_capture_keeps_a_user_pause() {
        let f = fixture();
        let (hotkey, core) = (f.hotkey.clone(), Arc::new(f.core));
        core.set_hotkey_paused(true);
        core.set_hotkey_capture(true);
        core.set_hotkey_capture(false);
        assert!(*hotkey.paused.lock().unwrap());
    }

    #[test]
    fn validate_hotkey_uses_the_hook_key_names() {
        assert!(AppCore::validate_hotkey(&["RightCtrl".into(), "F13".into()]).is_ok());
        assert_eq!(AppCore::validate_hotkey(&[]).unwrap_err().code, "invalid_input");
        assert_eq!(AppCore::validate_hotkey(&["Banana".into()]).unwrap_err().code, "invalid_input");
    }

    #[test]
    fn set_active_profile_saves_and_returns_the_config() {
        let f = fixture();
        let saved = f.core.set_active_profile("openai").unwrap();
        assert_eq!(saved.active_profile_id, "openai");
        assert_eq!(f.core.config().active_profile_id, "openai");
        let on_disk = AppConfig::load(&f.core.paths.config).unwrap();
        assert_eq!(on_disk.active_profile_id, "openai");
    }

    #[test]
    fn set_active_profile_rejects_an_unknown_id_without_saving() {
        let f = fixture();
        let err = f.core.set_active_profile("nope").unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert!(!f.core.paths.config.exists());
    }

    #[test]
    fn update_config_applies_the_change_under_the_lock() {
        let f = fixture();
        let saved = f.core.update_config(|c| {
            c.paste.trailing_space = false;
            Ok(())
        });
        assert!(!saved.unwrap().paste.trailing_space);
        assert!(!f.core.config().paste.trailing_space);
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

    const USER_YAML: &str = "# my header\n\nschema: 1\nid: user\nname: Me\ncorrections:\n  Claude Code: [cloud code]\n";

    #[test]
    fn render_user_rules_keeps_the_header_and_validates() {
        let pack = AppCore::parse_user_rules(USER_YAML).unwrap();
        let yaml = AppCore::render_user_rules(&pack, USER_YAML).unwrap();
        assert!(yaml.starts_with("# my header\n"));
        assert_eq!(AppCore::parse_user_rules(&yaml).unwrap(), pack);
        let mut bad = pack.clone();
        bad.id = "Not Valid".into();
        assert_eq!(AppCore::render_user_rules(&bad, USER_YAML).unwrap_err().code, "rules");
    }

    #[test]
    fn correction_draft_adds_to_the_saved_file_without_saving() {
        let f = fixture();
        std::fs::create_dir_all(f.core.paths.user_rules.parent().unwrap()).unwrap();
        std::fs::write(&f.core.paths.user_rules, USER_YAML).unwrap();
        let draft = f.core.correction_draft("Claude Code", "klod kod").unwrap();
        assert_eq!(draft.outcome, CorrectionOutcome::AddedVariant);
        assert!(draft.yaml.starts_with("# my header\n"));
        assert!(draft.yaml.contains("klod kod"));
        assert_eq!(std::fs::read_to_string(&f.core.paths.user_rules).unwrap(), USER_YAML);
        let preview = f.core.rules_preview("klod kod açtım", Some(&draft.yaml)).unwrap();
        assert!(preview.text.starts_with("Claude Code"));
    }

    #[test]
    fn correction_draft_works_without_a_user_file_and_rejects_bad_input() {
        let f = fixture();
        let draft = f.core.correction_draft("Opit", "opet").unwrap();
        assert!(draft.yaml.contains("Opit"));
        assert_eq!(f.core.correction_draft("", "x").unwrap_err().code, "invalid_input");
    }

    #[test]
    fn history_audio_reads_only_files_in_the_audio_folder() {
        let f = fixture();
        let history = f.core.history.as_ref().unwrap();
        let entry = |text: &'static str| NewDictation {
            created_at_ms: 1,
            profile_id: "groq",
            raw_text: text,
            text,
            status: TranscriptStatus::Ok,
            audio_ms: 1000,
            latency_ms: 500,
        };
        // Row with an audio file inside paths.audio.
        let inside = history.store().insert(&entry("a")).unwrap();
        std::fs::create_dir_all(&f.core.paths.audio).unwrap();
        let wav = f.core.paths.audio.join(format!("{inside}.wav"));
        std::fs::write(&wav, b"RIFF1234").unwrap();
        history.store().set_audio_path(inside, &wav.display().to_string()).unwrap();
        assert_eq!(f.core.history_audio(inside).unwrap(), b"RIFF1234");
        // Row pointing outside the audio folder.
        let outside = history.store().insert(&entry("b")).unwrap();
        let elsewhere = f.core.paths.root.join("config.json");
        std::fs::create_dir_all(&f.core.paths.root).unwrap();
        std::fs::write(&elsewhere, "{}").unwrap();
        history.store().set_audio_path(outside, &elsewhere.display().to_string()).unwrap();
        assert_eq!(f.core.history_audio(outside).unwrap_err().code, "invalid_input");
        // Row without audio, and a missing row.
        let none = history.store().insert(&entry("c")).unwrap();
        assert_eq!(f.core.history_audio(none).unwrap_err().code, "unavailable");
        assert_eq!(f.core.history_audio(9999).unwrap_err().code, "unavailable");
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
