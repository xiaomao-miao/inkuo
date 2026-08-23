import { useCallback, useEffect, useRef } from 'react';
import { useTauriEvent } from '../../hooks/useTauriEvent';
import { useSidebarStore } from '../../store';
import { areFilePathsEqual, resolveWorkspaceFilePath } from '../../utils/path';

interface FileWrittenPayload {
  path?: string;
}

interface SemanticFileChangePayload {
  type?: 'Created' | 'Modified' | 'Deleted';
  data?: { path?: string };
}

const EXTERNAL_REFRESH_DEBOUNCE_MS = 90;

/** Extract the path from both file event shapes emitted by the Rust side. */
export function externalFileEventPath(
  payload: FileWrittenPayload | SemanticFileChangePayload | null | undefined,
): string {
  if (!payload) return '';
  if ('path' in payload && typeof payload.path === 'string') return payload.path;
  return 'data' in payload && typeof payload.data?.path === 'string' ? payload.data.path : '';
}

export function useExternalFileSync(selectedFile: string | null, onRefreshRequired: () => void) {
  const refreshRef = useRef(onRefreshRequired);
  refreshRef.current = onRefreshRequired;
  const refreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleFileWritten = useCallback((payload: FileWrittenPayload | SemanticFileChangePayload) => {
    if (!selectedFile) return;

    const rawChangedPath = externalFileEventPath(payload);
    if (!rawChangedPath) return;
    const workspacePath = useSidebarStore.getState().workspacePath;
    const changedPath = resolveWorkspaceFilePath(rawChangedPath, workspacePath);
    const openPath = resolveWorkspaceFilePath(selectedFile, workspacePath);
    if (!areFilePathsEqual(changedPath, openPath)) return;

    // A successful AI write normally produces both `file-change` and
    // `file-written`. Coalesce that pair into one reload; loading the same
    // DOCX twice concurrently is expensive and can wedge the canvas editor.
    if (refreshTimerRef.current !== null) {
      clearTimeout(refreshTimerRef.current);
    }
    refreshTimerRef.current = setTimeout(() => {
      refreshTimerRef.current = null;
      refreshRef.current();
    }, EXTERNAL_REFRESH_DEBOUNCE_MS);
  }, [selectedFile]);

  useTauriEvent('file-written', handleFileWritten);
  useTauriEvent('file-change', handleFileWritten);

  useEffect(() => () => {
    if (refreshTimerRef.current !== null) {
      clearTimeout(refreshTimerRef.current);
      refreshTimerRef.current = null;
    }
  }, []);
}
