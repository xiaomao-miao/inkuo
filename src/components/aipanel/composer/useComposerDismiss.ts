// Dismiss behavior for the AI-panel composer's toggle tray.
//
// Three signals can close the toolbar while it's expanded:
//
//   (1) Mouse click outside the composer card — but clicks *inside*
//       the toggle panel (the user is interacting with rows) and
//       clicks inside the rest of the composer (textarea, expand
//       button) are intentional and should NOT close.
//
//   (2) Keyboard focus leaves the composer entirely — listening on
//       `focusout` rather than `blur` so we catch moves between
//       elements in the document. Microtask debounce so
//       focus-then-click sequences settle cleanly.
//
//   (3) Escape closes the panel and returns focus to the textarea —
//       the canonical "dismiss an overlay without leaving the
//       composer" pattern. ESC during IME composition is ignored so
//       CJK input isn't disrupted.
//
// The mouse listener is deferred by one tick (`setTimeout(…, 0)`)
// because the click that *opened* the panel fires a `mousedown` in
// the same gesture — without the defer, the panel would open and
// immediately close on the same event.

import { useEffect, type RefObject } from 'react';

import { useAIPanelStore } from '../../../store';

/** Selector for the composer root (used for the "click outside" check). */
const COMPOSER_ROOT_SELECTOR = '[data-composer-root]';

export function useComposerDismiss(
  panelRef: RefObject<HTMLElement | null>,
  expanded: boolean,
): void {
  useEffect(() => {
    if (!expanded) return;

    // (1) Mouse click outside the composer card.
    const onMouseDown = (e: MouseEvent) => {
      const target = e.target as HTMLElement | null;
      if (!target) return;
      // Clicks inside the toggle panel → user is interacting with
      // the toolbar, leave it open.
      if (panelRef.current?.contains(target)) return;
      // Clicks anywhere inside the composer (textarea, header
      // strip, expand button) but outside the toggle rows should
      // NOT close — the user is still actively editing.
      if (target.closest(COMPOSER_ROOT_SELECTOR)) return;
      useAIPanelStore.getState().setFeatureToolbarExpanded(false);
    };

    // (2) Keyboard focus leaves the composer entirely.
    const onFocusOut = (e: FocusEvent) => {
      const next = e.relatedTarget as Node | null;
      // Focus is moving to something inside the composer → stay open.
      if (next && next instanceof Element && next.closest(COMPOSER_ROOT_SELECTOR)) {
        return;
      }
      // Focus is moving to an element inside the toggle panel → stay open.
      if (next && panelRef.current?.contains(next)) return;
      // Focus moved to nothing (window/tab switch) or to something
      // outside the composer → collapse. We use a microtask so that
      // focus-then-click sequences settle cleanly.
      queueMicrotask(() => {
        if (!useAIPanelStore.getState().featureToolbarExpanded) return;
        useAIPanelStore.getState().setFeatureToolbarExpanded(false);
      });
    };

    // (3) Escape closes the panel and returns focus to the textarea.
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      // Don't intercept Escape if the user is mid-composition (IME).
      // `isComposing` is true during IME pre-edit on Chromium/WebKit.
      if (e.isComposing || e.keyCode === 229) return;
      e.preventDefault();
      useAIPanelStore.getState().setFeatureToolbarExpanded(false);
      // Best-effort focus restoration; if the textarea isn't in the
      // document yet, we silently skip.
      const ta = document.querySelector<HTMLTextAreaElement>(
        `${COMPOSER_ROOT_SELECTOR} textarea`,
      );
      ta?.focus();
    };

    // Defer the mouse listener by a tick so the click that opened
    // the panel doesn't immediately re-close it via the same
    // mousedown.
    const mouseId = window.setTimeout(() => {
      document.addEventListener('mousedown', onMouseDown);
    }, 0);
    document.addEventListener('focusout', onFocusOut);
    document.addEventListener('keydown', onKeyDown);

    return () => {
      window.clearTimeout(mouseId);
      document.removeEventListener('mousedown', onMouseDown);
      document.removeEventListener('focusout', onFocusOut);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [expanded, panelRef]);
}