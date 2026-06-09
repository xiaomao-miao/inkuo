import { useCallback } from 'react';
import { persistDocument } from '../../services/documentSave';

export function useDocumentSave(selectedFile: string | null, currentContent: string, isDirty: boolean) {
  return useCallback(async () => {
    await persistDocument({
      path: selectedFile,
      content: currentContent,
      isDirty,
    });
  }, [selectedFile, currentContent, isDirty]);
}
