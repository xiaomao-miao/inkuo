import { useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useEditorStore, useSidebarStore } from '../../store';

export function useDocumentSave(selectedFile: string | null, currentContent: string, isDirty: boolean) {
  const { markSaved, updateTabDirty } = useEditorStore();
  const { setOpenTabDirty } = useSidebarStore();

  return useCallback(async () => {
    if (!selectedFile || !isDirty) return;

    try {
      await invoke('write_document', {
        path: selectedFile,
        content: currentContent,
      });
      markSaved(selectedFile);
      updateTabDirty(selectedFile, false);
      setOpenTabDirty(selectedFile, false);
    } catch (err) {
      console.error('Failed to save document:', err);
    }
  }, [selectedFile, currentContent, isDirty, markSaved, updateTabDirty, setOpenTabDirty]);
}
