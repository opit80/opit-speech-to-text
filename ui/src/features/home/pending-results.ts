import type { PendingResults } from "../../lib/bridge/types";

/** Command reads and change events may finish out of order. Native revisions own the order. */
export function mergePendingResults(current: PendingResults, next: PendingResults): PendingResults {
  return next.revision >= current.revision ? next : current;
}
