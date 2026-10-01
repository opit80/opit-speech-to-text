// Letters and digits (any script) are word characters; apostrophes end a word,
// so "GitHub'a" selects "GitHub" without its Turkish suffix.
const WORD = /[\p{L}\p{N}]/u;

export function expandToWords(text: string, start: number, end: number): { start: number; end: number } {
  let s = Math.max(0, Math.min(start, end));
  let e = Math.min(text.length, Math.max(start, end));
  while (s < e && !WORD.test(text[s])) s++;
  while (e > s && !WORD.test(text[e - 1])) e--;
  if (s === e) return { start: s, end: s };
  while (s > 0 && WORD.test(text[s - 1])) s--;
  while (e < text.length && WORD.test(text[e])) e++;
  return { start: s, end: e };
}
