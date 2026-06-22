import React, { useState, useCallback, useEffect, useRef, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { DocxEditor, type DocxEditorRef } from '@eigenpal/docx-editor-react';
import { Workbook } from '@fortune-sheet/react';
import type { WorkbookInstance } from '@fortune-sheet/react';
import type { Sheet as FortuneSheetCoreSheet } from '@fortune-sheet/core';
import { Save, Table2, FileText } from 'lucide-react';
import { useKeyboardSave } from './useKeyboardSave';
import { useSidebarStore, useEditorStore, useInlineCompleteStore, useNotificationStore } from '../../store';
import {
  rustWorkbookToFortuneSheets,
} from './fortuneSheetConverter';
import { fortuneSheetsToSheetJSBuffer } from './fortuneSheetConverter';
import type { RustXlsxWorkbook } from './fortuneSheetConverter';
import { reportError } from '../../utils/errors';
import { scheduleWordInlineCompletion } from '../inline-complete/useWordInlineCompleteTrigger';
import { createWordInlineCompletePlugin } from '../inline-complete/wordInlineCompletePlugin';
import type { EditorView } from 'prosemirror-view';
import styles from './OfficeViewer.module.css';
import '@eigenpal/docx-editor-react/styles.css';
import '@fortune-sheet/react/dist/index.css';

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

  // Re-read from disk when the backing file version changes.
  useEffect(() => {
    if (wordLastVersionRef.current >= officeBufferVersion) return;
    wordLastVersionRef.current = officeBufferVersion;

    if (officeBufferVersion > 0) {
      loadFromDiskRef.current();
      return;
    }

    if (initialBuffer) {
      hasInitializedFromCacheRef.current = true;
      setDocumentBuffer(initialBuffer);
      setLoading(false);
      return;
    }

    loadFromDiskRef.current();
  }, [officeBufferVersion, initialBuffer]);

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
      <OfficeToolbar
        fileName={fileName}
        isDirty={isDirty}
        onSave={handleSave}
        canSave={isDirty && !loading && !error}
        formatIcon={<FileText size={16} />}
        editLabel={loading ? '加载中...' : error ? '加载失败' : '可编辑'}
      />
      <div ref={containerRef} className={styles.docxContainer} data-office-editor-root="word">
        <DocxEditor
          ref={editorRef}
          documentBuffer={documentBuffer}
          mode="editing"
          onChange={handleChange}
          onEditorViewReady={handleEditorViewReady}
          externalPlugins={[wordInlineCompletePlugin]}
          renderLogo={() => null}
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
      setIsDirty(true);

      // Formula calculation: FortuneSheet does NOT compute formula results on initial
      // data load. Triggering calculation here (on first onChange) is the only
      // reliable hook after initSheetData has populated the data matrices.
      console.log('[formula] onChange called, formulaInitDone:', formulaInitDoneRef.current, 'workbook:', !!workbookRef.current);
      if (!formulaInitDoneRef.current && workbookRef.current) {
        formulaInitDoneRef.current = true;
        const wb = workbookRef.current;
        const allCtx = wb.getAllSheets ? wb.getAllSheets() : [];
        console.log('[formula] ctx sheets:', allCtx.length, allCtx.map((s: any) => `${s.name}(${s.id}): data=${!!s.data} celldata=${!!s.celldata}`));
        for (const sheet of allCtx) {
          const formulaCellsBefore = [];
          if (sheet.data) {
            for (let r = 0; r < Math.min(sheet.data.length, 20); r++) {
              for (let c = 0; c < (sheet.data[r]?.length ?? 0); c++) {
                const cell = sheet.data[r]?.[c];
                if (cell?.f) formulaCellsBefore.push(`(${r},${c}): f=${cell.f.slice(0,30)} v=${cell.v}`);
              }
            }
          }
          console.log('[formula] sheet:', sheet.name, 'id:', sheet.id, 'formulas found:', formulaCellsBefore.length, formulaCellsBefore.slice(0, 3));
          wb.calculateFormula(sheet.id);
          const afterData = sheet.data;
          let calculated = 0;
          const calculatedExamples = [];
          if (afterData) {
            for (let r = 0; r < Math.min(afterData.length, 20); r++) {
              for (let c = 0; c < (afterData[r]?.length ?? 0); c++) {
                const cell = afterData[r]?.[c];
                if (cell?.f && cell?.v !== undefined && cell?.v !== null) {
                  calculated++;
                  if (calculatedExamples.length < 3) calculatedExamples.push(`(${r},${c}): v=${JSON.stringify(cell.v)}`);
                }
              }
            }
          }
          console.log('[formula] after calculate:', sheet.name, 'calculatedCells:', calculated, calculatedExamples);
        }
      }
    },
    [],
  );

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
      if (!latestSheets?.length) return;

      // Use SheetJS (xlsx) to export directly from the browser.
      // This bypasses the fragile Rust→FortuneSheet→Rust conversion layer.
      // The Rust backend only needs to receive raw bytes to write to disk.
      const buffer = await fortuneSheetsToSheetJSBuffer(latestSheets);
      const bufferArray = Array.from(buffer);
      await invoke('write_office_file', { path: filePath, data: bufferArray });

      loadedSheetsRef.current = latestSheets;
      setFortuneSheetsToStore(filePath, { sheets: latestSheets });
      setIsDirty(false);
      setOpenTabDirty(filePath, false);
    } catch (err) {
      const message = reportError('office-excel-save', err);
      pushNotification({ kind: 'error', title: '保存 Excel 文档失败', message });
      console.error('[Excel Save] Error:', err);
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

