// Pure helpers for the update notice (App.svelte) and Settings → Updates.
import type { MessageKey, Params } from "./i18n";
import type { DictationState, UpdateState } from "./types";

/** An event received during an invoke is newer than its potentially stale reply. */
export async function checkUpdateState(
  check: () => Promise<UpdateState>,
  revision: () => number,
  adopt: (state: UpdateState) => void,
): Promise<void> {
  const started = revision();
  const state = await check();
  if (revision() === started) adopt(state);
}

/** The version the shell banner announces: only an available update the user has not dismissed. */
export function bannerVersion(state: UpdateState, dismissed: string | null): string | null {
  if (state.kind !== "available") return null;
  return state.info.version === dismissed ? null : state.info.version;
}

/** Whole percent downloaded, or null while the size is unknown. */
export function progressPercent(downloaded: number, total: number | null): number | null {
  if (total === null || total <= 0) return null;
  return Math.min(100, Math.max(0, Math.floor((downloaded * 100) / total)));
}

/** The status line on Settings → Updates. */
export function statusMessage(state: UpdateState): { key: MessageKey; params?: Params } {
  switch (state.kind) {
    case "idle":
      return { key: "update.status_idle" };
    case "checking":
      return { key: "update.status_checking" };
    case "up_to_date":
      return { key: "update.status_up_to_date" };
    case "available":
      return { key: "update.status_available", params: { version: state.info.version, current: state.info.current_version } };
    case "installing":
      return { key: "update.status_installing", params: { version: state.info.version } };
    case "check_failed":
      return { key: "update.status_failed", params: { message: state.message } };
  }
}

/** Installing closes the app, so it waits while a dictation is recording, transcribing or pasting. */
export function dictationBusy(state: DictationState): boolean {
  return state === "recording" || state === "transcribing" || state === "pasting";
}

/**
 * A failed install's error lives in shared state (the button that started the install is gone
 * by the time it fails). It stays while that update is on offer or installing again and is
 * dropped once a new check starts or the update is gone.
 */
export function keepInstallError(error: string | null, state: UpdateState): string | null {
  return state.kind === "available" || state.kind === "installing" ? error : null;
}
