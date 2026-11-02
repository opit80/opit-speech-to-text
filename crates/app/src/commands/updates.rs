use tauri::AppHandle;

use crate::updates as update_service;
use crate::updates::UpdateState;

use super::{Core, Result};

#[tauri::command]
pub(super) fn get_update_state(app: AppHandle) -> UpdateState {
    update_service::current(&app)
}

/// Runs a check now (or returns the state of the one already running).
#[tauri::command]
pub(super) async fn check_for_updates(app: AppHandle) -> UpdateState {
    update_service::check(&app).await
}

/// On success the app exits and the installer takes over.
#[tauri::command]
pub(super) async fn install_update(app: AppHandle, core: Core<'_>) -> Result<()> {
    update_service::install(&app, &core.controller).await
}
