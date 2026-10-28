import { describe, expect, it } from "vitest";
import { cleanupPreset, cleanupValid } from "./cleanup";
import type { CleanupConfig } from "./types";

const config: CleanupConfig = {
  enabled: false, base_url: "http://127.0.0.1:8080/v1", model: "my-model", api_key_ref: null,
  prompt_override: "", timeout_seconds: 15, reasoning_effort: null,
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
});
