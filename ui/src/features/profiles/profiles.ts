import type { MessageKey } from "../../lib/i18n";
import type { AudioFormat, Profile, ProviderKind } from "../../lib/bridge/types";

/** How each provider kind is named in the profile form. */
export const PROVIDER_KEYS: Record<ProviderKind, MessageKey> = {
  openai_compatible: "profiles.provider.openai",
  elevenlabs: "profiles.provider.elevenlabs",
  local_whisper: "profiles.preset.local",
  deepgram: "profiles.provider.deepgram",
};

/** Providers that can show live text while recording (mirrors `streaming_supported`, for unsaved drafts). */
export function supportsLiveText(provider: ProviderKind): boolean {
  return provider === "elevenlabs" || provider === "deepgram";
}

/** Whether the upload format is chosen by the provider adapter rather than the user. */
export function fixedAudioFormat(provider: ProviderKind): boolean {
  return provider !== "openai_compatible";
}

/** The audio format Rust stores for a profile: Deepgram always uses WAV. */
export function storedAudioFormat(provider: ProviderKind, format: AudioFormat): AudioFormat {
  return provider === "deepgram" ? "wav" : format;
}

const TURKISH: Record<string, string> = { ç: "c", ğ: "g", ı: "i", i̇: "i", ö: "o", ş: "s", ü: "u" };

/** Lowercase ASCII id: Turkish letters folded, anything else → "-". Also used as the key name. */
export function slugify(name: string): string {
  const folded = name
    .toLocaleLowerCase("tr-TR")
    .replace(/i̇|[çğıöşü]/g, (c) => TURKISH[c] ?? c)
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "");
  const slug = folded.replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  return slug || "custom";
}

export function uniqueProfileId(name: string, existingIds: string[]): string {
  const base = slugify(name);
  if (!existingIds.includes(base)) return base;
  for (let n = 2; ; n++) if (!existingIds.includes(`${base}-${n}`)) return `${base}-${n}`;
}

/**
 * The profile a Groq/OpenAI choice stands for: an existing profile with that id is kept as it is
 * (language, fallback, key name …); the preset is used only when there is none.
 */
export function presetProfile(id: string, presets: Profile[], existing: Profile[]): Profile | null {
  return existing.find((p) => p.id === id) ?? presets.find((p) => p.id === id) ?? null;
}

export type ProfileProblem = "name" | "base_url" | "model" | "fallback_self" | "fallback_missing";

function validUrl(url: string): boolean {
  try {
    const parsed = new URL(url.trim());
    return (parsed.protocol === "http:" || parsed.protocol === "https:") && parsed.host !== "";
  } catch {
    return false;
  }
}

export function profileProblems(profile: Profile, allProfiles: Profile[]): ProfileProblem[] {
  const problems: ProfileProblem[] = [];
  if (!profile.name.trim()) problems.push("name");
  if (!validUrl(profile.base_url)) problems.push("base_url");
  if (!profile.model.trim()) problems.push("model");
  const fallback = profile.fallback_profile_id;
  if (fallback === profile.id) problems.push("fallback_self");
  else if (fallback && !allProfiles.some((p) => p.id === fallback)) problems.push("fallback_missing");
  return problems;
}

/** Transcription languages offered in the profile form (ISO-639-1, plus "auto"). */
export const LANGUAGES = ["tr", "en", "de", "fr", "es", "it", "ru", "ar", "auto"] as const;
