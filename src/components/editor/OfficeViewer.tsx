import React, { useState, useCallback, useEffect, useRef, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { DocxEditor, type DocxEditorRef } from '@eigenpal/docx-editor-react';
import { Workbook } from '@fortune-sheet/react';
import type { WorkbookInstance } from '@fortune-sheet/react';
import type { Sheet as FortuneSheetCoreSheet } from '@fortune-sheet/core';
import { Save, Table2 } from 'lucide-react';
import { WordToolbar } from './word-toolbar';
import { useKeyboardSave } from './useKeyboardSave';
import { useSidebarStore, useEditorStore, useInlineCompleteStore, useNotificationStore } from '../../store';
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
  const hasLoadedRef = useRef(false);
  const pmViewRef = useRef<EditorView | null>(null);
  const wordLastVersionRef = useRef(-1);
  const hasInitializedFromCacheRef = useRef(false);

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

  const loadFromDiskRef = useRef<() => Promise<void>>(() => Promise.resolve());

  const [documentBuffer, setDocumentBuffer] = useState<Uint8Array | null>(() => {
    if (initialBuffer) {
      hasLoadedRef.current = true;
      return initialBuffer;
    }
    return null;
  });
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

  useEffect(() => {
    const doLoad = async () => {
      setLoading(true);
      setError(null);
      try {
        const data = await invoke<number[]>('read_office_file', { path: filePath });
        const buffer = new Uint8Array(data);
        setDocumentBuffer(buffer);
        setDocxBuffer(filePath, data);
        setIsDirty(false);
        setOpenTabDirty(filePath, false);
      } catch (err) {
        const message = reportError('office-word-load', err);
        setError(message);
        pushNotification({ kind: 'error', title: '加载 Word 文档失败', message });
      } finally {
        setLoading(false);
      }
    };
    loadFromDiskRef.current = doLoad;
  }, [filePath, setOpenTabDirty, setDocxBuffer, pushNotification]);

  // Re-read from disk when the backing file version changes. The
  // @eigenpal/docx-editor-react editor DOES reactively reload on prop
  // changes (Ji({documentBuffer}) has a useEffect dependency on the
  // buffer), but we still call `loadDocumentBuffer` imperatively on the
  // editor ref so the reload is explicit and synchronous. This matters
  // for the AI workflow: when `create_word_doc` finishes we want the
  // new headers/footers (or any other AI-driven edits) to repaint
  // immediately, not on whatever the next React commit cycle happens to
  // be.
  useEffect(() => {
    if (wordLastVersionRef.current >= officeBufferVersion) return;
    wordLastVersionRef.current = officeBufferVersion;

    if (officeBufferVersion > 0) {
      // Refresh from disk. We read the file once via Tauri, then push
      // the bytes through both paths so the editor repaints:
      //   (a) `setDocumentBuffer(buf)` updates the `documentBuffer`
      //       prop, which the library's own Ji({documentBuffer})
      //       useEffect will pick up and reload;
      //   (b) `editorRef.current?.loadDocumentBuffer(buf)` does the
      //       same thing imperatively and synchronously, removing any
      //       dependency on React commit scheduling.
      (async () => {
        try {
          const data = await invoke<number[]>('read_office_file', { path: filePath });
          const buf = new Uint8Array(data);
          setDocumentBuffer(buf);
          setDocxBuffer(filePath, data);
          setIsDirty(false);
          setOpenTabDirty(filePath, false);
          await editorRef.current?.loadDocumentBuffer(buf);
        } catch (err) {
          const message = reportError('office-word-reload', err);
          setError(message);
          pushNotification({
            kind: 'error',
            title: '刷新 Word 文档失败',
            message,
          });
        }
      })();
      return;
    }

    if (initialBuffer) {
      hasInitializedFromCacheRef.current = true;
      setDocumentBuffer(initialBuffer);
      setLoading(false);
      return;
    }

    loadFromDiskRef.current();
  }, [officeBufferVersion, initialBuffer, filePath, pushNotification, setDocxBuffer, setOpenTabDirty]);

  useEffect(() => {
    if (!loading || hasInitializedFromCacheRef.current) return;
    if (initialBuffer) {
      hasInitializedFromCacheRef.current = true;
      setDocumentBuffer(initialBuffer);
      setLoading(false);
    }
  }, [initialBuffer, loading]);

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
      // eslint-disable-next-line no-console
      console.error('[WordToolbar]', message);
    } else {
      // eslint-disable-next-line no-console
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
  const hasLoadedRef = useRef(false);

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
  const excelLastVersionRef = useRef(-1);
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

  // ── Load from disk ──────────────────────────────────────────────────────
  const loadFromDisk = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const rustWorkbook = await invoke<RustXlsxWorkbook>('read_xlsx_structured', { path: filePath });
      const sheets = rustWorkbookToFortuneSheets(rustWorkbook);
      loadedSheetsRef.current = sheets;
      setFortuneSheets(sheets);
      setFortuneSheetsToStore(filePath, { sheets });
      // Record the array reference and a structural fingerprint so
      // handleFortuneChange can ignore Workbook's post-load onChange echoes.
      // Reset the user-edit gate: only after onOp fires (i.e. the user
      // performs an actual operation) do subsequent onChange calls flip
      // isDirty. Without this guard, opening a file would flip isDirty=true
      // even though no user edit has occurred.
      lastLoadedSheetsRef.current = sheets;
      lastLoadedFingerprintRef.current = fingerprintSheets(sheets);
      userEditSeenRef.current = false;
      setIsDirty(false);
      setOpenTabDirty(filePath, false);
    } catch (err) {
      const message = reportError('office-excel-load', err);
      setError(message);
      pushNotification({ kind: 'error', title: '加载 Excel 文档失败', message });
    } finally {
      setLoading(false);
    }
  }, [filePath, setFortuneSheetsToStore, setOpenTabDirty, pushNotification]);

  // Persist the load function
  const loadFromDiskRef = useRef(loadFromDisk);
  useEffect(() => { loadFromDiskRef.current = loadFromDisk; }, [loadFromDisk]);

  // Initial load
  useEffect(() => {
    if (hasLoadedRef.current) return;
    hasLoadedRef.current = true;
    loadFromDiskRef.current();
  }, [filePath]);

  // Re-read when external file version changes
  useEffect(() => {
    if (excelLastVersionRef.current >= officeBufferVersion) return;
    excelLastVersionRef.current = officeBufferVersion;
    if (officeBufferVersion > 0) {
      hasLoadedRef.current = false;
      loadFromDiskRef.current();
    }
  }, [officeBufferVersion]);

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

