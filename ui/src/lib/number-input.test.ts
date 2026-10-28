import { describe, expect, it } from "vitest";
import { duckPercentRange, parseNumberInput } from "./number-input";

describe("parseNumberInput", () => {
  const { min, max, step } = duckPercentRange;

  it("keeps whole percentages inside 0–100", () => {
    expect(parseNumberInput("0", min, max, step)).toBe(0);
    expect(parseNumberInput("20", min, max, step)).toBe(20);
    expect(parseNumberInput(" 100 ", min, max, step)).toBe(100);
  });

  it("clamps out-of-range percentages and rounds to whole numbers", () => {
    expect(parseNumberInput("-5", min, max, step)).toBe(0);
    expect(parseNumberInput("150", min, max, step)).toBe(100);
    expect(parseNumberInput("33.6", min, max, step)).toBe(34);
  });

  it("rejects empty or non-numeric text", () => {
    for (const text of ["", "   ", "abc", "NaN", "Infinity"]) {
      expect(parseNumberInput(text, min, max, step)).toBeNull();
    }
  });

  it("keeps two decimals for fractional steps", () => {
    expect(parseNumberInput("0.333", 0.03, 5, 0.01)).toBe(0.33);
    expect(parseNumberInput("0.001", 0.03, 5, 0.01)).toBe(0.03);
  });
});
