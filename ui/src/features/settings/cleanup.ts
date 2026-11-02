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

/** Hosted presets need only a key; saved connection overrides must remain editable. */
export function cleanupShowsConnection(config: CleanupConfig, presets: CleanupPreset[]): boolean {
  const id = cleanupPreset(config, presets);
  const preset = presets.find((p) => p.id === id);
  return id === "custom" || id === "local" || !preset || config.api_key_ref !== preset.config.api_key_ref;
}

/** Picking a service changes its identity, preserving hidden tuning and style values. */
export function withCleanupPreset(saved: CleanupConfig, preset: CleanupConfig): CleanupConfig {
  return { ...saved, base_url: preset.base_url, model: preset.model, api_key_ref: preset.api_key_ref };
}

/** Saving a key or key-free connection authorizes verification; turning cleanup off does not. */
export function cleanupSaveNeedsTest(config: CleanupConfig, keySaved: boolean, wasEnabled: boolean): boolean {
  return keySaved || (config.api_key_ref === null && !(wasEnabled && !config.enabled));
}

/** A raw-style result never contacted the provider, so it cannot establish a working key. */
export function cleanupEnabledAfterTest(
  enabled: boolean, credentialSaved: boolean, outcome: "passed" | "failed" | "skipped",
): boolean {
  return enabled || (credentialSaved && outcome === "passed");
}

/** A response for an older connection must not enable a newer connection saved elsewhere. */
export function sameCleanupConnection(a: CleanupConfig, b: CleanupConfig): boolean {
  return a.base_url === b.base_url && a.model === b.model && a.api_key_ref === b.api_key_ref
    && a.timeout_seconds === b.timeout_seconds && a.reasoning_effort === b.reasoning_effort;
}
