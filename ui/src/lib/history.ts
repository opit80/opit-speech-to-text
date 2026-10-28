import type { Dictation } from "./types";

/** Reload the visible window from storage so automatic pruning removes stale rows.
 * Read in pages to respect the command's limit, even after many Load more clicks.
 */
export async function refreshHistory(
  recent: (limit: number, before: number | null) => Promise<Dictation[]>,
  visibleCount: number,
): Promise<{ rows: Dictation[]; hasMore: boolean }> {
  const rows: Dictation[] = [];
  const target = Math.max(50, visibleCount);
  let hasMore = true;
  while (rows.length < target && hasMore) {
    const limit = Math.min(50, target - rows.length);
    const page = await recent(limit, rows.at(-1)?.id ?? null);
    rows.push(...page);
    hasMore = page.length === limit;
  }
  return { rows, hasMore };
}
