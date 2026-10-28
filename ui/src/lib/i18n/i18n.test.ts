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

/**
 * Signs of broken encoding rather than legitimate punctuation: a "?" replacing a letter (it is
 * followed by a letter, a digit or another "?"), U+FFFD, and UTF-8 read as Windows-1252/Latin-1
 * (a lead byte shown as Â/Ã/Ä/Å followed by a continuation byte, or "â€" for a 3-byte sequence).
 */
const corruption = /\?(?=[\p{L}\p{N}?])|\uFFFD|[ÂÃÄÅ][\u0080-\u00BF\u0152\u0153\u0160\u0161\u0178\u017D\u017E\u0192\u02C6\u02DC\u2013\u2014\u2018-\u201A\u201C-\u201E\u2020-\u2022\u2026\u2030\u2039\u203A\u20AC\u2122]|â€/u;

describe("corruption check", () => {
  it("catches replaced letters and mojibake", () => {
    for (const bad of ["Yerel (?evrimd???)", "?al???yor", "ba?ar?s?z", "Ã§alÄ±ÅŸÄ±yor", "Ã¼cret", "Ã¶nizleme", "Ä°ptal",
      "deÄŸil", "Ã‡alÄ±ÅŸma", "Ãœcret", "Ã–nizleme", "Åžimdi", "YÃ¼kleniyorâ€¦", "bozuk \uFFFD metin"]) {
      expect(bad, bad).toMatch(corruption);
    }
  });
  it("accepts legitimate questions and Turkish circumflex letters", () => {
    for (const good of ["Delete 3 items? This cannot be undone.", "Silinsin mi?", "Continue? (y/n)", "Âdem’in kâr payı",
      "Hâlâ yükleniyor…", "hâlâ…", "Kâğıt", "İptal edilsin mi?"]) {
      expect(good, good).not.toMatch(corruption);
    }
  });
});

describe("message tables", () => {
  it("all translations preserve text encoding", () => {
    for (const table of [tr, en]) for (const [key, value] of Object.entries(table)) {
      expect(value, key).not.toMatch(corruption);
    }
    expect(tr["ask.description"]).toContain("ajanın yanıtını yapıştırın");
    expect(tr["ask.permission_warning"]).toContain("Çalışma dizinine erişim");
    expect(en["ask.description"]).toContain("agent’s answer");
    expect(en["ask.history_agent"]).toBe("Ask · {agent}");
  });
  it("local transcription messages preserve Turkish characters", () => {
    expect(tr["profiles.preset.local"]).toBe("Yerel (çevrimdışı)");
    expect(tr["local.running"]).toBe("Çalışıyor");
  });
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
