use crate::app_core::CommandError;

use super::{Core, Result};

#[tauri::command]
pub(super) async fn ask_status(core: Core<'_>) -> Result<Vec<crate::ask::AgentStatus>> {
    tauri::async_runtime::spawn_blocking(crate::ask::statuses)
        .await
        .map_err(|_| CommandError::new("unavailable", core.settings.current().lang.ask_detection_failed()))
}

#[tauri::command]
pub(super) async fn test_ask(core: Core<'_>, config: opit_core::ask::AskConfig) -> Result<String> {
    core.controller.test_ask(config).await.map_err(|kind| CommandError {
        code: "ask",
        message: core.settings.current().lang.error(kind).into(),
        kind: Some(kind),
    })
}
