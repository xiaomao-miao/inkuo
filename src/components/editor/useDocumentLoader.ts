import { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { Document } from '../../types';
import { useEditorStore, useSidebarStore } from '../../store';
import { reportError } from '../../utils/errors';
import { areFilePathsEqual } from '../../utils/path';
import { shouldApplyDiskDocument } from './documentLoadPolicy';

export function useDocumentLoader(
  selectedFile: string | null,
  cachedDocument: { content: string; mtime: number } | null,
  refreshToken = 0,
  allowDirtyReload = false,
  onDirtyConflict?: () => void,
) {
  // Assumptions this hook depends on:
  //
  // 1. mtime is a sufficient change signal.
  //    The Tauri backend sets mtime to the file's reported modification time.
  //    If only metadata (e.g. permissions) changes without the content changing,
  //    the hook will not detect the change. For this app's use pattern this
  //    is acceptable — user edits are the primary trigger and those always
  //    update mtime.
  //
  // 2. mtime granularity matches the editor's write-time granularity.
  //    On some filesystems (e.g. FAT32) mtime has 2-second precision.
  //    Rapid edits within the same 2-second window may not trigger a reload.
  //    This is a known limitation of relying on mtime; compensating mechanisms
  //    (e.g. a manual "force refresh" action) can be added later if needed.
  //
  const setDocumentContent = useEditorStore((state) => state.setDocumentContent);
  const setOpenTabDirty = useSidebarStore((state) => state.setOpenTabDirty);
  const hasCachedDocument = cachedDocument !== null;
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

        const needsReload = !hasCachedDocument || cachedMtime === 0 || result.mtime !== cachedMtime;
        if (needsReload) {
          // Re-check immediately before applying the disk result. The editor
          // may have become dirty while `read_document` was in flight; using
          // the render-time `cachedDocument` alone would silently overwrite
          // those newly typed changes. Initial loads have no local buffer and
          // are always safe, while an explicit user-approved reload may opt in
          // to discarding the dirty buffer.
          const liveTabIsDirty = useSidebarStore.getState().openTabs
            .some((tab) => areFilePathsEqual(tab.path, selectedFile) && tab.isDirty);
          if (!shouldApplyDiskDocument(
            hasCachedDocument,
            liveTabIsDirty,
            allowDirtyReload,
          )) {
            onDirtyConflict?.();
            return;
          }

          setDocumentContent(selectedFile, result.document, result.content, result.mtime);
          setOpenTabDirty(selectedFile, false);
        }

        lastLoadedRef.current = {
          path: selectedFile,
          refreshToken,
          mtime: result.mtime,
        };
      } catch (err) {
        reportError('document-load', err);
      }
    };

    loadDocument();

    return () => {
      cancelled = true;
    };
  }, [
    selectedFile,
    hasCachedDocument,
    cachedMtime,
    refreshToken,
    allowDirtyReload,
    onDirtyConflict,
    setDocumentContent,
    setOpenTabDirty,
  ]);
}
