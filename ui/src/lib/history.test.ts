import { expect, it } from "vitest";
import { refreshHistory } from "./history";
import type { Dictation } from "./types";

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
