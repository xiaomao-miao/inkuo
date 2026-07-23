import { useEffect, useRef } from 'react';
import { useSidebarStore } from '../store';
import { useAIPanelStore } from '../store';
import { createNewSession } from '../store/aiPanelReducers';
import { loadSnapshot } from '../services/workspace';
import type { WorkspaceSnapshot } from '../store';
import type { TodoSnapshot } from '../types';

/**
 * On app startup, the sidebarStore is hydrated from localStorage with a
 * `workspacePath`. If that path has a saved workspace snapshot in the Rust
 * backend, load it and restore the AI sessions so the user sees their
 * chat history immediately — not just after switching workspaces.
 *
 * This runs exactly once (tracked via a ref to avoid double-firing on
 * StrictMode double-invocation), after the initial render, and only when
 * a `workspacePath` is already set (i.e. not on the welcome page).
 *
 * The existing snapshot autosave (`useWorkspaceSnapshotAutosave`) continues
 * to keep the backend file up-to-date as the user chats.
 */
export function useInitialSnapshotLoader(): void {
  const workspacePath = useSidebarStore((s) => s.workspacePath);
  const hasLoadedRef = useRef(false);

  useEffect(() => {
    if (!workspacePath || hasLoadedRef.current) return;
    hasLoadedRef.current = true;

    const restore = async () => {
      try {
        const snapshot: WorkspaceSnapshot | null = await loadSnapshot(workspacePath);
        if (!snapshot) return;

        const sessions =
          snapshot.aiSessions.length > 0 ? snapshot.aiSessions : [createNewSession(1)];
        const activeId = snapshot.activeSessionId ?? sessions[0].id;
        // If the persisted activeSession points at a session that no
        // longer exists or has been archived, fall back to the first
        // non-archived session, then to the head of the list.
        const stillActive = sessions.some((s) => s.id === activeId);
        const resolvedActiveId = stillActive
          ? activeId
          : sessions.find((s) => !s.archived)?.id ?? sessions[0].id;

        // Restore the in-flight `update_todo` chip — same defensive
        // prune as `switchWorkspace`, since the snapshot could
        // (theoretically) reference a session id we no longer have.
        const sessionIds = new Set(sessions.map((s) => s.id));
        const todoSnapshotBySession: Record<string, TodoSnapshot> = {};
        if (snapshot.todoSnapshotBySession) {
          for (const [id, snap] of Object.entries(snapshot.todoSnapshotBySession)) {
            if (sessionIds.has(id)) todoSnapshotBySession[id] = snap;
          }
        }

        useAIPanelStore.setState({
          sessions,
          activeSessionId: resolvedActiveId,
          todoSnapshotBySession,
        });
      } catch (err) {
        console.warn('[useInitialSnapshotLoader] failed to restore sessions:', err);
      }
    };

    void restore();
  }, [workspacePath]);
}
