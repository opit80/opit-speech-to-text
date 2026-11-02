//! Per-application overrides resolved once when a microphone dictation starts.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::config::{AppConfig, PasteAfter, PasteMethod};
use crate::text::fold;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppRulesConfig {
    pub enabled: bool,
    pub rules: Vec<AppRule>,
}

impl Default for AppRulesConfig {
    fn default() -> Self {
        Self { enabled: true, rules: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppRule {
    pub id: String,
    pub enabled: bool,
    /// Process image file name only, never a path.
    pub exe: String,
    /// Empty matches any title. Compared using the shared Turkish-aware fold.
    pub title_contains: String,
    pub style_id: Option<String>,
    pub profile_id: Option<String>,
    pub paste_method: Option<PasteMethod>,
    pub paste_after: Option<PasteAfter>,
}

impl Default for AppRule {
    fn default() -> Self {
        Self {
            id: String::new(),
            enabled: true,
            exe: String::new(),
            title_contains: String::new(),
            style_id: None,
            profile_id: None,
            paste_method: None,
            paste_after: None,
        }
    }
}

impl AppRulesConfig {
    pub fn needs_title(&self) -> bool {
        self.enabled && self.rules.iter().any(|r| r.enabled && !r.title_contains.trim().is_empty())
    }

    /// First enabled match wins, even if it specifies no overrides.
    pub fn matching(&self, exe: &str, title: &str) -> Option<&AppRule> {
        if !self.enabled || !valid_exe(exe) {
            return None;
        }
        let exe = fold(exe);
        let title = fold(title);
        self.rules.iter().find(|r| {
            r.enabled
                && valid_exe(&r.exe)
                && fold(&r.exe) == exe
                && (r.title_contains.is_empty() || title.contains(&fold(&r.title_contains)))
        })
    }

    pub fn normalize(&mut self, style_ids: &HashSet<String>, profile_ids: &HashSet<String>) {
        for rule in &mut self.rules {
            rule.id = rule.id.trim().into();
            rule.exe = fold(rule.exe.trim());
            rule.title_contains = rule.title_contains.trim().into();
            for (reference, ids) in [(&mut rule.style_id, style_ids), (&mut rule.profile_id, profile_ids)] {
                *reference = reference.take().map(|id| id.trim().to_owned()).filter(|id| ids.contains(id));
            }
        }
        self.rules.retain(|r| valid_exe(&r.exe));
        let reserved: HashSet<_> = self.rules.iter().map(|r| r.id.clone()).collect();
        let mut seen = HashSet::new();
        let mut next = 1;
        for rule in &mut self.rules {
            if !rule.id.is_empty() && seen.insert(rule.id.clone()) {
                continue;
            }
            loop {
                let id = format!("app-rule-{next}");
                next += 1;
                if !reserved.contains(&id) && seen.insert(id.clone()) {
                    rule.id = id;
                    break;
                }
            }
        }
    }
}

fn valid_exe(exe: &str) -> bool {
    !exe.is_empty() && !matches!(exe, "." | "..") && !exe.chars().any(|c| c.is_control() || r#"/\:*?"<>|"#.contains(c))
}

impl AppRule {
    /// Apply only to a private dictation snapshot. Dangling references keep global values.
    pub fn apply(&self, config: &mut AppConfig) {
        if let Some(id) = &self.style_id
            && config.cleanup.style(id).is_some()
        {
            config.cleanup.active_style_id = id.clone();
        }
        if let Some(id) = &self.profile_id
            && config.profile(id).is_some()
        {
            config.active_profile_id = id.clone();
        }
        if let Some(method) = self.paste_method {
            config.paste.method = method;
        }
        if let Some(after) = self.paste_after {
            config.paste.after = after;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, PasteAfter, PasteMethod};

    fn rule(id: &str, exe: &str, title: &str) -> AppRule {
        AppRule { id: id.into(), exe: exe.into(), title_contains: title.into(), ..AppRule::default() }
    }

    #[test]
    fn matching_folds_case_and_turkish_titles_and_preserves_order() {
        let mut rules = AppRulesConfig {
            enabled: true,
            rules: vec![rule("title", "CODE.EXE", "İŞ IŞIK"), rule("any", "code.exe", "")],
        };
        assert!(rules.needs_title());
        assert_eq!(rules.matching("Code.exe", "Project: iş ışık").unwrap().id, "title");
        assert_eq!(rules.matching("code.exe", "other").unwrap().id, "any");
        rules.rules[0].enabled = false;
        assert!(!rules.needs_title());
        assert_eq!(rules.matching("code.exe", "İŞ IŞIK").unwrap().id, "any");
        rules.rules.swap(0, 1);
        rules.rules[1].enabled = true;
        assert_eq!(rules.matching("code.exe", "İŞ IŞIK").unwrap().id, "any");
        assert!(rules.matching("other.exe", "İŞ IŞIK").is_none());
        // The first matching rule has no overrides and still blocks later matches.
        rules.rules[0].style_id = None;
        rules.rules[1].style_id = Some("formal".into());
        assert!(rules.matching("code.exe", "İŞ IŞIK").unwrap().style_id.is_none());
        rules.enabled = false;
        assert!(!rules.needs_title());
        assert!(rules.matching("code.exe", "İŞ IŞIK").is_none());
    }

    #[test]
    fn old_and_partial_configs_use_defaults_and_round_trip() {
        for json in ["{}", r#"{"app_rules":{}}"#] {
            assert_eq!(AppConfig::from_json(json).unwrap().app_rules, AppRulesConfig::default());
        }
        let config = AppConfig::from_json(r#"{"app_rules":{"rules":[{"exe":"code.exe"}]}}"#).unwrap();
        let rule = &config.app_rules.rules[0];
        assert!(rule.enabled);
        assert!(!rule.id.is_empty());
        assert!(rule.title_contains.is_empty());
        assert_eq!(
            (rule.style_id.as_ref(), rule.profile_id.as_ref(), rule.paste_method, rule.paste_after),
            (None, None, None, None)
        );
        assert_eq!(AppConfig::from_json(&config.to_json()).unwrap(), config);
    }

    #[test]
    fn normalization_repairs_ids_filenames_and_references_idempotently() {
        let mut config = AppConfig::default();
        config.app_rules.rules = vec![
            rule(" same ", " CODE.EXE ", " Window "),
            rule("same", "other.exe", ""),
            rule("app-rule-1", "third.exe", ""),
            rule("", "fourth.exe", ""),
            rule("empty", " ", ""),
            rule("path", r"C:\apps\code.exe", ""),
            rule("path2", "/usr/code.exe", ""),
        ];
        config.app_rules.rules[0].style_id = Some(" formal ".into());
        config.app_rules.rules[0].profile_id = Some(" openai ".into());
        config.app_rules.rules[1].style_id = Some("deleted".into());
        config.app_rules.rules[1].profile_id = Some("deleted".into());
        config.normalize();
        let rules = &config.app_rules.rules;
        assert_eq!(rules.len(), 4);
        assert_eq!(rules[0].id, "same");
        assert_eq!(rules[0].exe, "code.exe");
        assert_eq!(rules[0].title_contains, "Window");
        assert_eq!(rules[0].style_id.as_deref(), Some("formal"));
        assert_eq!(rules[0].profile_id.as_deref(), Some("openai"));
        assert_eq!(rules[2].id, "app-rule-1", "repair must not steal a later rule's ID");
        assert_eq!(rules.iter().map(|r| &r.id).collect::<std::collections::HashSet<_>>().len(), 4);
        assert!(rules[1].style_id.is_none() && rules[1].profile_id.is_none());
        assert_eq!(AppConfig::from_json(&config.to_json()).unwrap(), config);
        config.cleanup.styles.retain(|s| s.id != "formal");
        config.profiles.retain(|p| p.id != "openai");
        // Application also ignores dangling references before normalization.
        let applied = config.app_rules.rules[0].clone();
        applied.apply(&mut config);
        assert_eq!(config.active_profile_id, "groq");
        assert_eq!(config.cleanup.active_style_id, "clean");
    }

    #[test]
    fn only_explicit_valid_overrides_apply_including_none_after_key() {
        let mut config = AppConfig::default();
        config.paste.after = PasteAfter::Enter;
        let mut matched = rule("editor", "code.exe", "");
        matched.style_id = Some("raw".into());
        matched.profile_id = Some("openai".into());
        matched.paste_method = Some(PasteMethod::Type);
        matched.paste_after = Some(PasteAfter::None);
        matched.apply(&mut config);
        assert_eq!(config.cleanup.active_style_id, "raw");
        assert_eq!(config.active_profile_id, "openai");
        assert_eq!(config.paste.method, PasteMethod::Type);
        assert_eq!(config.paste.after, PasteAfter::None);
        AppRule::default().apply(&mut config);
        assert_eq!(config.active_profile_id, "openai");
        assert_eq!(config.paste.after, PasteAfter::None);
    }

    #[test]
    fn override_enums_accept_exact_wire_values_and_reject_invalid_values() {
        for method in ["ctrl_v", "shift_insert", "ctrl_shift_v", "type"] {
            for after in ["none", "enter", "ctrl_enter"] {
                let json = serde_json::json!({"app_rules":{"rules":[{
                    "exe":"code.exe", "paste_method": method, "paste_after": after
                }]}})
                .to_string();
                let config = AppConfig::from_json(&json).unwrap();
                let saved = serde_json::to_value(&config).unwrap();
                assert_eq!(saved["app_rules"]["rules"][0]["paste_method"], method);
                assert_eq!(saved["app_rules"]["rules"][0]["paste_after"], after);
                assert_eq!(AppConfig::from_json(&config.to_json()).unwrap(), config);
            }
        }
        for field in ["paste_method", "paste_after"] {
            let json = serde_json::json!({"app_rules":{"rules":[{"exe":"code.exe", field: "invalid"}]}}).to_string();
            assert!(AppConfig::from_json(&json).is_err());
        }
    }
}
