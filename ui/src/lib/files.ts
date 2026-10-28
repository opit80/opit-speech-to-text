import type { FileEvent, FilePhase, FileState, FileWork } from "./types";

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

/**
 * Merges a file snapshot or event into the shell's view. Saving or copying a result keeps its
 * revision and only clears `unsaved`, so within one revision `unsaved` may only turn false.
 */
export function mergeFileWork(current: FileWork, next: FileWork): FileWork {
  if (next.revision > current.revision) return { revision: next.revision, phase: next.phase, unsaved: next.unsaved };
  if (next.revision === current.revision && current.unsaved && !next.unsaved) return { ...current, unsaved: false };
  return current;
}

/** Installing an update closes the app, which would lose a running job or an unsaved result. */
export function fileBlocksUpdate(work: FileWork): boolean {
  return fileRunning(work.phase) || work.unsaved;
}
