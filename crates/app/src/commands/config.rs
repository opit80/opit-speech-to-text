use opit_core::config::AppConfig;
use tauri::AppHandle;

use crate::events;

use super::{Core, Result, blocking};

#[tauri::command]
pub(super) fn get_config(core: Core<'_>) -> AppConfig {
    core.config()
}

#[tauri::command]
pub(super) async fn save_config(app: AppHandle, core: Core<'_>, config: AppConfig) -> Result<AppConfig> {
    let core = core.inner().clone();
    let saved = blocking(core.clone(), move |core| core.save_config(config)).await?;
    events::config_changed(&app, &saved);
    events::hotkey_state(&app, &core.hotkey_state());
    Ok(saved)
}

#[tauri::command]
pub(super) async fn set_active_profile(app: AppHandle, core: Core<'_>, id: String) -> Result<AppConfig> {
    let saved = blocking(core.inner().clone(), move |core| core.set_active_profile(&id)).await?;
    events::config_changed(&app, &saved);
    Ok(saved)
}

#[tauri::command]
pub(super) async fn set_active_style(app: AppHandle, core: Core<'_>, id: String) -> Result<AppConfig> {
    let saved = blocking(core.inner().clone(), move |core| core.set_active_style(&id)).await?;
    events::config_changed(&app, &saved);
    Ok(saved)
}
