//! Rust → web UI events. Payloads are the same serde types the commands return.

use opit_core::config::AppConfig;
use tauri::{AppHandle, Emitter};
use tracing::warn;

use crate::controller::{DictationStatus, UiEvents};
use crate::{tray, window};

/// Payload: `DictationStatus`.
pub const STATUS: &str = "dictation-status";
/// Payload: the new history row id (`number`).
pub const HISTORY_ADDED: &str = "history-added";
/// Payload: the saved `AppConfig`.
pub const CONFIG_CHANGED: &str = "config-changed";
/// Payload: a route name such as `"settings"`.
pub const NAVIGATE: &str = "navigate";

pub struct TauriEvents {
    app: AppHandle,
}

impl TauriEvents {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl UiEvents for TauriEvents {
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

pub fn emit<S: serde::Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    if let Err(err) = app.emit(event, payload) {
        warn!(event, error = %err, "could not emit an event");
    }
}
