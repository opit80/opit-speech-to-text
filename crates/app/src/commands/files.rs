use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::app_core::CommandError;
use crate::events;

use super::{Core, Result};

#[tauri::command]
pub(super) async fn file_pick(app: AppHandle) -> Result<crate::file_jobs::FileState> {
    crate::file_jobs::pick(app).await
}

#[tauri::command]
pub(super) fn file_state(jobs: State<'_, Arc<crate::file_jobs::FileJobs>>) -> crate::file_jobs::FileState {
    jobs.snapshot()
}

#[tauri::command]
pub(super) fn file_start(
    app: AppHandle,
    core: Core<'_>,
    options: crate::file_jobs::FileOptions,
) -> Result<crate::file_jobs::FileState> {
    crate::file_jobs::start(app, core.inner().clone(), options)
}

#[tauri::command]
pub(super) fn file_cancel(app: AppHandle, jobs: State<'_, Arc<crate::file_jobs::FileJobs>>) {
    jobs.cancel();
    let state = jobs.snapshot();
    events::emit(&app, events::FILE_PROGRESS, crate::file_jobs::FileEvent::from(&state));
}

/// The UI copied the result of `revision`; it no longer blocks update installation.
#[tauri::command]
pub(super) fn file_handled(
    app: AppHandle,
    jobs: State<'_, Arc<crate::file_jobs::FileJobs>>,
    revision: u64,
) -> crate::file_jobs::FileState {
    let state = jobs.handled(revision);
    events::emit(&app, events::FILE_PROGRESS, crate::file_jobs::FileEvent::from(&state));
    state
}

#[tauri::command]
pub(super) fn file_discard(
    app: AppHandle,
    jobs: State<'_, Arc<crate::file_jobs::FileJobs>>,
    revision: u64,
) -> crate::file_jobs::FileState {
    let state = jobs.discard(revision);
    events::emit(&app, events::FILE_PROGRESS, crate::file_jobs::FileEvent::from(&state));
    state
}

#[tauri::command]
pub(super) async fn file_render(
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
pub(super) async fn file_save(
    app: AppHandle,
    format: crate::file_jobs::OutputFormat,
    prefixes: bool,
    speakers: bool,
) -> Result<bool> {
    crate::file_jobs::save(app, format, prefixes, speakers).await
}
