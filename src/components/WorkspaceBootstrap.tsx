import { useEffect } from 'react';
import { useWorkspaceSnapshotAutosave } from '../hooks/useWorkspaceSnapshotAutosave';
import { useAgentStream } from './aipanel/useAgentStream';

/**
 * Mounted once per window. Wires up:
 *  1. Debounced auto-save of the current workspace's tabs + AI sessions
 *     through `useWorkspaceSnapshotAutosave`. The hook subscribes to the
 *     sidebar and AI panel stores and writes through to the Rust backend
 *     with a 1.5 s debounce, so disk is kept continuously up-to-date.
 *
 *  2. A best-effort final save when the window unloads (component unmount).
 *     We deliberately do NOT register a Tauri `onCloseRequested` handler —
 *     that API blocks window close until the handler resolves, which made
 *     Super+Q / Alt+F4 hang on Linux.  The 1.5 s debounce is short enough
 *     that any state the user touched in the last second or two is already
 *     in Rust's in-memory snapshot map; if the process dies before the next
 *     flush, the worst case is losing the very last keystroke, which is an
 *     acceptable trade-off for keeping window close responsive.
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
