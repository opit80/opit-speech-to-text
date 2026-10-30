use serde::Serialize;

use super::{Core, Result, blocking};

#[tauri::command]
pub(super) fn cleanup_style_defaults() -> Vec<opit_core::cleanup::styles::StyleDefaults> {
    opit_core::cleanup::styles::defaults()
}

#[tauri::command]
pub(super) fn cleanup_presets() -> Vec<opit_core::cleanup::CleanupPreset> {
    opit_core::cleanup::presets()
}

#[tauri::command]
pub(super) fn cleanup_prompts() -> std::collections::BTreeMap<&'static str, &'static str> {
    [("en", opit_core::cleanup::EN_PROMPT), ("tr", opit_core::cleanup::TR_PROMPT)].into_iter().collect()
}

#[derive(Serialize)]
pub(super) struct CleanupTestResult {
    before: String,
    after: String,
}

#[tauri::command]
pub(super) async fn test_cleanup(
    core: Core<'_>,
    config: opit_core::cleanup::CleanupConfig,
) -> Result<CleanupTestResult> {
    let (client, system, before) = blocking(core.inner().clone(), move |core| core.cleanup_test_client(config)).await?;
    let after = match client {
        Some(client) => client.clean(&before, &system).await?,
        None => before.clone(),
    };
    Ok(CleanupTestResult { before, after })
}
