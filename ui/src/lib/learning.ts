// Learning dictionary: History edits that offer personal rule/term additions (docs/learning-dictionary.md).
import type { Dictation, LearningItem, LearningTarget, TermChange } from "./types";

/** Must match `MAX_HISTORY_TEXT_BYTES` in crates/app (UTF-8 bytes, measured before trimming). */
export const MAX_HISTORY_TEXT_BYTES = 100_000;

/** A per-pair choice in the offer dialog. */
export type LearningChoice = LearningTarget | "skip";

/** Same folding as `opit_core::text::fold`: I/ı/İ/i → i, everything else to its first lowercase char. */
export function foldText(text: string): string {
  let folded = "";
  for (const c of text) {
    folded += c === "I" || c === "ı" || c === "İ" || c === "i" ? "i" : ([...c.toLowerCase()][0] ?? c);
  }
  return folded;
}

function collapse(text: string): string {
  return text.trim().split(/\s+/u).join(" ");
}

/** True when the two spellings differ only in letter case (and spacing). */
export function caseOnly(pair: TermChange): boolean {
  return foldText(collapse(pair.heard)) === foldText(collapse(pair.corrected));
}

/** A casing fix is a term (it fixes the casing everywhere); a different word is a correction rule. */
export function defaultChoice(pair: TermChange): LearningTarget {
  return caseOnly(pair) ? "term" : "rule";
}

export function initialChoices(pairs: TermChange[]): LearningChoice[] {
  return pairs.map(defaultChoice);
}

/** What "Add selected" sends: the chosen pairs to `learning_apply`, the skipped ones to `learning_dismiss`. */
export function splitChoices(pairs: TermChange[], choices: LearningChoice[]): { apply: LearningItem[]; dismiss: TermChange[] } {
  const apply: LearningItem[] = [];
  const dismiss: TermChange[] = [];
  pairs.forEach(({ heard, corrected }, index) => {
    const choice = choices[index] ?? "skip";
    if (choice === "skip") dismiss.push({ heard, corrected });
    else apply.push({ heard, corrected, target: choice });
  });
  return { apply, dismiss };
}

export type EditIssue = "empty" | "nul" | "too_long";

/** Mirrors `history_update_text` validation; the text is saved exactly as typed. */
export function editIssue(text: string): EditIssue | null {
  if (!text.trim()) return "empty";
  if (text.includes("\0")) return "nul";
  if (new TextEncoder().encode(text).length > MAX_HISTORY_TEXT_BYTES) return "too_long";
  return null;
}

/** Rust edits only successful rows; failed, empty or filtered rows cannot become successful ones. */
export function canEditRow(row: Dictation): boolean {
  return row.status === "ok";
}

/** Only plain dictations teach the dictionary; answers, voice edits and translations are not what was said. */
export function canLearnFrom(row: Dictation): boolean {
  return row.mode === "dictation";
}

/** Height of the History edit box: roughly the wrapped line count, kept between 3 and 14 rows. */
export function editorRows(text: string): number {
  const lines = text.split("\n").reduce((count, line) => count + Math.max(1, Math.ceil([...line].length / 70)), 0);
  return Math.min(14, Math.max(3, lines));
}
