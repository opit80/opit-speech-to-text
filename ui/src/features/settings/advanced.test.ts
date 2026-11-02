import { describe, expect, it } from "vitest";
import { advancedShouldOpen } from "./advanced";

describe("advanced settings disclosure", () => {
  it("starts closed and ignores errors in the primary sections", () => {
    expect(advancedShouldOpen({}, true, null)).toBe(false);
    for (const field of ["language", "hotkey_keys", "mode", "mic", "media_mode", "retention", "update_check"]) {
      expect(advancedShouldOpen({ [field]: "failed" }, true, null, true)).toBe(false);
    }
  });
  it("reveals every advanced field error", () => {
    for (const field of ["pause_keys", "style_cycle_keys", "edit_keys", "translate_keys", "translate_language",
      "translate_languages", "translate_model", "hotkey_enabled", "max_seconds", "live_text", "duck_volume",
      "paste_after", "restore_clipboard", "trailing_space", "overlay", "max_entries", "learning",
      "copy_data", "copy_log", "copy_cli", "speech_reset"]) {
      expect(advancedShouldOpen({ [field]: "failed" }, true, null)).toBe(true);
      expect(advancedShouldOpen({ [field]: null }, true, null)).toBe(false);
    }
  });
  it("opens for media-mode errors only when the control is in Advanced", () => {
    expect(advancedShouldOpen({ media_mode: "failed" }, true, null)).toBe(true);
    expect(advancedShouldOpen({ media_mode: "failed" }, true, null, true)).toBe(false);
  });
  it("reveals disabled shortcuts and registration problems, including conflicts", () => {
    expect(advancedShouldOpen({}, false, null)).toBe(true);
    expect(advancedShouldOpen({}, true, "shortcut conflict")).toBe(true);
  });
});
