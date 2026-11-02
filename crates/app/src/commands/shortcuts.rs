//! Explicit Linux desktop shortcut effects, always off the GTK thread.

use super::{Core, Result};
use crate::app_core::CommandError;
use crate::platform::linux::shortcuts::{self, Operation, RegistryState};

#[tauri::command]
pub(super) async fn linux_shortcut_registry(
    core: Core<'_>,
    action: String,
    keys: Vec<String>,
    operation: Operation,
) -> Result<RegistryState> {
    if matches!(operation, Operation::Install) && shortcuts::keys_for(&core.config(), &action) != Some(keys.as_slice())
    {
        return Err(CommandError::new("shortcut_config", "Save the shortcut before installing its desktop command"));
    }
    shortcuts::manage(action, keys, operation)
        .await
        .map_err(|_| CommandError::new("shortcut_registry", "Could not update or read the desktop shortcut registry"))
}

#[tauri::command]
pub(super) async fn probe_shortcut_command(core: Core<'_>) -> Result<()> {
    let controller = core.controller.clone();
    if !controller.shortcut_test().active {
        return Err(CommandError::new("unavailable", "Start the shortcut test before probing the command"));
    }
    // Exercise exactly the executable registered with the desktop, including an AppImage remount.
    let success = tauri::async_runtime::spawn_blocking(|| {
        let exe = crate::platform::linux::cli::executable().ok()?;
        std::process::Command::new(exe)
            .args(["--shortcut", "probe"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()
            .map(|status| status.success())
    })
    .await
    .ok()
    .flatten()
    .unwrap_or(false);
    if success && controller.shortcut_test().confirmed {
        Ok(())
    } else {
        Err(CommandError::new("shortcut_probe", "The shortcut command round trip was not confirmed"))
    }
}
