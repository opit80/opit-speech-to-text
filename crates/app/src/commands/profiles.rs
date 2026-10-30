use opit_core::provider::Profile;

use crate::app_core::CommandError;

use super::{Core, Result, blocking};

/// Provider presets and a blank custom template (the UI fills in id, name, URL and model).
#[tauri::command]
pub(super) fn profile_presets() -> Vec<Profile> {
    use opit_core::provider::presets;
    vec![
        presets::groq(),
        presets::openai(),
        presets::elevenlabs(),
        presets::deepgram(),
        presets::local(),
        presets::custom("custom", "", "", ""),
    ]
}

#[tauri::command]
pub(super) async fn streaming_supported(core: Core<'_>, profile_id: String) -> Result<bool> {
    blocking(core.inner().clone(), move |core| {
        core.config()
            .profile(&profile_id)
            .map(Profile::supports_streaming)
            .ok_or_else(|| CommandError::new("invalid_input", "unknown profile"))
    })
    .await
}

#[tauri::command]
pub(super) async fn translate_languages() -> Vec<opit_core::voice_modes::TranslateLanguage> {
    opit_core::voice_modes::translate_languages()
}
