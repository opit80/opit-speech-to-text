import { describe, expect, it } from "vitest";
import {
  deleteRule, exeProblem, foldExe, moveRule, newRule, newRuleId, prepareRule, ruleSummary, upsertRule,
} from "./app-rules";
import type { AppRule } from "./types";

const rule = (id: string, extra: Partial<AppRule> = {}): AppRule => ({ ...newRule(id), exe: "code.exe", ...extra });

describe("rule ids", () => {
  it("creates rule-<base36> ids that are not taken", () => {
    expect(newRuleId([])).toMatch(/^rule-[a-z0-9]{8}$/);
    const fixed = newRuleId([], () => 0.5);
    let calls = 0;
    // The first draw repeats the taken id, the second one differs.
    const id = newRuleId([fixed], () => (calls++ < 8 ? 0.5 : 0.1));
    expect(id).not.toBe(fixed);
    expect(id).toMatch(/^rule-[a-z0-9]{8}$/);
  });
  it("starts a rule with the documented defaults", () => {
    expect(newRule("rule-a")).toEqual({
      id: "rule-a", enabled: true, exe: "", title_contains: "", style_id: null, profile_id: null, paste_method: null, paste_after: null,
    });
  });
});

describe("exeProblem", () => {
  it("requires a file name", () => {
    expect(exeProblem("")).toBe("required");
    expect(exeProblem("   ")).toBe("required");
  });
  it("accepts plain .exe file names in any case", () => {
    for (const ok of ["code.exe", "Code.EXE", " WindowsTerminal.exe ", "my app.exe", "notepad++.exe", "çay.exe"]) {
      expect(exeProblem(ok), ok).toBeNull();
    }
  });
  it("rejects paths, wildcards, reserved characters and names without .exe", () => {
    for (const bad of ["C:\\Program Files\\code.exe", "bin/code.exe", "c:code.exe", "*.exe", "co?e.exe", "a\"b.exe", "a<b.exe",
      "a>b.exe", "a|b.exe", "code", "code.ex", "code.exe.txt", ".exe", ".", "..", "co\u0001de.exe"]) {
      expect(exeProblem(bad), bad).toBe("invalid");
    }
  });
});

describe("prepareRule", () => {
  it("trims, folds the file name like Rust and turns empty choices into null", () => {
    expect(prepareRule(rule("r", { exe: "  İnternet.EXE ", title_contains: "  GitHub ", style_id: "", profile_id: " groq " }))).toEqual(
      rule("r", { exe: "internet.exe", title_contains: "GitHub", style_id: null, profile_id: "groq" }),
    );
  });
  it("folds every Turkish i to i", () => {
    expect(foldExe("IıİiDIŞ.exe")).toBe("iiiidiş.exe");
  });
});

describe("list editing", () => {
  const rules = [rule("a"), rule("b"), rule("c")];
  const ids = (list: AppRule[]) => list.map((r) => r.id);

  it("moves a rule up or down and stops at the ends", () => {
    expect(ids(moveRule(rules, "b", -1))).toEqual(["b", "a", "c"]);
    expect(ids(moveRule(rules, "b", 1))).toEqual(["a", "c", "b"]);
    expect(ids(moveRule(rules, "a", -1))).toEqual(["a", "b", "c"]);
    expect(ids(moveRule(rules, "c", 1))).toEqual(["a", "b", "c"]);
    expect(ids(moveRule(rules, "missing", 1))).toEqual(["a", "b", "c"]);
    expect(ids(rules)).toEqual(["a", "b", "c"]);
  });
  it("replaces a rule in place or appends a new one", () => {
    const changed = upsertRule(rules, rule("b", { exe: "slack.exe" }));
    expect(ids(changed)).toEqual(["a", "b", "c"]);
    expect(changed[1]?.exe).toBe("slack.exe");
    expect(ids(upsertRule(rules, rule("d")))).toEqual(["a", "b", "c", "d"]);
  });
  it("deletes by id", () => {
    expect(ids(deleteRule(rules, "b"))).toEqual(["a", "c"]);
    expect(ids(deleteRule(rules, "missing"))).toEqual(["a", "b", "c"]);
  });
});

describe("ruleSummary", () => {
  it("lists the title filter and every override in a fixed order", () => {
    expect(ruleSummary(rule("r", {
      title_contains: "PR", style_id: "formal", profile_id: "groq", paste_method: "type", paste_after: "none",
    }))).toEqual([
      { key: "app_rules.summary.title", value: "PR" },
      { key: "app_rules.summary.style", value: "formal" },
      { key: "app_rules.summary.profile", value: "groq" },
      { key: "app_rules.summary.paste_method", value: "type" },
      { key: "app_rules.summary.paste_after", value: "none" },
    ]);
  });
  it("says when a rule overrides nothing", () => {
    expect(ruleSummary(rule("r"))).toEqual([{ key: "app_rules.summary.none", value: "" }]);
    expect(ruleSummary(rule("r", { title_contains: "x" }))).toEqual([
      { key: "app_rules.summary.title", value: "x" },
      { key: "app_rules.summary.none", value: "" },
    ]);
  });
});
