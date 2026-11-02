// WebView capture and Linux typed combinations. KeyboardEvent.code maps onto the
// key names the Rust hook understands (crates/app/src/platform/keys.rs `vk_for`).
import type { Lang, PlatformCapabilities } from "../../lib/bridge/types";
import { translate } from "../../lib/i18n";

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

  /** Snapshot for Linux, which finishes on the first valid non-modifier keydown. */
  current(): string[] {
    return sortCombo([...this.held]);
  }
}

type Platform = PlatformCapabilities["platform"];

export type ComboProblem = "empty" | "shape" | "typing";

/** Platform defaults apply only to new choices; never migrate a saved chord. */
export function defaultKeys(platform: Platform = "windows"): string[] {
  return platform === "linux" ? ["LeftCtrl", "LeftShift", "F9"] : ["RightCtrl", "RightShift"];
}

const MODIFIER = /^(left|right)(ctrl|shift|alt|win)$/i;
const TYPING_KEY = /^(?:[a-z0-9]|space|enter|tab|backspace|delete|insert|home|end|pageup|pagedown|arrow(?:left|right|up|down)|capslock|numlock|numpad(?:[0-9]|add|subtract|multiply|divide|decimal)|backquote|minus|equal|bracketleft|bracketright|backslash|semicolon|quote|comma|period|slash|intlbackslash)$/i;

/** Linux needs one non-modifier key and must not capture a bare typing key globally. */
export function comboProblem(keys: string[], platform: Platform = "windows"): ComboProblem | null {
  if (keys.length === 0) return "empty";
  if (platform === "linux") {
    const ordinary = keys.map((key) => key.trim()).filter((key) => !MODIFIER.test(key));
    if (ordinary.length !== 1) return "shape";
    if (!keys.some((key) => MODIFIER.test(key.trim())) && TYPING_KEY.test(ordinary[0])) return "typing";
  }
  return null;
}

function keyLabel(key: string, lang: Lang, platform: Platform): string {
  const side = /^(Left|Right)(Ctrl|Shift|Alt|Win)$/.exec(key);
  if (side) {
    const name = side[2] === "Win" && platform === "linux" ? "Super" : side[2];
    return `${translate(lang, side[1] === "Left" ? "hotkey.left" : "hotkey.right")} ${name}`;
  }
  if (key === "Space") return translate(lang, "hotkey.space");
  return key;
}

export function formatCombo(keys: string[], lang: Lang, platform: Platform = "windows"): string {
  if (platform === "linux") return formatLinuxCombo(keys);
  return keys.map((k) => keyLabel(k, lang, platform)).join(" + ");
}


// Avoid Ctrl+Alt+Fn, which desktops can reserve for virtual console switching.
export const LINUX_COMBOS = ["Ctrl+Shift+F9", "Ctrl+Shift+F10", "Shift+Super+F8", "Ctrl+Shift+F11", "Ctrl+Alt+Space"];
const PLAIN_MODIFIERS: Record<string, string> = {
  ctrl: "LeftCtrl", control: "LeftCtrl", shift: "LeftShift", alt: "LeftAlt",
  super: "LeftWin", meta: "LeftWin", win: "LeftWin",
};

/** Typed text only: this parser never listens to keyboard events. */
export function parseComboText(text: string): string[] | null {
  if (!text.trim()) return [];
  const keys: string[] = [];
  const canonical = new Map(SELECTABLE_KEYS.map((key) => [key.toLowerCase(), key]));
  for (const token of text.split("+")) {
    const name = token.trim().toLowerCase();
    const key = PLAIN_MODIFIERS[name] ?? canonical.get(name);
    if (!key) return null;
    const plain = plainKey(key);
    if (keys.some((other) => plainKey(other) === plain)) return null;
    keys.push(key);
  }
  return sortCombo(keys);
}

function plainKey(key: string): string {
  const modifier = /^(?:Left|Right)(Ctrl|Shift|Alt|Win)$/i.exec(key);
  if (!modifier) return key;
  return modifier[1].toLowerCase() === "win" ? "Super" : modifier[1][0].toUpperCase() + modifier[1].slice(1).toLowerCase();
}

export function formatLinuxCombo(keys: string[]): string {
  return [...new Set(sortCombo(keys).map(plainKey))].join("+");
}
