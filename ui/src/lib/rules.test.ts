import { describe, expect, it } from "vitest";
import { budgetLevel, hasInlineComments } from "./rules";

describe("hasInlineComments", () => {
  it("ignores the leading comment block", () => {
    expect(hasInlineComments("# header\n\nschema: 1\nid: user\n")).toBe(false);
  });
  it("finds comments after the header", () => {
    expect(hasInlineComments("# header\nschema: 1\n# note\nid: user\n")).toBe(true);
    expect(hasInlineComments("schema: 1\nterms: [a] # trailing\n")).toBe(true);
  });
  it("does not count # inside quoted values", () => {
    expect(hasInlineComments("schema: 1\nterms: ['C#', \"F#\"]\n")).toBe(false);
  });
});

describe("budgetLevel", () => {
  it("grades the prompt budget", () => {
    expect(budgetLevel(100)).toBe("ok");
    expect(budgetLevel(168)).toBe("warning");
    expect(budgetLevel(224)).toBe("full");
    expect(budgetLevel(300)).toBe("full");
  });
});
