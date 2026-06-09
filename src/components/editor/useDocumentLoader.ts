import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { Document } from '../../types';
import { useEditorStore, useSidebarStore } from '../../store';

export function useDocumentLoader(
  selectedFile: string | null,
  cachedDocument: { content: string; mtime: number } | null,
  refreshToken = 0,
) {
  const { setDocumentContent } = useEditorStore();
  const { setOpenTabDirty } = useSidebarStore();

  useEffect(() => {
    let cancelled = false;

    const loadDocument = async () => {
      if (!selectedFile) return;

      try {
        const result = await invoke<{ document: Document; content: string; mtime: number }>('read_document', {
          path: selectedFile,
        });

        const needsReload = !cachedDocument || cachedDocument.content === '' || cachedDocument.mtime === 0 || result.mtime !== cachedDocument.mtime;

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
  }, [selectedFile, cachedDocument, refreshToken, setDocumentContent, setOpenTabDirty]);
}
