use std::sync::{Arc, PoisonError};

use opit_core::learning::{Dismissals, LearningItem, LearningTarget, MAX_SUGGESTIONS, TermChange};
use opit_core::rules::pack::header_comments;
use opit_core::rules::prompt::{BuiltPrompt, build_prompt};
use opit_core::rules::{RulePack, RuleSet, RuleWarning};
use tracing::info;

use super::{AppCore, CommandError, CorrectionDraft, RulesPreview, USER_PACK_TEMPLATE};

impl AppCore {
    // ----- rules -----

    pub fn parse_user_rules(yaml: &str) -> Result<RulePack, CommandError> {
        Ok(RulePack::from_yaml(yaml)?)
    }

    /// YAML for `pack`, keeping the leading comment block of `previous_yaml`. The result is
    /// parsed back so an invalid pack (bad id, schema) is refused here, not on save.
    pub fn render_user_rules(pack: &RulePack, previous_yaml: &str) -> Result<String, CommandError> {
        let yaml = pack.to_yaml_with_header(&header_comments(previous_yaml))?;
        RulePack::from_yaml(&yaml)?;
        Ok(yaml)
    }

    pub fn correction_draft(&self, canonical: &str, variant: &str) -> Result<CorrectionDraft, CommandError> {
        let current = self.user_rules_yaml()?;
        let mut pack = RulePack::from_yaml(&current)?;
        let outcome = pack.add_correction(canonical, variant).map_err(|_| {
            CommandError::new("invalid_input", "the correction is empty, unchanged or conflicts with an existing rule")
        })?;
        Ok(CorrectionDraft { yaml: Self::render_user_rules(&pack, &current)?, outcome })
    }

    pub fn user_rules_yaml(&self) -> Result<String, CommandError> {
        match std::fs::read_to_string(&self.paths.user_rules) {
            Ok(yaml) => Ok(yaml),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(USER_PACK_TEMPLATE.to_string()),
            Err(err) => Err(CommandError::new("rules", err.to_string())),
        }
    }

    /// Validates and stores `user.yaml`, then recompiles the rules; returns skipped-rule warnings.
    pub fn save_user_rules(&self, yaml: &str) -> Result<Vec<RuleWarning>, CommandError> {
        // Async commands may save rules and config concurrently; serialize their settings rebuilds.
        let _guard = self.config_lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.save_user_rules_locked(yaml)
    }

    /// Caller holds config_lock across the complete read-modify-write operation.
    pub(super) fn save_user_rules_locked(&self, yaml: &str) -> Result<Vec<RuleWarning>, CommandError> {
        let pack = RulePack::from_yaml(yaml)?;
        if let Some(dir) = self.paths.user_rules.parent() {
            std::fs::create_dir_all(dir).map_err(|e| CommandError::new("rules", e.to_string()))?;
        }
        let tmp = self.paths.user_rules.with_extension("yaml.tmp");
        std::fs::write(&tmp, yaml)
            .and_then(|()| std::fs::rename(&tmp, &self.paths.user_rules))
            .map_err(|e| CommandError::new("rules", e.to_string()))?;
        *self.user_pack.lock().unwrap_or_else(PoisonError::into_inner) = Some(pack);
        info!("user rules saved");
        Ok(self.rebuild(self.config()))
    }

    pub fn learning_apply(&self, items: &[LearningItem]) -> Result<Vec<RuleWarning>, CommandError> {
        Self::validate_learning_count(items.len())?;
        let _guard = self.config_lock.lock().unwrap_or_else(PoisonError::into_inner);
        let safe_rules = || CommandError::new("rules", "could not read or save personal rules");
        let current = self.user_rules_yaml().map_err(|_| safe_rules())?;
        let mut pack = RulePack::from_yaml(&current).map_err(|_| safe_rules())?;
        let mut pairs = Vec::new();
        for item in items {
            let pair = item.change.normalized().map_err(Self::learning_error)?;
            let result = match item.target {
                LearningTarget::Rule => pack.add_correction(&pair.corrected, &pair.heard),
                LearningTarget::Term => pack.add_term(&pair.corrected),
            };
            result.map_err(|_| CommandError::new("invalid_input", "the correction conflicts with an existing rule"))?;
            pairs.push(pair);
        }
        let path = self.paths.root.join("learning.json");
        let mut dismissed = Dismissals::load(&path).map_err(Self::learning_error)?;
        // Remember approved pairs too: a keyword-only choice must not be offered again.
        dismissed.dismiss(&pairs).map_err(Self::learning_error)?;
        let yaml = Self::render_user_rules(&pack, &current).map_err(|_| safe_rules())?;
        let warnings = self.save_user_rules_locked(&yaml).map_err(|_| safe_rules())?;
        dismissed.save(&path).map_err(|_| {
            CommandError::new(
                "learning",
                "personal rules were saved, but learning preferences could not be saved; retry to remember this choice",
            )
        })?;
        Ok(warnings)
    }

    pub fn learning_dismiss(&self, items: &[TermChange]) -> Result<(), CommandError> {
        Self::validate_learning_count(items.len())?;
        let _guard = self.config_lock.lock().unwrap_or_else(PoisonError::into_inner);
        let path = self.paths.root.join("learning.json");
        let mut dismissed = Dismissals::load(&path).map_err(Self::learning_error)?;
        dismissed.dismiss(items).map_err(Self::learning_error)?;
        dismissed.save(&path).map_err(Self::learning_error)
    }

    fn validate_learning_count(len: usize) -> Result<(), CommandError> {
        if !(1..=MAX_SUGGESTIONS).contains(&len) {
            return Err(CommandError::new("invalid_input", "select between one and eight learning suggestions"));
        }
        Ok(())
    }

    pub(super) fn learning_error(error: opit_core::learning::LearningError) -> CommandError {
        use opit_core::learning::LearningError;
        let code = match error {
            LearningError::InvalidPair => "invalid_input",
            _ => "learning",
        };
        CommandError::new(code, error.to_string())
    }

    /// Runs `text` through the rules, optionally with an unsaved `user.yaml` draft.
    pub fn rules_preview(&self, text: &str, draft_yaml: Option<&str>) -> Result<RulesPreview, CommandError> {
        let current = self.settings.current();
        let (rules, warnings) = match draft_yaml {
            None => (current.rules.clone(), Vec::new()),
            Some(yaml) => {
                let draft = RulePack::from_yaml(yaml)?;
                let (rules, warnings) = RuleSet::compile(&[draft]);
                (Arc::new(rules), warnings)
            }
        };
        let hallucination = rules.is_hallucination(text.trim());
        let (mut text, mut hits) = rules.apply_traced(text.trim());
        if current.config.rules.numbers_as_words {
            let language = current.config.active_profile().map_or("tr", |p| p.language.as_str());
            let (converted, number_hits) = opit_core::rules::numbers::spell_integers(&text, language);
            text = converted;
            hits.extend(number_hits);
        }
        Ok(RulesPreview { text, hits, warnings, hallucination })
    }

    /// The Whisper prompt the active profile would send right now.
    pub fn prompt_budget(&self) -> BuiltPrompt {
        let current = self.settings.current();
        let language = current.config.active_profile().map_or("tr", |p| p.language.as_str());
        build_prompt(&current.config.rules.prompt_context, current.rules.terms(), language)
    }
}
