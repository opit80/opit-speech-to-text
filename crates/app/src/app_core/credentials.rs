use opit_core::provider::{Profile, ProviderClient};
use tracing::info;

use super::{AppCore, CommandError};

impl AppCore {
    // ----- API keys -----

    pub fn has_api_key(&self, key_ref: &str) -> Result<bool, CommandError> {
        Ok(self.platform.secrets.get(key_ref)?.is_some_and(|k| !k.trim().is_empty()))
    }

    pub fn set_api_key(&self, key_ref: &str, key: &str) -> Result<(), CommandError> {
        let key = key.trim();
        if key_ref.trim().is_empty() || key.is_empty() {
            return Err(CommandError::new("invalid_input", "the key and its name must not be empty"));
        }
        self.platform.secrets.set(key_ref, key)?;
        info!(key_ref, "API key stored");
        Ok(())
    }

    pub fn delete_api_key(&self, key_ref: &str) -> Result<(), CommandError> {
        self.platform.secrets.delete(key_ref)?;
        info!(key_ref, "API key deleted");
        Ok(())
    }

    /// Tests `profile` with `api_key`, or with the stored key when `api_key` is `None`.
    pub async fn test_connection(&self, profile: Profile, api_key: Option<String>) -> Result<(), CommandError> {
        if profile.provider == opit_core::provider::ProviderKind::LocalWhisper {
            return Ok(self.local.test(&profile).await?);
        }
        let key = match api_key {
            Some(key) => Some(key),
            None => match &profile.api_key_ref {
                Some(key_ref) => self.platform.secrets.get(key_ref)?,
                None => None,
            },
        };
        let client = ProviderClient::new(profile, key)?;
        Ok(client.test_connection().await?)
    }

    /// Resolve credentials and snapshot glossary on a blocking worker before the test request.
    pub fn cleanup_test_client(
        &self,
        mut config: opit_core::cleanup::CleanupConfig,
    ) -> Result<(Option<opit_core::cleanup::CleanupClient>, String, String), CommandError> {
        use opit_core::cleanup::{CleanupClient, CleanupError, system_prompt};
        config.normalize_styles();
        let settings = self.settings.current();
        let raw = config.active_style_id == "raw";
        let key = match &config.api_key_ref {
            _ if raw => None,
            Some(key_ref) => {
                self.platform.secrets.get(key_ref).map_err(|_| CommandError::from(CleanupError::MissingKey))?
            }
            None => None,
        };
        let language = if settings.lang == crate::i18n::Lang::Tr { "tr" } else { "en" };
        let system = system_prompt(&config, language, &settings.rules, &settings.config.rules.prompt_context);
        let sample = if language == "tr" {
            "ee bugün bugün toplantı var saat üçte başlayacak"
        } else {
            "um today today we have a meeting it starts at three"
        };
        let client = if raw { None } else { Some(CleanupClient::new(config, key)?) };
        Ok((client, system, sample.into()))
    }
}
