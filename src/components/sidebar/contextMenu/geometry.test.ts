// Unit tests for the `clampToViewport` viewport-clamp helper.

import { describe, expect, it } from 'vitest';

import { clampToViewport, type Viewport } from './geometry';

describe('clampToViewport', () => {
  // The helper no longer reads `window` directly; pass an explicit
  // viewport so tests don't need a DOM.
  const viewport: Viewport = { width: 1024, height: 768 };

  function makeMenu(width: number, height: number): HTMLElement {
    return {
      getBoundingClientRect: () =>
        ({
          width,
          height,
          top: 0,
          left: 0,
          right: width,
          bottom: height,
          x: 0,
          y: 0,
          toJSON: () => ({}),
        }) as DOMRect,
    } as unknown as HTMLElement;
  }

  it('returns the input coordinates unchanged when menu is null', () => {
    expect(clampToViewport(100, 200, null, viewport)).toEqual({ left: 100, top: 200 });
  });

  it('returns the input coordinates unchanged when both fit inside the viewport', () => {
    const menu = makeMenu(200, 200);
    expect(clampToViewport(10, 10, menu, viewport)).toEqual({ left: 10, top: 10 });
  });

  it('clamps the left edge to the configured margin', () => {
    const menu = makeMenu(200, 200);
    expect(clampToViewport(-50, 100, menu, viewport)).toEqual({ left: 4, top: 100 });
  });

  it('clamps the right edge to viewport - rect.width - margin', () => {
    // Menu: 200 wide, viewport 1024, so right-most left = 1024 - 200 - 4 = 820
    const menu = makeMenu(200, 200);
    expect(clampToViewport(2000, 100, menu, viewport)).toEqual({ left: 820, top: 100 });
  });

  it('clamps the top edge to the configured margin', () => {
    const menu = makeMenu(200, 200);
    expect(clampToViewport(100, -50, menu, viewport)).toEqual({ left: 100, top: 4 });
  });

  it('clamps the bottom edge to viewport - rect.height - margin', () => {
    // Menu: 200 tall, viewport 768, so right-most top = 768 - 200 - 4 = 564
    const menu = makeMenu(200, 200);
    expect(clampToViewport(100, 2000, menu, viewport)).toEqual({ left: 100, top: 564 });
  });

  it('handles smaller-than-margin viewports gracefully', () => {
    const tiny: Viewport = { width: 50, height: 50 };
    const menu = makeMenu(200, 200);
    // Min right edge must be at least margin (4). viewport - width - margin
    // is negative; the helper clamps to max(margin, that) → still margin.
    const result = clampToViewport(100, 100, menu, tiny);
    expect(result.left).toBe(4);
    expect(result.top).toBe(4);
  });

  it('does not mutate the input integers', () => {
    const menu = makeMenu(200, 200);
    expect(clampToViewport(10, 10, menu, viewport)).toEqual({ left: 10, top: 10 });
  });
});
