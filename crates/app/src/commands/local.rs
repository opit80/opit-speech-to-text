use std::sync::Arc;

use tauri::AppHandle;

use crate::events;

use super::{Core, Result, blocking};

#[tauri::command]
pub(super) async fn local_state(core: Core<'_>) -> Result<crate::local::Snapshot> {
    blocking(core.inner().clone(), |core| Ok(core.local.snapshot()?)).await
}

#[tauri::command]
pub(super) async fn local_download(app: AppHandle, core: Core<'_>, id: String) -> Result<()> {
    let service = core.local.clone();
    service.install(id, Arc::new(move |progress| events::emit(&app, "local-progress", progress))).await?;
    Ok(())
}

#[tauri::command]
pub(super) fn local_cancel(core: Core<'_>) {
    core.local.cancel();
}

#[tauri::command]
pub(super) async fn local_delete(core: Core<'_>, id: String) -> Result<()> {
    Ok(core.local.delete(id).await?)
}
