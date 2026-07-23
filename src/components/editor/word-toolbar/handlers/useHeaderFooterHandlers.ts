// Subset of `useWordToolbarHandlers` for the header / footer popovers.
//
// Both popovers share the same confirm logic, so we expose a single
// `handleHeaderFooterConfirm` that dispatches by `kind`. The pure
// mutation lives in `applyHeaderFooter` (see `./domMutations`) so
// the React hook here stays focused on reading the editor handle and
// reporting errors.

import { useCallback } from 'react';

import {
  applyHeaderFooter,
  type DocModel,
  type HeaderFooterApply,
  type HeaderFooterKind,
  type Notify,
} from './domMutations';
import type { EditorHandle } from './types';

export type { DocModel, HeaderFooterApply, HeaderFooterKind } from './domMutations';

export interface HeaderFooterHandlers {
  handleInsertHeader: () => void;
  handleInsertFooter: () => void;
  handleHeaderFooterConfirm: (
    kind: HeaderFooterKind,
    cfg: HeaderFooterApply,
  ) => void;
}

export interface HeaderFooterDeps {
  editor: EditorHandle | Pick<EditorHandle, 'getDocument' | 'loadDocument'> | null;
  notify?: Notify;
  openHeader: () => void;
  closeHeader: () => void;
  openFooter: () => void;
  closeFooter: () => void;
}

export function useHeaderFooterHandlers({
  editor,
  notify,
  openHeader,
  closeHeader,
  openFooter,
  closeFooter,
}: HeaderFooterDeps): HeaderFooterHandlers {
  const handleInsertHeader = useCallback(() => {
    if (!editor) return;
    openHeader();
  }, [editor, openHeader]);

  const handleInsertFooter = useCallback(() => {
    if (!editor) return;
    openFooter();
  }, [editor, openFooter]);

  const handleHeaderFooterConfirm = useCallback(
    (kind: HeaderFooterKind, cfg: HeaderFooterApply) => {
      if (kind === 'header') closeHeader();
      else closeFooter();
      if (!editor?.getDocument || !editor?.loadDocument) {
        notify?.('error', '页眉页脚需要编辑器支持,当前不可用');
        return;
      }
      let doc: DocModel | null = null;
      try {
        doc = editor.getDocument() as DocModel | null;
      } catch {
        notify?.('error', '无法读取文档模型');
        return;
      }
      if (!doc || !doc.body) {
        notify?.('error', '无法读取文档模型');
        return;
      }
      const next = applyHeaderFooter(doc, kind, cfg);
      if (!next) return; // empty cfg → no-op
      try {
        editor.loadDocument(next);
      } catch (e) {
        notify?.('error', `插入${kind === 'header' ? '页眉' : '页脚'}失败: ${(e as Error).message}`);
      }
    },
    [editor, notify, closeHeader, closeFooter],
  );

  return { handleInsertHeader, handleInsertFooter, handleHeaderFooterConfirm };
}