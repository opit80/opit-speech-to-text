import type { Profile } from "./types";

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
