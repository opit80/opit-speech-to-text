/** Range of `media.duck_volume_percent` (Rust normalizes to the same bounds). */
export const duckPercentRange = { min: 0, max: 100, step: 1 } as const;

/**
 * Turns the text of a number input into the value to save: clamped to `min`–`max`, whole for a
 * step of 1 or more, two decimals otherwise. Null when the text is empty or not a finite number.
 */
export function parseNumberInput(text: string, min: number, max: number, step: number): number | null {
  if (text.trim() === "") return null;
  const n = Number(text);
  if (!Number.isFinite(n)) return null;
  const rounded = step < 1 ? Number(n.toFixed(2)) : Math.round(n);
  return Math.min(max, Math.max(min, rounded));
}
