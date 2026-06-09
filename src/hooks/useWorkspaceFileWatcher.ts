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
    let disposed = false;

    const setupWatcher = async () => {
      if (!workspacePath) return;

      try {
        await invoke('watch_directory', { path: workspacePath });
        if (disposed) {
          await invoke('unwatch_directory', { path: workspacePath }).catch((error) => {
            reportError('workspace-file-watcher-cleanup', error);
          });
          return;
        }

        watchingPath = workspacePath;

        const unlistenFn = await listen<FileChangePayload>('file-change', (event) => {
          onFileChange(event.payload);
        });

        if (disposed) {
          unlistenFn();
          await invoke('unwatch_directory', { path: workspacePath }).catch((error) => {
            reportError('workspace-file-watcher-cleanup', error);
          });
          return;
        }

        unlisten = unlistenFn;
      } catch (error) {
        reportError('workspace-file-watcher-setup', error);
      }
    };

    void setupWatcher();

    return () => {
      disposed = true;
      if (unlisten) {
        unlisten();
      }
      if (watchingPath) {
        void invoke('unwatch_directory', { path: watchingPath }).catch((error) => {
          reportError('workspace-file-watcher-cleanup', error);
        });
      }
    };
  }, [workspacePath, onFileChange]);
}
