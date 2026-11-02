import { expect, it } from "vitest";
import { mergePendingResults } from "./pending-results";

it("a slow recovery snapshot cannot resurrect a dismissed result", () => {
  const newer = { revision: 4, results: [{ id: 2, text: "second" }] };
  const stale = { revision: 3, results: [{ id: 1, text: "first" }, { id: 2, text: "second" }] };
  expect(mergePendingResults(newer, stale)).toBe(newer);
  expect(mergePendingResults(stale, newer)).toBe(newer);
});
