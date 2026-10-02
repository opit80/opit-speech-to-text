export type UsagePeriod = "today" | "week" | "all";

/** Local calendar bounds include today and exclude tomorrow, even across DST. */
export function usageBounds(period: UsagePeriod, now = new Date()): { sinceMs: number | null; untilMs: number } {
  const end = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate() - (period === "week" ? 6 : 0));
  return { sinceMs: period === "all" ? null : start.getTime(), untilMs: end.getTime() };
}
