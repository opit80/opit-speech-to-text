//! Everything the invoke commands and the tray do, without Tauri types, so it can be
//! tested with the platform fakes. `commands.rs` is a thin wrapper over this.

mod config;
mod credentials;
mod history;
mod input;
mod rules;
#[cfg(test)]
mod tests;
mod types;

pub use config::autostart_wanted;
pub use types::{
    CommandError, CorrectionDraft, HOTKEY_CAPTURE_LIMIT, HistoryUpdate, HotkeyState, MAX_HISTORY_TEXT_BYTES,
    MIC_TEST_LIMIT, MicTestEvent, MicTestSink, Platform, RulesPreview, USER_PACK_TEMPLATE,
};

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use opit_core::rules::RulePack;

use crate::controller::{ControllerHandle, DictationStatus};
use crate::history_service::HistoryService;
use crate::platform::{Capture, OverlayView, Tone};
use crate::settings::SettingsHandle;
use crate::startup::{Paths, StartupNotice};

const STARTUP_NOTICE_HIDE: Duration = Duration::from_secs(6);

pub struct AppCore {
    pub local: Arc<crate::local::LocalService>,
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
    /// Linux command relays also check a deadline if the expiry worker is delayed.
    #[cfg(target_os = "linux")]
    linux_capture_deadline: Mutex<Option<std::time::Instant>>,
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
            local: crate::local::LocalService::new(paths.local.clone(), settings.clone()),
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
            #[cfg(target_os = "linux")]
            linux_capture_deadline: Mutex::default(),
            notices: Mutex::new(notices),
            config_lock: Mutex::default(),
            bad_config_kept: AtomicBool::new(bad_config_kept),
        }
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
            live_text: None,
            tone: Tone::Warning,
            text: text.into(),
            level: None,
            recording: None,
            button: None,
            hide_after: Some(STARTUP_NOTICE_HIDE),
        })
    }
}
