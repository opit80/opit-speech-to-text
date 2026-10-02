import { describe, expect, it } from "vitest";
import { ComboRecorder, comboProblem, formatCombo, keyNameFromCode, sortCombo } from "./hotkey";

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
  it("orders Ctrl, Shift, Alt (left before right), then the rest in press order", () => {
    expect(sortCombo(["D", "RightShift", "LeftCtrl", "F2"])).toEqual(["LeftCtrl", "RightShift", "D", "F2"]);
  });
});
