import { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { Document } from '../../types';
import { useEditorStore, useSidebarStore } from '../../store';

export function useDocumentLoader(selectedFile: string | null) {
  const { setDocumentContent } = useEditorStore();
  const { setOpenTabDirty } = useSidebarStore();
  const forceRefreshRef = useRef<Record<string, number>>({});
  const forceRefreshCount = forceRefreshRef.current[selectedFile || ''] ?? 0;

  useEffect(() => {
    let cancelled = false;

    const loadDocument = async () => {
      if (!selectedFile) return;

      const cached = useEditorStore.getState().documentContents[selectedFile];

      try {
        const result = await invoke<{ document: Document; content: string; mtime: number }>('read_document', {
          path: selectedFile,
        });

        const needsReload = !cached || cached.content === '' || cached.mtime === 0 || result.mtime !== cached.mtime;

        if (!cancelled && needsReload) {
          setDocumentContent(selectedFile, result.document, result.content, result.mtime);
          setOpenTabDirty(selectedFile, false);
        }
      } catch (err) {
        console.error('Failed to load document:', err);
      }
    };

    loadDocument();

    return () => {
      cancelled = true;
    };
  }, [selectedFile, setDocumentContent, setOpenTabDirty, forceRefreshCount]);

  return forceRefreshRef;
}
