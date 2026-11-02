import { expect, it } from "vitest";
import { HISTORY_PAGE, refreshHistory, refreshWindow, sameList } from "./history";
import type { Dictation } from "../../lib/bridge/types";

const row = (id: number) => ({ id, text: `row ${id}`, audio_path: `${id}.wav` }) as Dictation;

it("a cap-triggered live refresh removes a pruned row instead of merging it", async () => {
  let stored = [row(1)];
  const recent = async () => stored;
  const initial = await refreshHistory(recent, 0);
  expect(initial.rows.map(r => r.id)).toEqual([1]);
  // Recording B with a cap of one atomically removes A from storage.
  stored = [row(2)];
  const live = await refreshHistory(recent, initial.rows.length);
  expect(live.rows.map(r => r.id)).toEqual([2]);
  expect(live.rows.some(r => r.audio_path === "1.wav")).toBe(false);
  expect(live.hasMore).toBe(false);
});

it("a live refresh during Load more also reads the page whose response it makes stale", async () => {
  const stored = Array.from({ length: 120 }, (_, i) => row(120 - i));
  const recent = async (limit: number, before: number | null) =>
    stored.filter(r => before === null || r.id < before).slice(0, limit);
  const visible = (await refreshHistory(recent, 0)).rows;
  expect(visible).toHaveLength(HISTORY_PAGE);
  // Load more starts for the list of request 1; a new dictation's refresh is request 2.
  const started = { epoch: 1, request: 1 };
  stored.unshift(row(121));
  const refreshed = await refreshHistory(recent, refreshWindow(visible.length, true));
  expect(sameList(started, { epoch: 1, request: 2 })).toBe(false);
  // The discarded page's rows still appear, together with the new dictation.
  expect(refreshed.rows.map(r => r.id)).toEqual(stored.slice(0, 2 * HISTORY_PAGE).map(r => r.id));
  expect(refreshed.rows[0].id).toBe(121);
  expect(refreshed.hasMore).toBe(true);
  expect(refreshWindow(visible.length, false)).toBe(visible.length);
});

it("a Load more response or failure applies only to the list it was requested for", () => {
  expect(sameList({ epoch: 3, request: 7 }, { epoch: 3, request: 7 })).toBe(true);
  expect(sameList({ epoch: 3, request: 7 }, { epoch: 3, request: 8 })).toBe(false);
  expect(sameList({ epoch: 3, request: 7 }, { epoch: 4, request: 7 })).toBe(false);
});

it("preserves the loaded window across command-sized pages and handles an exhausted cap", async () => {
  const stored = Array.from({ length: 620 }, (_, i) => row(620 - i));
  const recent = async (limit: number, before: number | null) =>
    stored.filter(r => before === null || r.id < before).slice(0, limit);
  const live = await refreshHistory(recent, 550);
  expect(live.rows).toEqual(stored.slice(0, 550));
  expect(live.hasMore).toBe(true);
  const exhausted = await refreshHistory(recent, 650);
  expect(exhausted.rows).toEqual(stored);
  expect(exhausted.hasMore).toBe(false);
});
