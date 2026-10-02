// Shortcut capture in the WebView. KeyboardEvent.code is side-specific, so it maps onto the
// key names the Rust hook understands (crates/app/src/platform/keys.rs `vk_for`).
import type { Lang } from "./types";
import { translate } from "./i18n";

const NAMED: Record<string, string> = {
  ControlLeft: "LeftCtrl", ControlRight: "RightCtrl", ShiftLeft: "LeftShift", ShiftRight: "RightShift",
  AltLeft: "LeftAlt", AltRight: "RightAlt", Space: "Space", CapsLock: "CapsLock", ScrollLock: "ScrollLock",
  Pause: "Pause", Insert: "Insert", Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown",
  MetaLeft: "LeftWin", MetaRight: "RightWin", Escape: "Escape", Tab: "Tab", Enter: "Enter", NumpadEnter: "Enter",
  Backspace: "Backspace", Delete: "Delete", ArrowLeft: "ArrowLeft", ArrowRight: "ArrowRight", ArrowUp: "ArrowUp", ArrowDown: "ArrowDown",
  PrintScreen: "PrintScreen", ContextMenu: "ContextMenu", NumLock: "NumLock",
  Numpad0: "Numpad0", Numpad1: "Numpad1", Numpad2: "Numpad2", Numpad3: "Numpad3", Numpad4: "Numpad4", Numpad5: "Numpad5", Numpad6: "Numpad6", Numpad7: "Numpad7", Numpad8: "Numpad8", Numpad9: "Numpad9",
  NumpadAdd: "NumpadAdd", NumpadSubtract: "NumpadSubtract", NumpadMultiply: "NumpadMultiply", NumpadDivide: "NumpadDivide", NumpadDecimal: "NumpadDecimal",
  Backquote: "Backquote", Minus: "Minus", Equal: "Equal", BracketLeft: "BracketLeft", BracketRight: "BracketRight", Backslash: "Backslash", Semicolon: "Semicolon", Quote: "Quote", Comma: "Comma", Period: "Period", Slash: "Slash", IntlBackslash: "IntlBackslash",
  AudioVolumeMute: "AudioVolumeMute", AudioVolumeUp: "AudioVolumeUp", AudioVolumeDown: "AudioVolumeDown", MediaPlayPause: "MediaPlayPause", MediaStop: "MediaStop", MediaTrackNext: "MediaTrackNext", MediaTrackPrevious: "MediaTrackPrevious",
};

export const SELECTABLE_KEYS = [...new Set(Object.values(NAMED)), ...Array.from({ length: 26 }, (_, i) => String.fromCharCode(65 + i)), ...Array.from({ length: 10 }, (_, i) => String(i)), ...Array.from({ length: 24 }, (_, i) => `F${i + 1}`)];

/** Matches the names in Rust's platform/keys.rs; the picker covers OS-intercepted keys. */
export function keyNameFromCode(code: string): string | null {
  if (Object.hasOwn(NAMED, code)) return NAMED[code];
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1];
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit) return digit[1];
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  return null;
}

const MODIFIER_ORDER = ["LeftCtrl", "RightCtrl", "LeftShift", "RightShift", "LeftAlt", "RightAlt", "LeftWin", "RightWin"];

export function sortCombo(keys: string[]): string[] {
  const mods = MODIFIER_ORDER.filter((m) => keys.includes(m));
  return [...mods, ...keys.filter((k) => !MODIFIER_ORDER.includes(k))];
}

/** Collects a chord: keys pressed while at least one is held; finished when all are released. */
export class ComboRecorder {
  private held = new Set<string>();
  private pressed: string[] = [];

  /** Returns false for keys that cannot be part of a shortcut. */
  down(code: string): boolean {
    const name = keyNameFromCode(code);
    if (!name) return false;
    this.held.add(name);
    if (!this.pressed.includes(name)) this.pressed.push(name);
    return true;
  }

  /** The finished combo once every pressed key is released, else null. */
  up(code: string): string[] | null {
    const name = keyNameFromCode(code);
    if (!name || !this.held.delete(name)) return null;
    if (this.held.size > 0 || this.pressed.length === 0) return null;
    const combo = sortCombo(this.pressed);
    this.pressed = [];
    return combo;
  }

  reset(): void {
    this.held.clear();
    this.pressed = [];
  }
}

export type ComboProblem = "empty";

/** Single letters and modifiers are deliberate choices; only an empty chord is refused. */
export function comboProblem(keys: string[]): ComboProblem | null {
  if (keys.length === 0) return "empty";
  return null;
}

function keyLabel(key: string, lang: Lang): string {
  const side = /^(Left|Right)(Ctrl|Shift|Alt|Win)$/.exec(key);
  if (side) return `${translate(lang, side[1] === "Left" ? "hotkey.left" : "hotkey.right")} ${side[2]}`;
  if (key === "Space") return translate(lang, "hotkey.space");
  return key;
}

export function formatCombo(keys: string[], lang: Lang): string {
  return keys.map((k) => keyLabel(k, lang)).join(" + ");
}
