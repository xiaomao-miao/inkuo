import { useCallback } from 'react';
import { useTauriEvent } from '../../hooks/useTauriEvent';

export function useExternalFileSync(selectedFile: string | null, forceRefreshRef: React.MutableRefObject<Record<string, number>>) {
  const handleFileWritten = useCallback((payload: { path: string }) => {
    if (!selectedFile) return;

    const changedPath = payload.path || '';
    if (changedPath === selectedFile) {
      forceRefreshRef.current[selectedFile] = (forceRefreshRef.current[selectedFile] || 0) + 1;
    }
  }, [selectedFile, forceRefreshRef]);

  useTauriEvent('file-written', handleFileWritten);
}
