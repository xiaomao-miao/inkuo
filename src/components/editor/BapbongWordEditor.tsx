import { useState, useCallback, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { BapbongEditorComponent, type BapbongEditorRef } from './BapbongEditor';
import { BapbongToolbar } from './BapbongToolbar';
import { useKeyboardSave } from './useKeyboardSave';
import { useExternalFileSync } from './useExternalFileSync';
import { ExternalFileConflictBanner } from './ExternalFileConflictBanner';
import { decideExternalRefresh } from './externalFileConflict';
import {
  useSidebarStore,
  useEditorStore,
  useEditorHandleStore,
  useNotificationStore,
} from '../../store';
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
  const [hasExternalConflict, setHasExternalConflict] = useState(false);
  const dirtyStateRef = useRef(false);
  const editGenerationRef = useRef(0);
  // Programmatic DOCX loads also emit editor change notifications. Keep those
  // out of the user-dirty path; otherwise an AI refresh immediately turns the
  // freshly-loaded tab dirty again.
  const suppressChangesRef = useRef(true);

  const loadTokenRef = useRef(0);
  const hasInitializedFromCacheRef = useRef(false);
  const explicitReloadInProgressRef = useRef(false);

  const setOpenTabDirty = useSidebarStore((state) => state.setOpenTabDirty);
  const officeBufferVersion = useEditorStore(s => s.documentContents[filePath]?.office.bufferVersion ?? 0);
  const setDocxBuffer = useEditorStore((state) => state.setDocxBuffer);
  const pushNotification = useNotificationStore((state) => state.pushNotification);
  const registerDocumentSaveHandler = useEditorHandleStore(
    (state) => state.registerDocumentSaveHandler,
  );
  const unregisterDocumentSaveHandler = useEditorHandleStore(
    (state) => state.unregisterDocumentSaveHandler,
  );

  // Read bytes from disk
  const readAndApplyBuffer = useCallback(
    async (token: number, discardLocalChanges: boolean): Promise<boolean> => {
      try {
        const data = await invoke<number[]>('read_office_file', { path: filePath });
        if (loadTokenRef.current !== token) return false;
        // The tab may have become dirty after the disk read started. Never
        // commit that async result unless the user explicitly chose to
        // discard the local version.
        if (dirtyStateRef.current && !discardLocalChanges) {
          setHasExternalConflict(true);
          return false;
        }
        const buf = new Uint8Array(data);
        suppressChangesRef.current = true;
        setDocumentBuffer(buf);
        setDocxBuffer(filePath, data);
        editGenerationRef.current = 0;
        dirtyStateRef.current = false;
        setIsDirty(false);
        setOpenTabDirty(filePath, false);
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
  const reloadFromDisk = useCallback(async (discardLocalChanges = false): Promise<boolean> => {
    const token = ++loadTokenRef.current;
    if (discardLocalChanges) explicitReloadInProgressRef.current = true;
    setLoading(true);
    setError(null);
    let applied = false;
    try {
      applied = await readAndApplyBuffer(token, discardLocalChanges);
      return applied;
    } finally {
      if (loadTokenRef.current === token) {
        setLoading(false);
      }
      if (discardLocalChanges) {
        explicitReloadInProgressRef.current = false;
        // A failed read must not strand the user without either their dirty
        // marker or a way to retry the disk version.
        if (!applied && dirtyStateRef.current) setHasExternalConflict(true);
      }
    }
  }, [readAndApplyBuffer]);

  // Both the stream reducer and the semantic file events can announce the
  // same write. Funnel them through one trailing reload so Bapbong parses a
  // new DOCX exactly once per write burst.
  const reloadTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const requestExternalReload = useCallback(() => {
    if (!hasInitializedFromCacheRef.current) return;
    if (reloadTimerRef.current !== null) clearTimeout(reloadTimerRef.current);
    reloadTimerRef.current = setTimeout(() => {
      reloadTimerRef.current = null;
      const decision = decideExternalRefresh(
        dirtyStateRef.current,
        explicitReloadInProgressRef.current,
      );
      if (decision === 'show-conflict') {
        setHasExternalConflict(true);
      } else if (decision === 'reload') {
        void reloadFromDisk(false);
      }
    }, 160);
  }, [reloadFromDisk]);

  useExternalFileSync(filePath, requestExternalReload);

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
    requestExternalReload();
  }, [officeBufferVersion, requestExternalReload]);

  useEffect(() => () => {
    loadTokenRef.current += 1;
    if (reloadTimerRef.current !== null) {
      clearTimeout(reloadTimerRef.current);
      reloadTimerRef.current = null;
    }
  }, []);

  useEffect(() => {
    if (isActive && isDirty) {
      setOpenTabDirty(filePath, true);
    }
  }, [isActive, isDirty, filePath, setOpenTabDirty]);

  // Save
  const handleSave = useCallback(async (): Promise<boolean> => {
    // Read the ref instead of the render-time state so an immediate close
    // after the first edit cannot hit a stale "clean" save closure.
    if (!dirtyStateRef.current) return true;
    const generationAtStart = editGenerationRef.current;
    try {
      const savedBuffer = await editorRef.current?.save();
      if (!savedBuffer) {
        pushNotification({
          kind: 'error',
          title: '保存 Word 文档失败',
          message: 'Word 编辑器尚未就绪，文件仍保持打开。',
        });
        return false;
      }
      const bufferArray = Array.from(new Uint8Array(savedBuffer));
      await invoke('write_office_file', { path: filePath, data: bufferArray });
      setDocxBuffer(filePath, bufferArray);
      setHasExternalConflict(false);
      if (editGenerationRef.current !== generationAtStart) {
        // Disk now contains the snapshot captured at save start, but the live
        // editor has newer input. Keep that newer generation dirty so a close
        // or workspace switch cannot discard it.
        dirtyStateRef.current = true;
        setIsDirty(true);
        setOpenTabDirty(filePath, true);
        return true;
      }
      dirtyStateRef.current = false;
      setIsDirty(false);
      setOpenTabDirty(filePath, false);
      return true;
    } catch (err) {
      const message = reportError('office-word-save', err);
      pushNotification({ kind: 'error', title: '保存 Word 文档失败', message });
      return false;
    }
  }, [filePath, setOpenTabDirty, setDocxBuffer, pushNotification]);

  useEffect(() => {
    registerDocumentSaveHandler(filePath, handleSave);
    return () => unregisterDocumentSaveHandler(filePath, handleSave);
  }, [
    filePath,
    handleSave,
    registerDocumentSaveHandler,
    unregisterDocumentSaveHandler,
  ]);

  useKeyboardSave({ onSave: handleSave, enabled: isDirty && isActive });

  const handleChange = useCallback(() => {
    if (suppressChangesRef.current) return;
    editGenerationRef.current += 1;
    if (dirtyStateRef.current) return;
    dirtyStateRef.current = true;
    setIsDirty(true);
    setOpenTabDirty(filePath, true);
  }, [filePath, setOpenTabDirty]);

  // Editor view ready callback
  const handleEditorViewReady = useCallback((editor: BapbongEditorRef) => {
    editorRef.current = editor;
    // BapbongEditorComponent owns loading `documentBuffer`. Loading it here
    // as well used to race the child's buffer effect and parse every DOCX
    // twice (three times on an external refresh).
  }, []);

  const handleEditorLoad = useCallback(() => {
    suppressChangesRef.current = false;
    editGenerationRef.current = 0;
    dirtyStateRef.current = false;
    setIsDirty(false);
    setOpenTabDirty(filePath, false);
  }, [filePath, setOpenTabDirty]);

  const handleKeepLocalVersion = useCallback(() => {
    setHasExternalConflict(false);
  }, []);

  const handleReloadExternalVersion = useCallback(() => {
    if (reloadTimerRef.current !== null) {
      clearTimeout(reloadTimerRef.current);
      reloadTimerRef.current = null;
    }
    setHasExternalConflict(false);
    void reloadFromDisk(true);
  }, [reloadFromDisk]);

  // Error handler
  const handleError = useCallback((err: Error) => {
    setError(err.message);
    pushNotification({
      kind: 'error',
      title: 'Word 编辑器错误',
      message: err.message,
    });
  }, [pushNotification]);

  // Toolbar handlers
  const handleFind = useCallback(() => {
    editorRef.current?.focus();
  }, []);

  const handlePrint = useCallback(() => {
    editorRef.current?.print();
  }, []);

  const handleZoomIn = useCallback(() => {
    const currentZoom = editorRef.current?.getZoom() ?? 1;
    editorRef.current?.setZoom(Math.min(currentZoom * 1.25, 3));
  }, []);

  const handleZoomOut = useCallback(() => {
    const currentZoom = editorRef.current?.getZoom() ?? 1;
    editorRef.current?.setZoom(Math.max(currentZoom / 1.25, 0.5));
  }, []);

  return (
    <div className={styles.officeEditor}>
      <BapbongToolbar
        editorRef={editorRef}
        fileName={fileName}
        isDirty={isDirty}
        isActive={isActive}
        onSave={handleSave}
        canSave={isDirty && !loading && !error}
        onFind={handleFind}
        onPrint={handlePrint}
        onZoomIn={handleZoomIn}
        onZoomOut={handleZoomOut}
      />

      {hasExternalConflict && (
        <ExternalFileConflictBanner
          fileName={fileName}
          onKeepLocal={handleKeepLocalVersion}
          onReloadFromDisk={handleReloadExternalVersion}
        />
      )}
      
      <div ref={containerRef} className={styles.docxContainer} data-office-editor-root="word">
        <BapbongEditorComponent
          documentBuffer={documentBuffer}
          onChange={handleChange}
          onEditorViewReady={handleEditorViewReady}
          onLoad={handleEditorLoad}
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
