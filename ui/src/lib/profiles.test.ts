import { describe, expect, it } from "vitest";
import { presetProfile, profileProblems, slugify, uniqueProfileId } from "./profiles";
import type { Profile } from "./types";

const base: Profile = {
  id: "home-gpu", name: "Home GPU", base_url: "http://192.168.1.10:8000/v1", model: "large-v3",
  api_key_ref: "home-gpu", language: "tr", audio_format: "wav", response_format: "json",
  send_prompt: true, send_keywords: false, apply_rules: true, fallback_profile_id: null,
};

describe("profile ids", () => {
  it("slugifies Turkish names", () => {
    expect(slugify("Ev Sunucusu Çalışma")).toBe("ev-sunucusu-calisma");
    expect(slugify("  İzmir  GPU!! ")).toBe("izmir-gpu");
    expect(slugify("???")).toBe("custom");
  });
  it("avoids existing ids", () => {
    expect(uniqueProfileId("Groq", ["groq", "openai"])).toBe("groq-2");
    expect(uniqueProfileId("Groq", ["groq", "groq-2"])).toBe("groq-3");
    expect(uniqueProfileId("Home", ["groq"])).toBe("home");
  });
});

describe("profileProblems", () => {
  it("accepts a complete profile", () => expect(profileProblems(base, [base])).toEqual([]));
  it("lists every problem", () => {
    const bad = { ...base, name: " ", base_url: "ftp://x", model: "", fallback_profile_id: "home-gpu" };
    expect(profileProblems(bad, [bad]).sort()).toEqual(["base_url", "fallback_self", "model", "name"]);
  });
  it("rejects a URL without a host and a fallback that does not exist", () => {
    expect(profileProblems({ ...base, base_url: "https://" }, [base])).toEqual(["base_url"]);
    expect(profileProblems({ ...base, fallback_profile_id: "gone" }, [base])).toEqual(["fallback_missing"]);
  });
});

describe("presetProfile", () => {
  const groqPreset: Profile = {
    ...base, id: "groq", name: "Groq", base_url: "https://api.groq.com/openai/v1", model: "whisper-large-v3",
    api_key_ref: "groq",
  };
  const openaiPreset: Profile = { ...groqPreset, id: "openai", name: "OpenAI", api_key_ref: "openai" };
  const presets = [groqPreset, openaiPreset];

  it("keeps an existing profile with the preset's id unchanged", () => {
    const mine: Profile = {
      ...groqPreset, language: "en", send_prompt: false, fallback_profile_id: "openai", api_key_ref: "work-groq",
    };
    expect(presetProfile("groq", presets, [base, mine])).toEqual(mine);
  });
  it("falls back to the preset when no such profile exists", () => {
    expect(presetProfile("openai", presets, [base])).toEqual(openaiPreset);
  });
  it("returns null when neither exists", () => {
    expect(presetProfile("groq", [], [base])).toBeNull();
  });
});
