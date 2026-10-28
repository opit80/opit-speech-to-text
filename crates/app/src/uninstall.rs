//! `opit-speech-to-text.exe --delete-credentials`, run by the NSIS uninstaller
//! (`crates/app/windows/hooks.nsh`) when "Delete the application data" is ticked. The uninstaller
//! deletes the data folder itself; API keys live in Windows Credential Manager, which only the app
//! knows how to address.

use std::collections::BTreeSet;

use opit_core::provider::presets;

use crate::platform::SecretStore;
use crate::platform::windows::KeyringStore;
use crate::startup::Paths;

/// Command-line flag the uninstaller passes. `run()` in lib.rs handles it before anything starts.
pub const DELETE_CREDENTIALS_FLAG: &str = "--delete-credentials";

/// Speech and cleanup preset refs, plus saved profile and cleanup `api_key_ref` values.
/// The file is read as plain JSON (no schema, no migration), so a damaged or newer config
/// still gives up its refs.
pub fn credential_refs(config_json: Option<&str>) -> Vec<String> {
    let mut refs: BTreeSet<String> = [presets::groq(), presets::openai(), presets::elevenlabs()]
        .into_iter()
        .filter_map(|profile| profile.api_key_ref)
        .collect();
    refs.extend(opit_core::cleanup::presets().into_iter().filter_map(|p| p.config.api_key_ref));
    let config = config_json.and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok());
    let profiles = config.as_ref().and_then(|c| c.get("profiles")).and_then(|p| p.as_array());
    for profile in profiles.into_iter().flatten() {
        let key_ref = profile.get("api_key_ref").and_then(|r| r.as_str()).map(str::trim);
        if let Some(key_ref) = key_ref.filter(|r| !r.is_empty()) {
            refs.insert(key_ref.to_string());
        }
    }
    if let Some(key_ref) = config
        .as_ref()
        .and_then(|c| c.get("cleanup"))
        .and_then(|c| c.get("api_key_ref"))
        .and_then(|r| r.as_str())
        .map(str::trim)
        .filter(|r| !r.is_empty())
    {
        refs.insert(key_ref.into());
    }
    refs.into_iter().collect()
}

/// Deletes each ref; a missing entry counts as deleted. Returns how many deletions failed.
pub fn delete_credentials(store: &dyn SecretStore, refs: &[String]) -> usize {
    refs.iter().filter(|key_ref| store.delete(key_ref).is_err()).count()
}

/// Exit code 0 when every entry is gone, 1 otherwise. Logs nothing: logging would recreate the
/// data folder the uninstaller is about to delete.
pub fn run() -> i32 {
    let config = Paths::from_system().and_then(|paths| std::fs::read_to_string(paths.config).ok());
    let refs = credential_refs(config.as_deref());
    match KeyringStore::new() {
        Ok(store) if delete_credentials(&store, &refs) == 0 => 0,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::fake::FakeSecrets;

    fn expected(extra: &[&str]) -> Vec<String> {
        let mut refs: Vec<String> = [
            "cleanup",
            "cleanup-custom",
            "cleanup-deepseek",
            "cleanup-google",
            "cleanup-groq",
            "cleanup-openai",
            "cleanup-openrouter",
            "elevenlabs",
            "groq",
            "openai",
        ]
        .into_iter()
        .chain(extra.iter().copied())
        .map(str::to_string)
        .collect();
        refs.sort();
        refs
    }

    #[test]
    fn refs_come_from_the_presets_and_every_profile() {
        let json = r#"{"profiles":[
            {"id":"groq","api_key_ref":"groq"},
            {"id":"home","api_key_ref":" home-gpu "},
            {"id":"local","api_key_ref":null},
            {"id":"blank","api_key_ref":""}
        ]}"#;
        assert_eq!(credential_refs(Some(json)), expected(&["home-gpu"]));
    }

    #[test]
    fn a_missing_or_damaged_config_still_gives_the_preset_refs() {
        assert_eq!(credential_refs(None), expected(&[]));
        assert_eq!(credential_refs(Some("{damaged")), expected(&[]));
        assert_eq!(credential_refs(Some(r#"{"profiles":"nope"}"#)), expected(&[]));
        assert_eq!(
            credential_refs(Some(r#"{"schema_version":99,"profiles":[{"api_key_ref":"new"}]}"#)),
            expected(&["new"])
        );
    }

    #[test]
    fn cleanup_credential_ref_is_read_even_without_profiles() {
        assert_eq!(
            credential_refs(Some(r#"{"cleanup":{"api_key_ref":" custom-cleanup "}}"#)),
            expected(&["custom-cleanup"])
        );
    }

    #[test]
    fn deletes_only_this_apps_refs() {
        let store = FakeSecrets::with("groq", "k1");
        store.set("home-gpu", "k2").unwrap();
        store.set("someone-else", "k3").unwrap();
        let refs = credential_refs(Some(r#"{"profiles":[{"api_key_ref":"home-gpu"}]}"#));
        assert_eq!(delete_credentials(&store, &refs), 0);
        assert_eq!(store.map.lock().unwrap().keys().collect::<Vec<_>>(), ["someone-else"]);
    }
}
