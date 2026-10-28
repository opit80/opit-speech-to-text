import { describe, expect, it } from "vitest";
import {
  MAX_HISTORY_TEXT_BYTES, canEditRow, caseOnly, defaultChoice, editIssue, editorRows, foldText, initialChoices, splitChoices,
} from "./learning";
import type { Dictation } from "./types";

describe("foldText", () => {
  it("folds dotted and dotless i like the rule engine", () => {
    expect(foldText("İSTANBUL")).toBe("istanbul");
    expect(foldText("ıIiİ")).toBe("iiii");
    expect(foldText("GitHub")).toBe("github");
    expect(foldText("ÇĞÖŞÜ")).toBe("çğöşü");
  });
});

describe("caseOnly / defaultChoice", () => {
  it("offers a term when only the letter case differs", () => {
    expect(caseOnly({ heard: "github", corrected: "GitHub" })).toBe(true);
    expect(caseOnly({ heard: "istanbul", corrected: "İstanbul" })).toBe(true);
    expect(caseOnly({ heard: "hala", corrected: "HALA" })).toBe(true);
    expect(defaultChoice({ heard: "github", corrected: "GitHub" })).toBe("term");
  });
  it("ignores spacing differences when deciding casing only", () => {
    expect(caseOnly({ heard: "claude  code", corrected: "Claude Code" })).toBe(true);
  });
  it("offers a correction rule when the words differ", () => {
    expect(caseOnly({ heard: "klod kod", corrected: "Claude Code" })).toBe(false);
    expect(defaultChoice({ heard: "klod kod", corrected: "Claude Code" })).toBe("rule");
    expect(defaultChoice({ heard: "hala", corrected: "hâlâ" })).toBe("rule");
  });
  it("builds one default choice per pair", () => {
    expect(initialChoices([{ heard: "a b", corrected: "AB" }, { heard: "api", corrected: "API" }])).toEqual(["rule", "term"]);
  });
});

describe("splitChoices", () => {
  const pairs = [
    { heard: "klod", corrected: "Claude" },
    { heard: "github", corrected: "GitHub" },
    { heard: "opet", corrected: "Opit" },
  ];
  it("applies the chosen targets and dismisses the skipped pairs", () => {
    expect(splitChoices(pairs, ["rule", "term", "skip"])).toEqual({
      apply: [
        { heard: "klod", corrected: "Claude", target: "rule" },
        { heard: "github", corrected: "GitHub", target: "term" },
      ],
      dismiss: [{ heard: "opet", corrected: "Opit" }],
    });
  });
  it("dismisses everything when every pair is skipped", () => {
    const split = splitChoices(pairs, ["skip", "skip", "skip"]);
    expect(split.apply).toEqual([]);
    expect(split.dismiss).toEqual(pairs);
  });
  it("treats a missing choice as skipped", () => {
    expect(splitChoices(pairs, ["term"]).dismiss).toHaveLength(2);
  });
  it("sends only heard and corrected, never extra fields", () => {
    const split = splitChoices([{ heard: "x", corrected: "y", extra: 1 } as never], ["skip"]);
    expect(Object.keys(split.dismiss[0])).toEqual(["heard", "corrected"]);
  });
});

describe("editIssue", () => {
  it("accepts ordinary text and keeps whitespace as typed", () => {
    expect(editIssue("  Merhaba dünya \n")).toBeNull();
  });
  it("rejects blank text", () => {
    expect(editIssue("")).toBe("empty");
    expect(editIssue(" \n\t ")).toBe("empty");
  });
  it("rejects a NUL character", () => {
    expect(editIssue("a\0b")).toBe("nul");
  });
  it("counts UTF-8 bytes, not UTF-16 units", () => {
    expect(editIssue("a".repeat(MAX_HISTORY_TEXT_BYTES))).toBeNull();
    expect(editIssue("a".repeat(MAX_HISTORY_TEXT_BYTES + 1))).toBe("too_long");
    // "ş" is two bytes in UTF-8 but one UTF-16 unit.
    expect(editIssue("ş".repeat(MAX_HISTORY_TEXT_BYTES / 2))).toBeNull();
    expect(editIssue("ş".repeat(MAX_HISTORY_TEXT_BYTES / 2 + 1))).toBe("too_long");
  });
});

describe("canEditRow", () => {
  const row = (status: Dictation["status"], mode: Dictation["mode"] = "dictation") => ({ status, mode }) as Dictation;
  it("allows only successful transcripts, in every mode", () => {
    expect(canEditRow(row("ok"))).toBe(true);
    expect(canEditRow(row("ok", "ask"))).toBe(true);
    expect(canEditRow(row("ok", "edit"))).toBe(true);
    expect(canEditRow(row("ok", "translate"))).toBe(true);
    expect(canEditRow(row("failed"))).toBe(false);
    expect(canEditRow(row("empty"))).toBe(false);
    expect(canEditRow(row("hallucination"))).toBe(false);
  });
});

describe("editorRows", () => {
  it("fits the text between a small and a large height", () => {
    expect(editorRows("")).toBe(3);
    expect(editorRows("one line")).toBe(3);
    expect(editorRows("a\nb\nc\nd")).toBe(4);
    expect(editorRows("x".repeat(70 * 6))).toBe(6);
    expect(editorRows("line\n".repeat(100))).toBe(14);
  });
});
