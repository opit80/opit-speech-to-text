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

#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    pub version: &'static str,
    pub data_dir: String,
    pub log_dir: String,
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
fn save_config(app: AppHandle, core: Core<'_>, config: AppConfig) -> Result<AppConfig> {
    let saved = core.save_config(config)?;
    events::config_changed(&app, &saved);
    Ok(saved)
}

/// Device enumeration can take a moment, so it runs off the main thread.
#[tauri::command]
async fn list_microphones(core: Core<'_>) -> Result<Vec<String>> {
    let core = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core.microphones())
        .await
        .map_err(|e| CommandError::new("unavailable", e.to_string()))
}

#[tauri::command]
fn has_api_key(core: Core<'_>, key_ref: String) -> Result<bool> {
    core.has_api_key(&key_ref)
}

#[tauri::command]
fn set_api_key(core: Core<'_>, key_ref: String, key: String) -> Result<()> {
    core.set_api_key(&key_ref, &key)
}

#[tauri::command]
fn delete_api_key(core: Core<'_>, key_ref: String) -> Result<()> {
    core.delete_api_key(&key_ref)
}

/// `api_key: null` tests with the stored key.
#[tauri::command]
async fn test_connection(core: Core<'_>, profile: Profile, api_key: Option<String>) -> Result<()> {
    core.test_connection(profile, api_key).await
}

#[tauri::command]
fn get_user_rules(core: Core<'_>) -> Result<String> {
    core.user_rules_yaml()
}

#[tauri::command]
fn save_user_rules(core: Core<'_>, yaml: String) -> Result<Vec<RuleWarning>> {
    core.save_user_rules(&yaml)
}

#[tauri::command]
fn rules_preview(core: Core<'_>, text: String, draft_yaml: Option<String>) -> Result<RulesPreview> {
    core.rules_preview(&text, draft_yaml.as_deref())
}

#[tauri::command]
fn prompt_budget(core: Core<'_>) -> BuiltPrompt {
    core.prompt_budget()
}

#[tauri::command]
fn history_recent(core: Core<'_>, limit: usize, before_id: Option<i64>) -> Result<Vec<Dictation>> {
    core.history_recent(limit, before_id)
}

#[tauri::command]
fn history_search(core: Core<'_>, query: String, limit: usize) -> Result<Vec<Dictation>> {
    core.history_search(&query, limit)
}

#[tauri::command]
fn history_delete(core: Core<'_>, id: i64) -> Result<()> {
    core.history_delete(id)
}

#[tauri::command]
fn history_clear(core: Core<'_>) -> Result<()> {
    core.history_clear()
}

#[tauri::command]
fn get_hotkey_state(core: Core<'_>) -> HotkeyState {
    core.hotkey_state()
}

#[tauri::command]
fn set_hotkey_paused(app: AppHandle, core: Core<'_>, paused: bool) -> HotkeyState {
    core.set_hotkey_paused(paused);
    tray::refresh(&app);
    core.hotkey_state()
}

#[tauri::command]
fn take_startup_notices(core: Core<'_>) -> Vec<StartupNotice> {
    core.take_notices()
}
