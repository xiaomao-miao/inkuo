// Composer for the Word toolbar's full handler bag.
//
// Splits the original 781-line `handlers.ts` into focused sub-hooks
// (each ~50-200 lines, single concern) and composes them here. The
// composer stays small because each sub-hook owns its own memoization
// and side effects.
//
// Sub-hooks:
//   - `useBasicCommandHandlers` — font, marks toggles, clipboard,
//     history, insert (table / symbol / page break / clear), line
//     spacing, link removal.
//   - `useLinkHandlers`         — link popover wiring (open / confirm).
//   - `useWatermarkHandlers`    — watermark popover wiring + state read.
//   - `useMathHandlers`         — math popover wiring.
//   - `usePageColorHandlers`    — page color picker.
//   - `useHeaderFooterHandlers` — header / footer popovers.
//   - `useFormatPainterHandlers` — format-painter state machine + ESC.
//   - `useMiscHandlers`         — sort, spell check, formatting marks.

import { useCallback } from 'react';
import type { EditorView } from 'prosemirror-view';

import { useBasicCommandHandlers } from './useBasicCommandHandlers';
import { useFormatPainterHandlers } from './useFormatPainterHandlers';
import { useHeaderFooterHandlers } from './useHeaderFooterHandlers';
import { useLinkHandlers } from './useLinkHandlers';
import { useMathHandlers } from './useMathHandlers';
import { useMiscHandlers } from './useMiscHandlers';
import { usePageColorHandlers } from './usePageColorHandlers';
import { useWatermarkHandlers } from './useWatermarkHandlers';

import type { BasicCommandHandlers } from './useBasicCommandHandlers';
import type { FormatPainterState } from './useFormatPainterHandlers';
import type { HeaderFooterHandlers } from './useHeaderFooterHandlers';
import type { LinkHandlers } from './useLinkHandlers';
import type { MathHandlers } from './useMathHandlers';
import type { MiscHandlers } from './useMiscHandlers';
import type { PageColorHandlers } from './usePageColorHandlers';
import type { WatermarkHandlers } from './useWatermarkHandlers';

import type { EditorHandle, Notify } from './types';

/** Page-color palette — matches the color picker UI in the toolbar. */
const PAGE_COLOR_PALETTE: string[] = [
  '#FFFFFF',
  '#F2F2F2',
  '#D9E1F2',
  '#FCE4D6',
  '#E2EFDA',
  '#FFF2CC',
  '#F8CBAD',
];

export { PAGE_COLOR_PALETTE };
export const WORD_TOOLBAR_PAGE_COLOR_PALETTE = PAGE_COLOR_PALETTE;

/**
 * Bundled callback surface returned to the toolbar JSX. Sub-hooks
 * each contribute a slice; the full bag is what `<WordToolbar />`
 * spreads into its buttons.
 */
export interface WordToolbarHandlers
  extends BasicCommandHandlers,
    LinkHandlers,
    WatermarkHandlers,
    MathHandlers,
    PageColorHandlers,
    HeaderFooterHandlers,
    FormatPainterState,
    MiscHandlers {}

/** Parent popover open/close callbacks passed through to sub-hooks. */
export interface WordToolbarHandlerOptions {
  openLink: (payload: { initialText: string; isEditingExisting: boolean }) => void;
  closeLink: () => void;
  openMath: () => void;
  closeMath: () => void;
  openWatermark: () => void;
  closeWatermark: () => void;
  openHeader: () => void;
  closeHeader: () => void;
  openFooter: () => void;
  closeFooter: () => void;
}

export interface WordToolbarHandlersArgs {
  view: EditorView | null;
  editor: EditorHandle | null;
  isLink: boolean;
  currentFontSizePt: number;
  notify: Notify | undefined;
  options: WordToolbarHandlerOptions;
}

/**
 * Backwards-compatible positional API used by `WordToolbar.tsx`.
 * The new object-style signature (`useWordToolbarHandlers({...})`) is
 * exposed for tests / future callers.
 */
export function useWordToolbarHandlers(
  view: EditorView | null,
  editor: EditorHandle | null,
  isLink: boolean,
  currentFontSizePt: number,
  notify: Notify | undefined,
  options: WordToolbarHandlerOptions,
): WordToolbarHandlers {
  return useWordToolbarHandlersInternal({ view, editor, isLink, currentFontSizePt, notify, options });
}

function useWordToolbarHandlersInternal(args: WordToolbarHandlersArgs): WordToolbarHandlers {
  const { view, editor, isLink, currentFontSizePt, notify, options } = args;

  // History (undo / redo) needs the editor handle, which other sub-hooks
  // don't. We pre-compute the dispatchers and pass them as deps.
  const runUndo = useCallback(() => {
    if (!editor) return;
    editor.undo();
    view?.focus();
  }, [editor, view]);
  const runRedo = useCallback(() => {
    if (!editor) return;
    editor.redo();
    view?.focus();
  }, [editor, view]);

  const basic = useBasicCommandHandlers({ view, runUndo, runRedo, currentFontSizePt });

  const link = useLinkHandlers({
    view,
    isLink,
    openLink: options.openLink,
    closeLink: options.closeLink,
  });

  const watermark = useWatermarkHandlers({
    view,
    openWatermark: options.openWatermark,
    closeWatermark: options.closeWatermark,
  });

  const math = useMathHandlers({
    view,
    openMath: options.openMath,
    closeMath: options.closeMath,
  });

  const pageColor = usePageColorHandlers({ editor, notify });

  const headerFooter = useHeaderFooterHandlers({
    editor,
    notify,
    openHeader: options.openHeader,
    closeHeader: options.closeHeader,
    openFooter: options.openFooter,
    closeFooter: options.closeFooter,
  });

  const formatPainter = useFormatPainterHandlers(view);

  const misc = useMiscHandlers({ view, notify });

  return {
    ...basic,
    ...link,
    ...watermark,
    ...math,
    ...pageColor,
    ...headerFooter,
    ...formatPainter,
    ...misc,
  };
}

// Re-export the pure helpers + selection utilities for tests / external use.
//
// Naming: types live in `./types` so the consumer-facing import
// path is consistent (`useWordToolbarHandlers`, `WatermarkApply`,
// etc. all come from the same surface). The implementation files
// (`./domMutations`, `./selection`, etc.) hold the runtime helpers.
//
// Note: `WatermarkApply` (the user-facing type name) is the same as
// the `WatermarkApplyConfig` shape internally — keeping the public
// name stable avoids churn at call sites that were already importing
// `WatermarkApply` from the old monolithic `handlers.ts`.

export {
  applyHeaderFooter,
  applyPageColor,
  buildHeaderFooterRuns,
  buildWatermarkSpec,
  type DocModel,
} from './domMutations';
export type { HeaderFooterApply, HeaderFooterKind, WatermarkApply, WatermarkState } from './types';

export {
  collectSortableTextblocks,
  compareLinesAsc,
  compareLinesDesc,
  extractSelectionText,
  readSelectionText,
  type GlobalSelection,
  type SortableLine,
} from './selection';

export { currentWatermarkFromView } from './useWatermarkHandlers';

export {
  dispatchLineSpacing,
  LINE_SPACING_OPTIONS,
  type LineSpacingCommand,
} from './lineSpacing';

// Re-export the individual sub-hooks so consumers / tests can use them in
// isolation if they only need a slice (e.g. a custom toolbar variant).
export { useBasicCommandHandlers } from './useBasicCommandHandlers';
export { useFormatPainterHandlers } from './useFormatPainterHandlers';
export { useHeaderFooterHandlers } from './useHeaderFooterHandlers';
export { useLinkHandlers } from './useLinkHandlers';
export { useMathHandlers } from './useMathHandlers';
export { useMiscHandlers } from './useMiscHandlers';
export { usePageColorHandlers } from './usePageColorHandlers';
export { useWatermarkHandlers } from './useWatermarkHandlers';