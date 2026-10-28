//! Invoke API for the web UI. Plan 3 builds the pages on top of it. Each command is a thin
//! wrapper over [`AppCore`]; JS passes camelCase argument names (`keyRef` → `key_ref`).
//! Events the UI can listen to are in `events.rs`.

use std::sync::Arc;

use opit_core::config::AppConfig;
use opit_core::history::{Dictation, UsageStats};
use opit_core::provider::Profile;
use opit_core::rules::builtin::PackInfo;
use opit_core::rules::prompt::BuiltPrompt;
use opit_core::rules::{RulePack, RuleWarning};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::app_core::{AppCore, CommandError, CorrectionDraft, HotkeyState, MicTestSink, RulesPreview};
use crate::controller::{DictationStatus, Msg};
use crate::startup::StartupNotice;
use crate::updates::{self, UpdateState};
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
    pub cli_path: String,
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
        profile_presets,
        local_state,
        local_download,
        local_cancel,
        local_delete,
        cleanup_presets,
        cleanup_prompts,
        test_cleanup,
        ask_status,
        test_ask,
        list_microphones,
        mic_test_start,
        mic_test_stop,
        has_api_key,
        set_api_key,
        delete_api_key,
        test_connection,
        get_user_rules,
        save_user_rules,
        rules_preview,
        prompt_budget,
        rule_packs,
        parse_user_rules,
        render_user_rules,
        correction_draft,
        history_recent,
        usage_stats,
        history_search,
        history_delete,
        history_clear,
        history_audio,
        history_retry,
        get_hotkey_state,
        set_hotkey_paused,
        set_hotkey_capture,
        validate_hotkey,
        take_startup_notices,
        get_update_state,
        check_for_updates,
        install_update,
        file_pick,
        file_state,
        file_start,
        file_cancel,
        file_render,
        file_save,
        file_handled,
        file_discard,
    ]
}

#[tauri::command]
async fn file_pick(app: AppHandle) -> Result<crate::file_jobs::FileState> {
    crate::file_jobs::pick(app).await
}

#[tauri::command]
fn file_state(jobs: State<'_, Arc<crate::file_jobs::FileJobs>>) -> crate::file_jobs::FileState {
    jobs.snapshot()
}

#[tauri::command]
fn file_start(
    app: AppHandle,
    core: Core<'_>,
    options: crate::file_jobs::FileOptions,
) -> Result<crate::file_jobs::FileState> {
    crate::file_jobs::start(app, core.inner().clone(), options)
}

#[tauri::command]
fn file_cancel(app: AppHandle, jobs: State<'_, Arc<crate::file_jobs::FileJobs>>) {
    jobs.cancel();
    let state = jobs.snapshot();
    events::emit(&app, events::FILE_PROGRESS, crate::file_jobs::FileEvent::from(&state));
}

/// The UI copied the result of `revision`; it no longer blocks update installation.
#[tauri::command]
fn file_handled(
    app: AppHandle,
    jobs: State<'_, Arc<crate::file_jobs::FileJobs>>,
    revision: u64,
) -> crate::file_jobs::FileState {
    let state = jobs.handled(revision);
    events::emit(&app, events::FILE_PROGRESS, crate::file_jobs::FileEvent::from(&state));
    state
}

#[tauri::command]
fn file_discard(
    app: AppHandle,
    jobs: State<'_, Arc<crate::file_jobs::FileJobs>>,
    revision: u64,
) -> crate::file_jobs::FileState {
    let state = jobs.discard(revision);
    events::emit(&app, events::FILE_PROGRESS, crate::file_jobs::FileEvent::from(&state));
    state
}

#[tauri::command]
async fn file_render(
    jobs: State<'_, Arc<crate::file_jobs::FileJobs>>,
    format: crate::file_jobs::OutputFormat,
    prefixes: bool,
    speakers: bool,
) -> Result<String> {
    let jobs = jobs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || jobs.render(format, prefixes, speakers))
        .await
        .map_err(|_| CommandError::new("file_worker", ""))?
}

#[tauri::command]
async fn file_save(
    app: AppHandle,
    format: crate::file_jobs::OutputFormat,
    prefixes: bool,
    speakers: bool,
) -> Result<bool> {
    crate::file_jobs::save(app, format, prefixes, speakers).await
}

#[tauri::command]
fn app_info(core: Core<'_>) -> AppInfo {
    AppInfo {
        cli_path: std::env::current_exe()
            .map(|p| p.with_file_name("opit.exe").to_string_lossy().into_owned())
            .unwrap_or_default(),
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

/// Groq, OpenAI, ElevenLabs, and a blank custom template (the UI fills in id, name, URL and model).
#[tauri::command]
fn profile_presets() -> Vec<Profile> {
    use opit_core::provider::presets;
    vec![
        presets::groq(),
        presets::openai(),
        presets::elevenlabs(),
        presets::local(),
        presets::custom("custom", "", "", ""),
    ]
}

#[tauri::command]
async fn local_state(core: Core<'_>) -> Result<crate::local::Snapshot> {
    blocking(core.inner().clone(), |core| Ok(core.local.snapshot()?)).await
}
#[tauri::command]
async fn local_download(app: AppHandle, core: Core<'_>, id: String) -> Result<()> {
    let service = core.local.clone();
    service.install(id, Arc::new(move |progress| events::emit(&app, "local-progress", progress))).await?;
    Ok(())
}
#[tauri::command]
fn local_cancel(core: Core<'_>) {
    core.local.cancel();
}
#[tauri::command]
async fn local_delete(core: Core<'_>, id: String) -> Result<()> {
    Ok(core.local.delete(id).await?)
}

#[tauri::command]
fn cleanup_presets() -> Vec<opit_core::cleanup::CleanupPreset> {
    opit_core::cleanup::presets()
}

#[tauri::command]
fn cleanup_prompts() -> std::collections::BTreeMap<&'static str, &'static str> {
    [("en", opit_core::cleanup::EN_PROMPT), ("tr", opit_core::cleanup::TR_PROMPT)].into_iter().collect()
}

#[derive(Serialize)]
struct CleanupTestResult {
    before: String,
    after: String,
}

#[tauri::command]
async fn test_cleanup(core: Core<'_>, config: opit_core::cleanup::CleanupConfig) -> Result<CleanupTestResult> {
    let (client, system, before) = blocking(core.inner().clone(), move |core| core.cleanup_test_client(config)).await?;
    let after = client.clean(&before, &system).await?;
    Ok(CleanupTestResult { before, after })
}

#[tauri::command]
async fn ask_status(core: Core<'_>) -> Result<Vec<crate::ask::AgentStatus>> {
    tauri::async_runtime::spawn_blocking(crate::ask::statuses)
        .await
        .map_err(|_| CommandError::new("unavailable", core.settings.current().lang.ask_detection_failed()))
}

#[tauri::command]
async fn test_ask(core: Core<'_>, config: opit_core::ask::AskConfig) -> Result<String> {
    core.controller.test_ask(config).await.map_err(|kind| CommandError {
        code: "ask",
        message: core.settings.current().lang.error(kind).into(),
        kind: Some(kind),
    })
}

/// Device enumeration can take a moment, so it runs off the main thread.
#[tauri::command]
async fn list_microphones(core: Core<'_>) -> Result<Vec<String>> {
    let core = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core.microphones())
        .await
        .map_err(|_| CommandError::new("unavailable", "the microphone list could not be read"))
}

/// Opening a device can block for a moment, so this runs on the blocking pool.
#[tauri::command]
async fn mic_test_start(app: AppHandle, core: Core<'_>, device: Option<String>) -> Result<String> {
    let core = core.inner().clone();
    let sink: MicTestSink = Arc::new(move |event| events::emit(&app, events::MIC_TEST, event));
    tauri::async_runtime::spawn_blocking(move || core.mic_test_start(device.as_deref(), sink))
        .await
        .map_err(|_| CommandError::new("unavailable", "the microphone test stopped unexpectedly"))?
}

/// Dropping a capture joins its thread, so stopping also runs on the blocking pool.
#[tauri::command]
async fn mic_test_stop(core: Core<'_>) -> Result<()> {
    blocking(core.inner().clone(), |core| {
        core.mic_test_stop();
        Ok(())
    })
    .await
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
fn rule_packs() -> Vec<PackInfo> {
    opit_core::rules::builtin::pack_infos()
}

#[tauri::command]
fn parse_user_rules(yaml: String) -> Result<RulePack> {
    AppCore::parse_user_rules(&yaml)
}

#[tauri::command]
fn render_user_rules(pack: RulePack, previous_yaml: String) -> Result<String> {
    AppCore::render_user_rules(&pack, &previous_yaml)
}

#[tauri::command]
async fn correction_draft(core: Core<'_>, canonical: String, variant: String) -> Result<CorrectionDraft> {
    blocking(core.inner().clone(), move |core| core.correction_draft(&canonical, &variant)).await
}

#[tauri::command]
async fn history_recent(core: Core<'_>, limit: usize, before_id: Option<i64>) -> Result<Vec<Dictation>> {
    blocking(core.inner().clone(), move |core| core.history_recent(limit, before_id)).await
}

#[tauri::command]
async fn usage_stats(core: Core<'_>, since_ms: Option<i64>, until_ms: i64) -> Result<UsageStats> {
    blocking(core.inner().clone(), move |core| core.usage_stats(since_ms, until_ms)).await
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
async fn history_audio(core: Core<'_>, id: i64) -> Result<tauri::ipc::Response> {
    let bytes = blocking(core.inner().clone(), move |core| core.history_audio(id)).await?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
async fn history_retry(core: Core<'_>, id: i64) -> Result<Dictation> {
    let core = core.inner().clone();
    let _claim = core
        .history
        .as_ref()
        .and_then(|history| history.claim_retry(id))
        .ok_or_else(|| CommandError::new("unavailable", "history retry is already running or unavailable"))?;
    let audio = blocking(core.clone(), move |core| core.history_retry_audio(id)).await?;
    core.controller.retry_history(id, audio).await.map_err(|kind| CommandError {
        code: "provider",
        message: core.settings.current().lang.error(kind).into(),
        kind: Some(kind),
    })?;
    blocking(core, move |core| {
        core.history
            .as_ref()
            .ok_or_else(|| CommandError::new("unavailable", "history is unavailable"))?
            .store()
            .get(id)?
            .ok_or_else(|| CommandError::new("unavailable", "history row was deleted"))
    })
    .await
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

#[tauri::command]
fn set_hotkey_capture(core: Core<'_>, active: bool) {
    core.inner().set_hotkey_capture(active);
}

#[tauri::command]
fn validate_hotkey(keys: Vec<String>) -> Result<()> {
    AppCore::validate_hotkey(&keys)
}

#[tauri::command]
fn get_update_state(app: AppHandle) -> UpdateState {
    updates::current(&app)
}

/// Runs a check now (or returns the state of the one already running).
#[tauri::command]
async fn check_for_updates(app: AppHandle) -> UpdateState {
    updates::check(&app).await
}

/// On success the app exits and the installer takes over.
#[tauri::command]
async fn install_update(app: AppHandle, core: Core<'_>) -> Result<()> {
    updates::install(&app, &core.controller).await
}
