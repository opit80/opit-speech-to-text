//! Invoke API for the web UI. Plan 3 builds the pages on top of it. Each command is a thin
//! wrapper over [`AppCore`]; JS passes camelCase argument names (`keyRef` → `key_ref`).
//! Events the UI can listen to are in `events.rs`.

mod app;
mod ask;
mod cleanup;
mod config;
mod credentials;
mod dictation;
mod files;
mod history;
mod input;
mod local;
mod profiles;
mod rules;
#[cfg(target_os = "linux")]
mod session;
#[cfg(target_os = "linux")]
mod shortcuts;
mod updates;

use std::sync::Arc;

use tauri::State;

use crate::app_core::{AppCore, CommandError};

pub use app::AppInfo;

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

pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        #[cfg(target_os = "linux")]
        session::pending_results,
        #[cfg(target_os = "linux")]
        session::dismiss_pending_result,
        #[cfg(target_os = "linux")]
        session::shortcut_test_state,
        #[cfg(target_os = "linux")]
        session::set_shortcut_test,
        #[cfg(target_os = "linux")]
        shortcuts::linux_shortcut_registry,
        #[cfg(target_os = "linux")]
        shortcuts::probe_shortcut_command,
        app::app_info,
        app::platform_capabilities,
        dictation::get_status,
        dictation::toggle_dictation,
        dictation::cancel_dictation,
        dictation::retry_dictation,
        config::get_config,
        config::save_config,
        config::set_active_profile,
        config::set_active_style,
        cleanup::cleanup_style_defaults,
        profiles::profile_presets,
        profiles::streaming_supported,
        local::local_state,
        local::local_download,
        local::local_cancel,
        local::local_delete,
        profiles::translate_languages,
        cleanup::cleanup_presets,
        cleanup::cleanup_prompts,
        cleanup::test_cleanup,
        ask::ask_status,
        ask::test_ask,
        input::list_microphones,
        input::list_running_apps,
        input::mic_test_start,
        input::mic_test_stop,
        credentials::has_api_key,
        credentials::set_api_key,
        credentials::delete_api_key,
        credentials::test_connection,
        rules::get_user_rules,
        rules::save_user_rules,
        rules::rules_preview,
        rules::prompt_budget,
        rules::parse_user_rules,
        rules::render_user_rules,
        rules::correction_draft,
        history::history_update_text,
        rules::learning_apply,
        rules::learning_dismiss,
        history::history_recent,
        history::usage_stats,
        history::history_search,
        history::history_delete,
        history::history_clear,
        history::history_audio,
        history::history_retry,
        input::get_hotkey_state,
        input::set_hotkey_paused,
        input::set_hotkey_capture,
        input::validate_hotkey,
        app::take_startup_notices,
        updates::get_update_state,
        updates::check_for_updates,
        updates::install_update,
        files::file_pick,
        files::file_state,
        files::file_start,
        files::file_cancel,
        files::file_render,
        files::file_save,
        files::file_handled,
        files::file_discard,
    ]
}
