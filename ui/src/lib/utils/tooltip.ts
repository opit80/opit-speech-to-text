export interface AnchorRect { left: number; top: number; right: number; bottom: number; }
export interface BoxSize { width: number; height: number; }
export interface TooltipPlacement { left: number; top: number; side: "below" | "above"; }

/**
 * Fixed-position coordinates for a tooltip next to `anchor`: below it, aligned to its left edge,
 * flipped above only when it does not fit below but fits above, and always kept `margin` pixels
 * inside the viewport (the left edge wins when the tooltip is wider than the viewport).
 */
export function placeTooltip(anchor: AnchorRect, tip: BoxSize, viewport: BoxSize, gap = 6, margin = 8): TooltipPlacement {
  const below = anchor.bottom + gap;
  const above = anchor.top - gap - tip.height;
  const fitsBelow = below + tip.height <= viewport.height - margin;
  const side = !fitsBelow && above >= margin ? "above" : "below";
  const top = side === "above" ? above : Math.max(margin, Math.min(below, viewport.height - margin - tip.height));
  const left = Math.max(margin, Math.min(anchor.left, viewport.width - margin - tip.width));
  return { left, top, side };
}
