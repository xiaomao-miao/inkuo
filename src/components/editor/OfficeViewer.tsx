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

  const enabled = true;
  const currentCompletion = useInlineCompleteStore((s) => s.currentCompletion);
  const isLoading = useInlineCompleteStore((s) => s.isLoading);
  const inlineError = useInlineCompleteStore((s) => s.error);

  useEffect(() => {
    if (import.meta.env.DEV) {
      console.log('[WordEditor] mounted, filePath:', filePath, 'isActive:', isActive);
    }
  }, []);

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

  const { setOpenTabDirty } = useSidebarStore();
  const officeBufferVersion = useEditorStore(s => s.documentContents[filePath]?.officeBufferVersion ?? 0);
  const { setDocxBuffer } = useEditorStore();

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
        console.error('Failed to load Word document:', err);
        setError(String(err));
      } finally {
        setLoading(false);
      }
    };
    loadFromDiskRef.current = doLoad;
  }, [filePath, setOpenTabDirty, setDocxBuffer]);

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

interface ExcelEditorProps {
  filePath: string;
  fileName: string;
  initialData: string[][] | null;
  isActive: boolean;
}

export const ExcelEditor: React.FC<ExcelEditorProps> = ({
  filePath,
  fileName,
  initialData,
  isActive,
}) => {
  const hasLoadedRef = useRef(false);

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
  const originalDataJsonRef = useRef<string>(JSON.stringify(initialData ?? []));

  useEffect(() => {
    originalDataJsonRef.current = JSON.stringify(originalData ?? []);
  }, [originalData]);

  const { setOpenTabDirty } = useSidebarStore();
  const officeBufferVersion = useEditorStore(s => s.documentContents[filePath]?.officeBufferVersion ?? 0);
  const { setExcelData } = useEditorStore();
  const excelLastVersionRef = useRef(-1);

  const loadFromDiskRef = useRef<() => Promise<void>>(() => Promise.resolve());

  useEffect(() => {
    const doLoad = async () => {
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
    };
    loadFromDiskRef.current = doLoad;
  }, [filePath, setOpenTabDirty, setExcelData]);

  // Re-read from disk when the backing file version changes.
  useEffect(() => {
    if (excelLastVersionRef.current >= officeBufferVersion) return;
    excelLastVersionRef.current = officeBufferVersion;

    if (officeBufferVersion > 0) {
      loadFromDiskRef.current();
      return;
    }

    if (initialData !== null) {
      setData(initialData);
      setOriginalData(initialData);
      setLoading(false);
      return;
    }

    loadFromDiskRef.current();
  }, [officeBufferVersion, initialData]);

  // ── Sync dirty state to sidebar ─────────────────────────────────────────
  useEffect(() => {
    if (isActive && isDirty) {
      setOpenTabDirty(filePath, true);
    }
  }, [isActive, isDirty, filePath, setOpenTabDirty]);

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

