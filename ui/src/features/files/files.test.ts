import { describe, expect, it } from "vitest";
import { acceptFileSnapshot, fileBlocksUpdate, fileRunning, mergeFileEvent, mergeFileWork } from "./files";
import type { FileState, FileWork } from "../../lib/bridge/types";

const initial: FileState = { revision: 2, phase: "decoding", name: "audio.wav", progress: null, result: null, error: null,
  options: { language: null, cleanup: false, speakers: false }, unsaved: false };

describe("file jobs", () => {
  it("keeps a newer completion when a start response or progress event arrives late", () => {
    const completed = { ...initial, revision: 5, phase: "completed" as const };
    expect(acceptFileSnapshot(completed, initial)).toBe(completed);
    expect(mergeFileEvent(completed, { revision: 4, phase: "transcribing", progress: { chunk: 1, chunks: 2, percent: 0 }, unsaved: false })).toBe(completed);
    expect(mergeFileEvent(initial, { revision: 3, phase: "transcribing", progress: null, unsaved: false }).phase).toBe("transcribing");
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
      const next = mergeFileEvent(previous, { revision: 3, phase, progress: null, unsaved: false });
      expect(next.result).toBeNull();
      expect(next.error).toBeNull();
    }
  });
  it("blocks updates while a job runs or its result is unsaved", () => {
    expect(fileBlocksUpdate({ revision: 1, phase: "transcribing", unsaved: false })).toBe(true);
    expect(fileBlocksUpdate({ revision: 2, phase: "completed", unsaved: true })).toBe(true);
    expect(fileBlocksUpdate({ revision: 2, phase: "completed", unsaved: false })).toBe(false);
    expect(fileBlocksUpdate({ revision: 3, phase: "idle", unsaved: false })).toBe(false);
  });
  it("tracks saved results without reviving stale snapshots", () => {
    const done: FileWork = { revision: 4, phase: "completed", unsaved: true };
    const saved = mergeFileWork(done, { revision: 4, phase: "completed", unsaved: false });
    expect(saved).toEqual({ revision: 4, phase: "completed", unsaved: false });
    // A snapshot taken before the save must not make the result unsaved again.
    expect(mergeFileWork(saved, { revision: 4, phase: "completed", unsaved: true })).toBe(saved);
    expect(mergeFileWork(done, { revision: 3, phase: "transcribing", unsaved: false })).toBe(done);
    expect(mergeFileWork(saved, { revision: 5, phase: "decoding", unsaved: false }).phase).toBe("decoding");
  });
});
