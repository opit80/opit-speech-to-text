import { describe, expect, it } from "vitest";
import { ComboRecorder, comboProblem, defaultKeys, formatCombo, keyNameFromCode, sortCombo, parseComboText, formatLinuxCombo, LINUX_COMBOS } from "./hotkey";

describe("keyNameFromCode", () => {
  it("maps side-specific modifiers and keys to the Rust names", () => {
    expect(keyNameFromCode("ControlRight")).toBe("RightCtrl");
    expect(keyNameFromCode("ShiftLeft")).toBe("LeftShift");
    expect(keyNameFromCode("AltRight")).toBe("RightAlt");
    expect(keyNameFromCode("KeyQ")).toBe("Q");
    expect(keyNameFromCode("Digit7")).toBe("7");
    expect(keyNameFromCode("F13")).toBe("F13");
    expect(keyNameFromCode("F24")).toBe("F24");
    expect(keyNameFromCode("Pause")).toBe("Pause");
  });
  it("rejects keys the hook does not know or that cannot be captured", () => {
    for (const code of ["Fn", "Unidentified", "F25"]) {
      expect(keyNameFromCode(code), code).toBeNull();
    }
  });
  it("supports navigation, Windows, punctuation and numpad keys", () => {
    expect(keyNameFromCode("MetaLeft")).toBe("LeftWin");
    for (const code of ["Escape", "Tab", "Enter", "Numpad1", "Backquote", "ArrowUp", "Delete", "MediaPlayPause"]) expect(keyNameFromCode(code)).toBe(code);
    expect(keyNameFromCode("NumpadEnter")).toBe("Enter");
  });
});

describe("ComboRecorder", () => {
  it("returns the combo once every key is released, modifiers first", () => {
    const r = new ComboRecorder();
    r.down("KeyD");
    r.down("ControlRight");
    r.down("ControlRight"); // auto-repeat
    expect(r.up("KeyD")).toBeNull();
    expect(r.up("ControlRight")).toEqual(["RightCtrl", "D"]);
  });
  it("ignores unknown keys and starts over after a combo", () => {
    const r = new ComboRecorder();
    expect(r.down("Fn")).toBe(false);
    r.down("ShiftRight");
    expect(r.up("ShiftRight")).toEqual(["RightShift"]);
    r.down("F13");
    expect(r.up("F13")).toEqual(["F13"]);
  });
});

describe("comboProblem", () => {
  it("needs one non-modifier key on Linux", () => {
    expect(comboProblem(["LeftCtrl", "LeftShift", "F9"], "linux")).toBeNull();
    expect(comboProblem(["RightCtrl", "RightShift"], "linux")).toBe("shape");
    expect(comboProblem(["A", "B"], "linux")).toBe("shape");
  });
  it("refuses bare typing keys only on Linux, including picker and saved names", () => {
    const typing = ["A", "7", "Space", "Enter", "Tab", "Backspace", "Delete", "Insert", "Home", "End", "PageUp", "PageDown", "ArrowLeft", "CapsLock", "NumLock", "Numpad0", "NumpadAdd", "NumpadDecimal", "Quote", "Backslash", "IntlBackslash", " a "];
    for (const key of typing) {
      expect(comboProblem([key], "linux"), key).toBe("typing");
      expect(comboProblem(["LeftCtrl", key], "linux"), key).toBeNull();
      expect(comboProblem([key], "windows"), key).toBeNull();
    }
    for (const key of ["F1", "F24", "MediaPlayPause", "AudioVolumeUp", "Pause", "PrintScreen"]) {
      expect(comboProblem([key], "linux"), key).toBeNull();
    }
  });
  it("offers a valid Linux reset default without changing the Windows default", () => {
    expect(defaultKeys("linux")).toEqual(["LeftCtrl", "LeftShift", "F9"]);
    expect(comboProblem(defaultKeys("linux"), "linux")).toBeNull();
    expect(defaultKeys("linux")).not.toContain("Escape");
    expect(defaultKeys("windows")).toEqual(["RightCtrl", "RightShift"]);
  });
  it("accepts the default and common combos", () => {
    expect(comboProblem(["RightCtrl", "RightShift"])).toBeNull();
    expect(comboProblem(["LeftCtrl", "LeftAlt", "D"])).toBeNull();
    expect(comboProblem(["F13"])).toBeNull();
    expect(comboProblem(["Pause"])).toBeNull();
  });
  it("allows deliberate single-key and large combinations but refuses empty ones", () => {
    expect(comboProblem([])).toBe("empty");
    for (const combo of [["A"], ["A", "B"], ["Space"], ["RightShift"], ["LeftCtrl"], ["Escape"], ["LeftCtrl", "LeftShift", "LeftAlt", "A", "B"]]) expect(comboProblem(combo)).toBeNull();
  });
});

describe("formatCombo / sortCombo", () => {
  it("formats in both languages", () => {
    expect(formatCombo(["RightCtrl", "RightShift"], "en")).toBe("Right Ctrl + Right Shift");
    expect(formatCombo(["RightCtrl", "RightShift"], "tr")).toBe("Sağ Ctrl + Sağ Shift");
    expect(formatCombo(["LeftAlt", "Space"], "tr")).toBe("Sol Alt + Boşluk");
  });
  it("labels the Meta key as Super on Linux", () => {
    expect(formatCombo(["LeftWin", "A"], "en", "linux")).toBe("Super+A");
  });
  it("orders Ctrl, Shift, Alt (left before right), then the rest in press order", () => {
    expect(sortCombo(["D", "RightShift", "LeftCtrl", "F2"])).toEqual(["LeftCtrl", "RightShift", "D", "F2"]);
  });
});


describe("Linux typed combinations", () => {
  it("maps aliases into the existing config key names", () => {
    expect(parseComboText(" Ctrl + Shift + f9 ")).toEqual(["LeftCtrl", "LeftShift", "F9"]);
    expect(parseComboText("Meta+Alt+space")).toEqual(["LeftAlt", "LeftWin", "Space"]);
    expect(parseComboText("Super+A")).toEqual(["LeftWin", "A"]);
    expect(parseComboText("")).toEqual([]);
  });
  it("refuses unknown names, duplicate modifiers and malformed separators", () => {
    for (const text of ["Ctrl++F9", "Ctrl+", "+F9", "Hyper+F9", "Ctrl+Control+F9", "F01", "Ctrl+F25", "Super+Meta+A"]) {
      expect(parseComboText(text), text).toBeNull();
    }
    expect(comboProblem(parseComboText("Space")!, "linux")).toBe("typing");
    expect(comboProblem(parseComboText("Ctrl+Shift")!, "linux")).toBe("shape");
    expect(comboProblem(parseComboText("Ctrl+A+B")!, "linux")).toBe("shape");
  });
  it("formats plain editable text and offers valid independent suggestions", () => {
    expect(formatLinuxCombo(["RightCtrl", "LeftShift", "F9"])).toBe("Ctrl+Shift+F9");
    for (const text of LINUX_COMBOS) {
      const keys = parseComboText(text)!;
      expect(comboProblem(keys, "linux"), text).toBeNull();
      expect(formatLinuxCombo(keys)).toBe(text);
    }
  });
});
