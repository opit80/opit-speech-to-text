import { describe, expect, it } from "vitest";
import { placeTooltip } from "./tooltip";

const viewport = { width: 800, height: 600 };
const anchor = (left: number, top: number, width = 100, height = 20) => ({ left, top, right: left + width, bottom: top + height });

describe("placeTooltip", () => {
  it("opens below the anchor, aligned to its left edge", () => {
    expect(placeTooltip(anchor(100, 100), { width: 200, height: 50 }, viewport)).toEqual({ left: 100, top: 126, side: "below" });
  });

  it("keeps the tooltip inside the right and left edges", () => {
    expect(placeTooltip(anchor(700, 100), { width: 200, height: 50 }, viewport).left).toBe(592);
    expect(placeTooltip(anchor(2, 100), { width: 200, height: 50 }, viewport).left).toBe(8);
  });

  it("flips above when there is no room below but room above", () => {
    expect(placeTooltip(anchor(100, 540), { width: 200, height: 50 }, viewport)).toEqual({ left: 100, top: 484, side: "above" });
  });

  it("stays below when neither side fits, clamped to the viewport", () => {
    const placed = placeTooltip(anchor(100, 300), { width: 200, height: 580 }, viewport);
    expect(placed.side).toBe("below");
    expect(placed.top).toBe(12);
  });

  it("never places a tooltip wider than the viewport off its left edge", () => {
    expect(placeTooltip(anchor(100, 100), { width: 900, height: 50 }, viewport).left).toBe(8);
  });
});
