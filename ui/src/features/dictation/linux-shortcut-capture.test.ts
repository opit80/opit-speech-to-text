import { describe, expect, it } from "vitest";
import { ComboRecorder } from "./hotkey";
import { LinuxShortcutCapture, type CaptureCancelReason } from "./linux-shortcut-capture";
import { linuxComboChoice } from "./linux-shortcut-picker";

const key = (code: string, flags = {}) => ({
  code, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false,
  repeat: false, isComposing: false, ...flags,
});
const capture = () => {
  const capture = new LinuxShortcutCapture(new ComboRecorder());
  capture.start("Control + F8");
  return capture;
};

describe("Linux in-window shortcut capture", () => {
  it("records the first non-modifier keydown, including modifiers held before focus", () => {
    const c = capture();
    expect(c.keydown(key("ControlRight", { ctrlKey: true })).kind).toBe("waiting");
    expect(c.keydown(key("F9", { ctrlKey: true, shiftKey: true }))).toEqual({ kind: "recorded", text: "Ctrl+Shift+F9" });
    expect(c.active).toBe(false);
    expect(c.keydown(key("KeyA"))).toEqual({ kind: "ignored" });
    expect(linuxComboChoice("Ctrl+Shift+F9")).toBe("Ctrl+Shift+F9");
  });

  it("supports Super, Alt, punctuation and standalone non-typing keys", () => {
    for (const [event, text] of [
      [key("Space", { metaKey: true, altKey: true }), "Alt+Super+Space"],
      [key("Quote", { ctrlKey: true }), "Ctrl+Quote"],
      [key("F13"), "F13"],
    ] as const) {
      expect(capture().keydown(event)).toEqual({ kind: "recorded", text });
    }
  });

  it("refuses bare typing keys and keeps recording for a corrected chord", () => {
    const c = capture();
    for (const code of ["KeyA", "Digit7", "Space", "Tab", "Enter", "Backspace", "Quote"]) {
      expect(c.keydown(key(code))).toEqual({ kind: "refused", problem: "typing" });
      expect(c.active).toBe(true);
    }
    expect(c.keydown(key("KeyA", { ctrlKey: true }))).toEqual({ kind: "recorded", text: "Ctrl+A" });
  });

  it("ignores repeats, composition and unknown keys", () => {
    const c = capture();
    for (const event of [key("KeyA", { repeat: true }), key("KeyA", { isComposing: true }), key("Fn")]) {
      expect(c.keydown(event)).toEqual({ kind: "ignored" });
      expect(c.active).toBe(true);
    }
    expect(c.keydown(key("F10", { shiftKey: true }))).toEqual({ kind: "recorded", text: "Shift+F10" });
  });

  it("Escape cancels even with modifiers and restores the exact previous draft", () => {
    const c = capture();
    expect(c.keydown(key("Escape", { ctrlKey: true }))).toEqual({
      kind: "cancelled", text: "Control + F8", status: "settings.capture_cancelled",
    });
    expect(c.active).toBe(false);
  });

  it.each([
    ["blur", "settings.capture_cancelled"],
    ["cancel", "settings.capture_cancelled"],
    ["timeout", "settings.capture_timeout_linux"],
    ["desktop", "settings.capture_desktop_linux"],
  ] as const)("%s restores the draft and chooses the right status", (reason, status) => {
    const c = capture();
    c.keydown(key("ControlLeft", { ctrlKey: true }));
    expect(c.cancel(reason as CaptureCancelReason)).toEqual({ kind: "cancelled", text: "Control + F8", status });
    expect(c.active).toBe(false);
    expect(c.cancel(reason)).toEqual({ kind: "ignored" });
    c.start("Super+F7");
    expect(c.keydown(key("F10"))).toEqual({ kind: "recorded", text: "F10" });
  });
});
