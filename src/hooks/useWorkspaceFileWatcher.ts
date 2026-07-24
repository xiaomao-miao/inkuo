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

  // Tracks the path the currently-active Rust watcher is bound to. We use a
  // ref so React's StrictMode double-invocation does NOT cause the first
  // mount's cleanup to tear down the watcher that the second mount still
  // owns. Without this guard the dev-mode StrictMode cycle stops the Rust
  // watcher (via `unwatch_directory`) immediately after starting it, and
  // nothing recreates it because the workspace path dependency is stable.
  const activePathRef = useRef<string | null>(null);

  useEffect(() => {
    if (!isTauriRuntime() || !workspacePath) {
      return;
    }

    // If the Rust watcher is already running for this exact path (e.g. from
    // a previous StrictMode mount), do not re-invoke `watch_directory` —
    // the first mount already kicked it off and the second mount shares it.
    if (activePathRef.current === workspacePath) {
      return;
    }

    let unlisten: UnlistenFn | null = null;
    let disposed = false;

    const setupWatcher = async () => {
      try {
        await invoke('watch_directory', { path: workspacePath });

        if (disposed) {
          // StrictMode cancelled us before the await completed. Let the
          // active mount (which owns `activePathRef`) take over.
          return;
        }

        activePathRef.current = workspacePath;

        const unlistenFn = await listen<DirsChangedPayload>('dirs-changed', (event) => {
          onDirsChangedRef.current(event.payload);
        });

        if (disposed) {
          unlistenFn();
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
      // Only tear down the Rust watcher if we (this effect instance)
      // actually installed it. StrictMode runs effect → cleanup → effect,
      // and the FIRST mount's cleanup must NOT stop the watcher that the
      // SECOND mount is still using.
      if (activePathRef.current === workspacePath) {
        if (unlisten) {
          unlisten();
        }
        activePathRef.current = null;
        void invoke('unwatch_directory', { path: workspacePath }).catch((error) => {
          reportError('workspace-file-watcher-cleanup', error);
        });
      }
    };
  }, [workspacePath]);
}
