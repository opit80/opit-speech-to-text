import { describe, expect, it } from "vitest";
import { comboProblem, defaultKeys, formatLinuxCombo, LINUX_COMBOS, parseComboText } from "./hotkey";
import { CUSTOM_COMBO, linuxComboChoice, linuxComboText } from "./linux-shortcut-picker";

describe("Linux shortcut picker", () => {
  it.each(LINUX_COMBOS)("round-trips the valid suggestion %s", (combo) => {
    const keys = parseComboText(combo);
    expect(keys).not.toBeNull();
    expect(comboProblem(keys!, "linux")).toBeNull();
    expect(formatLinuxCombo(keys!)).toBe(combo);
    expect(linuxComboText(combo)).toBe(combo);
    expect(linuxComboChoice(linuxComboText(combo))).toBe(combo);
  });

  it("recognizes typed aliases, spacing, modifier order and saved modifier sides", () => {
    expect(linuxComboChoice(" shift + CONTROL + f9 ")).toBe("Ctrl+Shift+F9");
    expect(linuxComboChoice("RightCtrl+RightShift+F9")).toBe("Ctrl+Shift+F9");
    expect(linuxComboChoice("Meta+Shift+F8")).toBe("Shift+Super+F8");
  });

  it.each(["", "Ctrl+Alt+A", "Ctrl+", "Hyper+F9", "Ctrl+Shift", "Space"])(
    "shows Custom for unlisted or unfinished text %j", (text) => {
      expect(linuxComboChoice(text)).toBe(CUSTOM_COMBO);
    },
  );

  it("starts an empty custom draft and recognizes a suggestion typed afterwards", () => {
    expect(linuxComboText(CUSTOM_COMBO)).toBe("");
    expect(linuxComboChoice(linuxComboText(CUSTOM_COMBO))).toBe(CUSTOM_COMBO);
    expect(linuxComboChoice("Ctrl+Shift+F9")).toBe(LINUX_COMBOS[0]);
  });

  it("keeps the default first, suggestions unique and common desktop defaults out", () => {
    expect(LINUX_COMBOS[0]).toBe(formatLinuxCombo(defaultKeys("linux")));
    expect(new Set(LINUX_COMBOS).size).toBe(LINUX_COMBOS.length);
    const reserved = [
      "Ctrl+Alt+T", "Super+L", "Ctrl+Alt+Delete", "Alt+F2", "Alt+F4", "Alt+Space",
      "Super+Space", "Shift+Super+Space", "Ctrl+F9", "Ctrl+F10", "Ctrl+F11",
      ...Array.from({ length: 12 }, (_, i) => `Ctrl+Alt+F${i + 1}`),
    ];
    for (const combo of reserved) expect(LINUX_COMBOS).not.toContain(combo);
  });
});
