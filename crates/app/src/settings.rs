//! The live settings snapshot (config + compiled rules + UI language). A dictation takes
//! one `Arc<Settings>` when it starts, so a config change mid-dictation cannot tear it.

use std::sync::{Arc, PoisonError, RwLock};

use opit_core::config::AppConfig;
use opit_core::rules::builtin::assemble;
use opit_core::rules::{RulePack, RuleSet, RuleWarning};

use crate::i18n::{self, Lang};

pub struct Settings {
    pub config: AppConfig,
    pub rules: Arc<RuleSet>,
    pub lang: Lang,
}

impl Settings {
    /// Compiles the personal pack (if any) and the enabled built-in packs. Broken rules
    /// are skipped and returned as warnings.
    pub fn build(
        config: AppConfig,
        user_pack: Option<RulePack>,
        system_locale: Option<&str>,
    ) -> (Self, Vec<RuleWarning>) {
        let (rules, warnings) = RuleSet::compile(&assemble(user_pack, &config.rules.enabled_packs));
        let lang = i18n::resolve(config.ui_language.as_deref(), system_locale);
        (Self { config, rules: Arc::new(rules), lang }, warnings)
    }
}

#[derive(Clone)]
pub struct SettingsHandle(Arc<RwLock<Arc<Settings>>>);

impl SettingsHandle {
    pub fn new(settings: Settings) -> Self {
        Self(Arc::new(RwLock::new(Arc::new(settings))))
    }

    pub fn current(&self) -> Arc<Settings> {
        self.0.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn replace(&self, settings: Settings) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personal_terms_come_first_and_language_is_resolved() {
        let user = RulePack::from_yaml("schema: 1\nid: user\nname: Me\nterms: [Opit]\n").unwrap();
        let (settings, warnings) = Settings::build(AppConfig::default(), Some(user), Some("tr-TR"));
        assert!(warnings.is_empty());
        assert_eq!(settings.rules.terms()[0], "Opit");
        assert_eq!(settings.lang, Lang::Tr);
    }

    #[test]
    fn broken_user_rules_become_warnings() {
        let user = RulePack::from_yaml(
            "schema: 1\nid: user\nname: Me\nreplacements:\n  - { from: '(', to: x, regex: true }\n",
        )
        .unwrap();
        let (_, warnings) = Settings::build(AppConfig::default(), Some(user), None);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn replace_is_seen_by_every_clone_but_not_by_held_snapshots() {
        let handle = SettingsHandle::new(Settings::build(AppConfig::default(), None, None).0);
        let clone = handle.clone();
        let held = handle.current();
        let mut config = AppConfig::default();
        config.paste.trailing_space = false;
        handle.replace(Settings::build(config, None, None).0);
        assert!(!clone.current().config.paste.trailing_space);
        assert!(held.config.paste.trailing_space);
    }
}
