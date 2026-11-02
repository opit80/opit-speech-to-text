use crate::controller::{DictationStatus, Msg};

use super::Core;

#[tauri::command]
pub(super) fn get_status(core: Core<'_>) -> DictationStatus {
    core.status()
}

#[tauri::command]
pub(super) fn toggle_dictation(core: Core<'_>) {
    core.controller.send(Msg::Toggle);
}

#[tauri::command]
pub(super) fn cancel_dictation(core: Core<'_>) {
    core.controller.send(Msg::Cancel);
}

#[tauri::command]
pub(super) fn retry_dictation(core: Core<'_>) {
    core.controller.send(Msg::Retry);
}
