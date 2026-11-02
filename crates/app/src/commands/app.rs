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

#[derive(Debug, Clone, Serialize)]
pub struct PlatformCapabilities {
    pub platform: &'static str,
    pub local_models: bool,
    pub per_app_rules: bool,
    pub overlay: bool,
    pub media_duck: bool,
    pub updates: bool,
    pub tray: bool,
    pub shortcut_release: bool,
    pub shortcut_capture: bool,
    pub shortcut_backend: &'static str,
}

#[tauri::command]
pub(super) async fn platform_capabilities(
    app: tauri::AppHandle,
    core: Core<'_>,
) -> Result<PlatformCapabilities, super::CommandError> {
    #[cfg(target_os = "linux")]
    let per_app_rules = tauri::async_runtime::spawn_blocking(|| {
        crate::platform::linux::session::current() == crate::platform::linux::session::SessionKind::X11
            && crate::platform::linux::x11::available()
    })
    .await
    .map_err(|_| super::CommandError::new("unavailable", "Session discovery failed"))?;
    #[cfg(windows)]
    let per_app_rules = true;
    Ok(PlatformCapabilities {
        platform: if cfg!(windows) { "windows" } else { "linux" },
        local_models: cfg!(windows),
        per_app_rules,
        overlay: if cfg!(windows) { true } else { core.platform.overlay.available() },
        media_duck: {
            #[cfg(windows)]
            {
                true
            }
            #[cfg(target_os = "linux")]
            {
                crate::platform::linux::media::available()
            }
        },
        updates: crate::shell::updates::supported(),
        tray: crate::shell::tray_available(&app),
        // Linux command shortcuts deliver presses without release events.
        shortcut_release: cfg!(windows),
        shortcut_capture: cfg!(windows),
        shortcut_backend: {
            #[cfg(windows)]
            {
                "windows"
            }
            #[cfg(target_os = "linux")]
            {
                use crate::platform::linux::shortcuts::{Backend, backend};
                match backend() {
                    Backend::Kde => "kde",
                    Backend::Gnome => "gnome",
                    Backend::X11 => "x11",
                    Backend::Manual => "manual",
                }
            }
        },
    })
}

#[tauri::command]
pub(super) fn app_info(core: Core<'_>) -> AppInfo {
    let cli_path = std::env::current_exe()
        .map(|p| p.with_file_name(if cfg!(windows) { "opit.exe" } else { "opit" }).to_string_lossy().into_owned())
        .unwrap_or_default();
    #[cfg(target_os = "linux")]
    let cli_path = {
        std::env::var_os("APPIMAGE")
            .and_then(|image| crate::platform::linux::cli::launch_command(std::path::Path::new(&image)))
            .unwrap_or(cli_path)
    };
    AppInfo {
        cli_path,
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
