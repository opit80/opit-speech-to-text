import { describe, expect, it } from "vitest";
import { acceptFileSnapshot, fileRunning, mergeFileEvent } from "./files";
import type { FileState } from "./types";

const initial: FileState = { revision: 2, phase: "decoding", name: "audio.wav", progress: null, result: null, error: null,
  options: { language: null, cleanup: false, speakers: false } };

describe("file jobs", () => {
  it("keeps a newer completion when a start response or progress event arrives late", () => {
    const completed = { ...initial, revision: 5, phase: "completed" as const };
    expect(acceptFileSnapshot(completed, initial)).toBe(completed);
    expect(mergeFileEvent(completed, { revision: 4, phase: "transcribing", progress: { chunk: 1, chunks: 2, percent: 0 } })).toBe(completed);
    expect(mergeFileEvent(initial, { revision: 3, phase: "transcribing", progress: null }).phase).toBe("transcribing");
    expect(acceptFileSnapshot(initial, completed)).toBe(completed);
  });
  it("holds the worker slot until cancellation has finished", () => {
    for (const phase of ["decoding", "transcribing", "cancelling"] as const) expect(fileRunning(phase)).toBe(true);
    for (const phase of ["idle", "completed", "cancelled", "failed"] as const) expect(fileRunning(phase)).toBe(false);
  });
  it("clears the previous result and error when a newer job event beats its snapshot", () => {
    const previous: FileState = { ...initial, phase: "completed",
      result: { text: "Previous file", segments: [], timestamps_available: false, duration_ms: 1000, cleanup_failed: false },
      error: { code: "file_worker", message: "", kind: null } };
    for (const phase of ["transcribing", "completed", "failed", "cancelled"] as const) {
      const next = mergeFileEvent(previous, { revision: 3, phase, progress: null });
      expect(next.result).toBeNull();
      expect(next.error).toBeNull();
    }
  });
});
