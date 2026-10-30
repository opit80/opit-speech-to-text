//! Rust → web UI events. Payloads are the same serde types the commands return.

use opit_core::config::AppConfig;
use tauri::{AppHandle, Emitter, Manager};
use tracing::warn;

use crate::app_core::HotkeyState;
use crate::controller::{DictationStatus, UiEvents};
use crate::{tray, window};

/// Payload: `DictationStatus`.
pub const STATUS: &str = "dictation-status";
/// Payload: the new history row id (`number`).
pub const HISTORY_ADDED: &str = "history-added";
/// Payload: edited history row id (`number`). No transcript content.
pub const HISTORY_UPDATED: &str = "history-updated";
/// Payload: null. Reload personal rules and prompt budget; preserve unsaved drafts.
pub const RULES_CHANGED: &str = "rules-changed";
/// Payload: the saved `AppConfig`.
pub const CONFIG_CHANGED: &str = "config-changed";
/// Payload: `HotkeyState`. Sent when the shortcut is paused/resumed or re-registered.
pub const HOTKEY_STATE: &str = "hotkey-state";
/// Payload: `MicTestEvent` (`{ kind: "level", value }` or `{ kind: "failed", message }`).
pub const MIC_TEST: &str = "mic-test";
/// Payload: a route name such as `"settings"`.
pub const NAVIGATE: &str = "navigate";
/// Payload: `UpdateState` (updates.rs).
pub const UPDATE_STATE: &str = "update-state";
/// Payload: `file_jobs::FileEvent`. Contains no transcript.
pub const FILE_PROGRESS: &str = "file-progress";

pub struct TauriEvents {
    app: AppHandle,
}

impl TauriEvents {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl UiEvents for TauriEvents {
    fn save_style(&self, id: &str) -> Option<opit_core::cleanup::CleanupStyle> {
        let core = self.app.try_state::<std::sync::Arc<crate::app_core::AppCore>>()?;
        match core.persist_active_style(id, false) {
            Ok(saved) => {
                config_changed(&self.app, &saved);
                Some(saved.cleanup.active_style())
            }
            Err(_) => {
                warn!("could not save cleanup style");
                None
            }
        }
    }
    fn status_changed(&self, status: &DictationStatus) {
        emit(&self.app, STATUS, status);
        tray::refresh(&self.app);
    }

    fn history_added(&self, id: i64) {
        emit(&self.app, HISTORY_ADDED, id);
    }

    fn open_settings(&self) {
        window::show_main(&self.app, Some("settings"));
    }
}

pub fn config_changed(app: &AppHandle, config: &AppConfig) {
    emit(app, CONFIG_CHANGED, config);
    tray::refresh(app);
}

pub fn hotkey_state(app: &AppHandle, state: &HotkeyState) {
    emit(app, HOTKEY_STATE, state.clone());
}

pub fn emit<S: serde::Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    if let Err(err) = app.emit(event, payload) {
        warn!(event, error = %err, "could not emit an event");
    }
}
