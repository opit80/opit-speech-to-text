// Voice edit of the selected text and dictate-and-translate (docs/reference/voice-modes.md).
import { cleanupValid } from "../settings/cleanup";
import type { MessageKey } from "../../lib/i18n";
import type { AppConfig, CleanupConfig, Dictation, DictationMode, Lang, TranslateLanguage } from "../../lib/bridge/types";

/** "Translate into" options, labelled in the UI language; the configured code is always selectable. */
export function languageOptions(languages: TranslateLanguage[], lang: Lang, current: string): { value: string; label: string }[] {
  const options = languages.map((l) => ({ value: l.code, label: lang === "tr" ? l.name_tr : l.name_en }));
  if (!options.some((o) => o.value === current)) options.push({ value: current, label: current.toUpperCase() });
  return options;
}

export function isVoiceMode(mode: DictationMode): mode is "edit" | "translate" {
  return mode === "edit" || mode === "translate";
}

/** The History badge of a row's mode; null for ordinary dictations and Ask (which has its own label). */
export function modeBadgeKey(mode: DictationMode): MessageKey | null {
  return mode === "edit" ? "history.mode_edit" : mode === "translate" ? "history.mode_translate" : null;
}

/** Rust rejects retrying edit and translate rows; the safe redo is recording the instruction again. */
export function canRetryRow(row: Dictation): boolean {
  return row.status === "failed" && !isVoiceMode(row.mode);
}

/** What "Show original" reveals: the selection or source transcript, and an edit's instruction. */
export function originalOf(row: Dictation): { source: string | null; instruction: string | null } | null {
  if (!isVoiceMode(row.mode)) return null;
  const source = row.source_text || null;
  const instruction = row.mode === "edit" ? row.instruction || null : null;
  return source || instruction ? { source, instruction } : null;
}

/** Edit and translate need a valid cleanup connection and, unless it is keyless, a stored key. */
export function cleanupConnectionReady(cleanup: CleanupConfig, keyStored: boolean): boolean {
  return cleanupValid(cleanup) && (cleanup.api_key_ref === null || keyStored);
}

/** Either voice mode is set up, so the AI cleanup connection must be ready. */
export function voiceModesInUse(config: AppConfig): boolean {
  return config.hotkey.edit_keys.length > 0 || config.hotkey.translate_keys.length > 0;
}
