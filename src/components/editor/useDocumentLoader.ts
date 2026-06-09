import { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { Document } from '../../types';
import { useEditorStore, useSidebarStore } from '../../store';
import { reportError } from '../../utils/errors';

export function useDocumentLoader(
  selectedFile: string | null,
  cachedDocument: { content: string; mtime: number } | null,
  refreshToken = 0,
) {
  const setDocumentContent = useEditorStore((state) => state.setDocumentContent);
  const setOpenTabDirty = useSidebarStore((state) => state.setOpenTabDirty);
  const cachedMtime = cachedDocument?.mtime ?? 0;
  const lastLoadedRef = useRef<{ path: string; refreshToken: number; mtime: number } | null>(null);

  useEffect(() => {
    let cancelled = false;

    const loadDocument = async () => {
      if (!selectedFile) return;

      const lastLoaded = lastLoadedRef.current;
      if (
        lastLoaded &&
        lastLoaded.path === selectedFile &&
        lastLoaded.refreshToken === refreshToken &&
        lastLoaded.mtime === cachedMtime
      ) {
        return;
      }

      try {
        const result = await invoke<{ document: Document; content: string; mtime: number }>('read_document', {
          path: selectedFile,
        });

        if (cancelled) return;

        lastLoadedRef.current = {
          path: selectedFile,
          refreshToken,
          mtime: result.mtime,
        };

        const needsReload = !cachedDocument || cachedMtime === 0 || result.mtime !== cachedMtime;
        if (needsReload) {
          setDocumentContent(selectedFile, result.document, result.content, result.mtime);
          setOpenTabDirty(selectedFile, false);
        }
      } catch (err) {
        reportError('document-load', err);
      }
    };

    loadDocument();

    return () => {
      cancelled = true;
    };
  }, [selectedFile, cachedMtime, refreshToken, setDocumentContent, setOpenTabDirty]);
}
