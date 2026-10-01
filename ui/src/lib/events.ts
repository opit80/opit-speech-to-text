// Typed wrappers for every Rust → UI event (crates/app/src/events.rs).
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppConfig, DictationStatus, HotkeyState, MicTestEvent, UpdateState } from "./types";

function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
}

export const onStatus = (h: (s: DictationStatus) => void) => on("dictation-status", h);
export const onHistoryAdded = (h: (id: number) => void) => on("history-added", h);
export const onConfigChanged = (h: (c: AppConfig) => void) => on("config-changed", h);
export const onNavigate = (h: (route: string) => void) => on("navigate", h);
export const onHotkeyState = (h: (s: HotkeyState) => void) => on("hotkey-state", h);
export const onMicTest = (h: (e: MicTestEvent) => void) => on("mic-test", h);
export const onUpdateState = (h: (s: UpdateState) => void) => on("update-state", h);
export type { UnlistenFn };
