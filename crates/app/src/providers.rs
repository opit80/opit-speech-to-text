//! Real provider clients: API keys from the credential store, one cached HTTP client per
//! profile so consecutive dictations reuse the pooled (already TLS-handshaked) connection.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use opit_core::provider::{OpenAiCompatible, Profile};

use crate::controller::{Providers, SetupError};
use crate::platform::SecretStore;

pub struct HttpProviders {
    secrets: Arc<dyn SecretStore>,
    cache: Mutex<HashMap<String, Cached>>,
}

/// Not `Debug`: it holds an API key.
struct Cached {
    profile: Profile,
    key: Option<String>,
    client: Arc<OpenAiCompatible>,
}

impl HttpProviders {
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self { secrets, cache: Mutex::default() }
    }

    fn key_for(&self, profile: &Profile) -> Result<Option<String>, SetupError> {
        let Some(key_ref) = &profile.api_key_ref else {
            return Ok(None);
        };
        match self.secrets.get(key_ref)? {
            Some(key) if !key.trim().is_empty() => Ok(Some(key)),
            _ => Err(SetupError::MissingKey(profile.name.clone())),
        }
    }
}

impl Providers for HttpProviders {
    type Client = Arc<OpenAiCompatible>;

    fn client(&self, profile: &Profile) -> Result<Self::Client, SetupError> {
        let key = self.key_for(profile)?;
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(cached) = cache.get(&profile.id)
            && cached.profile == *profile
            && cached.key == key
        {
            return Ok(cached.client.clone());
        }
        let client = Arc::new(
            OpenAiCompatible::new(profile.clone(), key.clone()).map_err(|e| SetupError::Client(e.to_string()))?,
        );
        cache.insert(profile.id.clone(), Cached { profile: profile.clone(), key, client: client.clone() });
        Ok(client)
    }

    fn warm_up(&self, client: &Self::Client) {
        let client = client.clone();
        tokio::spawn(async move { client.warm_up().await });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use opit_core::provider::presets;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::platform::fake::FakeSecrets;

    #[test]
    fn clients_are_cached_until_the_profile_or_key_changes() {
        let secrets = Arc::new(FakeSecrets::with("groq", "sk-1"));
        let providers = HttpProviders::new(secrets.clone());
        let groq = presets::groq();
        let first = providers.client(&groq).unwrap();
        assert!(Arc::ptr_eq(&first, &providers.client(&groq).unwrap()));

        secrets.set("groq", "sk-2").unwrap();
        let rekeyed = providers.client(&groq).unwrap();
        assert!(!Arc::ptr_eq(&first, &rekeyed));

        let mut edited = groq.clone();
        edited.model = "whisper-large-v3-turbo".into();
        assert!(!Arc::ptr_eq(&rekeyed, &providers.client(&edited).unwrap()));
    }

    #[test]
    fn a_missing_or_blank_key_is_reported_by_profile_name() {
        let secrets = Arc::new(FakeSecrets::with("openai", "  "));
        let providers = HttpProviders::new(secrets);
        assert_eq!(providers.client(&presets::groq()).err(), Some(SetupError::MissingKey("Groq".into())));
        assert_eq!(providers.client(&presets::openai()).err(), Some(SetupError::MissingKey("OpenAI".into())));
    }

    #[test]
    fn keyless_profiles_need_no_secret_and_store_errors_surface() {
        let secrets = Arc::new(FakeSecrets::default());
        *secrets.broken.lock().unwrap() = true;
        let providers = HttpProviders::new(secrets);
        let mut local = presets::custom("gpu", "GPU", "http://127.0.0.1:8888/v1", "large-v3");
        local.api_key_ref = None;
        assert!(providers.client(&local).is_ok());
        assert!(matches!(providers.client(&presets::groq()), Err(SetupError::Secret(_))));
    }

    #[tokio::test]
    async fn warm_up_touches_the_models_endpoint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&server)
            .await;
        let mut profile = presets::custom("local", "Local", &format!("{}/v1", server.uri()), "m");
        profile.api_key_ref = None;
        let providers = HttpProviders::new(Arc::new(FakeSecrets::default()));
        let client = providers.client(&profile).unwrap();
        providers.warm_up(&client);
        for _ in 0..100 {
            if !server.received_requests().await.unwrap().is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("warm-up never reached the server");
    }
}
