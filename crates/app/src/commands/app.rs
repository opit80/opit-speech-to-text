use serde::Serialize;

use crate::startup::StartupNotice;

use super::Core;

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

#[tauri::command]
pub(super) fn app_info(core: Core<'_>) -> AppInfo {
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
pub(super) fn take_startup_notices(core: Core<'_>) -> Vec<StartupNotice> {
    core.take_notices()
}
