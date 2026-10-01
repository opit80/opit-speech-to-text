import { describe, expect, it } from "vitest";
import { en } from "./en";
import { hasKey, placeholders, resolveLang, translate } from "./index";
import { tr } from "./tr";

describe("resolveLang", () => {
  it("follows the explicit setting, then the system locale, then English", () => {
    expect(resolveLang("tr", "en-US")).toBe("tr");
    expect(resolveLang("en", "tr-TR")).toBe("en");
    expect(resolveLang(null, "tr-TR")).toBe("tr");
    expect(resolveLang(null, "TR")).toBe("tr");
    expect(resolveLang(null, "de-DE")).toBe("en");
    expect(resolveLang(null, null)).toBe("en");
  });
});

describe("translate", () => {
  it("fills named placeholders and leaves unknown ones", () => {
    expect(translate("en", "common.saved_at", { time: "14:05" })).toBe("Saved at 14:05");
    expect(translate("en", "common.saved_at")).toBe("Saved at {time}");
  });
});

describe("message tables", () => {
  it("tr has exactly the en keys", () => {
    expect(Object.keys(tr).sort()).toEqual(Object.keys(en).sort());
  });
  it("every key uses the same placeholders in both languages", () => {
    for (const key of Object.keys(en) as (keyof typeof en)[]) {
      expect(placeholders(tr[key]).sort(), key).toEqual(placeholders(en[key]).sort());
    }
  });
  it("no message is empty", () => {
    for (const table of [en, tr]) for (const [k, v] of Object.entries(table)) expect(v.trim(), k).not.toBe("");
  });
  it("hasKey guards dynamic keys", () => {
    expect(hasKey("nav.home")).toBe(true);
    expect(hasKey("nav.nope")).toBe(false);
  });
});
