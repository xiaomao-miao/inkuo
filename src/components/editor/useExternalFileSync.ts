import { useCallback } from 'react';
import { useTauriEvent } from '../../hooks/useTauriEvent';

export function useExternalFileSync(selectedFile: string | null, onRefreshRequired: () => void) {
  const handleFileWritten = useCallback((payload: { path: string }) => {
    if (!selectedFile) return;

    const changedPath = payload.path || '';
    if (changedPath === selectedFile) {
      onRefreshRequired();
    }
  }, [selectedFile, onRefreshRequired]);

  useTauriEvent('file-written', handleFileWritten);
}
