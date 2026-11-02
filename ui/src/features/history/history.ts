import type { Dictation } from "../../lib/bridge/types";

/** Rows per History page ("Load more"). */
export const HISTORY_PAGE = 50;

/**
 * Rows a live refresh must re-read. A refresh makes an in-flight "Load more" response stale (it
 * is discarded), so the refresh also covers the page that request would have added.
 */
export function refreshWindow(visibleCount: number, loadingMore: boolean): number {
  return visibleCount + (loadingMore ? HISTORY_PAGE : 0);
}

/** Identifies the list a request was made for: the search generation and the latest list load. */
export interface ListToken {
  epoch: number;
  request: number;
}

/** A "Load more" response, or its failure, applies only to the list it was requested for. */
export function sameList(started: ListToken, current: ListToken): boolean {
  return started.epoch === current.epoch && started.request === current.request;
}

/** Reload the visible window from storage so automatic pruning removes stale rows.
 * Read in pages to respect the command's limit, even after many Load more clicks.
 */
export async function refreshHistory(
  recent: (limit: number, before: number | null) => Promise<Dictation[]>,
  visibleCount: number,
): Promise<{ rows: Dictation[]; hasMore: boolean }> {
  const rows: Dictation[] = [];
  const target = Math.max(HISTORY_PAGE, visibleCount);
  let hasMore = true;
  while (rows.length < target && hasMore) {
    const limit = Math.min(HISTORY_PAGE, target - rows.length);
    const page = await recent(limit, rows.at(-1)?.id ?? null);
    rows.push(...page);
    hasMore = page.length === limit;
  }
  return { rows, hasMore };
}
