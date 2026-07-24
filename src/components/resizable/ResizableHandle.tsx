import React, { useCallback, useEffect, useRef } from 'react';
import styles from './ResizableHandle.module.css';

interface ResizableHandleProps {
  direction: 'horizontal' | 'vertical';
  /** Fired once per animation frame while the user drags. Receives the
   *  delta (in CSS pixels) from the previous fired tick — NOT the raw
   *  `pointermove` delta. Callers are expected to apply the delta to
   *  whatever they own (CSS variables, refs, store state) without
   *  forcing additional React renders. */
  onResize: (delta: number) => void;
  /** Fired once on pointerdown. Lets callers capture a baseline (e.g.
   *  read the current `--sidebar-width` so the per-frame delta is
   *  applied correctly even if the value has drifted). */
  onResizeStart?: () => void;
  /** Fired once on pointerup / pointercancel with the final measured
   *  dimension of the affected panel. Callers use this to commit the
   *  value into persistent state (Zustand store, localStorage, etc.). */
  onResizeEnd?: () => void;
}

/**
 * Pointer-event based resize handle.
 *
 * Performance notes
 * -----------------
 * High-frequency `pointermove` events fire at the OS pointer-polling
 * rate (typically 120–1000 Hz on modern hardware). Naively forwarding
 * every event to a React state setter causes the whole `Layout` subtree
 * to re-render on each tick, which on large docx files cascades into
 * the DocxEditor's internal `ResizeObserver` chain and makes dragging
 * feel laggy.
 *
 * Two mitigations:
 *   1. The per-frame work is throttled with `requestAnimationFrame` —
 *      at most one `onResize(delta)` call per browser frame (~60 Hz),
 *      regardless of how many `pointermove` events were coalesced.
 *      The delta passed in is the *cumulative* delta since the last
 *      fired tick, not the per-event delta, so callers can apply it
 *      to the CSS variable directly with `+= delta`.
 *   2. The DOM mutation itself is the caller's responsibility (see
 *      `Layout.tsx`). The handle does NOT touch React state during a
 *      drag; it only emits rAF-throttled deltas.
 */
export const ResizableHandle = ({
  direction,
  onResize,
  onResizeStart,
  onResizeEnd,
}: ResizableHandleProps) => {
  const activePointerId = useRef<number | null>(null);
  const startPos = useRef(0);
  const lastPos = useRef(0);
  const pendingDelta = useRef(0);
  const rafId = useRef<number | null>(null);

  // Cancel any pending rAF on unmount so we don't dispatch a stray
  // `onResize` after the handle has been removed.
  useEffect(() => () => {
    if (rafId.current !== null) {
      cancelAnimationFrame(rafId.current);
      rafId.current = null;
    }
  }, []);

  const flush = useCallback(() => {
    rafId.current = null;
    if (pendingDelta.current !== 0) {
      const delta = pendingDelta.current;
      pendingDelta.current = 0;
      onResize(delta);
    }
  }, [onResize]);

  const stopDragging = useCallback((target?: HTMLElement | null) => {
    if (target && activePointerId.current !== null) {
      target.releasePointerCapture(activePointerId.current);
    }
    // Flush any pending delta before notifying the caller that the drag
    // is over, so the final DOM state and the committed-on-release
    // state are in lock-step.
    if (pendingDelta.current !== 0) {
      const delta = pendingDelta.current;
      pendingDelta.current = 0;
      onResize(delta);
    }
    if (rafId.current !== null) {
      cancelAnimationFrame(rafId.current);
      rafId.current = null;
    }
    activePointerId.current = null;
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
    onResizeEnd?.();
  }, [onResize, onResizeEnd]);

  const handlePointerMove = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (activePointerId.current !== event.pointerId) return;

    const currentPos = direction === 'horizontal' ? event.clientX : event.clientY;
    pendingDelta.current += currentPos - lastPos.current;
    lastPos.current = currentPos;

    if (rafId.current === null) {
      rafId.current = requestAnimationFrame(flush);
    }
  }, [direction, flush]);

  const handlePointerDown = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    activePointerId.current = event.pointerId;
    const pos = direction === 'horizontal' ? event.clientX : event.clientY;
    startPos.current = pos;
    lastPos.current = pos;
    pendingDelta.current = 0;
    event.currentTarget.setPointerCapture(event.pointerId);
    document.body.style.cursor = direction === 'horizontal' ? 'ew-resize' : 'ns-resize';
    document.body.style.userSelect = 'none';
    onResizeStart?.();
  }, [direction, onResizeStart]);

  const handlePointerUp = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (activePointerId.current !== event.pointerId) return;
    stopDragging(event.currentTarget);
  }, [stopDragging]);

  const handlePointerCancel = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (activePointerId.current !== event.pointerId) return;
    stopDragging(event.currentTarget);
  }, [stopDragging]);

  return (
    <div
      className={`${styles.handle} ${direction === 'horizontal' ? styles.horizontal : styles.vertical}`}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerCancel}
    >
      <div className={styles.line} />
    </div>
  );
};
