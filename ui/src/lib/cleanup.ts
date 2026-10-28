import type { CleanupConfig, CleanupPreset } from "./types";

/** Model IDs stay free-form, including unknown or locally installed models. */
export function cleanupValid(config: CleanupConfig): boolean {
  try {
    const url = new URL(config.base_url.trim());
    return ["http:", "https:"].includes(url.protocol) && !url.username && !url.password && !url.search && !url.hash
      && !!config.model.trim() && Number.isInteger(config.timeout_seconds)
      && config.timeout_seconds >= 1 && config.timeout_seconds <= 120;
  } catch { return false; }
}

export function cleanupPreset(config: CleanupConfig, presets: CleanupPreset[]): string {
  return presets.find((p) => p.config.base_url === config.base_url && p.config.model === config.model)?.id ?? "custom";
}
