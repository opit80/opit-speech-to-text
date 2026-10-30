// Cleanup styles: pure helpers mirroring crates/core/src/cleanup/styles.rs (docs/reference/cleanup-styles.md).
import type { BuiltinStyleId, CleanupConfig, CleanupStyle, Lang, StyleDefaults } from "./types";

export const BUILTIN_STYLE_IDS: readonly BuiltinStyleId[] = ["raw", "clean", "formal", "casual", "prompt"];
const ID_CHARS = 8;

export function isBuiltinStyle(id: string): id is BuiltinStyleId {
  return (BUILTIN_STYLE_IDS as readonly string[]).includes(id);
}

/** `custom-` + 8 random base-36 characters, not in `taken` (valid for Rust: ASCII, ≤ 48 chars). */
export function newStyleId(taken: Iterable<string>, random: () => number = Math.random): string {
  const used = new Set(taken);
  for (;;) {
    const suffix = Array.from({ length: ID_CHARS }, () => Math.floor(random() * 36).toString(36)).join("");
    const id = `custom-${suffix}`;
    if (!used.has(id)) return id;
  }
}

export function newStyle(id: string): CleanupStyle {
  return { id, name: "", builtin: false, prompt: "", model: null, in_cycle: true, hotkey: [] };
}

function defaultsFor(id: string, defaults: StyleDefaults[] | null): StyleDefaults | undefined {
  return defaults?.find((d) => d.id === id);
}

/** Like the tray: an unchanged (English) built-in name is localized; any other name is shown verbatim. */
export function styleDisplayName(style: Pick<CleanupStyle, "id" | "name">, defaults: StyleDefaults[] | null, lang: Lang): string {
  const entry = defaultsFor(style.id, defaults);
  return entry && style.name === entry.names.en ? entry.names[lang] : style.name;
}

/** The name to store: an untouched (localized) or blank built-in name goes back to the English default. */
export function storedStyleName(input: string, id: string, defaults: StyleDefaults[] | null, lang: Lang): string {
  const name = input.trim();
  const entry = defaultsFor(id, defaults);
  if (entry && (name === "" || name === entry.names[lang])) return entry.names.en;
  return name;
}

/** The name History shows next to the cleanup marker; null for clean, no style or a deleted style. */
export function historyStyleName(
  id: string | null, styles: CleanupStyle[], defaults: StyleDefaults[] | null, lang: Lang,
): string | null {
  if (id === null || id === "clean") return null;
  const style = styles.find((s) => s.id === id);
  if (style) return styleDisplayName(style, defaults, lang);
  return defaultsFor(id, defaults)?.names[lang] ?? null;
}

/** What an empty prompt means: the built-in default, the clean default for user styles, none for raw. */
export function defaultStylePrompt(id: string, defaults: StyleDefaults[] | null, lang: Lang): string {
  return (defaultsFor(id, defaults) ?? defaultsFor("clean", defaults))?.prompts[lang] ?? "";
}

export type StyleProblem = "name" | "prompt";

/** User styles need their own name and prompt; built-ins fall back to their defaults. */
export function styleProblem(style: CleanupStyle): StyleProblem | null {
  if (isBuiltinStyle(style.id)) return null;
  if (!style.name.trim()) return "name";
  if (!style.prompt.trim()) return "prompt";
  return null;
}

/** A form draft as it is saved: stored name, blank prompt/model as default, `builtin` from the id. */
export function prepareStyle(style: CleanupStyle, defaults: StyleDefaults[] | null, lang: Lang): CleanupStyle {
  return {
    ...style,
    name: storedStyleName(style.name, style.id, defaults, lang),
    builtin: isBuiltinStyle(style.id),
    prompt: style.prompt.trim() ? style.prompt : "",
    model: style.model?.trim() || null,
    hotkey: [...style.hotkey],
  };
}

/** Replaces the style with the same id in place, or appends a new one. */
export function upsertStyle(styles: CleanupStyle[], style: CleanupStyle): CleanupStyle[] {
  return styles.some((s) => s.id === style.id)
    ? styles.map((s) => (s.id === style.id ? style : s))
    : [...styles, style];
}

type StyleList = Pick<CleanupConfig, "styles" | "active_style_id">;

/** Removes a user style (built-ins stay); deleting the active style makes clean active. */
export function deleteStyle(cleanup: StyleList, id: string): StyleList {
  if (isBuiltinStyle(id)) return { styles: cleanup.styles, active_style_id: cleanup.active_style_id };
  return {
    styles: cleanup.styles.filter((s) => s.id !== id),
    active_style_id: cleanup.active_style_id === id ? "clean" : cleanup.active_style_id,
  };
}
