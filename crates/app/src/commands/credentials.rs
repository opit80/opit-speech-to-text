use opit_core::provider::Profile;

use super::{Core, Result, blocking};

#[tauri::command]
pub(super) async fn has_api_key(core: Core<'_>, key_ref: String) -> Result<bool> {
    blocking(core.inner().clone(), move |core| core.has_api_key(&key_ref)).await
}

#[tauri::command]
pub(super) async fn set_api_key(core: Core<'_>, key_ref: String, key: String) -> Result<()> {
    blocking(core.inner().clone(), move |core| core.set_api_key(&key_ref, &key)).await
}

#[tauri::command]
pub(super) async fn delete_api_key(core: Core<'_>, key_ref: String) -> Result<()> {
    blocking(core.inner().clone(), move |core| core.delete_api_key(&key_ref)).await
}

/// `api_key: null` tests with the stored key.
#[tauri::command]
pub(super) async fn test_connection(core: Core<'_>, profile: Profile, api_key: Option<String>) -> Result<()> {
    core.test_connection(profile, api_key).await
}
