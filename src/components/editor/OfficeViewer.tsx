import React, { useState, useCallback, useEffect, useRef, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { DocxEditor, type DocxEditorRef } from '@eigenpal/docx-editor-react';
import { ExcelGrid } from 'react-excel-lite';
import { Save, Table2, FileText } from 'lucide-react';
import { useKeyboardSave } from './useKeyboardSave';
import { useSidebarStore, useEditorStore, useInlineCompleteStore } from '../../store';
import { scheduleWordInlineCompletion } from '../inline-complete/useWordInlineCompleteTrigger';
import { createWordInlineCompletePlugin } from '../inline-complete/wordInlineCompletePlugin';
import type { EditorView } from 'prosemirror-view';
import styles from './OfficeViewer.module.css';
import '@eigenpal/docx-editor-react/styles.css';
import { TIMING } from '../../constants/timing';

// ============================================================================
// Type Definitions
// ============================================================================

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

export interface Sheet {
  name: string;
  headers: string[];
  rows: string[][];
  max_col: number;
  max_row: number;
}

export interface ExcelWorkbook {
  sheets: Sheet[];
}

// ============================================================================
// Shared Office Toolbar
// ============================================================================

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

// ============================================================================
// Word Editor Component
//
// Key design: the component is NEVER unmounted while a tab is open. The
// `documentBuffer` state is initialized ONCE from the `initialBuffer` prop
// (store cache) using lazy initialization — if the prop is null it will load
// from disk. `isActive` only controls CSS visibility, not rendering.
// ============================================================================

interface WordEditorProps {
  filePath: string;
  fileName: string;
  /** Buffer cached in the store (survives tab switches). Lazy-initialized. */
  initialBuffer: Uint8Array | null;
  /** Whether this tab is currently active. Controls CSS visibility only. */
  isActive: boolean;
}

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
  // Initialize to -1 so the first mount always passes the >= check.
  // Incremented to >= 0 after loading completes.
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

  const enabled = true; // Word inline completion is always available
  const currentCompletion = useInlineCompleteStore((s) => s.currentCompletion);
  const isLoading = useInlineCompleteStore((s) => s.isLoading);
  const inlineError = useInlineCompleteStore((s) => s.error);

  // Debug: log component mount (disabled in production)
  useEffect(() => {
    if (import.meta.env.DEV) {
      console.log('[WordEditor] mounted, filePath:', filePath, 'isActive:', isActive);
    }
  }, []);

  // Capture the ProseMirror view when it's ready
  const handleEditorViewReady = useCallback((view: EditorView) => {
    pmViewRef.current = view;
  }, []);

  // ── Persistent state: initialized once from the store cache ──────────────
  // Using lazy initialization: only loads from disk on first render,
  // subsequent renders reuse the in-memory state.
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

  const { setOpenTabDirty } = useSidebarStore();
  const officeBufferVersion = useEditorStore(s => s.documentContents[filePath]?.officeBufferVersion ?? 0);
  const { setDocxBuffer } = useEditorStore();

  // ── Re-read from disk when AI agent modified the file ─────────────────────

  const loadFromDisk = useCallback(async () => {
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
      console.error('Failed to load Word document:', err);
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [filePath, setOpenTabDirty, setDocxBuffer]);

  // ── Load on mount and re-load when AI agent modifies the file.
  // wordLastVersionRef tracks the version we last loaded, so both
  // the initial mount and version-triggered invalidations are handled
  // without double-loading.
  useEffect(() => {
    // Already loaded this version — skip
    if (wordLastVersionRef.current >= officeBufferVersion) return;
    wordLastVersionRef.current = officeBufferVersion;

    // When officeBufferVersion > 0, the AI agent has written a new file.
    // Always read from disk to get the latest content, ignoring the stale cache.
    if (officeBufferVersion > 0) {
      loadFromDisk();
      return;
    }

    // Initial mount (version === 0): use cached buffer if available
    if (initialBuffer) {
      hasInitializedFromCacheRef.current = true;
      setDocumentBuffer(initialBuffer);
      setLoading(false);
      return;
    }

    // No cached buffer — must read from disk
    loadFromDisk();
    // Note: Uses ref-based version tracking (wordLastVersionRef) to avoid duplicate loads.
    // Deps exclude loadFromDisk and setDocumentBuffer intentionally.
  }, [officeBufferVersion, initialBuffer]); // eslint-disable-line react-hooks/exhaustive-deps

  // ── When the store cache becomes available (e.g., restored after reload) ─
  // This fires when the prop changes from null → non-null (after localStorage restore).
  // The version-keyed effect above may have already skipped (initialBuffer was null at that time).
  useEffect(() => {
    if (!loading || hasInitializedFromCacheRef.current) return;
    if (initialBuffer) {
      hasInitializedFromCacheRef.current = true;
      setDocumentBuffer(initialBuffer);
      setLoading(false);
    }
    // Note: Uses loading state to detect when cache becomes available.
  }, [initialBuffer, loading]); // eslint-disable-line react-hooks/exhaustive-deps

  // ── Sync dirty state to sidebar on mount (handles restored dirty state) ───
  useEffect(() => {
    if (isActive && isDirty) {
      setOpenTabDirty(filePath, true);
    }
    // Note: Only syncs on mount/isDirty changes, not on every setOpenTabDirty reference change.
  }, [isActive, isDirty]); // eslint-disable-line react-hooks/exhaustive-deps

  // ── Hide DocxEditor's top menu buttons ─────────────────────────────────
  useEffect(() => {
    if (!documentBuffer) return;

    const hideMenuButtons = () => {
      if (!containerRef.current || !document.contains(containerRef.current)) return;
      const buttons = containerRef.current.querySelectorAll('button');
      buttons.forEach((button) => {
        const text = button.textContent?.trim() || '';
        if (['File', 'Format', 'Insert', 'Help'].includes(text)) {
          (button as HTMLButtonElement).style.display = 'none';
        }
      });
    };

    const timer = setTimeout(hideMenuButtons, TIMING.OFFICE_MENU_HIDE_DELAY_MS);
    const observer = new MutationObserver(hideMenuButtons);
    if (containerRef.current) {
      observer.observe(containerRef.current, { childList: true, subtree: true });
    }
    return () => {
      clearTimeout(timer);
      observer.disconnect();
    };
  }, [documentBuffer]);

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
      console.error('Failed to save Word document:', err);
    }
  }, [filePath, isDirty, setOpenTabDirty, setDocxBuffer]);

  useKeyboardSave({ onSave: handleSave, enabled: isDirty && isActive });

  const handleChange = useCallback(() => {
    setIsDirty(true);
    setOpenTabDirty(filePath, true);
  }, [filePath, setOpenTabDirty]);

  // ── Render: CSS visibility, NOT conditional unmount ───────────────────────
  if (loading) {
    return (
      <div className={styles.officeEditor} style={{ display: isActive ? undefined : 'none' }}>
        <OfficeToolbar fileName={fileName} isDirty={false} onSave={handleSave} canSave={false} formatIcon={<FileText size={16} />} editLabel="加载中..." />
        <div className={styles.editorLoading}>
          <div className={styles.loadingSpinner} />
          <span>正在加载 Word 文档...</span>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className={styles.officeEditor} style={{ display: isActive ? undefined : 'none' }}>
        <OfficeToolbar fileName={fileName} isDirty={false} onSave={handleSave} canSave={false} formatIcon={<FileText size={16} />} editLabel="加载失败" />
        <div className={styles.editorError}>
          <span>加载失败: {error}</span>
        </div>
      </div>
    );
  }

  if (!documentBuffer) {
    return (
      <div className={styles.officeEditor} style={{ display: isActive ? undefined : 'none' }}>
        <OfficeToolbar fileName={fileName} isDirty={false} onSave={handleSave} canSave={false} formatIcon={<FileText size={16} />} editLabel="无文档" />
        <div className={styles.editorError}>
          <span>无法加载文档</span>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.officeEditor} style={{ display: isActive ? undefined : 'none' }}>
      <OfficeToolbar
        fileName={fileName}
        isDirty={isDirty}
        onSave={handleSave}
        canSave={isDirty}
        formatIcon={<FileText size={16} />}
        editLabel="可编辑"
      />
      <div ref={containerRef} className={styles.docxContainer} data-ghost-container="true">
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
      </div>
      {enabled && (
        <div className={styles.officeStatusBar}>
          {isLoading && (
            <span className={styles.inlineLoading}>
              <span className={styles.loadingDot} />
              <span className={styles.loadingDot} />
              <span className={styles.loadingDot} />
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
      )}
    </div>
  );
};

// ============================================================================
// Excel Editor Component
//
// Same principle: lazy initialization from store cache, CSS visibility only,
// never unmounts while tab is open.
// ============================================================================

interface ExcelEditorProps {
  filePath: string;
  fileName: string;
  /** Data cached in the store (survives tab switches). Lazy-initialized. */
  initialData: string[][] | null;
  /** Whether this tab is currently active. Controls CSS visibility only. */
  isActive: boolean;
}

export const ExcelEditor: React.FC<ExcelEditorProps> = ({
  filePath,
  fileName,
  initialData,
  isActive,
}) => {
  const hasLoadedRef = useRef(false);

  // Lazy initialization from store cache
  const [data, setData] = useState<string[][] | null>(() => {
    if (initialData !== null) {
      hasLoadedRef.current = true;
      return initialData;
    }
    return null;
  });
  const [loading, setLoading] = useState<boolean>(() => initialData === null);
  const [error, setError] = useState<string | null>(null);
  const [isDirty, setIsDirty] = useState(false);
  const [originalData, setOriginalData] = useState<string[][] | null>(() => initialData);
  // Cache the serialized form of originalData to avoid re-stringifying on every change.
  const originalDataJsonRef = useRef<string>(JSON.stringify(initialData ?? []));

  // Re-serialize when originalData changes (mount, load, save)
  useEffect(() => {
    originalDataJsonRef.current = JSON.stringify(originalData ?? []);
  }, [originalData]);

  const { setOpenTabDirty } = useSidebarStore();
  const officeBufferVersion = useEditorStore(s => s.documentContents[filePath]?.officeBufferVersion ?? 0);
  const { setExcelData } = useEditorStore();
  const excelLastVersionRef = useRef(-1);

  // ── Re-read from disk when AI agent modified the file ─────────────────────
  const loadFromDisk = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const fileData = await invoke<number[]>('read_office_file', { path: filePath });
      const buffer = new Uint8Array(fileData);
      const XLSX = await import('xlsx');
      const workbook = XLSX.read(buffer, { type: 'array' });
      const firstSheet = workbook.Sheets[workbook.SheetNames[0]];
      const jsonData = XLSX.utils.sheet_to_json(firstSheet, { header: 1 }) as (string | number | null)[][];
      const stringData = jsonData.map(row =>
        row.map(cell => {
          if (cell === null || cell === undefined) return '';
          if (typeof cell === 'number') return cell.toString();
          return String(cell);
        })
      );
      setData(stringData);
      setOriginalData(stringData);
      setExcelData(filePath, stringData);
      setIsDirty(false);
      setOpenTabDirty(filePath, false);
    } catch (err) {
      console.error('Failed to load Excel document:', err);
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [filePath, setOpenTabDirty, setExcelData]);

  // ── Load on mount and re-load when AI agent modifies the file.
  useEffect(() => {
    // Already loaded this version — skip
    if (excelLastVersionRef.current >= officeBufferVersion) return;
    excelLastVersionRef.current = officeBufferVersion;

    // When officeBufferVersion > 0, the AI agent has written a new file.
    // Always read from disk to get the latest content, ignoring the stale cache.
    if (officeBufferVersion > 0) {
      loadFromDisk();
      return;
    }

    // Initial mount: use cached data if available, otherwise read from disk.
    if (initialData !== null) {
      setData(initialData);
      setOriginalData(initialData);
      setLoading(false);
      return;
    }

    loadFromDisk();
    // Note: Uses ref-based version tracking (excelLastVersionRef) to avoid duplicate loads.
  }, [officeBufferVersion]); // eslint-disable-line react-hooks/exhaustive-deps

  // ── Sync dirty state to sidebar ─────────────────────────────────────────
  useEffect(() => {
    if (isActive && isDirty) {
      setOpenTabDirty(filePath, true);
    }
    // Note: Only syncs when active or dirty state changes.
  }, [isActive, isDirty]); // eslint-disable-line react-hooks/exhaustive-deps

  const handleSave = useCallback(async () => {
    if (!isDirty || !data) return;
    try {
      const XLSX = await import('xlsx');
      const worksheet = XLSX.utils.aoa_to_sheet(data);
      const workbook = XLSX.utils.book_new();
      XLSX.utils.book_append_sheet(workbook, worksheet, 'Sheet1');
      const xlsxData = XLSX.write(workbook, { bookType: 'xlsx', type: 'array' });
      await invoke('write_office_file', { path: filePath, data: Array.from(xlsxData) });
      setOriginalData(data);
      setExcelData(filePath, data);
      setIsDirty(false);
      setOpenTabDirty(filePath, false);
    } catch (err) {
      console.error('Failed to save Excel document:', err);
    }
  }, [filePath, data, isDirty, setOpenTabDirty, setExcelData]);

  useKeyboardSave({ onSave: handleSave, enabled: isDirty && isActive });

  const handleChange = useCallback((newData: string[][]) => {
    setData(newData);
    const changed = JSON.stringify(newData) !== originalDataJsonRef.current;
    setIsDirty(changed);
    setOpenTabDirty(filePath, changed);
  }, [filePath, setOpenTabDirty]);

  // ── Render: CSS visibility only ──────────────────────────────────────────
  if (loading) {
    return (
      <div className={styles.officeEditor} style={{ display: isActive ? undefined : 'none' }}>
        <OfficeToolbar fileName={fileName} isDirty={false} onSave={handleSave} canSave={false} formatIcon={<Table2 size={16} />} editLabel="加载中..." />
        <div className={styles.editorLoading}>
          <div className={styles.loadingSpinner} />
          <span>正在加载 Excel 文档...</span>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className={styles.officeEditor} style={{ display: isActive ? undefined : 'none' }}>
        <OfficeToolbar fileName={fileName} isDirty={false} onSave={handleSave} canSave={false} formatIcon={<Table2 size={16} />} editLabel="加载失败" />
        <div className={styles.editorError}>
          <span>加载失败: {error}</span>
        </div>
      </div>
    );
  }

  if (data === null) {
    return (
      <div className={styles.officeEditor} style={{ display: isActive ? undefined : 'none' }}>
        <OfficeToolbar fileName={fileName} isDirty={false} onSave={handleSave} canSave={false} formatIcon={<Table2 size={16} />} editLabel="无数据" />
        <div className={styles.editorError}>
          <span>无法加载文档</span>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.officeEditor} style={{ display: isActive ? undefined : 'none' }}>
      <OfficeToolbar
        fileName={fileName}
        isDirty={isDirty}
        onSave={handleSave}
        canSave={isDirty}
        formatIcon={<Table2 size={16} />}
        editLabel="可编辑"
      />
      <div className={styles.excelContainer}>
        <ExcelGrid
          data={data}
          onChange={handleChange}
        />
      </div>
    </div>
  );
};

