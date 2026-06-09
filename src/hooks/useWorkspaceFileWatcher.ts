import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { reportError } from '../utils/errors';

interface FileChangePayload {
  type: string;
  data: { path: string };
}

export function useWorkspaceFileWatcher(
  workspacePath: string | null,
  onFileChange: (event: FileChangePayload) => void,
) {
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let watchingPath: string | null = null;

    const setupWatcher = async () => {
      if (!workspacePath) return;

      try {
        await invoke('watch_directory', { path: workspacePath });
        watchingPath = workspacePath;
        unlisten = await listen<FileChangePayload>('file-change', (event) => {
          onFileChange(event.payload);
        });
      } catch (error) {
        reportError('workspace-file-watcher-setup', error);
      }
    };

    setupWatcher();

    return () => {
      if (unlisten) {
        unlisten();
      }
      if (watchingPath) {
        invoke('unwatch_directory', { path: watchingPath }).catch((error) => {
          reportError('workspace-file-watcher-cleanup', error);
        });
      }
    };
  }, [workspacePath, onFileChange]);
}
