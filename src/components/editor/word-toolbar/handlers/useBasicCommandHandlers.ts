// Subset of `useWordToolbarHandlers` for the simple
// `runCommand(view, command)` wrappers — font family / size / color,
// marks toggles, history, clipboard, line spacing, and the
// single-command inserts (table / page break / symbol / clear).
//
// All callbacks returned here are stable across renders when `view`
// doesn't change (via `useCallback`). They take no other inputs
// from the editor handle so they're safe to memo on `view` alone.

import { useCallback } from 'react';
import type { EditorView } from 'prosemirror-view';
import {
  alignCenter,
  alignJustify,
  alignLeft,
  alignRight,
  applyStyle,
  clearFormatting,
  clearHighlight,
  clearStyle,
  decreaseIndent,
  increaseIndent,
  insertImageFromFile,
  insertPageBreak,
  insertTable,
  removeHyperlink,
  setFontFamily,
  setFontSize,
  setHighlight,
  setTextColor,
  toggleBold,
  toggleBulletList,
  toggleItalic,
  toggleNumberedList,
  toggleStrike,
  toggleSubscript,
  toggleSuperscript,
  toggleUnderline,
} from '@eigenpal/docx-editor-core/prosemirror/commands';

import { isViewReady, runCommand } from '../helpers';
import { ptToHalfPoints, stepFontSizePt } from '../numeric';
import { dispatchLineSpacing } from './lineSpacing';

export interface BasicCommandHandlers {
  // Font / marks
  handleFontFamily: (v: string) => void;
  handleFontSize: (pt: number) => void;
  handleFontSizeStep: (delta: number) => void;
  handleFontColor: (hex: string) => void;
  handleHighlight: (color: string) => void;
  handleStyleChange: (id: string) => void;
  // Marks toggles
  toggleBold: () => void;
  toggleItalic: () => void;
  toggleUnderline: () => void;
  toggleStrike: () => void;
  toggleSuperscript: () => void;
  toggleSubscript: () => void;
  toggleBulletList: () => void;
  toggleNumberedList: () => void;
  decreaseIndent: () => void;
  increaseIndent: () => void;
  alignLeft: () => void;
  alignCenter: () => void;
  alignRight: () => void;
  alignJustify: () => void;
  // Clipboard
  handleCopy: () => void;
  handleCut: () => void;
  handlePaste: () => Promise<void>;
  handleSelectAll: () => void;
  // History
  handleUndo: () => void;
  handleRedo: () => void;
  // Insert
  handleInsertTable: (rows: number, cols: number) => void;
  handleInsertImage: () => void;
  handleInsertSymbol: (sym: string) => void;
  handleClearFormatting: () => void;
  handleInsertPageBreak: () => void;
  // Link removal (link creation lives in `useLinkHandlers`).
  handleRemoveLink: () => void;
  // Line spacing
  handleLineSpacing: (v: string) => void;
}

interface BasicDeps {
  view: EditorView | null;
  /** Called from `useWordToolbarHandlers` — undo / redo need the editor handle. */
  runUndo: () => void;
  runRedo: () => void;
  /**
   * Current font size in points from the parent `useWordToolbarState`.
   * Used for the +/- stepper so it can preserve the toolbar's
   * "what font size is active" view.
   */
  currentFontSizePt: number;
}

/**
 * Insert an image by opening the OS file picker and forwarding the
 * selected file to the editor core's `insertImageFromFile` command.
 * Kept as a top-level export so future callers (custom toolbar
 * variants, snapshot tests) can re-use it without re-defining the
 * DOM dance.
 */
export function useInsertImageHandler(view: EditorView | null): () => void {
  return useCallback(() => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'image/*';
    input.onchange = () => {
      const file = input.files?.[0];
      if (file && view) insertImageFromFile(view, file);
    };
    input.click();
  }, [view]);
}

/**
 * Returns the bag of basic command-wrapping handlers. Most are
 * trivial `useCallback(() => runCommand(view, command), [view])`
 * expressions, but extracted into named callbacks so the parent
 * composer and the toolbar's JSX have stable references.
 */
export function useBasicCommandHandlers(
  deps: BasicDeps,
): BasicCommandHandlers {
  const { view, runUndo, runRedo, currentFontSizePt } = deps;

  // ── Font / marks ──────────────────────────────────────────────────────────
  const handleFontFamily = useCallback(
    (v: string) => runCommand(view, setFontFamily(v)),
    [view],
  );
  const handleFontSize = useCallback(
    (pt: number) => runCommand(view, setFontSize(ptToHalfPoints(pt))),
    [view],
  );
  const handleFontSizeStep = useCallback(
    (delta: number) => {
      // Preserve the parent's view of "active font size" so the
      // stepper doesn't fight the toolbar's display state. The
      // fallback is the current pt — `stepFontSizePt` collapses any
      // overflow back into the legal `[1, 400]` range.
      const next = stepFontSizePt(currentFontSizePt, delta, currentFontSizePt);
      runCommand(view, setFontSize(ptToHalfPoints(next)));
    },
    [view, currentFontSizePt],
  );
  const handleFontColor = useCallback(
    (hex: string) => runCommand(view, setTextColor({ rgb: hex.replace('#', '') })),
    [view],
  );
  const handleHighlight = useCallback(
    (color: string) =>
      color === 'none'
        ? runCommand(view, clearHighlight)
        : runCommand(view, setHighlight(color)),
    [view],
  );
  const handleStyleChange = useCallback(
    (id: string) => {
      if (id === 'Normal') runCommand(view, clearStyle);
      else runCommand(view, applyStyle(id));
    },
    [view],
  );

  // ── Marks toggles ─────────────────────────────────────────────────────────
  const toggleBoldCb = useCallback(() => runCommand(view, toggleBold), [view]);
  const toggleItalicCb = useCallback(() => runCommand(view, toggleItalic), [view]);
  const toggleUnderlineCb = useCallback(() => runCommand(view, toggleUnderline), [view]);
  const toggleStrikeCb = useCallback(() => runCommand(view, toggleStrike), [view]);
  const toggleSuperscriptCb = useCallback(() => runCommand(view, toggleSuperscript), [view]);
  const toggleSubscriptCb = useCallback(() => runCommand(view, toggleSubscript), [view]);
  const toggleBulletListCb = useCallback(() => runCommand(view, toggleBulletList), [view]);
  const toggleNumberedListCb = useCallback(() => runCommand(view, toggleNumberedList), [view]);
  const decreaseIndentCb = useCallback(
    () => runCommand(view, decreaseIndent()),
    [view],
  );
  const increaseIndentCb = useCallback(
    () => runCommand(view, increaseIndent()),
    [view],
  );
  const alignLeftCb = useCallback(() => runCommand(view, alignLeft), [view]);
  const alignCenterCb = useCallback(() => runCommand(view, alignCenter), [view]);
  const alignRightCb = useCallback(() => runCommand(view, alignRight), [view]);
  const alignJustifyCb = useCallback(() => runCommand(view, alignJustify), [view]);

  // ── Clipboard ────────────────────────────────────────────────────────────
  const handleCopy = useCallback(() => {
    document.execCommand('copy');
  }, []);
  const handleCut = useCallback(() => {
    document.execCommand('cut');
  }, []);
  const handlePaste = useCallback(async () => {
    try {
      const text = await navigator.clipboard.readText();
      if (isViewReady(view) && text) {
        view.dispatch(
          view.state.tr.insertText(
            text,
            view.state.selection.from,
            view.state.selection.to,
          ),
        );
        view.focus();
      }
    } catch {
      // Fall back to native paste via execCommand when the Clipboard
      // API isn't available (e.g. insecure context).
      document.execCommand('paste');
    }
  }, [view]);
  const handleSelectAll = useCallback(() => {
    document.execCommand('selectAll');
  }, []);

  // ── History (delegated to the deps because the composer has the editor handle) ──
  const handleUndo = useCallback(() => runUndo(), [runUndo]);
  const handleRedo = useCallback(() => runRedo(), [runRedo]);

  // ── Insert ───────────────────────────────────────────────────────────────
  const handleInsertTable = useCallback(
    (rows: number, cols: number) => runCommand(view, insertTable(rows, cols)),
    [view],
  );
  const handleInsertImage = useInsertImageHandler(view);
  const handleInsertSymbol = useCallback(
    (sym: string) => {
      if (!isViewReady(view)) return;
      view.dispatch(
        view.state.tr.insertText(
          sym,
          view.state.selection.from,
          view.state.selection.to,
        ),
      );
    },
    [view],
  );
  const handleClearFormatting = useCallback(
    () => runCommand(view, clearFormatting),
    [view],
  );
  const handleInsertPageBreak = useCallback(
    () => runCommand(view, insertPageBreak),
    [view],
  );

  // ── Link removal (creation lives in `useLinkHandlers`) ───────────────────
  const handleRemoveLink = useCallback(
    () => runCommand(view, removeHyperlink),
    [view],
  );

  // ── Line spacing ─────────────────────────────────────────────────────────
  const handleLineSpacing = useCallback(
    (v: string) => {
      dispatchLineSpacing(view, v);
    },
    [view],
  );

  return {
    handleFontFamily,
    handleFontSize,
    handleFontSizeStep,
    handleFontColor,
    handleHighlight,
    handleStyleChange,

    toggleBold: toggleBoldCb,
    toggleItalic: toggleItalicCb,
    toggleUnderline: toggleUnderlineCb,
    toggleStrike: toggleStrikeCb,
    toggleSuperscript: toggleSuperscriptCb,
    toggleSubscript: toggleSubscriptCb,
    toggleBulletList: toggleBulletListCb,
    toggleNumberedList: toggleNumberedListCb,
    decreaseIndent: decreaseIndentCb,
    increaseIndent: increaseIndentCb,
    alignLeft: alignLeftCb,
    alignCenter: alignCenterCb,
    alignRight: alignRightCb,
    alignJustify: alignJustifyCb,

    handleCopy,
    handleCut,
    handlePaste,
    handleSelectAll,

    handleUndo,
    handleRedo,

    handleInsertTable,
    handleInsertImage,
    handleInsertSymbol,
    handleClearFormatting,
    handleInsertPageBreak,

    handleRemoveLink,

    handleLineSpacing,
  };
}