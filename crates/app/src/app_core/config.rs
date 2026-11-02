use std::sync::PoisonError;
use std::sync::atomic::Ordering;

use opit_core::config::AppConfig;
use opit_core::rules::RuleWarning;
use tracing::{info, warn};

use super::input::shortcut_mappings_changed;
use super::{AppCore, CommandError};
use crate::settings::Settings;
use crate::startup;

/// Start with Windows only once the first-run wizard was finished, so a fresh install
/// never registers itself before the user has seen the choice (spec §7, Plan 4 ruling).
pub fn autostart_wanted(config: &AppConfig) -> bool {
    config.ui.autostart && config.ui.setup_done
}

impl AppCore {
    /// The Linux startup worker may run after a UI save. Read current settings
    /// under the same lock, so its stale startup snapshot cannot replace that save.
    #[cfg(not(windows))]
    pub fn apply_startup_settings(&self) {
        let _guard = self.config_lock.lock().unwrap_or_else(PoisonError::into_inner);
        let config = self.config();
        self.apply_hotkey(&config);
        self.apply_autostart(autostart_wanted(&config));
    }

    pub fn config(&self) -> AppConfig {
        self.settings.current().config.clone()
    }

    pub(super) fn rebuild(&self, config: AppConfig) -> Vec<RuleWarning> {
        let user = self.user_pack.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let (settings, warnings) = Settings::build(config, user, self.system_locale.as_deref());
        for warning in &warnings {
            warn!(pack = %warning.pack_id, "rule skipped");
        }
        self.settings.replace(settings);
        warnings
    }

    /// Validates, saves and applies a config from the UI; returns what was stored.
    pub fn save_config(&self, config: AppConfig) -> Result<AppConfig, CommandError> {
        crate::local::validate_config(&config.local)?;
        let _guard = self.config_lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.save_locked(config, true)
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
        self.save_locked(config, true)
    }

    /// Caller holds `config_lock`.
    pub(super) fn save_locked(&self, mut config: AppConfig, notify_style: bool) -> Result<AppConfig, CommandError> {
        super::input::validate_shortcuts(&config).map_err(|err| self.shortcut_error(err))?;
        config.normalize();
        if !config.ask.valid() || crate::ask::resolve_directory(&config.ask.working_dir).is_err() {
            return Err(CommandError::new("invalid_input", self.settings.current().lang.ask_invalid()));
        }
        if self.bad_config_kept.load(Ordering::SeqCst) {
            self.move_bad_config_aside()?;
        }
        let old = self.config();
        config.save(&self.paths.config)?;
        self.rebuild(config.clone());
        #[cfg(target_os = "linux")]
        let retry_failed = self.hotkey_state().error.is_some();
        #[cfg(not(target_os = "linux"))]
        let retry_failed = false;
        if shortcut_mappings_changed(&old, &config) || retry_failed {
            // Saving the same keys also retries a failed installation (for example,
            // after the desktop entry was installed while this process stayed alive).
            self.apply_hotkey(&config);
        } else if old.hotkey.enabled != config.hotkey.enabled {
            self.sync_pause(&config);
        }
        if notify_style && old.cleanup.active_style_id != config.cleanup.active_style_id {
            self.controller.send(crate::controller::Msg::StyleChanged(config.cleanup.active_style()));
        }
        if old.ui.overlay_position != config.ui.overlay_position {
            self.platform.overlay.set_position(config.ui.overlay_position);
        }
        if autostart_wanted(&old) != autostart_wanted(&config) {
            self.apply_autostart(autostart_wanted(&config));
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

    pub fn set_active_style(&self, id: &str) -> Result<AppConfig, CommandError> {
        self.persist_active_style(id, true)
    }

    /// The controller applies its own notification after the blocking save completes.
    pub fn persist_active_style(&self, id: &str, notify: bool) -> Result<AppConfig, CommandError> {
        let _guard = self.config_lock.lock().unwrap_or_else(PoisonError::into_inner);
        let mut config = self.config();
        if config.cleanup.style(id).is_none() {
            return Err(CommandError::new("invalid_input", "unknown cleanup style"));
        }
        config.cleanup.active_style_id = id.into();
        let saved = self.save_locked(config, false)?;
        if notify {
            self.controller.send(crate::controller::Msg::StyleChanged(saved.cleanup.active_style()));
        }
        Ok(saved)
    }

    pub fn system_locale(&self) -> Option<&str> {
        self.system_locale.as_deref()
    }

    pub fn apply_autostart(&self, enabled: bool) {
        if let Err(err) = self.platform.autostart.set_enabled(enabled) {
            warn!(error = %err, "could not update autostart");
        }
    }
}
