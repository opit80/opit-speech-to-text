//! Linux session state. Recovery text is returned only on an explicit UI read.

use tauri::AppHandle;

use crate::app_core::CommandError;
use crate::controller::{PendingResults, ShortcutTest};
use crate::events;

use super::{Core, Result};

#[tauri::command]
pub(super) fn pending_results(core: Core<'_>) -> PendingResults {
    core.controller.pending_results()
}

#[tauri::command]
pub(super) fn dismiss_pending_result(app: AppHandle, core: Core<'_>, id: u64) -> PendingResults {
    let result = core.controller.dismiss_pending_result(id);
    events::emit(&app, events::PENDING_RESULTS_CHANGED, ());
    result
}

#[tauri::command]
pub(super) fn shortcut_test_state(core: Core<'_>) -> ShortcutTest {
    core.controller.shortcut_test()
}

#[tauri::command]
pub(super) async fn set_shortcut_test(core: Core<'_>, active: bool) -> Result<ShortcutTest> {
    core.controller.set_shortcut_test(active).await.map_err(|kind| CommandError {
        code: "unavailable",
        message: "The shortcut test requires an idle app".into(),
        kind: Some(kind),
    })
}
