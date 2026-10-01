import { PROMPT_TOKEN_BUDGET } from "./types";

/** True when a `#` comment appears after the leading comment block (lost on a table edit). */
export function hasInlineComments(yaml: string): boolean {
  const lines = yaml.replace(/^\uFEFF/, "").split(/\r?\n/);
  let i = 0;
  while (i < lines.length && (lines[i].trim() === "" || lines[i].trim().startsWith("#"))) i++;
  return lines.slice(i).some((line) => commentStart(line) >= 0);
}

/** Index of a `#` that starts a YAML comment (outside quotes, at line start or after a space). */
function commentStart(line: string): number {
  let quote: string | null = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote) {
      if (c === quote) quote = null;
    } else if (c === "'" || c === '"') {
      quote = c;
    } else if (c === "#" && (i === 0 || /\s/.test(line[i - 1]))) {
      return i;
    }
  }
  return -1;
}

export function budgetLevel(tokens: number): "ok" | "warning" | "full" {
  if (tokens >= PROMPT_TOKEN_BUDGET) return "full";
  return tokens >= PROMPT_TOKEN_BUDGET * 0.75 ? "warning" : "ok";
}
