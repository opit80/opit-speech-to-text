import { describe, expect, it } from "vitest";
import { formatClock, formatDateTime, formatMilliseconds, formatSeconds } from "./format";

describe("format", () => {
  const now = new Date(2026, 9, 1, 18, 0).getTime();
  it("shows only the time for today", () => {
    expect(formatDateTime(new Date(2026, 9, 1, 9, 5).getTime(), "tr", now)).toBe("09:05");
  });
  it("adds the date for other days and the year for other years", () => {
    expect(formatDateTime(new Date(2026, 8, 12, 14, 5).getTime(), "tr", now)).toMatch(/12 Eyl.*14:05/);
    expect(formatDateTime(new Date(2025, 8, 12, 14, 5).getTime(), "en", now)).toMatch(/2025/);
  });
  it("formats seconds with the right decimal mark and unit", () => {
    expect(formatSeconds(900, "tr")).toBe("0,9 sn");
    expect(formatSeconds(900, "en")).toBe("0.9 s");
    expect(formatSeconds(12000, "en")).toBe("12 s");
  });
  it("formats a clock", () => {
    expect(formatClock(5000)).toBe("0:05");
    expect(formatClock(125400)).toBe("2:05");
  });
  it("keeps latency in milliseconds, rounding averages and grouping by locale", () => {
    expect(formatMilliseconds(900, "tr")).toBe("900 ms");
    expect(formatMilliseconds(1200.4, "tr")).toBe("1.200 ms");
    expect(formatMilliseconds(1200.6, "en")).toBe("1,201 ms");
    expect(formatMilliseconds(0, "en")).toBe("0 ms");
  });
});
