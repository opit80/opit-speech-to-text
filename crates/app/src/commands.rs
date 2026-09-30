//! Invoke API for the web UI. Plan 3 builds the pages on top of it. Each command is a thin
//! wrapper over [`AppCore`]; JS passes camelCase argument names (`keyRef` → `key_ref`).
//! Events the UI can listen to are in `events.rs`.

use std::sync::Arc;

use opit_core::config::AppConfig;
use opit_core::history::Dictation;
use opit_core::provider::Profile;
use opit_core::rules::RuleWarning;
use opit_core::rules::prompt::BuiltPrompt;
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::app_core::{AppCore, CommandError, HotkeyState, RulesPreview};
use crate::controller::{DictationStatus, Msg};
use crate::startup::StartupNotice;
use crate::{events, tray};

type Core<'a> = State<'a, Arc<AppCore>>;
type Result<T> = std::result::Result<T, CommandError>;

/// Runs blocking `AppCore` work on the blocking pool so the WebView's main thread stays free.
async fn blocking<T, F>(core: Arc<AppCore>, work: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&AppCore) -> Result<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || work(&core))
        .await
        .map_err(|_| CommandError::new("unavailable", "the operation stopped unexpectedly"))?
}

#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    pub version: &'static str,
    pub data_dir: String,
    pub log_dir: String,
    /// OS locale such as "tr-TR"; the UI resolves its language from it like `i18n::resolve`.
    pub system_locale: Option<String>,
    /// Debug builds use a no-op autostart, so the UI disables that toggle.
    pub debug_build: bool,
}

pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        app_info,
        get_status,
        toggle_dictation,
        cancel_dictation,
        retry_dictation,
        get_config,
        save_config,
        set_active_profile,
        list_microphones,
        has_api_key,
        set_api_key,
        delete_api_key,
        test_connection,
        get_user_rules,
        save_user_rules,
        rules_preview,
        prompt_budget,
        history_recent,
        history_search,
        history_delete,
        history_clear,
        get_hotkey_state,
        set_hotkey_paused,
        take_startup_notices,
    ]
}

#[tauri::command]
fn app_info(core: Core<'_>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        data_dir: core.paths.root.display().to_string(),
        log_dir: core.paths.logs.display().to_string(),
        system_locale: core.system_locale().map(str::to_string),
        debug_build: cfg!(debug_assertions),
    }
}

#[tauri::command]
fn get_status(core: Core<'_>) -> DictationStatus {
    core.status()
}

#[tauri::command]
fn toggle_dictation(core: Core<'_>) {
    core.controller.send(Msg::Toggle);
}

#[tauri::command]
fn cancel_dictation(core: Core<'_>) {
    core.controller.send(Msg::Cancel);
}

#[tauri::command]
fn retry_dictation(core: Core<'_>) {
    core.controller.send(Msg::Retry);
}

#[tauri::command]
fn get_config(core: Core<'_>) -> AppConfig {
    core.config()
}

#[tauri::command]
async fn save_config(app: AppHandle, core: Core<'_>, config: AppConfig) -> Result<AppConfig> {
    let core = core.inner().clone();
    let saved = blocking(core.clone(), move |core| core.save_config(config)).await?;
    events::config_changed(&app, &saved);
    events::hotkey_state(&app, &core.hotkey_state());
    Ok(saved)
}

#[tauri::command]
async fn set_active_profile(app: AppHandle, core: Core<'_>, id: String) -> Result<AppConfig> {
    let saved = blocking(core.inner().clone(), move |core| core.set_active_profile(&id)).await?;
    events::config_changed(&app, &saved);
    Ok(saved)
}

/// Device enumeration can take a moment, so it runs off the main thread.
#[tauri::command]
async fn list_microphones(core: Core<'_>) -> Result<Vec<String>> {
    let core = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core.microphones())
        .await
        .map_err(|_| CommandError::new("unavailable", "the microphone list could not be read"))
}

#[tauri::command]
async fn has_api_key(core: Core<'_>, key_ref: String) -> Result<bool> {
    blocking(core.inner().clone(), move |core| core.has_api_key(&key_ref)).await
}

#[tauri::command]
async fn set_api_key(core: Core<'_>, key_ref: String, key: String) -> Result<()> {
    blocking(core.inner().clone(), move |core| core.set_api_key(&key_ref, &key)).await
}

#[tauri::command]
async fn delete_api_key(core: Core<'_>, key_ref: String) -> Result<()> {
    blocking(core.inner().clone(), move |core| core.delete_api_key(&key_ref)).await
}

/// `api_key: null` tests with the stored key.
#[tauri::command]
async fn test_connection(core: Core<'_>, profile: Profile, api_key: Option<String>) -> Result<()> {
    core.test_connection(profile, api_key).await
}

#[tauri::command]
async fn get_user_rules(core: Core<'_>) -> Result<String> {
    blocking(core.inner().clone(), AppCore::user_rules_yaml).await
}

#[tauri::command]
async fn save_user_rules(core: Core<'_>, yaml: String) -> Result<Vec<RuleWarning>> {
    blocking(core.inner().clone(), move |core| core.save_user_rules(&yaml)).await
}

#[tauri::command]
async fn rules_preview(core: Core<'_>, text: String, draft_yaml: Option<String>) -> Result<RulesPreview> {
    blocking(core.inner().clone(), move |core| core.rules_preview(&text, draft_yaml.as_deref())).await
}

#[tauri::command]
fn prompt_budget(core: Core<'_>) -> BuiltPrompt {
    core.prompt_budget()
}

#[tauri::command]
async fn history_recent(core: Core<'_>, limit: usize, before_id: Option<i64>) -> Result<Vec<Dictation>> {
    blocking(core.inner().clone(), move |core| core.history_recent(limit, before_id)).await
}

#[tauri::command]
async fn history_search(core: Core<'_>, query: String, limit: usize) -> Result<Vec<Dictation>> {
    blocking(core.inner().clone(), move |core| core.history_search(&query, limit)).await
}

#[tauri::command]
async fn history_delete(core: Core<'_>, id: i64) -> Result<()> {
    blocking(core.inner().clone(), move |core| core.history_delete(id)).await
}

#[tauri::command]
async fn history_clear(core: Core<'_>) -> Result<()> {
    blocking(core.inner().clone(), AppCore::history_clear).await
}

#[tauri::command]
fn get_hotkey_state(core: Core<'_>) -> HotkeyState {
    core.hotkey_state()
}

#[tauri::command]
fn set_hotkey_paused(app: AppHandle, core: Core<'_>, paused: bool) -> HotkeyState {
    core.set_hotkey_paused(paused);
    tray::refresh(&app);
    let state = core.hotkey_state();
    events::hotkey_state(&app, &state);
    state
}

#[tauri::command]
fn take_startup_notices(core: Core<'_>) -> Vec<StartupNotice> {
    core.take_notices()
}
