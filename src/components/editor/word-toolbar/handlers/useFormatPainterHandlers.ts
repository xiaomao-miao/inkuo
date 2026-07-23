// Subset of `useWordToolbarHandlers` for the format-painter state
// machine.
//
// The toolbar's format-painter works in two phases:
//
//   1. **Armed** — user clicks the brush, the selection's anchor
//      marks are stashed into `paintedMarks`, and a global ESC
//      listener is registered. The toolbar highlights the brush to
//      signal the armed state.
//   2. **Apply** — user makes a new selection; clicking the brush
//      again (or clicking into a fresh selection and clicking it)
//      applies the stashed marks to the new range and disarms.
//
// The state machine itself (the click handler) lives here, alongside
// the global ESC effect that disarms on cancel. Selection walking
// is plain ProseMirror — no helper needed.

import { useCallback, useEffect, useState } from 'react';
import type { EditorView } from 'prosemirror-view';
import type { Mark } from 'prosemirror-model';

import { isViewReady } from '../helpers';

export interface FormatPainterState {
  /** Marks captured from the previous selection, or null when disarmed. */
  paintedMarks: readonly Mark[] | null;
  /** Toggle armed / apply. */
  handleFormatPainter: () => void;
  /** Imperative setter — exposed so the parent can clear on certain events. */
  setPaintedMarks: (next: readonly Mark[] | null) => void;
}

export function useFormatPainterHandlers(view: EditorView | null): FormatPainterState {
  const [paintedMarks, setPaintedMarks] = useState<readonly Mark[] | null>(null);

  // Apply captured marks to `from..to` in the current document.
  const applyMarks = useCallback(
    (from: number, to: number) => {
      if (!view || !paintedMarks || paintedMarks.length === 0) return;
      const tr = view.state.tr;
      const targetTypes = new Set(paintedMarks.map((m) => m.type));
      tr.removeMark(from, to, ...targetTypes);
      for (const mark of paintedMarks) {
        tr.addMark(from, to, mark);
      }
      view.dispatch(tr);
      view.focus();
    },
    [view, paintedMarks],
  );

  const handleFormatPainter = useCallback(() => {
    if (!isViewReady(view)) return;
    // Already armed → apply the captured marks to the current selection.
    if (paintedMarks) {
      applyMarks(view.state.selection.from, view.state.selection.to);
      setPaintedMarks(null);
      return;
    }
    // Disarmed → capture marks from the current selection's anchor.
    const $from = view.state.selection.$from;
    const marks: readonly Mark[] = $from.marks();
    if (marks.length === 0) {
      // No marks on the cursor; fall back to the first run of the
      // current paragraph (matches Word's behavior when the cursor
      // is at the start of a formatted run).
      const parent = $from.parent;
      const firstRun = parent.childAfter(0);
      if (firstRun.node && firstRun.node.marks.length > 0) {
        setPaintedMarks(firstRun.node.marks);
        view.focus();
        return;
      }
      // Nothing to copy — caller will be told via notify at the
      // composer layer, but we don't have notify here. The hook
      // returns without arming so a downstream caller can show a
      // toast; for now we silently no-op.
      return;
    }
    setPaintedMarks(marks);
    view.focus();
  }, [view, paintedMarks, applyMarks]);

  // Cancel format painter on Escape while armed.
  useEffect(() => {
    if (!paintedMarks) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        setPaintedMarks(null);
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [paintedMarks]);

  return { paintedMarks, handleFormatPainter, setPaintedMarks };
}