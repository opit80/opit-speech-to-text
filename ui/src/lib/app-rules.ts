// Per-app rules: pure helpers mirroring crates/core/src/app_rules.rs (docs/app-rules.md).
import type { MessageKey } from "./i18n";
import type { AppRule, PasteAfter, PasteMethod } from "./types";

const ID_CHARS = 8;

/** Option labels shared with the global Pasting settings. */
export const PASTE_METHOD_KEYS: Record<PasteMethod, MessageKey> = {
  ctrl_v: "settings.paste_method.ctrl_v",
  shift_insert: "settings.paste_method.shift_insert",
  ctrl_shift_v: "settings.paste_method.ctrl_shift_v",
  type: "settings.paste_method.type",
};
export const PASTE_AFTER_KEYS: Record<PasteAfter, MessageKey> = {
  none: "settings.paste_after.none",
  enter: "settings.paste_after.enter",
  ctrl_enter: "settings.paste_after.ctrl_enter",
};

/** `rule-` + 8 random base-36 characters, not in `taken`. */
export function newRuleId(taken: Iterable<string>, random: () => number = Math.random): string {
  const used = new Set(taken);
  for (;;) {
    const suffix = Array.from({ length: ID_CHARS }, () => Math.floor(random() * 36).toString(36)).join("");
    const id = `rule-${suffix}`;
    if (!used.has(id)) return id;
  }
}

export function newRule(id: string): AppRule {
  return { id, enabled: true, exe: "", title_contains: "", style_id: null, profile_id: null, paste_method: null, paste_after: null };
}

/** The shared Turkish-aware case fold Rust stores file names with: I, i, İ and ı all become i. */
export function foldExe(exe: string): string {
  return exe.replace(/[Iİı]/g, "i").toLowerCase();
}

export type ExeProblem = "required" | "invalid";

/** A process file name such as `code.exe`: no path, wildcard, reserved or control character. */
export function exeProblem(exe: string): ExeProblem | null {
  const name = exe.trim();
  if (!name) return "required";
  if (/[\u0000-\u001f\u007f/\\:*?"<>|]/.test(name)) return "invalid";
  if (!/^.+\.exe$/i.test(name)) return "invalid";
  return null;
}

/** A form draft as it is saved: trimmed text, the folded file name, empty choices as null. */
export function prepareRule(rule: AppRule): AppRule {
  return {
    ...rule,
    exe: foldExe(rule.exe.trim()),
    title_contains: rule.title_contains.trim(),
    style_id: rule.style_id?.trim() || null,
    profile_id: rule.profile_id?.trim() || null,
  };
}

/** Moves a rule one place up (-1) or down (+1); unchanged at the ends or for an unknown id. */
export function moveRule(rules: AppRule[], id: string, delta: -1 | 1): AppRule[] {
  const from = rules.findIndex((r) => r.id === id);
  const to = from + delta;
  if (from < 0 || to < 0 || to >= rules.length) return [...rules];
  const next = [...rules];
  [next[from], next[to]] = [next[to]!, next[from]!];
  return next;
}

/** Replaces the rule with the same id in place, or appends a new one. */
export function upsertRule(rules: AppRule[], rule: AppRule): AppRule[] {
  return rules.some((r) => r.id === rule.id) ? rules.map((r) => (r.id === rule.id ? rule : r)) : [...rules, rule];
}

export function deleteRule(rules: AppRule[], id: string): AppRule[] {
  return rules.filter((r) => r.id !== id);
}

export interface SummaryPart {
  key: MessageKey;
  /** The title filter or the raw id/value; the component turns it into a display name. */
  value: string;
}

/** What a rule shows in the list: its title filter and overrides, or that it overrides nothing. */
export function ruleSummary(rule: AppRule): SummaryPart[] {
  const parts: SummaryPart[] = [];
  if (rule.title_contains) parts.push({ key: "app_rules.summary.title", value: rule.title_contains });
  const overrides: SummaryPart[] = [];
  if (rule.style_id !== null) overrides.push({ key: "app_rules.summary.style", value: rule.style_id });
  if (rule.profile_id !== null) overrides.push({ key: "app_rules.summary.profile", value: rule.profile_id });
  if (rule.paste_method !== null) overrides.push({ key: "app_rules.summary.paste_method", value: rule.paste_method });
  if (rule.paste_after !== null) overrides.push({ key: "app_rules.summary.paste_after", value: rule.paste_after });
  return [...parts, ...(overrides.length ? overrides : [{ key: "app_rules.summary.none" as const, value: "" }])];
}
