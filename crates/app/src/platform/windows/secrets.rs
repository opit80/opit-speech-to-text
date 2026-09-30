//! API keys in the Windows Credential Manager (keyring-core + windows-native-keyring-store).
//!
//! Each secret is a generic credential with target name `<key_ref>.<service>`
//! (the store's default, keyring-v1-compatible naming).

use std::sync::Arc;

use keyring_core::Error as KeyringError;
use keyring_core::api::CredentialStoreApi;
use windows_native_keyring_store::Store;

use crate::platform::{SecretError, SecretStore};

/// Service name used for every credential this app writes.
pub const SERVICE: &str = "opit-speech-to-text";

/// [`SecretStore`] backed by the Windows Credential Manager.
#[derive(Debug)]
pub struct KeyringStore {
    store: Arc<Store>,
    service: String,
}

impl KeyringStore {
    /// Opens the credential store with the app's service name.
    pub fn new() -> Result<Self, SecretError> {
        Self::with_service(SERVICE)
    }

    /// Opens the credential store with a custom service name (tests).
    pub fn with_service(service: &str) -> Result<Self, SecretError> {
        let store = Store::new().map_err(to_secret_error)?;
        Ok(Self { store, service: service.to_owned() })
    }

    fn entry(&self, key_ref: &str) -> Result<keyring_core::Entry, SecretError> {
        self.store.build(&self.service, key_ref, None).map_err(to_secret_error)
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key_ref: &str) -> Result<Option<String>, SecretError> {
        match self.entry(key_ref)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(e) => Err(to_secret_error(e)),
        }
    }

    fn set(&self, key_ref: &str, secret: &str) -> Result<(), SecretError> {
        self.entry(key_ref)?.set_password(secret).map_err(to_secret_error)
    }

    fn delete(&self, key_ref: &str) -> Result<(), SecretError> {
        match self.entry(key_ref)?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(e) => Err(to_secret_error(e)),
        }
    }
}

/// Uses `Display`, which never includes secret bytes (unlike `Debug` of `BadEncoding`).
fn to_secret_error(e: KeyringError) -> SecretError {
    SecretError(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "writes to the real Credential Manager"]
    fn round_trip_with_throwaway_key() {
        let store = KeyringStore::new().expect("credential store");
        let key_ref = format!("opit-test-{}", std::process::id());
        store.delete(&key_ref).unwrap();
        assert_eq!(store.get(&key_ref).unwrap(), None);

        store.set(&key_ref, "sk-test-ğüşiöç-123").unwrap();
        assert_eq!(store.get(&key_ref).unwrap().as_deref(), Some("sk-test-ğüşiöç-123"));
        store.set(&key_ref, "sk-overwritten").unwrap();
        assert_eq!(store.get(&key_ref).unwrap().as_deref(), Some("sk-overwritten"));

        store.delete(&key_ref).unwrap();
        assert_eq!(store.get(&key_ref).unwrap(), None);
        store.delete(&key_ref).expect("deleting a missing entry is ok");
    }
}
