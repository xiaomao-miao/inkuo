import { useEffect } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useWorkspaceSnapshotAutosave } from '../hooks/useWorkspaceSnapshotAutosave';
import { confirmWindowClose } from '../services/openTabLifecycle';
import { reportError } from '../utils/errors';
import { isTauriRuntime } from '../utils/tauri';
import { useNotificationStore } from '../store';
import { useAgentStream } from './aipanel/useAgentStream';

/**
 * Mounted once per window. Wires up:
 *  1. Debounced auto-save of the current workspace's tabs + AI sessions
 *     through `useWorkspaceSnapshotAutosave`. The hook subscribes to the
 *     sidebar and AI panel stores and writes through to the Rust backend
 *     with a 1.5 s debounce, so disk is kept continuously up-to-date.
 *
 *  2. A native close guard. `preventDefault()` runs synchronously, then the
 *     shared lifecycle asks Save / Don't Save / Cancel without blocking the
 *     native callback. An accepted close is re-issued once behind a bypass
 *     flag, avoiding both silent data loss and recursive close prompts.
 *
 *  3. A best-effort final workspace-snapshot flush on browser unload.
 *
 * Renders nothing.
 */
export function WorkspaceBootstrap(): null {
  useWorkspaceSnapshotAutosave();
  // Stream events must outlive the visible panel. Previously closing the AI
  // panel unmounted its only listener, silently dropping done/error events and
  // leaving the session permanently "running" until an app restart.
  useAgentStream({ mode: 'agent' });

  useEffect(() => {
    if (!isTauriRuntime()) return;

    const appWindow = getCurrentWindow();
    let disposed = false;
    let allowNextClose = false;
    let closeInProgress = false;
    let unlisten: (() => void) | null = null;

    void appWindow.onCloseRequested((event) => {
      if (allowNextClose) return;

      // Tauri requires this to happen before the callback returns. The async
      // dialog/save work below is deliberately detached from the callback.
      event.preventDefault();
      if (closeInProgress) return;
      closeInProgress = true;

      void (async () => {
        try {
          const mayClose = await confirmWindowClose();
          if (!mayClose || disposed) return;

          await flushPendingSnapshotSave();
          if (disposed) return;
          allowNextClose = true;
          try {
            await appWindow.close();
          } catch (error) {
            // The window is still alive, so a future close must go through the
            // guard again instead of inheriting a stale one-shot bypass.
            allowNextClose = false;
            reportError('workspace-window-close', error);
          }
        } catch (error) {
          allowNextClose = false;
          useNotificationStore.getState().pushNotification({
            kind: 'error',
            title: '暂时无法关闭窗口',
            message: reportError('workspace-close-flow', error),
          });
        } finally {
          closeInProgress = false;
        }
      })();
    }).then((disposeListener) => {
      if (disposed) disposeListener();
      else unlisten = disposeListener;
    }).catch((error) => {
      reportError('workspace-close-listener', error);
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const handleBeforeUnload = () => {
      // Fire-and-forget; we cannot await here because the webview is being
      // torn down synchronously. The IPC message will at least reach the
      // Rust side, which holds the snapshot in memory and persists on each
      // mutation.
      void flushPendingSnapshotSave();
    };
    window.addEventListener('beforeunload', handleBeforeUnload);
    return () => {
      window.removeEventListener('beforeunload', handleBeforeUnload);
      void flushPendingSnapshotSave();
    };
  }, []);

  return null;
}

async function flushPendingSnapshotSave(): Promise<void> {
  const { useSidebarStore, useAIPanelStore } = await import('../store');
  const { saveCurrentSnapshot } = await import('../services/workspace');
  const sidebar = useSidebarStore.getState();
  const aiPanel = useAIPanelStore.getState();
  if (!sidebar.workspacePath) return;
  try {
    await saveCurrentSnapshot(
      sidebar.workspacePath,
      sidebar.openTabs,
      sidebar.activeTabId,
      aiPanel.sessions,
      aiPanel.activeSessionId,
    );
  } catch {
    /* best-effort */
  }
}
