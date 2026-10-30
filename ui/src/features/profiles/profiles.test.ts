import { describe, expect, it } from "vitest";
import {
  PROVIDER_KEYS, fixedAudioFormat, presetProfile, profileProblems, slugify, storedAudioFormat, supportsLiveText, uniqueProfileId,
} from "./profiles";
import type { Profile, ProviderKind } from "../../lib/bridge/types";

const base: Profile = {
  provider: "openai_compatible",
  id: "home-gpu", name: "Home GPU", base_url: "http://192.168.1.10:8000/v1", model: "large-v3",
  api_key_ref: "home-gpu", language: "tr", audio_format: "wav", response_format: "json",
  send_prompt: true, send_keywords: false, apply_rules: true, apply_cleanup: true, fallback_profile_id: null,
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

  it("adds a native Scribe preset without changing an existing Scribe account", () => {
    const scribe: Profile = { ...base, provider: "elevenlabs", id: "elevenlabs", name: "ElevenLabs Scribe v2",
      base_url: "https://api.elevenlabs.io/v1", model: "scribe_v2", api_key_ref: "elevenlabs",
      send_prompt: false, send_keywords: false };
    expect(presetProfile("elevenlabs", [...presets, scribe], [base])).toEqual(scribe);
    const mine = { ...scribe, language: "en", api_key_ref: "work-scribe" };
    expect(presetProfile("elevenlabs", [...presets, scribe], [mine])).toEqual(mine);
    expect(uniqueProfileId(scribe.name, ["elevenlabs-scribe-v2"])).toBe("elevenlabs-scribe-v2-2");
  });

  it("adds a native Deepgram preset without changing an existing Deepgram account", () => {
    const deepgram: Profile = { ...base, provider: "deepgram", id: "deepgram", name: "Deepgram",
      base_url: "https://api.deepgram.com/v1", model: "nova-3", api_key_ref: "deepgram",
      send_prompt: false, send_keywords: false };
    expect(presetProfile("deepgram", [...presets, deepgram], [base])).toEqual(deepgram);
    const mine = { ...deepgram, language: "auto", api_key_ref: "work-deepgram" };
    expect(presetProfile("deepgram", [...presets, deepgram], [mine])).toEqual(mine);
  });

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

describe("provider kinds", () => {
  const kinds: ProviderKind[] = ["openai_compatible", "elevenlabs", "local_whisper", "deepgram"];

  it("shows live text only for streaming providers", () => {
    expect(kinds.filter(supportsLiveText)).toEqual(["elevenlabs", "deepgram"]);
  });
  it("names every provider", () => {
    expect(kinds.map((kind) => PROVIDER_KEYS[kind])).toEqual([
      "profiles.provider.openai", "profiles.provider.elevenlabs", "profiles.preset.local", "profiles.provider.deepgram",
    ]);
  });
  it("fixes the upload format where the provider decides it", () => {
    expect(kinds.map(fixedAudioFormat)).toEqual([false, true, true, true]);
  });
  it("sends Deepgram audio as WAV", () => {
    expect(storedAudioFormat("deepgram", "flac")).toBe("wav");
    expect(storedAudioFormat("openai_compatible", "flac")).toBe("flac");
    expect(storedAudioFormat("elevenlabs", "flac")).toBe("flac");
  });
});
