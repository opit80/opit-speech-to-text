import { formatLinuxCombo, LINUX_COMBOS, parseComboText } from "./hotkey";

export const CUSTOM_COMBO = "custom";

/** Match typed aliases and saved modifier sides without rewriting the editable draft. */
export function linuxComboChoice(text: string): string {
  const keys = parseComboText(text);
  const combo = keys ? formatLinuxCombo(keys) : "";
  return LINUX_COMBOS.includes(combo) ? combo : CUSTOM_COMBO;
}

/** Selecting Custom starts a blank draft; choosing a suggestion never installs it. */
export function linuxComboText(choice: string): string {
  return LINUX_COMBOS.includes(choice) ? choice : "";
}
