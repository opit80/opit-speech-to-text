import type { Lang } from "./types";
import { translate } from "./i18n";

const LOCALES: Record<Lang, string> = { en: "en-US", tr: "tr-TR" };

function sameDay(a: Date, b: Date): boolean {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

/** Today → "14:05"; this year → "12 Eyl 14:05"; older → with the year. */
export function formatDateTime(ms: number, lang: Lang, now: number = Date.now()): string {
  const date = new Date(ms);
  const today = new Date(now);
  const time: Intl.DateTimeFormatOptions = { hour: "2-digit", minute: "2-digit", hour12: false };
  if (sameDay(date, today)) return new Intl.DateTimeFormat(LOCALES[lang], time).format(date);
  const options: Intl.DateTimeFormatOptions = { ...time, day: "numeric", month: "short" };
  if (date.getFullYear() !== today.getFullYear()) options.year = "numeric";
  return new Intl.DateTimeFormat(LOCALES[lang], options).format(date);
}

/** 900 → "0,9 sn" (tr) / "0.9 s" (en). */
export function formatSeconds(ms: number, lang: Lang): string {
  const n = new Intl.NumberFormat(LOCALES[lang], { maximumFractionDigits: 1 }).format(ms / 1000);
  return `${n} ${translate(lang, "common.seconds")}`;
}

/** 125400 → "2:05". */
export function formatClock(ms: number): string {
  const total = Math.round(ms / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

export function formatNumber(n: number, lang: Lang): string {
  return new Intl.NumberFormat(LOCALES[lang]).format(n);
}
