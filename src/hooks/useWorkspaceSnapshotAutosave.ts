import { useEffect, useRef } from 'react';
import { useSidebarStore, useAIPanelStore } from '../store';
import { saveCurrentSnapshot } from '../services/workspace';

const DEBOUNCE_MS = 1500;

/**
 * Auto-persist the current workspace's tabs + AI sessions to the shared
 * Rust-side snapshot store whenever they change.
 *
 * Uses a debounced save so that a burst of tab operations (open five files
 * at once, close several tabs in a row, AI streaming a long reply) collapses
 * into one disk write. The `WorkspaceBootstrap` component mounts this hook
 * once per window — every other consumer can rely on snapshot durability
 * without any extra wiring.
 *
 * On unmount, we flush any pending debounced save so the user's last action
 * is never lost (matters when the user closes the window — see
 * `WorkspaceBootstrap` for the close-event integration).
 */
export function useWorkspaceSnapshotAutosave(): void {
  const pendingTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    // Subscribe to the parts of the sidebar store that affect the snapshot.
    const unsubSidebar = useSidebarStore.subscribe((state, prev) => {
      if (
        state.workspacePath !== prev.workspacePath ||
        state.openTabs !== prev.openTabs ||
        state.activeTabId !== prev.activeTabId
      ) {
        scheduleSave();
      }
    });

    // Subscribe to AI sessions. `sessions` is a new array reference whenever
    // a message/tool call is appended, so reference comparison is enough.
    // `todoSnapshotBySession` is updated in place on every `update_todo`
    // tool call — its top-level reference also changes, so the same
    // subscription fires for both.
    const unsubAi = useAIPanelStore.subscribe((state, prev) => {
      if (
        state.sessions !== prev.sessions ||
        state.activeSessionId !== prev.activeSessionId ||
        state.todoSnapshotBySession !== prev.todoSnapshotBySession
      ) {
        scheduleSave();
      }
    });

    function scheduleSave() {
      if (pendingTimer.current !== null) {
        clearTimeout(pendingTimer.current);
      }
      pendingTimer.current = setTimeout(flushNow, DEBOUNCE_MS);
    }

    async function flushNow() {
      pendingTimer.current = null;
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
          aiPanel.todoSnapshotBySession,
        );
      } catch (err) {
        console.warn('Workspace snapshot autosave failed:', err);
      }
    }

    // Flush any pending save when the hook unmounts (e.g. window close).
    // Tauri's webview may not give us a reliable `beforeunload`, so we also
    // expose `flushPendingSave` via a synchronous best-effort call.
    return () => {
      unsubSidebar();
      unsubAi();
      if (pendingTimer.current !== null) {
        clearTimeout(pendingTimer.current);
        pendingTimer.current = null;
        // Fire-and-forget final save; window may close before the await
        // resolves, but Rust will write the in-memory state regardless.
        void flushNow();
      }
    };
  }, []);
}

/**
 * Synchronously request the debounced save to flush immediately. Used when
 * the window is about to close so the most recent state hits disk before the
 * webview is destroyed.
 */
export async function flushPendingSnapshotSave(): Promise<void> {
  // The debounce lives inside `useWorkspaceSnapshotAutosave`. We can't reach
  // it from here, so we just do a fresh save with the current state — the
  // Rust side keeps an in-memory copy of the latest snapshot, so even if the
  // debounced write hasn't fired yet, this explicit call is at-least-as-good
  // (and usually more recent).
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
      aiPanel.todoSnapshotBySession,
    );
  } catch (err) {
    console.warn('Final workspace snapshot flush failed:', err);
  }
}
