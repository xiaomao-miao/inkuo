import React, { useState, useCallback, useEffect, useRef, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { DocxEditor, type DocxEditorRef } from '@eigenpal/docx-editor-react';
import { Workbook } from '@fortune-sheet/react';
import type { WorkbookInstance } from '@fortune-sheet/react';
import type { Sheet as FortuneSheetCoreSheet } from '@fortune-sheet/core';
import { Save, Table2 } from 'lucide-react';
import { WordToolbar } from './word-toolbar';
import { useKeyboardSave } from './useKeyboardSave';
import { useContextMenuStore, useSidebarStore, useEditorStore, useInlineCompleteStore, useNotificationStore } from '../../store';
import type { DocxCommands } from '../../store';
import {
  rustWorkbookToFortuneSheets,
} from './fortuneSheetConverter';
import { fortuneSheetsToSheetJSBuffer } from './fortuneSheetConverter';
import type { RustXlsxWorkbook } from './fortuneSheetConverter';
import { reportError } from '../../utils/errors';
import {
  clearWordTimersForEditor,
  scheduleWordInlineCompletion,
} from '../inline-complete/useWordInlineCompleteTrigger';
import { createWordInlineCompletePlugin } from '../inline-complete/wordInlineCompletePlugin';
import type { EditorView } from 'prosemirror-view';
import { TextSelection } from 'prosemirror-state';
import { undoDepth, redoDepth } from 'prosemirror-history';
import styles from './OfficeViewer.module.css';
import '@eigenpal/docx-editor-react/styles.css';
import '@fortune-sheet/react/dist/index.css';

interface FortuneSheetCellLike {
  v?: unknown;
  m?: unknown;
  f?: unknown;
  ct?: unknown;
}

interface FortuneSheetRowLike {
  [col: number]: FortuneSheetCellLike | null;
}

interface FortuneSheetDataMatrix {
  [row: number]: FortuneSheetRowLike;
}

interface FortuneSheetForFingerprint {
  id?: string;
  name?: string;
  status?: number;
  order?: number;
  hide?: number;
  data?: FortuneSheetDataMatrix | null;
  celldata?: Array<{ r?: number; c?: number; v?: unknown }> | null;
}

// Build a fingerprint of user-visible content (cell values, formulas,
// statuses, names). Workbook's internal context rebuilds (e.g. when
// ensureSheetIndex patches sheet ids, or when immer produce reconstructs
// the sheet array) can produce new object references but leave this
// fingerprint unchanged — those rebuilds correspond to non-user-driven
// emissions. Real user edits change a cell's `v`, `f`, `m`, or the sheet's
// name/status, so they DO change this fingerprint.
function fingerprintSheets(sheets: FortuneSheetCoreSheet[]): string {
  const parts: string[] = [];
  for (const sheet of sheets as unknown as FortuneSheetForFingerprint[]) {
    parts.push(`#${sheet.id ?? ''}|${sheet.name ?? ''}|${sheet.status ?? ''}|${sheet.order ?? ''}|${sheet.hide ?? ''}`);
    const data = sheet.data;
    if (data && typeof data === 'object') {
      const rowKeys = Object.keys(data).map(Number).sort((a, b) => a - b);
      for (const r of rowKeys) {
        const row = data[r];
        if (!row) continue;
        const colKeys = Object.keys(row).map(Number).sort((a, b) => a - b);
        for (const c of colKeys) {
          const cell = row[c];
          if (cell == null) continue;
          const v = cell.v;
          const m = cell.m;
          const f = cell.f;
          if (v !== undefined || m !== undefined || f !== undefined) {
            parts.push(`${r},${c}:v=${JSON.stringify(v ?? null)};m=${JSON.stringify(m ?? null)};f=${JSON.stringify(f ?? null)};`);
          }
        }
      }
    } else if (Array.isArray(sheet.celldata)) {
      const sorted = [...sheet.celldata].sort((a, b) => (a.r ?? 0) - (b.r ?? 0) || (a.c ?? 0) - (b.c ?? 0));
      for (const cell of sorted) {
        parts.push(`${cell.r ?? ''},${cell.c ?? ''}:${JSON.stringify(cell.v ?? null)};`);
      }
    }
    parts.push('|');
  }
  return parts.join('\n');
}

export interface Paragraph {
  text: string;
  style: string;
  is_bold: boolean;
  is_italic: boolean;
  is_heading: boolean;
  level: number;
}

export interface Table {
  headers: string[];
  rows: string[][];
}

export interface WordDocument {
  title: string;
  paragraphs: Paragraph[];
  tables: Table[];
}

interface OfficeToolbarProps {
  fileName: string;
  isDirty: boolean;
  onSave: () => void;
  canSave: boolean;
  formatIcon?: React.ReactNode;
  editLabel?: string;
}

const OfficeToolbar: React.FC<OfficeToolbarProps> = ({
  fileName,
  isDirty,
  onSave,
  canSave,
  formatIcon,
  editLabel = '可编辑',
}) => {
  return (
    <div className={styles.editorToolbar}>
      <div className={styles.toolbarLeft}>
        {formatIcon}
        <span className={styles.fileName}>
          {fileName}
          {isDirty && <span className={styles.dirtyDot}>·</span>}
        </span>
      </div>
      <div className={styles.toolbarRight}>
        <span className={`${styles.editMode} ${isDirty ? styles.dirtyBadge : ''}`}>
          {editLabel}
        </span>
        <button
          className={`${styles.saveButton} ${isDirty ? styles.dirty : ''}`}
          onClick={onSave}
          disabled={!canSave}
          title="保存 (Ctrl+S)"
        >
          <Save size={14} />
          <span>保存</span>
        </button>
      </div>
    </div>
  );
};

interface WordEditorProps {
  filePath: string;
  fileName: string;
  initialBuffer: Uint8Array | null;
  isActive: boolean;
}

const WordInlineStatusBar: React.FC = () => {
  const enabled = true;
  const currentCompletion = useInlineCompleteStore((state) => state.currentCompletion);
  const isLoading = useInlineCompleteStore((state) => state.isLoading);
  const inlineError = useInlineCompleteStore((state) => state.error);

  if (!enabled) {
    return null;
  }

  return (
    <div className={styles.officeStatusBar}>
      {isLoading && (
        <span className={styles.inlineLoading}>
          <span className={styles.loadingDot} />
          <span className={styles.loadingDot} />
          <span className={styles.loadingDot} />
          <span className={styles.inlineLoadingText}>正在补全</span>
        </span>
      )}
      {!isLoading && currentCompletion && (
        <span className={styles.inlineReady}>
          <kbd>Tab</kbd> 接受 · <kbd>Esc</kbd> 拒绝
        </span>
      )}
      {!isLoading && inlineError && (
        <span className={styles.inlineError} title={inlineError}>补全失败</span>
      )}
      {!isLoading && !currentCompletion && !inlineError && (
        <span className={styles.inlineHint}>
          <kbd>Tab</kbd> AI 补全
        </span>
      )}
    </div>
  );
};

export const WordEditor: React.FC<WordEditorProps> = ({
  filePath,
  fileName,
  initialBuffer,
  isActive,
}) => {
  const editorRef = useRef<DocxEditorRef>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const pmViewRef = useRef<EditorView | null>(null);

  const [mode, setMode] = useState<'editing' | 'suggesting' | 'viewing'>('editing');
  const [pmView, setPmView] = useState<EditorView | null>(null);

  const wordInlineCompletePlugin = useMemo(
    () =>
      createWordInlineCompletePlugin({
        onUserInput: (view) => {
          scheduleWordInlineCompletion(view, filePath);
        },
      }),
    [filePath]
  );

  const handleEditorViewReady = useCallback((view: EditorView) => {
    pmViewRef.current = view;
    setPmView(view);
  }, []);

  // When this editor unmounts (tab closed, file switched, etc.) the ProseMirror
  // view is torn down. Drop our cached reference and the inline-complete
  // module-level state for this view so the underlying DOM/transaction machinery
  // can be GC'd promptly. Without this, `editorContexts` / `trackedViews` would
  // keep a live reference path to the (already destroyed) view.
  useEffect(() => {
    return () => {
      const view = pmViewRef.current;
      if (view) {
        clearWordTimersForEditor(view);
      }
      pmViewRef.current = null;
      setPmView(null);
    };
  }, []);

  const [documentBuffer, setDocumentBuffer] = useState<Uint8Array | null>(() => initialBuffer);
  const [loading, setLoading] = useState<boolean>(() => initialBuffer === null);
  const [error, setError] = useState<string | null>(null);
  const [isDirty, setIsDirty] = useState(false);
  const dirtyStateRef = useRef(false);

  useEffect(() => {
    dirtyStateRef.current = isDirty;
  }, [isDirty]);

  const setOpenTabDirty = useSidebarStore((state) => state.setOpenTabDirty);
  const officeBufferVersion = useEditorStore(s => s.documentContents[filePath]?.office.bufferVersion ?? 0);
  const setDocxBuffer = useEditorStore((state) => state.setDocxBuffer);
  const pushNotification = useNotificationStore((state) => state.pushNotification);

  // Suppress the webview's native context menu (and any third-party
  // menu wired up by the docx editor) and route every right-click
  // inside the editor container to our own `ContextMenu`. The host is
  // `DocxEditor` / ProseMirror, which owns its own event loop and may
  // call `stopPropagation` internally — we therefore attach a native
  // `contextmenu` listener in the capture phase so we always get the
  // event before the editor sees it.
  //
  // Two branches:
  //   - non-empty selection → `kind: 'selection'`, the existing AI /
  //     search / copy menu.
  //   - empty / collapsed selection → `kind: 'docx'`, a small doc-
  //     text action menu (Undo / Redo / Cut / Copy / Paste / Select
  //     All). We previously let the webview handle empty selections,
  //     but Chromium's stock menu is positioned independently of our
  //     code and tends to render somewhere far from the cursor (e.g.
  //     the bottom-right of the viewport for spell-check items).
  //     Routing through our app menu keeps the position consistent.
  useEffect(() => {
    const node = containerRef.current;
    if (!node) return undefined;
    const onContextMenu = (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();

      const selection = window.getSelection();
      const text = selection?.toString() ?? '';
      const trimmed = text.trim();
      const x = e.clientX;
      const y = e.clientY;

      if (trimmed.length > 0) {
        useContextMenuStore.getState().open({
          kind: 'selection',
          path: filePath,
          x,
          y,
          selectionText: text,
        });
        return;
      }

      // Empty-selection branch: build a snapshot of the imperative
      // PM commands the menu can dispatch. We snapshot closures that
      // resolve the live editor view at click time, so the menu
      // actions still work even if the editor unmounts before the
      // user picks a row.
      const editor = editorRef.current?.getEditorRef() ?? null;
      const guardView = (): EditorView | null => {
        // `pmViewRef.current` is the topology PM view set by the
        // editor's `onEditorViewReady` callback. `editor?.getView()`
        // is the same view but reached via the docx editor's public
        // ref. Both are kept in sync by the editor component; we
        // fall back to the editor ref to be safe.
        const v = pmViewRef.current ?? editor?.getView() ?? null;
        if (!v || v.isDestroyed) return null;
        return v;
      };

      const commands: DocxCommands = {
        undo: () => {
          const v = guardView();
          if (!v) return;
          editor?.focus();
          // `editorRef.undo()` returns true if a step was undone,
          // false if there was no history (e.g. fresh document). We
          // don't need the return value — the menu disables the
          // Undo row when `canUndo` is false at click time.
          editor?.undo();
          v.focus();
        },
        redo: () => {
          const v = guardView();
          if (!v) return;
          editor?.focus();
          editor?.redo();
          v.focus();
        },
        cut: () => {
          const v = guardView();
          if (!v) return;
          v.focus();
          // `execCommand` is deprecated but still works in Tauri
          // WebView (Chromium). The editor's PM view is focused, so
          // the browser routes the command to the right element.
          try {
            document.execCommand('cut');
          } catch {
            // Fallback: copy + delete the selection range.
            document.execCommand('copy');
            const tr = v.state.tr.deleteSelection();
            v.dispatch(tr);
          }
        },
        copy: () => {
          const v = guardView();
          if (!v) return;
          v.focus();
          try {
            document.execCommand('copy');
          } catch {
            // No-op: copying requires a live browser selection.
          }
        },
        paste: () => {
          const v = guardView();
          if (!v) return;
          v.focus();
          try {
            document.execCommand('paste');
          } catch (err) {
            // Browsers may block programmatic paste (e.g. without
            // a transient user-activation token). We just surface
            // the failure silently — the user can retry with ⌘V.
            console.warn('[docx-context-menu] paste failed', err);
          }
        },
        selectAll: () => {
          const v = guardView();
          if (!v) return;
          const docSize = v.state.doc.content.size;
          const tr = v.state.tr.setSelection(
            TextSelection.create(v.state.doc, 0, docSize),
          );
          v.dispatch(tr);
          v.focus();
        },
        // Find / Replace: focus the editor and dispatch a synthetic
        // keydown matching Ctrl+F / Ctrl+H. The editor's own keymap
        // (mounted in capture phase on the contenteditable surface)
        // intercepts these and opens its built-in dialog. This is
        // the same trick `WordToolbar.handleFind` /
        // `WordToolbar.handleReplace` use, just callable from the
        // context menu.
        find: () => {
          const v = guardView();
          v?.focus();
          const root = document.querySelector<HTMLElement>(
            '[data-office-editor-root="word"]',
          );
          const target = root ?? document.body;
          const evt = new KeyboardEvent('keydown', {
            key: 'f',
            code: 'KeyF',
            keyCode: 70,
            which: 70,
            ctrlKey: true,
            metaKey: true,
            bubbles: true,
            cancelable: true,
          });
          target.dispatchEvent(evt);
        },
        replace: () => {
          const v = guardView();
          v?.focus();
          const root = document.querySelector<HTMLElement>(
            '[data-office-editor-root="word"]',
          );
          const target = root ?? document.body;
          const evt = new KeyboardEvent('keydown', {
            key: 'h',
            code: 'KeyH',
            keyCode: 72,
            which: 72,
            ctrlKey: true,
            metaKey: true,
            bubbles: true,
            cancelable: true,
          });
          target.dispatchEvent(evt);
        },
        // Capability flags. ProseMirror's history plugin stores
        // event counts in plugin state; `undoDepth` /
        // `redoDepth` expose them. We read them lazily so the
        // snapshot is taken at the exact click moment.
        canUndo: (() => {
          const v = guardView();
          if (!v) return false;
          return (undoDepth(v.state) as number) > 0;
        })(),
        canRedo: (() => {
          const v = guardView();
          if (!v) return false;
          return (redoDepth(v.state) as number) > 0;
        })(),
        hasSelection: trimmed.length > 0,
        // We can't synchronously know whether the user has
        // something on the OS clipboard without querying the
        // Clipboard API (which is async). Instead, the user
        // will see a brief no-op if they click Paste with an
        // empty clipboard — the menu still closes via the
        // `wrap` helper in `buildDocxMenu`, so the UX is
        // indistinguishable from a successful paste attempt.
        hasClipboard: true,
      };

      useContextMenuStore.getState().open({
        kind: 'docx',
        path: filePath,
        x,
        y,
        docxCommands: commands,
      });
    };
    node.addEventListener('contextmenu', onContextMenu, { capture: true });
    return () => {
      node.removeEventListener('contextmenu', onContextMenu, { capture: true } as EventListenerOptions);
    };
  }, [filePath]);

  // Token counter to abort stale loads. When the user asks AI to modify
  // the document we kick off a disk read for the new bytes — but if the
  // user immediately asks for another change, two reads race against
  // each other. Without an abort token the slower read (which is reading
  // the *earlier* version) can land last and overwrite the newer buffer,
  // making the editor show stale content even though the file on disk is
  // already the new one. Bumping this counter on every reload
  // invalidates every in-flight read so only the latest one is allowed
  // to commit.
  const loadTokenRef = useRef(0);
  // Set after the first successful load (cache or disk). Guards the
  // "no-op on mount while initialBuffer settles" branches below.
  const hasInitializedFromCacheRef = useRef(false);

  // Read bytes from disk and push them through every surface that
  // displays the document:
  //   - `documentBuffer` state → the `<DocxEditor documentBuffer=...>` reactively reloads;
  //   - `setDocxBuffer(...)` → mirrors bytes into the editor store so a
  //     later tab switch / file re-open sees the same content;
  //   - `editorRef.current?.loadDocumentBuffer(buf)` → imperative reload
  //     so the editor repaints immediately, not on whatever React commit
  //     happens to be next.
  // Returns true if the load committed (i.e. wasn't aborted by a newer
  // load); the caller uses the return value to know whether to clear the
  // loading flag.
  const readAndApplyBuffer = useCallback(
    async (token: number): Promise<boolean> => {
      try {
        const data = await invoke<number[]>('read_office_file', { path: filePath });
        if (loadTokenRef.current !== token) return false;
        const buf = new Uint8Array(data);
        setDocumentBuffer(buf);
        setDocxBuffer(filePath, data);
        setIsDirty(false);
        setOpenTabDirty(filePath, false);
        await editorRef.current?.loadDocumentBuffer(buf);
        return loadTokenRef.current === token;
      } catch (err) {
        if (loadTokenRef.current !== token) return false;
        const message = reportError('office-word-reload', err);
        setError(message);
        pushNotification({
          kind: 'error',
          title: '刷新 Word 文档失败',
          message,
        });
        return false;
      }
    },
    [filePath, setDocxBuffer, setOpenTabDirty, pushNotification]
  );

  // Bump the load token and re-read the document. Used by every code
  // path that wants the editor to repaint with fresh bytes — including
  // the initial disk load, the AI-driven version watcher, and any manual
  // retry.
  const reloadFromDisk = useCallback(async () => {
    const token = ++loadTokenRef.current;
    setLoading(true);
    setError(null);
    try {
      await readAndApplyBuffer(token);
    } finally {
      if (loadTokenRef.current === token) {
        setLoading(false);
      }
    }
  }, [readAndApplyBuffer]);

  // Stable ref so other code paths (legacy callers, e.g. retry buttons)
  // can kick off a reload without holding a stale closure.
  const loadFromDiskRef = useRef<() => Promise<void>>(reloadFromDisk);
  loadFromDiskRef.current = reloadFromDisk;

  // Initial load: only run once per mounted editor instance. The
  // `initialBuffer` prop is a snapshot of what the parent had in its
  // cache when it first mounted us; if it has any bytes we can paint
  // immediately. Otherwise we read from disk. Either way we don't
  // re-trigger this effect on subsequent renders — re-renders happen
  // every time the parent re-renders (which happens any time *any*
  // field on `state.documentContents[path].office` changes), so
  // depending on `initialBuffer` would cause a reload loop. The
  // explicit `filePath` guard handles tab-switching.
  useEffect(() => {
    if (hasInitializedFromCacheRef.current) return;
    hasInitializedFromCacheRef.current = true;
    if (initialBuffer) {
      setDocumentBuffer(initialBuffer);
      setLoading(false);
    } else {
      void reloadFromDisk();
    }
    // Intentionally NOT depending on `initialBuffer`: it's a one-shot
    // snapshot from the parent. Subsequent reloads happen via the
    // `officeBufferVersion` effect below. Re-running this effect every
    // time the parent re-renders would cause a feedback loop with the
    // parent's `new Uint8Array(tabCached)` snapshot.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filePath]);

  // Re-read from disk when the backing file version changes. The
  // @eigenpal/docx-editor-react editor DOES reactively reload on prop
  // changes (Ji({documentBuffer}) has a useEffect dependency on the
  // buffer), but we still call `loadDocumentBuffer` imperatively on the
  // editor ref so the reload is explicit and synchronous. This matters
  // for the AI workflow: when `create_word_doc` finishes we want the
  // new headers/footers (or any other AI-driven edits) to repaint
  // immediately, not on whatever the next React commit cycle happens to
  // be.
  //
  // Why this used to be broken: the previous version mixed the initial
  // load and the version watcher into one effect, with `initialBuffer`
  // in the dependency list. The parent re-renders this component
  // every time `state.documentContents[path].office` changes (which
  // happens whenever `setDocxBuffer` or `invalidateOfficeBuffer` runs,
  // even if the new content is byte-identical to the old), and the
  // parent constructs `new Uint8Array(tabCached)` on every render — a
  // fresh Uint8Array reference. That meant this useEffect re-fired on
  // every parent render. The `wordLastVersionRef` guard caught most of
  // those re-fires, but two real bugs slipped through:
  //   1. If `invalidateOfficeBuffer` ever runs *before* the first
  //      `setDocxBuffer` for a fresh file, the store still has the
  //      bufferVersion > 0 but `wordLastVersionRef.current` is sitting
  //      at 0 from the initial-load pass — which makes the guard fire
  //      (`0 >= 1` false) and the effect reads the file, but the read
  //      races against another render and can land in a stale state.
  //   2. Two AI calls back-to-back (e.g. user notices the first rewrite
  //      was wrong and asks again before the first reload finishes)
  //      both pass the `wordLastVersionRef` guard because
  //      `officeBufferVersion` jumps 0 → 1 → 2 in the same React
  //      batch, and the second `setDocumentBuffer` can land before the
  //      first one's `loadDocumentBuffer` resolves — leaving the editor
  //      showing the older of the two writes.
  // The token-based abort in `readAndApplyBuffer` closes both holes: any
  // stale read is detected and its `setDocumentBuffer` is skipped.
  useEffect(() => {
    // Skip the first paint — the initial-load useEffect above already
    // handled that. After that, every increment of `officeBufferVersion`
    // (driven by `invalidateOfficeBuffer` from the AI pipeline) is a
    // signal to re-read the file.
    if (officeBufferVersion === 0) return;
    if (!hasInitializedFromCacheRef.current) return;
    void reloadFromDisk();
  }, [officeBufferVersion, reloadFromDisk]);

  useEffect(() => {
    if (isActive && isDirty) {
      setOpenTabDirty(filePath, true);
    }
  }, [isActive, isDirty, filePath, setOpenTabDirty]);

  // ── Save ────────────────────────────────────────────────────────────────
  const handleSave = useCallback(async () => {
    if (!isDirty) return;
    try {
      const savedBuffer = await editorRef.current?.save({ selective: false });
      if (!savedBuffer) return;
      const bufferArray = Array.from(new Uint8Array(savedBuffer));
      await invoke('write_office_file', { path: filePath, data: bufferArray });
      setDocxBuffer(filePath, bufferArray);
      setIsDirty(false);
      setOpenTabDirty(filePath, false);
    } catch (err) {
      const message = reportError('office-word-save', err);
      pushNotification({ kind: 'error', title: '保存 Word 文档失败', message });
    }
  }, [filePath, isDirty, setOpenTabDirty, setDocxBuffer, pushNotification]);

  useKeyboardSave({ onSave: handleSave, enabled: isDirty && isActive });

  const handleChange = useCallback(() => {
    if (dirtyStateRef.current) {
      return;
    }

    dirtyStateRef.current = true;
    setIsDirty(true);
    setOpenTabDirty(filePath, true);
  }, [filePath, setOpenTabDirty]);

  // ── Toolbar wiring ───────────────────────────────────────────────────────
  const handleFind = useCallback(() => {
    // FindReplace is built into the editor; the keyboard shortcut Ctrl+F is
    // already wired by DocxEditor (unless `disableFindReplaceShortcuts` is
    // set). We focus the editor so the shortcut is delivered to it.
    editorRef.current?.focus();
  }, []);

  const handleReplace = useCallback(() => {
    // The editor binds Ctrl+H to its built-in replace dialog by default. We
    // dispatch the key event from the document root so it reaches the editor's
    // own keymap (which is mounted on the inner contenteditable surface).
    const root = document.querySelector<HTMLElement>('[data-office-editor-root="word"]');
    if (!root) {
      editorRef.current?.focus();
      return;
    }
    const evt = new KeyboardEvent('keydown', {
      key: 'h',
      code: 'KeyH',
      keyCode: 72,
      which: 72,
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });
    root.dispatchEvent(evt);
    // As a safety net, fall back to focusing the editor — its keymap listens
    // at the editor root via capture phase.
    editorRef.current?.focus();
  }, []);

  /**
   * Imperative handle the WordToolbar uses for undo/redo and for the few
   * document-model actions (page color, header/footer) that don't have a
   * ProseMirror command. The shape is intentionally narrow — the toolbar
   * never gets the full DocxEditorRef, only the surfaces it needs.
   */
  const editorHandle = useMemo(
    () => ({
      undo: () => editorRef.current?.getEditorRef()?.undo() ?? false,
      redo: () => editorRef.current?.getEditorRef()?.redo() ?? false,
      getDocument: () => editorRef.current?.getEditorRef()?.getDocument() ?? null,
      loadDocument: (doc: unknown) => {
        editorRef.current?.loadDocument(doc as Parameters<DocxEditorRef['loadDocument']>[0]);
      },
    }),
    [],
  );

  const notify = useCallback((kind: 'error' | 'info', message: string) => {
    if (kind === 'error') {
      console.error('[WordToolbar]', message);
    } else {
      console.info('[WordToolbar]', message);
    }
  }, []);

  const handleTriggerAI = useCallback(() => {
    // Force-focus the editor and dispatch a no-op txn so the inline-complete
    // machinery schedules a completion on the next render-frame. Falling
    // back to a synthetic input event when scheduling doesn't auto-fire.
    const view = pmViewRef.current;
    if (!view) return;
    view.focus();
    const tr = view.state.tr.insertText('', view.state.selection.head, view.state.selection.head);
    view.dispatch(tr);
  }, []);

  const handleZoom = useCallback((z: number) => {
    editorRef.current?.setZoom(z);
  }, []);

  const handleGetZoom = useCallback(() => editorRef.current?.getZoom() ?? 1, []);

  const handlePrint = useCallback(() => {
    editorRef.current?.print();
  }, []);

  // ── Render
  // We keep the `<DocxEditor>` mounted at all times (loading, error, ready)
  // so that the right-hand scrollbar's containing block — the paged-editor's
  // scroll container — exists from the very first paint. Toggling between
  // a loading spinner and the editor caused the scrollbar to flicker on the
  // first open, because each branch changed the height of `.docxContainer`
  // and forced a fresh reflow. With the editor always mounted, switching
  // branches only updates opacity / text inside an existing box.
  // Visibility (display vs none) is controlled by the parent stack container
  // (see OfficeTabRenderer in Editor.tsx).
  return (
    <div className={styles.officeEditor}>
      <WordToolbar
        view={pmView}
        fileName={fileName}
        isDirty={isDirty}
        isLoading={loading}
        mode={mode}
        onModeChange={setMode}
        onSave={handleSave}
        canSave={isDirty && !loading && !error}
        onTriggerAI={handleTriggerAI}
        onFind={handleFind}
        onReplace={handleReplace}
        setZoom={handleZoom}
        getZoom={handleGetZoom}
        print={handlePrint}
        editor={editorHandle}
        notify={notify}
      />
      <div ref={containerRef} className={styles.docxContainer} data-office-editor-root="word">
        <DocxEditor
          ref={editorRef}
          documentBuffer={documentBuffer}
          mode={mode}
          onChange={handleChange}
          onModeChange={setMode}
          onEditorViewReady={handleEditorViewReady}
          externalPlugins={[wordInlineCompletePlugin]}
          renderLogo={() => null}
          showToolbar={false}
          showHelpMenu={false}
          showFileOpen={false}
        />
        {/* Word inline completion ghost is rendered by ProseMirror decorations (externalPlugins) */}
        {(loading || error) && (
          <div className={styles.editorOverlay} role="status" aria-live="polite">
            {loading ? (
              <>
                <div className={styles.loadingSpinner} />
                <span>正在加载 Word 文档...</span>
              </>
            ) : (
              <span className={styles.editorErrorMessage}>加载失败: {error}</span>
            )}
          </div>
        )}
      </div>
      <WordInlineStatusBar />
    </div>
  );
};

// ─── Excel Editor (FortuneSheet) ─────────────────────────────────────────────────

interface ExcelEditorProps {
  filePath: string;
  fileName: string;
  isActive: boolean;
}

export const ExcelEditor: React.FC<ExcelEditorProps> = ({
  filePath,
  fileName,
  isActive,
}) => {
  const workbookRef = useRef<WorkbookInstance | null>(null);

  // ── State that actually needs to trigger re-renders ──────────────────────
  // fortuneSheets is the single source of truth for the Workbook data prop.
  // We intentionally update it ONLY for initial load and external file changes,
  // NOT for every edit (onChange only sets loadedSheetsRef for dirty tracking).
  const [fortuneSheets, setFortuneSheets] = useState<FortuneSheetCoreSheet[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [isDirty, setIsDirty] = useState(false);

  // ── Store actions (stable selectors, no re-render on other state) ─────────
  const setOpenTabDirty = useSidebarStore((s) => s.setOpenTabDirty);
  const setFortuneSheetsToStore = useEditorStore((s) => s.setFortuneSheets);
  const pushNotification = useNotificationStore((s) => s.pushNotification);

  // ── External file version watcher ─────────────────────────────────────────
  const officeBufferVersion = useEditorStore(
    (s) => s.documentContents[filePath]?.office.bufferVersion ?? 0,
  );

  // ── Stable callback refs — updated via ref, not state ────────────────────
  // These avoid creating new function references on every render,
  // which would cause FortuneSheet's useEffect([onChange]) to re-fire.
  const recalcAllSheetsRef = useRef<() => void>(() => {});
  const loadedSheetsRef = useRef<FortuneSheetCoreSheet[]>([]);
  const formulaInitDoneRef = useRef(false);
  // Snapshot of the most recently set sheets data. Workbook echoes this back
  // through onChange whenever its internal context is rebuilt from the data
  // prop (file open, settings change). When the echo equals the snapshot, no
  // user edit has occurred — don't flip isDirty.
  const lastLoadedSheetsRef = useRef<FortuneSheetCoreSheet[] | null>(null);
  // Cached fingerprint of lastLoadedSheetsRef, used as a cheap structural
  // comparison key to distinguish Workbook's internal context rebuilds from
  // genuine user edits.
  const lastLoadedFingerprintRef = useRef<string | null>(null);
  // Tracks the most recent onOp we observed from Workbook. Workbook's onOp
  // fires only for real operations (user edits, undo/redo, paste, etc.) —
  // it does NOT fire for internal context rebuilds (which is why it doesn't
  // fire for file open). We only flip isDirty when onOp fires with an op
  // that touches a user-visible field.
  const userEditSeenRef = useRef(false);

  // ── Hooks — empty. Formula recalculation is handled by FortuneSheet internally.
  const hooks = useMemo((): import('@fortune-sheet/core').Hooks => ({}), []);

  // onChange handler: stable ref function, called by Workbook on every change.
  // Updates loadedSheetsRef for save; does NOT call setFortuneSheets to avoid
  // triggering a Workbook data prop change on every keystroke (which re-initializes
  // the entire sheet and causes lag).
  const handleFortuneChange = useCallback(
    (changedSheets: FortuneSheetCoreSheet[]) => {
      // loadedSheetsRef is kept for backward compatibility, but save now reads
      // directly from workbookRef.getAllSheets() which is always current.
      loadedSheetsRef.current = changedSheets;

      // Formula calculation: FortuneSheet does NOT compute formula results on
      // initial data load — `setFormulaCellInfoMap` only builds the dependency
      // graph. Trigger `calculateFormula` for every sheet the first time the
      // Workbook reports its data is ready (workbookRef + a populated sheet).
      // This MUST run regardless of `userEditSeenRef`, because that flag only
      // fires after the user's first real op — by then the user has already
      // observed the uncalculated cells. So the calc block is intentionally
      // outside the dirty-gate early return below.
      if (!formulaInitDoneRef.current && workbookRef.current) {
        formulaInitDoneRef.current = true;
        const wb = workbookRef.current;
        for (const sheet of wb.getAllSheets()) {
          wb.calculateFormula(sheet.id);
        }
      }

      // Dirty rule: only mark dirty when onOp has fired since the last load
      // or save. Workbook's onChange echoes its internal context on every
      // rebuild (file open, settings change, selection, etc.) and does not
      // by itself indicate user input. onOp, in contrast, fires only for
      // genuine operations (cell edits, format changes, sheet operations)
      // and never for the post-load context rebuild — so we gate dirty on
      // that signal.
      if (!userEditSeenRef.current) {
        // Workbook-internal rebuild echo — refresh the snapshot but don't
        // mark dirty.
        lastLoadedSheetsRef.current = changedSheets;
        if (lastLoadedFingerprintRef.current !== null) {
          lastLoadedFingerprintRef.current = fingerprintSheets(changedSheets);
        }
        return;
      }
      // User-driven change confirmed by onOp. Update the snapshot so future
      // Workbook rebuilds don't re-flip the dirty state.
      lastLoadedSheetsRef.current = changedSheets;
      lastLoadedFingerprintRef.current = fingerprintSheets(changedSheets);
      setIsDirty(true);
    },
    [],
  );

  // ── onOp handler: tracks genuine user-driven operations. Workbook's
  //    onOp fires for cell edits, format changes, sheet ops, undo/redo, and
  //    paste actions — never for the post-load context rebuild. We use it as
  //    the gating signal that allows handleFortuneChange to flip isDirty.
  const handleFortuneOp = useCallback(() => {
    userEditSeenRef.current = true;
  }, []);

  // ── Recalculate all sheets for save ───────────────────────────────────────────
  recalcAllSheetsRef.current = () => {
    const wb = workbookRef.current;
    if (!wb) return;
    for (const sheet of wb.getAllSheets()) {
      wb.calculateFormula(sheet.id);
    }
  };

  // Token counter to abort stale loads. See the matching comment in
  // WordEditor for the full rationale — the same TOCTOU bug exists here:
  // when the user asks AI to modify the spreadsheet back-to-back, two
  // `loadFromDisk` calls race against each other. The slower one reads
  // an *earlier* version and can land last, overwriting the newer
  // sheets data and leaving the editor showing stale content. Bumping
  // this counter on every reload invalidates every in-flight load so
  // only the latest one is allowed to commit.
  const loadTokenRef = useRef(0);
  // Set after the first load completes. Guards the version-watcher
  // effect against firing during the initial paint (where the parent
  // already passed us the right data).
  const hasInitializedRef = useRef(false);

  // Load bytes from disk and push them through every surface that
  // displays the workbook:
  //   - `fortuneSheets` state → the `<Workbook data=...>` reactively reloads;
  //   - `setFortuneSheetsToStore(...)` → mirrors sheets into the editor
  //     store so a later tab switch / file re-open sees the same content;
  // Returns true if the load committed (i.e. wasn't aborted by a newer
  // load); the caller uses the return value to know whether to clear
  // the loading flag.
  const readAndApplySheets = useCallback(
    async (token: number): Promise<boolean> => {
      try {
        const rustWorkbook = await invoke<RustXlsxWorkbook>('read_xlsx_structured', { path: filePath });
        if (loadTokenRef.current !== token) return false;
        const sheets = rustWorkbookToFortuneSheets(rustWorkbook);
        loadedSheetsRef.current = sheets;
        setFortuneSheets(sheets);
        setFortuneSheetsToStore(filePath, { sheets });
        // Reset the user-edit gate so the post-load Workbook onChange
        // echo doesn't flip isDirty. See handleFortuneChange for the
        // full rationale.
        lastLoadedSheetsRef.current = sheets;
        lastLoadedFingerprintRef.current = fingerprintSheets(sheets);
        userEditSeenRef.current = false;
        setIsDirty(false);
        setOpenTabDirty(filePath, false);
        return loadTokenRef.current === token;
      } catch (err) {
        if (loadTokenRef.current !== token) return false;
        const message = reportError('office-excel-reload', err);
        setError(message);
        pushNotification({
          kind: 'error',
          title: '刷新 Excel 文档失败',
          message,
        });
        return false;
      }
    },
    [filePath, setFortuneSheetsToStore, setOpenTabDirty, pushNotification]
  );

  // Bump the load token and re-read the workbook. Used by every code
  // path that wants the editor to repaint with fresh bytes — including
  // the initial disk load and the AI-driven version watcher.
  const loadFromDisk = useCallback(async () => {
    const token = ++loadTokenRef.current;
    setLoading(true);
    setError(null);
    try {
      await readAndApplySheets(token);
    } finally {
      if (loadTokenRef.current === token) {
        setLoading(false);
      }
    }
  }, [readAndApplySheets]);

  // Persist the load function (kept for any legacy callers that grab
  // the ref instead of going through the version-watcher).
  const loadFromDiskRef = useRef(loadFromDisk);
  loadFromDiskRef.current = loadFromDisk;

  // Initial load: only run once per mounted editor instance. Re-runs
  // when `filePath` changes (tab switching), which is the only case we
  // actually want to reload from disk.
  useEffect(() => {
    hasInitializedRef.current = false;
    loadTokenRef.current++; // invalidate any in-flight load from the previous path
    void loadFromDisk().then(() => {
      hasInitializedRef.current = true;
    });
  }, [filePath, loadFromDisk]);

  // Re-read when external file version changes. Driven by the AI
  // pipeline (`invalidateOfficeBuffer` from streamEventHandlers.ts).
  //
  // The previous version mixed `hasLoadedRef = false` into this effect,
  // trying to piggy-back on the initial-load effect — but that effect
  // depends on `[filePath]`, which doesn't change here, so the
  // `hasLoadedRef = false` write was a no-op and the version bump
  // only worked via the explicit `loadFromDiskRef.current()` call below.
  // We now read the bytes through the same token-gated path the Word
  // editor uses, so concurrent AI writes can't race against each other
  // and leave the editor showing stale content.
  useEffect(() => {
    // Skip the first paint — the initial-load effect above already
    // handled that. After that, every increment of `officeBufferVersion`
    // (driven by `invalidateOfficeBuffer` from the AI pipeline) is a
    // signal to re-read the file.
    if (officeBufferVersion === 0) return;
    if (!hasInitializedRef.current) return;
    void loadFromDisk();
  }, [officeBufferVersion, loadFromDisk]);

  // Sync dirty state to sidebar tab
  useEffect(() => {
    if (isActive && isDirty) {
      setOpenTabDirty(filePath, true);
    }
  }, [isActive, isDirty, filePath, setOpenTabDirty]);

  // ── Save ────────────────────────────────────────────────────────────────
  const handleSave = useCallback(async () => {
    const wb = workbookRef.current;
    if (!wb) {
      pushNotification({ kind: 'error', title: '保存失败', message: '表格引擎未就绪' });
      return;
    }
    const sheets = wb.getAllSheets();
    if (!sheets?.length) {
      pushNotification({ kind: 'error', title: '保存失败', message: '没有可用的工作表数据' });
      return;
    }
    try {
      recalcAllSheetsRef.current();
      const latestSheets = wb.getAllSheets();
      const buffer = await fortuneSheetsToSheetJSBuffer(latestSheets, wb.dataToCelldata.bind(wb));
      const bufferArray = Array.from(buffer);
      await invoke('write_office_file', { path: filePath, data: bufferArray });

      loadedSheetsRef.current = latestSheets;
      setFortuneSheetsToStore(filePath, { sheets: latestSheets });
      // Reset the dirty filter snapshot and user-edit gate to the just-saved
      // state. Future echoes from the Workbook are recognized as non-user-
      // driven until onOp fires again.
      lastLoadedSheetsRef.current = latestSheets;
      lastLoadedFingerprintRef.current = fingerprintSheets(latestSheets);
      userEditSeenRef.current = false;
      setIsDirty(false);
      setOpenTabDirty(filePath, false);
    } catch (err) {
      const message = reportError('office-excel-save', err);
      pushNotification({ kind: 'error', title: '保存 Excel 文档失败', message });
    }
  }, [filePath, setFortuneSheetsToStore, setOpenTabDirty, pushNotification]);

  useKeyboardSave({ onSave: handleSave, enabled: isDirty && isActive });

  // ── Render ───────────────────────────────────────────────────────────────
  return (
    <div className={styles.officeEditor}>
      <OfficeToolbar
        fileName={fileName}
        isDirty={isDirty}
        onSave={handleSave}
        canSave={isDirty && !loading && !error}
        formatIcon={<Table2 size={16} />}
        editLabel={loading ? '加载中...' : error ? '加载失败' : '可编辑'}
      />
      <div className={styles.excelContainer}>
        {(loading || error) ? (
          <div className={styles.editorOverlay} role="status" aria-live="polite">
            {loading ? (
              <>
                <div className={styles.loadingSpinner} />
                <span>正在加载 Excel 文档...</span>
              </>
            ) : (
              <span className={styles.editorErrorMessage}>加载失败: {error}</span>
            )}
          </div>
        ) : (
          <Workbook
            data={fortuneSheets}
            onChange={handleFortuneChange}
            onOp={handleFortuneOp}
            ref={workbookRef}
            showToolbar={true}
            showFormulaBar={true}
            showSheetTabs={true}
            allowEdit={true}
            forceCalculation={true}
            hooks={hooks}
          />
        )}
      </div>
    </div>
  );
};

