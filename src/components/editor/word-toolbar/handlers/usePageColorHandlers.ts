// Subset of `useWordToolbarHandlers` for the page-color picker.
//
// The page-color UI surfaces a palette (see `PAGE_COLOR_PALETTE`) and
// forwards the picked color to this hook, which:
//   1. Reads the current document model via `editor.getDocument()`.
//   2. Mutates it via the pure `applyPageColor` helper.
//   3. Loads the mutated model back via `editor.loadDocument()`.
//
// We split the pure mutation into `applyPageColor` (no React, no
// editor handle) and keep the React wiring here so the math is
// independently testable.

import { useCallback } from 'react';

import { applyPageColor, type DocModel, type Notify } from './domMutations';
import type { EditorHandle } from './types';

export type { DocModel } from './domMutations';

export interface PageColorHandlers {
  handlePageColor: (color: string) => void;
}

export interface PageColorDeps {
  /** Must expose `getDocument` / `loadDocument` to mutate the model. */
  editor: EditorHandle | Pick<EditorHandle, 'getDocument' | 'loadDocument'> | null;
  notify?: Notify;
}

/** Read the document model from `editor`, returning null when unavailable. */
function readDoc(
  editor: PageColorDeps['editor'],
): DocModel | null {
  if (!editor?.getDocument) return null;
  try {
    // The editor handle's structural type is `unknown` so we can
    // round-trip any JSON shape; the page-color helper narrows to
    // `DocModel` internally.
    return editor.getDocument() as DocModel | null;
  } catch {
    return null;
  }
}

export function usePageColorHandlers({ editor, notify }: PageColorDeps): PageColorHandlers {
  const handlePageColor = useCallback(
    (color: string) => {
      if (!editor?.getDocument || !editor?.loadDocument) {
        notify?.('error', '页面颜色需要编辑器支持,当前不可用');
        return;
      }
      const doc = readDoc(editor);
      if (!doc || !doc.body) {
        notify?.('error', '无法读取文档模型,无法设置页面颜色');
        return;
      }
      const next = applyPageColor(doc, color);
      if (!next) return; // no-op (e.g. clearing a non-existent color)
      try {
        editor.loadDocument(next);
      } catch (e) {
        notify?.('error', `设置页面颜色失败: ${(e as Error).message}`);
      }
    },
    [editor, notify],
  );

  return { handlePageColor };
}