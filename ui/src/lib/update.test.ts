import { describe, expect, it } from "vitest";
import { bannerVersion, dictationBusy, progressPercent, statusMessage } from "./update";
import type { DictationState, UpdateInfo, UpdateState } from "./types";

const info: UpdateInfo = { version: "0.2.0", current_version: "0.1.0", notes: null };

describe("bannerVersion", () => {
  it("announces an available update until that version is dismissed", () => {
    expect(bannerVersion({ kind: "available", info }, null)).toBe("0.2.0");
    expect(bannerVersion({ kind: "available", info }, "0.2.0")).toBeNull();
    expect(bannerVersion({ kind: "available", info }, "0.1.5")).toBe("0.2.0");
  });
  it("stays quiet for every other state, failed checks included", () => {
    const quiet: UpdateState[] = [
      { kind: "idle" },
      { kind: "checking" },
      { kind: "up_to_date" },
      { kind: "check_failed", message: "Could not fetch a valid release JSON from the remote" },
      { kind: "installing", info, downloaded: 1, total: 2 },
    ];
    for (const state of quiet) expect(bannerVersion(state, null), state.kind).toBeNull();
  });
});

describe("progressPercent", () => {
  it("floors and clamps", () => {
    expect(progressPercent(0, 200)).toBe(0);
    expect(progressPercent(199, 200)).toBe(99);
    expect(progressPercent(200, 200)).toBe(100);
    expect(progressPercent(250, 200)).toBe(100);
  });
  it("is null while the size is unknown", () => {
    expect(progressPercent(10, null)).toBeNull();
    expect(progressPercent(10, 0)).toBeNull();
  });
});

describe("statusMessage", () => {
  it("maps every state to a message with its values", () => {
    expect(statusMessage({ kind: "idle" })).toEqual({ key: "update.status_idle" });
    expect(statusMessage({ kind: "checking" })).toEqual({ key: "update.status_checking" });
    expect(statusMessage({ kind: "up_to_date" })).toEqual({ key: "update.status_up_to_date" });
    expect(statusMessage({ kind: "available", info })).toEqual({
      key: "update.status_available",
      params: { version: "0.2.0", current: "0.1.0" },
    });
    expect(statusMessage({ kind: "installing", info, downloaded: 0, total: null })).toEqual({
      key: "update.status_installing",
      params: { version: "0.2.0" },
    });
    expect(statusMessage({ kind: "check_failed", message: "offline" })).toEqual({
      key: "update.status_failed",
      params: { message: "offline" },
    });
  });
});

describe("dictationBusy", () => {
  it("is true only while a dictation is in flight", () => {
    const busy: DictationState[] = ["recording", "transcribing", "pasting"];
    const free: DictationState[] = ["idle", "cancelled", "error"];
    for (const s of busy) expect(dictationBusy(s), s).toBe(true);
    for (const s of free) expect(dictationBusy(s), s).toBe(false);
  });
});
