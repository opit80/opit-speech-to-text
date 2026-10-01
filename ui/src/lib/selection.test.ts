import { describe, expect, it } from "vitest";
import { expandToWords } from "./selection";

describe("expandToWords", () => {
  const text = "cloud code'u açtım, sonra GitHub'a push";
  it("grows a partial selection to whole words", () => {
    // "oud co" → "cloud code"
    expect(expandToWords(text, 2, 8)).toEqual({ start: 0, end: 10 });
  });
  it("stops at apostrophes so Turkish suffixes stay out", () => {
    const start = text.indexOf("GitHub");
    expect(expandToWords(text, start + 1, start + 3)).toEqual({ start, end: start + 6 });
  });
  it("trims surrounding spaces and punctuation", () => {
    const s = text.indexOf(" açtım,");
    expect(expandToWords(text, s, s + 7)).toEqual({ start: s + 1, end: s + 6 });
  });
  it("returns an empty range for a selection of only spaces", () => {
    const r = expandToWords("a   b", 1, 3);
    expect(r.end).toBe(r.start);
  });
});
