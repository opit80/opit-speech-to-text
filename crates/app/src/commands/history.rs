use opit_core::history::{Dictation, UsageStats};
use tauri::AppHandle;

use crate::app_core::{AppCore, CommandError, HistoryUpdate};
use crate::events;

use super::{Core, Result, blocking};

#[tauri::command]
pub(super) async fn history_update_text(
    app: AppHandle,
    core: Core<'_>,
    id: i64,
    text: String,
) -> Result<HistoryUpdate> {
    let updated = blocking(core.inner().clone(), move |core| core.history_update_text(id, &text)).await?;
    events::emit(&app, events::HISTORY_UPDATED, id);
    Ok(updated)
}

#[tauri::command]
pub(super) async fn history_recent(core: Core<'_>, limit: usize, before_id: Option<i64>) -> Result<Vec<Dictation>> {
    blocking(core.inner().clone(), move |core| core.history_recent(limit, before_id)).await
}

#[tauri::command]
pub(super) async fn usage_stats(core: Core<'_>, since_ms: Option<i64>, until_ms: i64) -> Result<UsageStats> {
    blocking(core.inner().clone(), move |core| core.usage_stats(since_ms, until_ms)).await
}

#[tauri::command]
pub(super) async fn history_search(core: Core<'_>, query: String, limit: usize) -> Result<Vec<Dictation>> {
    blocking(core.inner().clone(), move |core| core.history_search(&query, limit)).await
}

#[tauri::command]
pub(super) async fn history_delete(core: Core<'_>, id: i64) -> Result<()> {
    blocking(core.inner().clone(), move |core| core.history_delete(id)).await
}

#[tauri::command]
pub(super) async fn history_clear(core: Core<'_>) -> Result<()> {
    blocking(core.inner().clone(), AppCore::history_clear).await
}

#[tauri::command]
pub(super) async fn history_audio(core: Core<'_>, id: i64) -> Result<tauri::ipc::Response> {
    let bytes = blocking(core.inner().clone(), move |core| core.history_audio(id)).await?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub(super) async fn history_retry(core: Core<'_>, id: i64) -> Result<Dictation> {
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
