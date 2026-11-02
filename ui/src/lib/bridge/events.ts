// Typed wrappers for every Rust → UI event (crates/app/src/shell/events.rs).
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppConfig, DictationStatus, HotkeyState, MicTestEvent, UpdateState } from "./types";
import type { FileEvent } from "./types";
import type { LocalProgress } from "./types";

function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
}

export const onStatus = (h: (s: DictationStatus) => void) => on("dictation-status", h);
export const onPlatformCapabilitiesChanged = (h: () => void) => on<null>("platform-capabilities-changed", () => h());
export const onPendingResultsChanged = (h: () => void) => on<null>("pending-results-changed", () => h());
export const onHistoryAdded = (h: (id: number) => void) => on("history-added", h);
/** A History row's text was edited; the payload is the row id (never the text). */
export const onHistoryUpdated = (h: (id: number) => void) => on("history-updated", h);
/** Personal rules were saved (Rules page or `learning_apply`, even a failed one). */
export const onRulesChanged = (h: () => void) => on<null>("rules-changed", () => h());
export const onConfigChanged = (h: (c: AppConfig) => void) => on("config-changed", h);
export const onNavigate = (h: (route: string) => void) => on("navigate", h);
export const onHotkeyState = (h: (s: HotkeyState) => void) => on("hotkey-state", h);
/** A Linux desktop relay was suppressed while the focused window records a shortcut. */
export const onHotkeyCaptureIntercepted = (h: () => void) => on<null>("hotkey-capture-intercepted", () => h());
export const onMicTest = (h: (e: MicTestEvent) => void) => on("mic-test", h);
export const onUpdateState = (h: (s: UpdateState) => void) => on("update-state", h);
export const onFileProgress = (h: (s: FileEvent) => void) => on("file-progress", h);
export const onLocalProgress = (h: (s: LocalProgress) => void) => on("local-progress", h);
export type { UnlistenFn };
