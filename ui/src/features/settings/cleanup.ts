import type { CleanupConfig, CleanupPreset } from "../../lib/bridge/types";

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

/**
 * The connection form owns only the shared connection fields; styles, the active style and the
 * legacy prompt always come from the live config, so saving or testing the form never reverts a
 * style edit made since the form was opened.
 */
export function withLiveStyles(connection: CleanupConfig, live: CleanupConfig): CleanupConfig {
  return {
    ...connection,
    prompt_override: live.prompt_override,
    active_style_id: live.active_style_id,
    styles: structuredClone(live.styles),
  };
}
