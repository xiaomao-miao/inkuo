import { useState, useCallback, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { BapbongEditorComponent, type BapbongEditorRef } from './BapbongEditor';
import { Save } from 'lucide-react';
import { useKeyboardSave } from './useKeyboardSave';
import { useSidebarStore, useEditorStore, useNotificationStore } from '../../store';
import { reportError } from '../../utils/errors';
import styles from './OfficeViewer.module.css';

interface WordEditorProps {
  filePath: string;
  fileName: string;
  initialBuffer: Uint8Array | null;
  isActive: boolean;
}

/**
 * Word editor using bapbong (canvas-rendered DOCX editor)
 * 
 * Key features:
 * - Multi-column layout support (unlike @eigenpal)
 * - Canvas-based rendering for pixel-accurate documents
 * - Full DOCX feature support
 */
export const BapbongWordEditor: React.FC<WordEditorProps> = ({
  filePath,
  fileName,
  initialBuffer,
  isActive,
}) => {
  const editorRef = useRef<BapbongEditorRef | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const [documentBuffer, setDocumentBuffer] = useState<Uint8Array | null>(() => initialBuffer);
  const [loading, setLoading] = useState<boolean>(() => initialBuffer === null);
  const [error, setError] = useState<string | null>(null);
  const [isDirty, setIsDirty] = useState(false);
  const dirtyStateRef = useRef(false);

  const loadTokenRef = useRef(0);
  const hasInitializedFromCacheRef = useRef(false);

  const setOpenTabDirty = useSidebarStore((state) => state.setOpenTabDirty);
  const officeBufferVersion = useEditorStore(s => s.documentContents[filePath]?.office.bufferVersion ?? 0);
  const setDocxBuffer = useEditorStore((state) => state.setDocxBuffer);
  const pushNotification = useNotificationStore((state) => state.pushNotification);

  // Read bytes from disk
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
        // Reload the editor if it exists
        if (editorRef.current) {
          await editorRef.current.loadDocx(buf.buffer);
        }
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

  // Reload from disk
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

  const loadFromDiskRef = useRef<() => Promise<void>>(reloadFromDisk);
  loadFromDiskRef.current = reloadFromDisk;

  // Initial load
  useEffect(() => {
    if (hasInitializedFromCacheRef.current) return;
    hasInitializedFromCacheRef.current = true;
    if (initialBuffer) {
      setDocumentBuffer(initialBuffer);
      setLoading(false);
    } else {
      void reloadFromDisk();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filePath]);

  // Re-read when external file version changes
  useEffect(() => {
    if (officeBufferVersion === 0) return;
    if (!hasInitializedFromCacheRef.current) return;
    void reloadFromDisk();
  }, [officeBufferVersion, reloadFromDisk]);

  useEffect(() => {
    if (isActive && isDirty) {
      setOpenTabDirty(filePath, true);
    }
  }, [isActive, isDirty, filePath, setOpenTabDirty]);

  // Save
  const handleSave = useCallback(async () => {
    if (!isDirty) return;
    try {
      const savedBuffer = await editorRef.current?.save();
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
    if (dirtyStateRef.current) return;
    dirtyStateRef.current = true;
    setIsDirty(true);
    setOpenTabDirty(filePath, true);
  }, [filePath, setOpenTabDirty]);

  // Editor view ready callback
  const handleEditorViewReady = useCallback((editor: BapbongEditorRef) => {
    editorRef.current = editor;
    // If we have a buffer ready, load it
    if (documentBuffer) {
      editor.loadDocx(documentBuffer.buffer).catch((err) => {
        setError((err as Error).message);
        pushNotification({
          kind: 'error',
          title: '加载 Word 文档失败',
          message: (err as Error).message,
        });
      });
    }
  }, [documentBuffer, pushNotification]);

  // Error handler
  const handleError = useCallback((err: Error) => {
    setError(err.message);
    pushNotification({
      kind: 'error',
      title: 'Word 编辑器错误',
      message: err.message,
    });
  }, [pushNotification]);

  return (
    <div className={styles.officeEditor}>
      {/* Toolbar */}
      <div className={styles.editorToolbar}>
        <div className={styles.toolbarLeft}>
          <span className={styles.fileName}>
            {fileName}
            {isDirty && <span className={styles.dirtyDot}>·</span>}
          </span>
        </div>
        <div className={styles.toolbarRight}>
          <span className={`${styles.editMode} ${isDirty ? styles.dirtyBadge : ''}`}>
            可编辑
          </span>
          <button
            className={`${styles.saveButton} ${isDirty ? styles.dirty : ''}`}
            onClick={handleSave}
            disabled={!isDirty || loading || !!error}
            title="保存 (Ctrl+S)"
          >
            <Save size={14} />
            <span>保存</span>
          </button>
        </div>
      </div>
      
      {/* Editor */}
      <div ref={containerRef} className={styles.docxContainer} data-office-editor-root="word">
        <BapbongEditorComponent
          documentBuffer={documentBuffer}
          onChange={handleChange}
          onEditorViewReady={handleEditorViewReady}
          onError={handleError}
        />
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
    </div>
  );
};
