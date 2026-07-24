import { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { reportError } from '../utils/errors';
import { isTauriRuntime } from '../utils/tauri';

/**
 * Payload emitted by the Rust file watcher.
 *
 * The backend coalesces a burst of OS events into one quiet window
 * (`notify-debouncer-full`, 200 ms), then drains a per-directory `HashSet`
 * so this payload lists every parent directory whose listing may have
 * changed exactly once. Re-listing the same directory twice is harmless
 * (the second fetch just overwrites the cache with the same content), so
 * the frontend only needs to debounce per-directory and run a follow-up
 * when concurrent fetches overlap.
 */
export interface DirsChangedPayload {
  dirs: string[];
}

export function useWorkspaceFileWatcher(
  workspacePath: string | null,
  onDirsChanged: (event: DirsChangedPayload) => void,
) {
  const onDirsChangedRef = useRef(onDirsChanged);
  onDirsChangedRef.current = onDirsChanged;

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

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

        const unlistenFn = await listen<DirsChangedPayload>('dirs-changed', (event) => {
          onDirsChangedRef.current(event.payload);
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
  }, [workspacePath]);
}
