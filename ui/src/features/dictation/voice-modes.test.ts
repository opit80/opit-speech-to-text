import { describe, expect, it } from "vitest";
import type { AppConfig, CleanupConfig, Dictation, TranslateLanguage } from "../../lib/bridge/types";
import {
  canRetryRow, cleanupConnectionReady, isVoiceMode, languageOptions, modeBadgeKey, originalOf, voiceModesInUse,
} from "./voice-modes";

const languages: TranslateLanguage[] = [
  { code: "en", name_en: "English", name_tr: "İngilizce" },
  { code: "tr", name_en: "Turkish", name_tr: "Türkçe" },
  { code: "de", name_en: "German", name_tr: "Almanca" },
];

function row(patch: Partial<Dictation>): Dictation {
  return {
    id: 1, created_at_ms: 0, profile_id: "p", raw_text: "", text: "", status: "ok", audio_ms: 0, latency_ms: 0,
    audio_path: null, error_code: null, language: null, cleanup_status: null, cleanup_model: null, cleanup_style_id: null,
    mode: "dictation", agent: null, source_text: null, instruction: null, ...patch,
  };
}

describe("languageOptions", () => {
  it("labels each language in the UI language, in the given order", () => {
    expect(languageOptions(languages, "tr", "en")).toEqual([
      { value: "en", label: "İngilizce" }, { value: "tr", label: "Türkçe" }, { value: "de", label: "Almanca" },
    ]);
    expect(languageOptions(languages, "en", "de").map((o) => o.label)).toEqual(["English", "Turkish", "German"]);
  });
  it("keeps the configured code selectable while the list is missing or lacks it", () => {
    expect(languageOptions([], "en", "ja")).toEqual([{ value: "ja", label: "JA" }]);
    expect(languageOptions(languages, "en", "ko").at(-1)).toEqual({ value: "ko", label: "KO" });
  });
});

describe("modes", () => {
  it("maps only edit and translate to a badge", () => {
    expect(modeBadgeKey("edit")).toBe("history.mode_edit");
    expect(modeBadgeKey("translate")).toBe("history.mode_translate");
    expect(modeBadgeKey("dictation")).toBeNull();
    expect(modeBadgeKey("ask")).toBeNull();
    expect(isVoiceMode("edit") && isVoiceMode("translate")).toBe(true);
    expect(isVoiceMode("ask") || isVoiceMode("dictation")).toBe(false);
  });
  it("offers Try again only for failed dictation rows", () => {
    expect(canRetryRow(row({ status: "failed" }))).toBe(true);
    expect(canRetryRow(row({ status: "failed", mode: "ask" }))).toBe(true);
    expect(canRetryRow(row({ status: "failed", mode: "edit" }))).toBe(false);
    expect(canRetryRow(row({ status: "failed", mode: "translate" }))).toBe(false);
    expect(canRetryRow(row({ status: "ok" }))).toBe(false);
  });
});

describe("originalOf", () => {
  it("returns the selection and instruction of an edit", () => {
    expect(originalOf(row({ mode: "edit", source_text: "hello", instruction: "make it formal" })))
      .toEqual({ source: "hello", instruction: "make it formal" });
  });
  it("returns only the instruction when an edit wrote new text", () => {
    expect(originalOf(row({ mode: "edit", source_text: null, instruction: "write a greeting" })))
      .toEqual({ source: null, instruction: "write a greeting" });
  });
  it("returns the source transcript of a translation and ignores an instruction", () => {
    expect(originalOf(row({ mode: "translate", source_text: "merhaba", instruction: "x" })))
      .toEqual({ source: "merhaba", instruction: null });
  });
  it("returns null when there is nothing to reveal", () => {
    expect(originalOf(row({ mode: "edit", source_text: "", instruction: null }))).toBeNull();
    expect(originalOf(row({ mode: "translate" }))).toBeNull();
    expect(originalOf(row({ mode: "dictation", source_text: "x" }))).toBeNull();
  });
});

describe("cleanupConnectionReady", () => {
  const cleanup = { base_url: "https://api.example.com/v1", model: "m", timeout_seconds: 15, api_key_ref: "cleanup" } as CleanupConfig;
  it("needs a valid connection and a stored key unless it is keyless", () => {
    expect(cleanupConnectionReady(cleanup, true)).toBe(true);
    expect(cleanupConnectionReady(cleanup, false)).toBe(false);
    expect(cleanupConnectionReady({ ...cleanup, api_key_ref: null }, false)).toBe(true);
    expect(cleanupConnectionReady({ ...cleanup, model: " " }, true)).toBe(false);
    expect(cleanupConnectionReady({ ...cleanup, base_url: "" }, true)).toBe(false);
  });
});

describe("voiceModesInUse", () => {
  it("is true when either shortcut is set", () => {
    const hotkey = (edit: string[], translate: string[]) => ({ hotkey: { edit_keys: edit, translate_keys: translate } }) as unknown as AppConfig;
    expect(voiceModesInUse(hotkey([], []))).toBe(false);
    expect(voiceModesInUse(hotkey(["F18"], []))).toBe(true);
    expect(voiceModesInUse(hotkey([], ["F19"]))).toBe(true);
  });
});
