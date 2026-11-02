import { expect, it } from "vitest";
import { usageBounds } from "./usage";

it("uses local midnight and crosses month boundaries for seven calendar days", () => {
  const now = new Date(2026, 9, 1, 15, 30);
  expect(usageBounds("today", now)).toEqual({ sinceMs: new Date(2026, 9, 1).getTime(), untilMs: new Date(2026, 9, 2).getTime() });
  expect(usageBounds("week", now)).toEqual({ sinceMs: new Date(2026, 8, 25).getTime(), untilMs: new Date(2026, 9, 2).getTime() });
  expect(usageBounds("all", now).sinceMs).toBeNull();
});
