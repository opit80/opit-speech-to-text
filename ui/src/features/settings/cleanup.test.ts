import { describe, expect, it } from "vitest";
import { cleanupEnabledAfterTest, cleanupSaveNeedsTest, sameCleanupConnection, cleanupPreset, cleanupShowsConnection, cleanupValid, withCleanupPreset, withLiveStyles } from "./cleanup";
import type { CleanupConfig } from "../../lib/bridge/types";

const config: CleanupConfig = {
  enabled: false, base_url: "http://127.0.0.1:8080/v1", model: "my-model", api_key_ref: null,
  prompt_override: "", timeout_seconds: 15, reasoning_effort: null, active_style_id: "clean",
  styles: [{ id: "clean", name: "Clean", builtin: true, prompt: "", model: null, in_cycle: true, hotkey: [] }],
};
describe("cleanup form", () => {
  it("accepts local and unknown models", () => {
    expect(cleanupValid(config)).toBe(true);
    expect(cleanupValid({ ...config, base_url: "https://inference.baseten.co/v1" })).toBe(true);
  });
  it("rejects invalid URLs, credential URLs, missing models and invalid timeouts", () => {
    for (const change of [{ base_url: "file:///tmp/model" }, { base_url: "https://secret@host/v1" },
      { base_url: "https://host/v1?key=secret" }, { model: " " }, { timeout_seconds: 0 },
      { timeout_seconds: 121 }, { timeout_seconds: 1.5 }]) {
      expect(cleanupValid({ ...config, ...change })).toBe(false);
    }
  });
  it("matches endpoint and model, allowing every field to be edited", () => {
    const presets = [{ id: "local", name: "Local", key_url: "", config }];
    expect(cleanupPreset({ ...config, enabled: true }, presets)).toBe("local");
    expect(cleanupPreset({ ...config, model: "another-model" }, presets)).toBe("custom");
  });
  it("takes styles from the live config and connection fields from the form", () => {
    const live: CleanupConfig = { ...config, prompt_override: "", active_style_id: "custom-a", styles: [
      ...config.styles, { id: "custom-a", name: "Notes", builtin: false, prompt: "Write notes.", model: "m", in_cycle: false, hotkey: ["F8"] },
    ] };
    const form: CleanupConfig = { ...config, enabled: true, model: "new-model", prompt_override: "stale" };
    const merged = withLiveStyles(form, live);
    expect(merged).toEqual({ ...form, prompt_override: "", active_style_id: "custom-a", styles: live.styles });
    expect(merged.styles).not.toBe(live.styles);
  });
});

describe("cleanup connection disclosure", () => {
  const presets = ["baseten", "openai", "local", "custom"].map((id) => ({
    id, name: id, key_url: "", config: { ...config, model: id, api_key_ref: id === "local" ? null : id },
  }));
  it("hides unchanged hosted presets and reveals Custom and Local", () => {
    for (const preset of presets) {
      expect(cleanupShowsConnection(preset.config, presets)).toBe(["local", "custom"].includes(preset.id));
    }
  });
  it("reveals unknown endpoints/models and mismatched key references, including no key", () => {
    const hosted = presets[0].config;
    for (const change of [{ base_url: "https://another.example/v1" }, { model: "saved-model" },
      { api_key_ref: "saved-key" }, { api_key_ref: null }]) {
      expect(cleanupShowsConnection({ ...hosted, ...change }, presets)).toBe(true);
    }
  });
  it("reveals saved reasoning without exposing the hosted connection", () => {
    const saved = { ...presets[0].config, reasoning_effort: "low", timeout_seconds: 37 };
    expect(cleanupShowsConnection(saved, presets)).toBe(false);
    expect(cleanupShowsConnection(saved, presets) || !!saved.reasoning_effort).toBe(true);
    expect(withLiveStyles(saved, config)).toMatchObject({ reasoning_effort: "low", timeout_seconds: 37 });
  });
});


it("changing service preserves hidden tuning, enablement and all style settings", () => {
  const saved = { ...config, enabled: true, timeout_seconds: 37, reasoning_effort: "low", prompt_override: "saved prompt" };
  const preset = { ...config, base_url: "https://service.example/v1", model: "preset-model", api_key_ref: "preset-key" };
  const changed = withCleanupPreset(saved, preset);
  expect(changed).toEqual({ ...saved, base_url: preset.base_url, model: preset.model, api_key_ref: preset.api_key_ref });
  expect(saved).toMatchObject({ timeout_seconds: 37, reasoning_effort: "low", model: "my-model" });
  expect(withLiveStyles(changed, saved)).toEqual(changed);
});


describe("cleanup enablement after saving credentials", () => {
  it("enables only after a successful test in a credential-saving flow", () => {
    expect(cleanupEnabledAfterTest(false, true, "passed")).toBe(true);
    expect(cleanupEnabledAfterTest(false, false, "passed")).toBe(false);
    for (const outcome of ["failed", "skipped"] as const) {
      for (const enabled of [false, true]) {
        expect(cleanupEnabledAfterTest(enabled, true, outcome)).toBe(enabled);
        expect(cleanupEnabledAfterTest(enabled, false, outcome)).toBe(enabled);
      }
    }
  });
  it("keeps a manual off choice until the next verified credential save", () => {
    expect(cleanupEnabledAfterTest(true, false, "passed")).toBe(true);
    expect(cleanupEnabledAfterTest(false, false, "skipped")).toBe(false);
    expect(cleanupEnabledAfterTest(false, true, "failed")).toBe(false);
    expect(cleanupEnabledAfterTest(false, true, "passed")).toBe(true);
  });
  it("verifies a new key or key-free connection without retesting stored keys on ordinary saves", () => {
    expect(cleanupSaveNeedsTest({ ...config, api_key_ref: "key" }, true, false)).toBe(true);
    expect(cleanupSaveNeedsTest({ ...config, api_key_ref: "key" }, false, false)).toBe(false);
    expect(cleanupSaveNeedsTest(config, false, false)).toBe(true);
    expect(cleanupSaveNeedsTest({ ...config, enabled: true }, false, true)).toBe(true);
    // The form's off switch is saved by the same button as the connection.
    expect(cleanupSaveNeedsTest(config, false, true)).toBe(false);
  });
  it("matches the verified connection while tolerating live style and enabled changes", () => {
    expect(sameCleanupConnection(config, { ...config, enabled: true, active_style_id: "raw", styles: [] })).toBe(true);
    for (const change of [{ model: "changed" }, { base_url: "https://other.example/v1" },
      { api_key_ref: "new-key" }, { timeout_seconds: 33 }, { reasoning_effort: "low" }]) {
      expect(sameCleanupConnection(config, { ...config, ...change })).toBe(false);
    }
  });
});
