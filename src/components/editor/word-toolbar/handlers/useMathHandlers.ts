// Subset of `useWordToolbarHandlers` for the math popover.
//
// The math popover captures LaTeX and inserts it as `$$<latex>$$` at
// the current selection. The confirm path wraps the inserted text in
// display-mode dollar signs — the editor core's renderer recognizes
// that as a math block at save time.

import { useCallback } from 'react';
import type { EditorView } from 'prosemirror-view';

import { isViewReady } from '../helpers';

export interface MathHandlers {
  handleInsertMath: () => void;
  handleMathConfirm: (latex: string) => void;
}

export interface MathDeps {
  view: EditorView | null;
  openMath: () => void;
  closeMath: () => void;
}

export function useMathHandlers({ view, openMath, closeMath }: MathDeps): MathHandlers {
  const handleInsertMath = useCallback(() => {
    if (!isViewReady(view)) return;
    openMath();
  }, [view, openMath]);

  const handleMathConfirm = useCallback(
    (latex: string) => {
      closeMath();
      if (!isViewReady(view)) return;
      const { from, to } = view.state.selection;
      view.dispatch(view.state.tr.insertText(`$$${latex}$$`, from, to));
      view.focus();
    },
    [view, closeMath],
  );

  return { handleInsertMath, handleMathConfirm };
}