// Pure geometry helper for the floating context menu.
//
// Kept separate from the React component so the math is trivially
// unit-testable without a DOM / jsdom. Callers (the orchestrating
// `ContextMenu`) extract `window.innerWidth/innerHeight` once and pass
// them in.

import type { Position } from './types';

/** Pixels of margin to keep between the menu and the viewport edge. */
export const VIEWPORT_MARGIN_PX = 4;

/** Inputs to `clampToViewport` — derived from the DOM at the call site. */
export interface Viewport {
  width: number;
  height: number;
}

/**
 * Clamp an (x, y) pair so the rendered menu stays inside the viewport.
 * `menu` is the menu element after first paint — we need its size to
 * know how much room is available. Returns the original coordinates
 * unchanged when `menu` is unavailable (e.g. SSR / hidden).
 */
export function clampToViewport(
  x: number,
  y: number,
  menu: HTMLElement | null,
  viewport: Viewport = defaultViewport(),
): Position {
  if (!menu) return { left: x, top: y };
  const rect = menu.getBoundingClientRect();
  const maxLeft = Math.max(
    VIEWPORT_MARGIN_PX,
    viewport.width - rect.width - VIEWPORT_MARGIN_PX,
  );
  const maxTop = Math.max(
    VIEWPORT_MARGIN_PX,
    viewport.height - rect.height - VIEWPORT_MARGIN_PX,
  );
  const left = Math.min(Math.max(VIEWPORT_MARGIN_PX, x), maxLeft);
  const top = Math.min(Math.max(VIEWPORT_MARGIN_PX, y), maxTop);
  return { left, top };
}

/** Read the current viewport from `window`, with a safe fallback. */
function defaultViewport(): Viewport {
  if (typeof window === 'undefined') return { width: 1024, height: 768 };
  return { width: window.innerWidth, height: window.innerHeight };
}
