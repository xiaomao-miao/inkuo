// Helpers + utility hooks shared across WordToolbar sub-components.
//
// Kept dependency-free so other parts of the editor (e.g. the Excel
// toolbar, future WordToolbars in other contexts) can reuse the same
// ProseMirror-safe dispatch path without pulling in all of WordToolbar's
// icon code.

import { useEffect, useLayoutEffect, useState } from 'react';
import type { EditorView } from 'prosemirror-view';

export interface DropdownPortalLayout {
  top: number;
  left: number;
  width: number;
  placement: 'bottom' | 'top';
}

/**
 * Returns true only if the view is alive AND has a valid state to dispatch
 * against. ProseMirror nulls `view.state` during teardown, so a stale `view`
 * ref captured by a click handler can survive a tab switch / file reload and
 * cause `Cannot read properties of undefined (reading 'schema')` deep inside
 * `chainCommands`. Treat that as "no view" rather than passing it on.
 */
export function isViewReady(view: EditorView | null): view is EditorView {
  return !!view && !!view.state && !!view.state.schema;
}

/**
 * Belt-and-suspenders dispatcher: every command goes through this even if the
 * caller already checked `isViewReady`, because the prosemirror command runner
 * destructures `state.schema` at the top of many commands (e.g. `insertPageBreak`
 * from `@eigenpal/docx-editor-core`). If `state` is undefined that throws an
 * uncatchable TypeError out of the click handler. We swallow teardown-race
 * crashes instead of unmounting the whole React tree (and the Tauri window).
 *
 * The parameters are typed as `any` so that callers can pass either a
 * ProseMirror `Command` (typed by prosemirror-state) or a loose function
 * without `as unknown as ...` casts at every dispatch site.
 */
export function runCommand(view: EditorView | null, command: any): void {
  if (!isViewReady(view)) return;
  try {
    command(view.state, view.dispatch, view);
  } catch (err) {
    if (import.meta.env?.DEV) {
      console.warn('[WordToolbar] command dispatch ignored:', err);
    }
  }
}

export function hpToPt(hp: unknown): number | null {
  if (hp == null) return null;
  const n = typeof hp === 'number' ? hp : Number(hp);
  if (!Number.isFinite(n)) return null;
  return Math.round(n / 2);
}

export function rgbToHex(rgb: unknown): string | null {
  if (!rgb) return null;
  const s = String(rgb);
  return s.startsWith('#') ? s : `#${s}`;
}

/**
 * Compute the fixed-position coordinates for a dropdown menu anchored to a
 * trigger element. Works regardless of any `overflow: hidden` / `contain`
 * ancestors the trigger sits inside, because the menu itself is rendered
 * into a portal at `document.body` (see `DropdownPortal`).
 */
export function useDropdownPosition(
  triggerRef: React.RefObject<HTMLElement | null>,
  open: boolean,
): DropdownPortalLayout | null {
  const [layout, setLayout] = useState<DropdownPortalLayout | null>(null);

  useLayoutEffect(() => {
    if (!open) {
      setLayout(null);
      return;
    }
    const compute = () => {
      const el = triggerRef.current;
      if (!el) return;
      const rect = el.getBoundingClientRect();
      const GAP = 2;
      const MARGIN = 8;
      const MIN_BELOW = 160; // heuristic: prefer flipping up if below space is tiny
      const viewportH = window.innerHeight;
      const spaceBelow = viewportH - rect.bottom - MARGIN;
      const spaceAbove = rect.top - MARGIN;
      const placement: 'bottom' | 'top' =
        spaceBelow >= MIN_BELOW || spaceBelow >= spaceAbove ? 'bottom' : 'top';
      setLayout({
        top: placement === 'bottom' ? rect.bottom + GAP : rect.top - GAP,
        left: rect.left,
        width: rect.width,
        placement,
      });
    };
    compute();
    window.addEventListener('resize', compute);
    window.addEventListener('scroll', compute, true);
    return () => {
      window.removeEventListener('resize', compute);
      window.removeEventListener('scroll', compute, true);
    };
  }, [open, triggerRef]);

  return layout;
}

/**
 * Re-apply the upward translate whenever the menu mounts or `placement`
 * changes (e.g. after a resize that pushes the trigger close to the bottom
 * of the viewport and flips the menu to open upward). Without this,
 * the ref callback alone would only run on initial mount.
 */
export function usePlacementTransform(
  menuRef: React.RefObject<HTMLDivElement | null>,
  layout: DropdownPortalLayout | null,
  open: boolean,
): void {
  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el || !layout) return;
    if (layout.placement === 'top') {
      el.style.transform = `translateY(-${el.offsetHeight}px)`;
    } else {
      el.style.transform = '';
    }
  }, [layout, open, menuRef]);
}

/**
 * Subscribe to Escape key globally while `open` is true. Returns nothing —
 * use as a side-effect-only hook so we don't have to plumb a `useEffect`
 * everywhere.
 */
export function useEscapeToClose(open: boolean, onClose: () => void): void {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [open, onClose]);
}