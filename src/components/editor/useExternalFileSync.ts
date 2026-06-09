import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';

export function useExternalFileSync(selectedFile: string | null, forceRefreshRef: React.MutableRefObject<Record<string, number>>) {
  useEffect(() => {
    let unlisten: (() => void) | null = null;

    const setup = async () => {
      unlisten = await listen<{ path: string }>('file-written', (event) => {
        if (!selectedFile) return;
        const changedPath = event.payload.path || '';
        if (changedPath === selectedFile) {
          forceRefreshRef.current[selectedFile] = (forceRefreshRef.current[selectedFile] || 0) + 1;
        }
      });
    };

    setup();
    return () => {
      if (unlisten) unlisten();
    };
  }, [selectedFile, forceRefreshRef]);
}
