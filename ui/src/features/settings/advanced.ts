const ADVANCED_FIELDS = new Set([
  "pause_keys", "style_cycle_keys", "edit_keys", "translate_keys", "translate_language", "translate_languages",
  "translate_model", "hotkey_enabled", "max_seconds", "live_text", "duck_volume", "paste_after",
  "restore_clipboard", "trailing_space", "overlay", "max_entries", "learning", "copy_data", "copy_log", "copy_cli", "speech_reset",
]);

/** Nested component errors also reveal their ancestor disclosures via revealFieldErrors. */
export function advancedShouldOpen(errors: Record<string, string | null>, hotkeyEnabled: boolean, hotkeyError: string | null, linux = false): boolean {
  return !hotkeyEnabled || !!hotkeyError || (!linux && !!errors.media_mode) || Object.entries(errors).some(([field, error]) => ADVANCED_FIELDS.has(field) && !!error);
}
