import { hasKey, translate } from "./i18n";
import type { CommandError, Lang } from "./types";

export function isCommandError(value: unknown): value is CommandError {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return typeof v.code === "string" && typeof v.message === "string";
}

/** One line of localized text for anything a command can reject with. */
export function describeError(error: unknown, lang: Lang): string {
  if (isCommandError(error)) {
    const kindKey = `error.kind.${error.kind}`;
    if (error.kind && hasKey(kindKey)) return translate(lang, kindKey);
    const codeKey = `error.code.${error.code}`;
    if (hasKey(codeKey)) return translate(lang, codeKey, { message: error.message });
    return translate(lang, "error.unexpected", { message: error.message });
  }
  const message = error instanceof Error ? error.message : String(error);
  return translate(lang, "error.unexpected", { message });
}
