import { describe, expect, it } from "vitest";
import {
  defaultStylePrompt, deleteStyle, historyStyleName, isBuiltinStyle, newStyle, newStyleId, prepareStyle,
  storedStyleName, styleDisplayName, styleProblem, upsertStyle,
} from "./styles";
import type { CleanupStyle, StyleDefaults } from "../../lib/bridge/types";

const defaults: StyleDefaults[] = [
  { id: "raw", names: { en: "Raw", tr: "Ham" }, prompts: { en: "", tr: "" } },
  { id: "clean", names: { en: "Clean", tr: "Düzenli" }, prompts: { en: "Clean EN", tr: "Clean TR" } },
  { id: "formal", names: { en: "Formal", tr: "Resmî" }, prompts: { en: "Formal EN", tr: "Formal TR" } },
  { id: "casual", names: { en: "Casual", tr: "Gündelik" }, prompts: { en: "Casual EN", tr: "Casual TR" } },
  { id: "prompt", names: { en: "AI prompt", tr: "Yapay zekâ istemi" }, prompts: { en: "Prompt EN", tr: "Prompt TR" } },
];
const style = (id: string, name: string, extra: Partial<CleanupStyle> = {}): CleanupStyle => ({
  id, name, builtin: isBuiltinStyle(id), prompt: "", model: null, in_cycle: true, hotkey: [], ...extra,
});
const builtins = defaults.map((d) => style(d.id, d.names.en));

describe("style ids", () => {
  it("recognises the five built-ins only", () => {
    for (const id of ["raw", "clean", "formal", "casual", "prompt"]) expect(isBuiltinStyle(id)).toBe(true);
    expect(isBuiltinStyle("custom-a")).toBe(false);
    expect(isBuiltinStyle("Clean")).toBe(false);
  });
  it("creates valid, unused custom ids", () => {
    const id = newStyleId([]);
    expect(id).toMatch(/^custom-[a-z0-9]+$/);
    expect(id.length).toBeLessThanOrEqual(48);
    const taken = newStyleId([], () => 0.5);
    // The first candidate is taken, so another one is drawn.
    let calls = 0;
    const random = () => (calls++ < 8 ? 0.5 : 0.25);
    const next = newStyleId([taken], random);
    expect(next).not.toBe(taken);
    expect(next).toMatch(/^custom-[a-z0-9]+$/);
  });
});

describe("style names", () => {
  it("localizes an unchanged built-in name and keeps any other name verbatim", () => {
    expect(styleDisplayName(style("clean", "Clean"), defaults, "tr")).toBe("Düzenli");
    expect(styleDisplayName(style("clean", "Clean"), defaults, "en")).toBe("Clean");
    expect(styleDisplayName(style("clean", "Tidy"), defaults, "tr")).toBe("Tidy");
    expect(styleDisplayName(style("custom-x", "Clean"), defaults, "tr")).toBe("Clean");
  });
  it("shows the stored name while the defaults are not loaded", () => {
    expect(styleDisplayName(style("formal", "Formal"), null, "tr")).toBe("Formal");
  });
  it("stores an untouched or blank built-in name as the English default", () => {
    expect(storedStyleName("Resmî", "formal", defaults, "tr")).toBe("Formal");
    expect(storedStyleName("  ", "formal", defaults, "tr")).toBe("Formal");
    expect(storedStyleName(" Formal ", "formal", defaults, "tr")).toBe("Formal");
    expect(storedStyleName(" E-posta ", "formal", defaults, "tr")).toBe("E-posta");
    expect(storedStyleName(" Notes ", "custom-a", defaults, "en")).toBe("Notes");
  });
  it("names history styles other than clean, tolerating deleted ones", () => {
    const styles = [...builtins, style("custom-a", "Notes")];
    expect(historyStyleName(null, styles, defaults, "tr")).toBeNull();
    expect(historyStyleName("clean", styles, defaults, "tr")).toBeNull();
    expect(historyStyleName("formal", styles, defaults, "tr")).toBe("Resmî");
    expect(historyStyleName("custom-a", styles, defaults, "tr")).toBe("Notes");
    expect(historyStyleName("custom-gone", styles, defaults, "tr")).toBeNull();
    // A built-in missing from the list still has its default name.
    expect(historyStyleName("casual", [], defaults, "en")).toBe("Casual");
  });
});

describe("style prompts", () => {
  it("uses the language default, the clean default for custom styles and none for raw", () => {
    expect(defaultStylePrompt("formal", defaults, "tr")).toBe("Formal TR");
    expect(defaultStylePrompt("custom-a", defaults, "en")).toBe("Clean EN");
    expect(defaultStylePrompt("raw", defaults, "en")).toBe("");
    expect(defaultStylePrompt("formal", null, "en")).toBe("");
  });
});

describe("style validation", () => {
  it("requires a name and a prompt for user styles only", () => {
    expect(styleProblem(style("formal", ""))).toBeNull();
    expect(styleProblem(style("custom-a", "Notes", { prompt: "Write notes." }))).toBeNull();
    expect(styleProblem(style("custom-a", "Notes", { prompt: "  " }))).toBe("prompt");
    expect(styleProblem(style("custom-a", " ", { prompt: "Write notes." }))).toBe("name");
  });
  it("prepares a draft for saving", () => {
    const prepared = prepareStyle(style("formal", "Resmî", { model: "  ", prompt: "   ", hotkey: ["F8"] }), defaults, "tr");
    expect(prepared).toEqual(style("formal", "Formal", { hotkey: ["F8"] }));
    expect(prepareStyle(style("custom-a", " Notes ", { model: " m ", prompt: " Keep this. " }), defaults, "en"))
      .toEqual(style("custom-a", "Notes", { model: "m", prompt: " Keep this. " }));
  });
  it("starts a new user style in the cycle without a shortcut", () => {
    expect(newStyle("custom-a")).toEqual(style("custom-a", "", { builtin: false }));
  });
});

describe("style list edits", () => {
  it("replaces in place or appends", () => {
    const edited = style("formal", "Mail");
    expect(upsertStyle(builtins, edited).map((s) => s.name)).toEqual(["Raw", "Clean", "Mail", "Casual", "AI prompt"]);
    expect(upsertStyle(builtins, style("custom-a", "Notes")).map((s) => s.id).at(-1)).toBe("custom-a");
  });
  it("deletes user styles, falling back to clean when the active one goes", () => {
    const styles = [...builtins, style("custom-a", "Notes"), style("custom-b", "Code")];
    expect(deleteStyle({ styles, active_style_id: "custom-a" }, "custom-a"))
      .toEqual({ styles: [...builtins, styles[6]], active_style_id: "clean" });
    expect(deleteStyle({ styles, active_style_id: "formal" }, "custom-b").active_style_id).toBe("formal");
  });
  it("never deletes a built-in", () => {
    expect(deleteStyle({ styles: builtins, active_style_id: "formal" }, "formal"))
      .toEqual({ styles: builtins, active_style_id: "formal" });
  });
});
