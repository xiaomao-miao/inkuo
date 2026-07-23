// Subset of `useWordToolbarHandlers` for the misc toolbar toggles:
//   - `sortSelection` — sort the selected paragraphs asc / desc
//   - `handleToggleSpellCheck` — toggle the browser spell-check attr
//   - `handleShowFormattingMarks` — toggle the formatting-marks CSS class
//
// The first reads the document via ProseMirror and dispatches a
// transaction; the latter two mutate DOM attributes / classes on the
// editor root. None of them need the popover wiring so they share
// this single sub-hook.

import { useCallback, useState } from 'react';
import type { EditorView } from 'prosemirror-view';

import { isViewReady } from '../helpers';
import { collectSortableTextblocks, compareLinesAsc, compareLinesDesc, type SortableLine } from './selection';
import type { Notify } from './domMutations';

/** Selector for the Word editor's root container. */
const WORD_EDITOR_ROOT_SELECTOR = '[data-office-editor-root="word"]';

export interface MiscHandlers {
  sortSelection: (direction: 'asc' | 'desc') => void;
  handleToggleSpellCheck: () => void;
  handleShowFormattingMarks: () => void;
}

export interface MiscDeps {
  view: EditorView | null;
  notify?: Notify;
}

export function useMiscHandlers({ view, notify }: MiscDeps): MiscHandlers {
  const [spellCheckOn, setSpellCheckOn] = useState(false);

  const sortSelection = useCallback(
    (direction: 'asc' | 'desc') => {
      if (!isViewReady(view)) return;
      const { state, dispatch } = view;
      const { from, to } = state.selection;
      if (from === to) {
        notify?.('info', '请先选择要排序的段落');
        return;
      }
      const collected: SortableLine[] = collectSortableTextblocks(view, from, to);
      if (collected.length < 2) {
        notify?.('info', '至少需要两段内容才能排序');
        return;
      }
      // Snapshot the marks of the first run of each paragraph so the
      // sorted replacement preserves the original formatting where
      // possible. The cursor position is the anchor for the new
      // selection's first run.
      const lines = collected.map((c) => {
        const firstRun = c.node.childAfter(0)?.node;
        const marks = firstRun && firstRun.isText ? firstRun.marks : [];
        return { ...c, marks };
      });
      const cmp = direction === 'asc' ? compareLinesAsc : compareLinesDesc;
      const sorted = [...lines].sort(cmp);

      const tr = state.tr;
      // Walk back-to-front so earlier positions stay valid after
      // the later deletes. The original toolbar does the same.
      for (let i = lines.length - 1; i >= 0; i -= 1) {
        const target = lines[i];
        const replacement = sorted[i];
        const fromInPara = target.start - target.pos;
        const toInPara = target.end - target.pos;
        if (fromInPara === toInPara) continue;
        tr.delete(target.start, target.end);
        const insertAt = target.start;
        tr.insertText(replacement.text, insertAt);
        if (replacement.marks.length > 0) {
          for (const mark of replacement.marks) {
            tr.addMark(insertAt, insertAt + replacement.text.length, mark);
          }
        }
      }
      dispatch(tr);
      view.focus();
    },
    [view, notify],
  );

  const handleToggleSpellCheck = useCallback(() => {
    const root = document.querySelector<HTMLElement>(WORD_EDITOR_ROOT_SELECTOR);
    if (!root) {
      notify?.('error', '找不到编辑器容器');
      return;
    }
    const next = !spellCheckOn;
    setSpellCheckOn(next);
    const editable = root.querySelectorAll<HTMLElement>(
      '[contenteditable="true"], .ProseMirror, [spellcheck]',
    );
    editable.forEach((el) => {
      el.setAttribute('spellcheck', next ? 'true' : 'false');
    });
    notify?.('info', next ? '已开启浏览器拼写检查' : '已关闭拼写检查');
  }, [spellCheckOn, notify]);

  const handleShowFormattingMarks = useCallback(() => {
    const el = document.querySelector(WORD_EDITOR_ROOT_SELECTOR);
    if (!el) return;
    el.classList.toggle('inkuo-show-formatting-marks');
  }, []);

  return { sortSelection, handleToggleSpellCheck, handleShowFormattingMarks };
}