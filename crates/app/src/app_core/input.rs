use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use opit_core::config::AppConfig;
use tracing::warn;

use super::{AppCore, CommandError, HOTKEY_CAPTURE_LIMIT, HotkeyState, MIC_TEST_LIMIT, MicTestEvent, MicTestSink};
use crate::controller::{DictationState, ErrorKind};
use crate::platform::{CaptureEvent, CaptureSink, HotkeyError, HotkeyEvent};

fn auxiliary_shortcuts(config: &AppConfig) -> Vec<(HotkeyEvent, Vec<String>)> {
    vec![
        (HotkeyEvent::Pause, config.hotkey.pause_keys.clone()),
        (HotkeyEvent::Ask, config.hotkey.ask_keys.clone()),
        (HotkeyEvent::Edit, config.hotkey.edit_keys.clone()),
        (HotkeyEvent::Translate, config.hotkey.translate_keys.clone()),
        (HotkeyEvent::StyleCycle, config.hotkey.style_cycle_keys.clone()),
    ]
    .into_iter()
    .chain(cfg!(target_os = "linux").then(|| (HotkeyEvent::Escape, config.hotkey.cancel_keys.clone())))
    .chain(config.cleanup.styles.iter().map(|s| (HotkeyEvent::Style(s.id.clone()), s.hotkey.clone())))
    .collect()
}

pub(super) fn validate_shortcuts(config: &AppConfig) -> Result<(), HotkeyError> {
    crate::platform::keys::validate_list(&config.shortcuts())?;
    #[cfg(target_os = "linux")]
    {
        use crate::platform::linux::shortcut_combo::{Combo, validate_list};
        let mut valid = config
            .shortcuts()
            .into_iter()
            .filter(|keys| !keys.is_empty() && Combo::new(keys).is_ok())
            .collect::<Vec<_>>();
        if !config.hotkey.cancel_keys.is_empty() {
            Combo::new(&config.hotkey.cancel_keys)?;
            valid.push(&config.hotkey.cancel_keys);
        }
        validate_list(&valid)?;
    }
    Ok(())
}

pub(super) fn shortcut_mappings_changed(old: &AppConfig, new: &AppConfig) -> bool {
    let mappings = |config: &AppConfig| {
        std::iter::once((HotkeyEvent::ComboDown, config.hotkey.keys.clone()))
            .chain(auxiliary_shortcuts(config))
            .filter(|(_, keys)| !keys.is_empty())
            .map(|(action, keys)| {
                let mut combo = crate::platform::keys::parse_combo(&keys).unwrap_or_default();
                combo.sort_unstable();
                (action, combo)
            })
            .collect::<Vec<_>>()
    };
    let (old, new) = (mappings(old), mappings(new));
    old.len() != new.len() || old.iter().any(|mapping| !new.contains(mapping))
}

impl AppCore {
    pub(super) fn shortcut_error(&self, err: HotkeyError) -> CommandError {
        let lang = self.settings.current().lang;
        let message = match err {
            HotkeyError::Collision => lang.shortcut_collision().into(),
            HotkeyError::PauseEscape => lang.pause_escape_reserved().into(),
            other => other.to_string(),
        };
        CommandError::new("invalid_input", message)
    }

    pub fn validate_shortcut(
        &self,
        keys: Vec<String>,
        target: &str,
        draft: Option<AppConfig>,
    ) -> Result<(), CommandError> {
        let mut config = draft.unwrap_or_else(|| self.config());
        let slot = match target {
            "dictation" => &mut config.hotkey.keys,
            "pause" => &mut config.hotkey.pause_keys,
            "cancel" => &mut config.hotkey.cancel_keys,
            "ask" => &mut config.hotkey.ask_keys,
            "edit" => &mut config.hotkey.edit_keys,
            "translate" => &mut config.hotkey.translate_keys,
            "style_cycle" => &mut config.hotkey.style_cycle_keys,
            _ => {
                let id = target
                    .strip_prefix("style:")
                    .ok_or_else(|| CommandError::new("invalid_input", "unknown shortcut target"))?;
                &mut config
                    .cleanup
                    .styles
                    .iter_mut()
                    .find(|s| s.id == id)
                    .ok_or_else(|| CommandError::new("invalid_input", "unknown cleanup style"))?
                    .hotkey
            }
        };
        *slot = keys;
        validate_shortcuts(&config).map_err(|err| self.shortcut_error(err))
    }

    /// (Re-)installs the global hook for the configured keys.
    pub fn apply_hotkey(&self, config: &AppConfig) {
        let shortcuts = auxiliary_shortcuts(config);
        let result = validate_shortcuts(config)
            .and_then(|()| self.platform.hotkey.register(&config.hotkey.keys, self.controller.hotkey_sink()))
            .and_then(|()| self.platform.hotkey.register_shortcuts(&shortcuts));
        let error = result.err().map(|err: HotkeyError| {
            warn!(error = %err, "global shortcut unavailable");
            err.to_string()
        });
        *self.hotkey_error.lock().unwrap_or_else(PoisonError::into_inner) = error;
        self.sync_pause(config);
    }

    pub(super) fn sync_pause(&self, config: &AppConfig) {
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
            pending: self.platform.hotkey.registration_pending(),
            error: self
                .hotkey_error
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .or_else(|| self.platform.hotkey.runtime_error()),
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
        let mut slot = self.mic_test.lock().unwrap_or_else(PoisonError::into_inner);
        // Reserve the generation before dropping the old capture, which can block.
        let generation = self.mic_test_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let old = slot.take();
        drop(slot);
        drop(old);
        let capture_sink: CaptureSink = Arc::new(move |event| match event {
            CaptureEvent::Level(value) => sink(MicTestEvent::Level { value }),
            CaptureEvent::Failed(message) => sink(MicTestEvent::Failed { message }),
        });
        let started = self.platform.mic.start(device, capture_sink).map_err(|e| CommandError {
            code: "unavailable",
            message: e.to_string(),
            kind: Some(ErrorKind::Microphone),
        })?;
        let mut slot = self.mic_test.lock().unwrap_or_else(PoisonError::into_inner);
        if self.mic_test_generation.load(Ordering::SeqCst) != generation {
            drop(slot);
            drop(started.capture);
            return Err(CommandError::new("unavailable", "the microphone test was stopped"));
        }
        let old = slot.replace((generation, started.capture));
        drop(slot);
        drop(old);
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
        let mut slot = self.mic_test.lock().unwrap_or_else(PoisonError::into_inner);
        self.mic_test_generation.fetch_add(1, Ordering::SeqCst);
        let taken = slot.take();
        drop(slot);
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
        #[cfg(target_os = "linux")]
        let mut deadline = self.linux_capture_deadline.lock().unwrap_or_else(PoisonError::into_inner);
        #[cfg(target_os = "linux")]
        {
            *deadline = active.then(|| std::time::Instant::now() + limit);
        }
        let generation = self.hotkey_capture_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.hotkey_capturing.store(active, Ordering::SeqCst);
        self.sync_pause(&self.config());
        if active {
            let core = Arc::downgrade(self);
            std::thread::spawn(move || {
                std::thread::sleep(limit);
                if let Some(core) = core.upgrade() {
                    // Serialize Linux expiry with start/cancel so an old timer cannot
                    // clear a new capture between the generation check and the store.
                    #[cfg(target_os = "linux")]
                    let _deadline = core.linux_capture_deadline.lock().unwrap_or_else(PoisonError::into_inner);
                    if core.hotkey_capture_generation.load(Ordering::SeqCst) == generation {
                        core.hotkey_capturing.store(false, Ordering::SeqCst);
                        core.sync_pause(&core.config());
                    }
                }
            });
        }
    }

    /// The relay must not start dictation while the focused WebView records keys.
    /// A monotonic deadline also releases it after a crashed or disconnected page.
    #[cfg(target_os = "linux")]
    pub(crate) fn linux_shortcut_capturing(&self) -> bool {
        self.linux_capture_deadline
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some_and(|deadline| std::time::Instant::now() < deadline)
    }

    pub fn validate_hotkey(keys: &[String]) -> Result<(), CommandError> {
        crate::platform::keys::parse_combo(keys)
            .map(|_| ())
            .map_err(|e| CommandError::new("invalid_input", e.to_string()))
    }

    pub fn microphones(&self) -> Vec<String> {
        self.platform.mic.devices()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod shortcut_tests {
    use super::*;
    #[test]
    fn cancel_is_opt_in_and_can_use_escape_without_implicitly_grabbing_it() {
        let mut config = AppConfig::default();
        assert!(config.hotkey.cancel_keys.is_empty());
        config.hotkey.cancel_keys = vec!["LeftCtrl".into(), "Escape".into()];
        assert!(validate_shortcuts(&config).is_ok());
        config.hotkey.cancel_keys = config.hotkey.keys.clone();
        assert_eq!(validate_shortcuts(&config), Err(HotkeyError::Collision));
    }
    #[test]
    fn saved_modifier_only_shortcuts_are_not_migrated_by_validation() {
        let mut config = AppConfig::default();
        config.hotkey.keys = vec!["RightCtrl".into(), "RightShift".into()];
        config.hotkey.mode = opit_core::config::HotkeyMode::PushToTalk;
        let saved = config.clone();
        assert!(validate_shortcuts(&config).is_ok());
        assert_eq!(config, saved);
    }
}
