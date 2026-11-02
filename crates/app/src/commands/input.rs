use std::sync::Arc;

use opit_core::config::AppConfig;
use tauri::AppHandle;

use crate::app_core::{AppCore, CommandError, HotkeyState, MicTestSink};
use crate::{events, tray};

use super::{Core, Result, blocking};

/// Device enumeration can take a moment, so it runs off the main thread.
#[tauri::command]
pub(super) async fn list_microphones(core: Core<'_>) -> Result<Vec<String>> {
    let core = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core.microphones())
        .await
        .map_err(|_| CommandError::new("unavailable", "the microphone list could not be read"))
}

/// Window/process enumeration is confined to a blocking worker. Titles are returned only to the UI.
#[tauri::command]
pub(super) async fn list_running_apps(core: Core<'_>) -> Result<Vec<crate::platform::ForegroundInfo>> {
    let core = core.inner().clone();
    let message = core.settings.current().lang.application_list_failed();
    tauri::async_runtime::spawn_blocking(move || core.platform.foreground.list_running())
        .await
        .ok()
        .and_then(std::result::Result::ok)
        .ok_or_else(|| CommandError::new("unavailable", message))
}

/// Opening a device can block for a moment, so this runs on the blocking pool.
#[tauri::command]
pub(super) async fn mic_test_start(app: AppHandle, core: Core<'_>, device: Option<String>) -> Result<String> {
    let core = core.inner().clone();
    let sink: MicTestSink = Arc::new(move |event| events::emit(&app, events::MIC_TEST, event));
    tauri::async_runtime::spawn_blocking(move || core.mic_test_start(device.as_deref(), sink))
        .await
        .map_err(|_| CommandError::new("unavailable", "the microphone test stopped unexpectedly"))?
}

/// Dropping a capture joins its thread, so stopping also runs on the blocking pool.
#[tauri::command]
pub(super) async fn mic_test_stop(core: Core<'_>) -> Result<()> {
    blocking(core.inner().clone(), |core| {
        core.mic_test_stop();
        Ok(())
    })
    .await
}

#[tauri::command]
pub(super) fn get_hotkey_state(core: Core<'_>) -> HotkeyState {
    core.hotkey_state()
}

#[tauri::command]
pub(super) fn set_hotkey_paused(app: AppHandle, core: Core<'_>, paused: bool) -> HotkeyState {
    core.set_hotkey_paused(paused);
    tray::refresh(&app);
    let state = core.hotkey_state();
    events::hotkey_state(&app, &state);
    state
}

#[tauri::command]
pub(super) fn set_hotkey_capture(core: Core<'_>, active: bool) {
    core.inner().set_hotkey_capture(active);
}

#[tauri::command]
pub(super) fn validate_hotkey(
    core: Core<'_>,
    keys: Vec<String>,
    target: Option<String>,
    config: Option<AppConfig>,
) -> Result<()> {
    #[cfg(target_os = "linux")]
    if !keys.is_empty() {
        crate::platform::linux::shortcut_combo::Combo::new(&keys)
            .map_err(|_| CommandError::new("invalid_input", "Invalid Linux shortcut combination"))?;
    }
    match target {
        Some(target) => core.validate_shortcut(keys, &target, config),
        None => AppCore::validate_hotkey(&keys),
    }
}
