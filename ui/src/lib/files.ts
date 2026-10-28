import type { FileEvent, FilePhase, FileState } from "./types";

export function fileRunning(phase: FilePhase): boolean {
  return phase === "decoding" || phase === "transcribing" || phase === "cancelling";
}

/** An event can beat a command response or the initial snapshot after reopening. */
export function mergeFileEvent(state: FileState, event: FileEvent): FileState {
  // Events omit results and errors. Never associate an older transcript with a
  // newer job revision while its authoritative snapshot is still in flight.
  return event.revision > state.revision ? { ...state, ...event, result: null, error: null } : state;
}

export function acceptFileSnapshot(state: FileState, snapshot: FileState): FileState {
  return snapshot.revision >= state.revision ? snapshot : state;
}
