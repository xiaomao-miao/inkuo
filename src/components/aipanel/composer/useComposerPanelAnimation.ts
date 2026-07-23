// Panel-height animation hook for the AI-panel composer's toggle tray.
//
// Why this exists:
//
// The composer grows and shrinks in place when the user toggles the
// feature toolbar — no popovers, no overlays. Driving that with
// `max-height` / `grid-template-rows` transitions forces a layout
// recalc on every animation frame; lower-end hardware reads as a
// stuttery open. We pin `height` to the measured pixel value once
// (in `useLayoutEffect` so the user never sees the old height) and
// let the compositor handle the actual transition.
//
// Trade-offs:
//   - Expand → measure `scrollHeight`, pin height to it. CSS
//     transition runs on a fixed px value.
//   - Collapse → pin the current pixel height first (so the
//     transition has a start value), then set it to 0. After the
//     transition ends we clear the inline height so the panel
//     returns to its natural flow.
//
// Why we *also* race a 260ms timeout against `transitionend`:
// WebView2 occasionally coalesces `transitionend` for `height`, or
// the value never starts a real transition because the start / end
// heights are equal. The timer guarantees the settle callback runs
// regardless.
//
// Reduced-motion shortcut:
// When the user (or OS) has opted out of motion, the global
// `prefers-reduced-motion: reduce` rule forces `transition-duration:
// 0ms` — which means the `transitionend` listener never fires and
// `el.style.height` would stay pinned forever. In that branch we
// skip the pin / RAF dance entirely and just write the target
// height synchronously. The cleanup function also clears the inline
// height unconditionally so the panel can always collapse even if
// React commits a new `expanded` value before the listener fires.

import { useLayoutEffect, useRef, type RefObject } from 'react';

import { getEffectiveMotionLevel } from '../../../hooks/useMotionLevel';

/** Hard fallback for the transition-end listener race. */
const TRANSITION_FALLBACK_MS = 260;

export function useComposerPanelAnimation(
  panelRef: RefObject<HTMLElement | null>,
  expanded: boolean,
): void {
  // Persists the measured panel height across effect runs — needed
  // because the old effect's cleanup clears `el.style.height`
  // *before* the new effect body reads `getBoundingClientRect()`.
  // Without this ref, a collapse → expand cycle would see height=0
  // (CSS already collapsed it) and the expand animation would
  // have no height to restore from.
  const measuredHeightRef = useRef<number | null>(null);

  // Token incremented on every effect run; used to ignore stale
  // settle callbacks after the cleanup fires (otherwise the old
  // RAF / timer could clear the new inline height set by the next
  // effect run).
  const effectTokenRef = useRef(0);

  useLayoutEffect(() => {
    const el = panelRef.current;
    if (!el) return;

    const motion = getEffectiveMotionLevel();
    const animate = motion !== 'off';

    const token = ++effectTokenRef.current;

    if (!animate) {
      // Snap straight to the final height and leave it inline —
      // clearing it in the same task as the write has no effect
      // (the second write wins), and the CSS rule
      // `.togglePanel { height: 0 }` would otherwise lock the panel
      // shut while `data-open=true` only flips opacity/transform.
      el.style.height = expanded ? `${el.scrollHeight}px` : '0px';
      return;
    }

    let rafId = 0;
    let fallbackTimer = 0;

    const isStale = () => effectTokenRef.current !== token;

    const scheduleHeightSettle = (done: () => void) => {
      // Race `transitionend` against a hard timeout — see the
      // reduced-motion note above for why we can't rely on the
      // event alone (some WebView2 builds coalesce or skip the
      // event entirely when the start / end values are very close,
      // or the transition is suppressed by a higher-specificity
      // rule).
      let finished = false;
      const finish = () => {
        if (finished || isStale()) return;
        finished = true;
        window.clearTimeout(fallbackTimer);
        el.removeEventListener('transitionend', onEnd);
        done();
      };
      fallbackTimer = window.setTimeout(finish, TRANSITION_FALLBACK_MS);
      const onEnd = (e: TransitionEvent) => {
        if (e.propertyName !== 'height') return;
        finish();
      };
      el.addEventListener('transitionend', onEnd);
    };

    if (expanded) {
      // Measure the natural height of the children.
      const target = el.scrollHeight;
      // Persist so the collapse branch can read it even if the old
      // effect's cleanup already cleared the inline style.
      measuredHeightRef.current = target;
      // If the panel was previously collapsed, jump straight to the
      // target (the CSS opacity/transform handles the visual entry).
      el.style.transition = 'none';
      el.style.height = `${target}px`;
      // Force a frame so the browser commits the height before we
      // re-enable the transition for the (now no-op) settle.
      rafId = window.requestAnimationFrame(() => {
        if (isStale()) return;
        el.style.transition = '';
        scheduleHeightSettle(() => {
          if (isStale()) return;
          el.style.height = '';
        });
      });
    } else {
      // Pin current height first so the transition has a meaningful
      // start value (transitioning from '' to '0px' would otherwise
      // skip straight to zero on most browsers).
      const current =
        measuredHeightRef.current ?? el.getBoundingClientRect().height;
      measuredHeightRef.current = null;
      el.style.transition = 'none';
      el.style.height = `${current}px`;
      rafId = window.requestAnimationFrame(() => {
        if (isStale()) return;
        el.style.transition = '';
        el.style.height = '0px';
        scheduleHeightSettle(() => {
          if (isStale()) return;
          el.style.height = '';
        });
      });
    }

    return () => {
      window.cancelAnimationFrame(rafId);
      // NOTE: we intentionally do NOT clear `el.style.height`
      // here. The settle callback is what clears the inline style
      // once the animation is genuinely done.
    };
  }, [expanded, panelRef]);
}