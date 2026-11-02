import type { MessageKey } from "../../lib/i18n";
import { ComboRecorder, comboProblem, formatLinuxCombo, keyNameFromCode, type ComboProblem } from "./hotkey";

export const LINUX_CAPTURE_TIMEOUT_MS = 10_000;
export type CaptureCancelReason = "escape" | "blur" | "cancel" | "timeout" | "desktop";
type CaptureKey = Pick<KeyboardEvent, "code" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey" | "repeat" | "isComposing">;
export type CaptureResult =
  | { kind: "ignored" | "waiting" }
  | { kind: "refused"; problem: ComboProblem }
  | { kind: "recorded"; text: string }
  | { kind: "cancelled"; text: string; status: MessageKey };

/** Pure, in-window capture. It changes only the draft; Install owns validation and saving. */
export class LinuxShortcutCapture {
  active = false;
  private previousText = "";

  constructor(private readonly recorder: ComboRecorder) {}

  start(text: string): void {
    this.previousText = text;
    this.recorder.reset();
    this.active = true;
  }

  keydown(event: CaptureKey): CaptureResult {
    if (!this.active) return { kind: "ignored" };
    if (event.code === "Escape") return this.cancel("escape");
    if (event.repeat || event.isComposing) return { kind: "ignored" };
    const name = keyNameFromCode(event.code);
    if (!name) return { kind: "ignored" };
    if (/^(Left|Right)(Ctrl|Shift|Alt|Win)$/.test(name)) return { kind: "waiting" };

    // Flags include modifiers pressed before the field gained focus. Rebuild each
    // candidate so a refused typing key never leaks into the next chord.
    this.recorder.reset();
    if (event.ctrlKey) this.recorder.down("ControlLeft");
    if (event.shiftKey) this.recorder.down("ShiftLeft");
    if (event.altKey) this.recorder.down("AltLeft");
    if (event.metaKey) this.recorder.down("MetaLeft");
    this.recorder.down(event.code);
    const keys = this.recorder.current();
    const problem = comboProblem(keys, "linux");
    if (problem) return { kind: "refused", problem };
    this.active = false;
    this.recorder.reset();
    return { kind: "recorded", text: formatLinuxCombo(keys) };
  }

  cancel(reason: CaptureCancelReason): CaptureResult {
    if (!this.active) return { kind: "ignored" };
    this.active = false;
    this.recorder.reset();
    return {
      kind: "cancelled", text: this.previousText,
      status: reason === "desktop" ? "settings.capture_desktop_linux"
        : reason === "timeout" ? "settings.capture_timeout_linux" : "settings.capture_cancelled",
    };
  }
}
