import { expect, it } from "vitest";
import { shortcutTestAllowsNext } from "./shortcut-test";

it("Linux setup advances only after the currently configured shortcut is confirmed", () => {
  expect(shortcutTestAllowsNext("linux", "Ctrl+F9", "Ctrl+F9", true)).toBe(true);
  expect(shortcutTestAllowsNext("linux", "Ctrl+F9", "Ctrl+F9", false)).toBe(false);
  expect(shortcutTestAllowsNext("linux", "Ctrl+F10", "Ctrl+F9", true)).toBe(false);
  expect(shortcutTestAllowsNext("windows", "Ctrl+F9", "", false)).toBe(true);
});
