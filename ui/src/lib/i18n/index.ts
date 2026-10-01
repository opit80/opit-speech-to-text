import type { Lang } from "../types";
import { en, type MessageKey } from "./en";
import { tr } from "./tr";

export type { MessageKey };
export type Params = Record<string, string | number>;

const tables: Record<Lang, Record<MessageKey, string>> = { en, tr };

/** Same rule as `i18n::resolve` in crates/app: tr* → Turkish, anything else → English. */
export function resolveLang(uiLanguage: string | null | undefined, systemLocale: string | null | undefined): Lang {
  const pick = uiLanguage ?? systemLocale ?? "en";
  return pick.toLowerCase().startsWith("tr") ? "tr" : "en";
}

export function translate(lang: Lang, key: MessageKey, params?: Params): string {
  const template = tables[lang][key];
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name: string) => (name in params ? String(params[name]) : whole));
}

export function placeholders(template: string): string[] {
  return [...template.matchAll(/\{(\w+)\}/g)].map((m) => m[1]);
}

export function hasKey(key: string): key is MessageKey {
  return Object.hasOwn(en, key);
}
