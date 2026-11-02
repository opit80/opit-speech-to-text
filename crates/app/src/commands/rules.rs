use opit_core::learning::{LearningItem, TermChange};
use opit_core::rules::prompt::BuiltPrompt;
use opit_core::rules::{RulePack, RuleWarning};
use tauri::AppHandle;

use crate::app_core::{AppCore, CorrectionDraft, RulesPreview};
use crate::events;

use super::{Core, Result, blocking};

#[tauri::command]
pub(super) async fn get_user_rules(core: Core<'_>) -> Result<String> {
    blocking(core.inner().clone(), AppCore::user_rules_yaml).await
}

#[tauri::command]
pub(super) async fn save_user_rules(app: AppHandle, core: Core<'_>, yaml: String) -> Result<Vec<RuleWarning>> {
    let warnings = blocking(core.inner().clone(), move |core| core.save_user_rules(&yaml)).await?;
    events::emit(&app, events::RULES_CHANGED, ());
    Ok(warnings)
}

#[tauri::command]
pub(super) async fn rules_preview(core: Core<'_>, text: String, draft_yaml: Option<String>) -> Result<RulesPreview> {
    blocking(core.inner().clone(), move |core| core.rules_preview(&text, draft_yaml.as_deref())).await
}

#[tauri::command]
pub(super) fn prompt_budget(core: Core<'_>) -> BuiltPrompt {
    core.prompt_budget()
}

#[tauri::command]
pub(super) fn parse_user_rules(yaml: String) -> Result<RulePack> {
    AppCore::parse_user_rules(&yaml)
}

#[tauri::command]
pub(super) fn render_user_rules(pack: RulePack, previous_yaml: String) -> Result<String> {
    AppCore::render_user_rules(&pack, &previous_yaml)
}

#[tauri::command]
pub(super) async fn correction_draft(core: Core<'_>, canonical: String, variant: String) -> Result<CorrectionDraft> {
    blocking(core.inner().clone(), move |core| core.correction_draft(&canonical, &variant)).await
}

#[tauri::command]
pub(super) async fn learning_apply(
    app: AppHandle,
    core: Core<'_>,
    items: Vec<LearningItem>,
) -> Result<Vec<RuleWarning>> {
    let warnings = blocking(core.inner().clone(), move |core| core.learning_apply(&items)).await;
    // A preference-file write can fail after user.yaml was saved. Always refresh
    // observers so they can inspect the actual saved state, including after retry.
    events::emit(&app, events::RULES_CHANGED, ());
    warnings
}

#[tauri::command]
pub(super) async fn learning_dismiss(core: Core<'_>, items: Vec<TermChange>) -> Result<()> {
    blocking(core.inner().clone(), move |core| core.learning_dismiss(&items)).await
}
