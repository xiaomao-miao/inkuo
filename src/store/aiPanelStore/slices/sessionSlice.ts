//! Session-lifecycle slice of the AI panel store.
//!
//! Owns the `sessions` map, the active-session pointer, per-session todo
//! snapshots, and the open/closed/deleted-session state. Reads heavily
//! from `get()` to avoid stale snapshots when a session is mutated while
//! another action is in flight.

import type { AIPanelState, AIPanelStateCreator } from '../../aiPanelStore.types';
import type { FeatureToggleMap } from '../../../types';
import {
  createNewSession,
  updateSessions,
  updateSessionState,
} from '../../aiPanelReducers';

export const createSessionSlice: AIPanelStateCreator<Pick<AIPanelState, 'sessions' | 'activeSessionId' | 'todoSnapshotBySession' | 'createSession' | 'deleteSession' | 'closeSession' | 'reopenSession' | 'setActiveSession' | 'setSessionMode' | 'setSessionFeatureToggle' | 'getSession' | 'updateSession' | 'setSessionTodoSnapshot' | 'clearSessionTodoSnapshot'>> = (set, get) => {
  const initialSession = createNewSession(1);

  return {
    sessions: [initialSession],
    activeSessionId: initialSession.id,
    todoSnapshotBySession: {},
    createSession: () => {
      const index = get().sessions.length + 1;
      const session = createNewSession(index);
      set((state) => ({
        sessions: [session, ...state.sessions],
        activeSessionId: session.id,
      }));
      return session.id;
    },
    /**
     * Hard delete. Permanent — the next snapshot save will omit the
     * session. Callers (e.g. HistorySidebar trash) must ask for explicit
     * confirmation first; a mis-click should not destroy history.
     */
    deleteSession: (sessionId) => {
      set((state) => {
        const remaining = state.sessions.filter((session) => session.id !== sessionId);
        const safeRemaining = remaining.length > 0 ? remaining : [createNewSession(1)];
        const nextActiveId =
          state.activeSessionId === sessionId ? safeRemaining[0].id : state.activeSessionId;

        // Also drop the todo panel snapshot for this session — the panel
        // is keyed on session.id, so a leftover snapshot for a deleted
        // session would resurrect in the UI if the user later creates a
        // new session with the same id (we use crypto.randomUUID, so
        // this is mostly defensive, but keeping the map clean avoids
        // confusion during debugging).
        const { [sessionId]: _drop, ...rest } = state.todoSnapshotBySession;
        return {
          sessions: safeRemaining,
          activeSessionId: nextActiveId,
          todoSnapshotBySession: rest,
        };
      });
    },
    /**
     * Soft-close. Marks the session as archived so it falls out of the
     * header chip bar, but the data stays put and is still loaded
     * back from disk after a restart.
     *
     * Invariant: after `closeSession`, `activeSessionId` always points
     * at a non-archived session (or a brand-new empty one). If the user
     * closes every single session we auto-create a fresh empty one
     * so the panel always has an active conversation in view — never
     * a closed one displayed as the "current" session.
     */
    closeSession: (sessionId) => {
      set((state) => {
        const sessions = state.sessions.map((session) =>
          session.id === sessionId ? { ...session, archived: true } : session,
        );

        let nextActiveId = state.activeSessionId;
        if (state.activeSessionId === sessionId) {
          // The session the user just closed was the active one — pick
          // a replacement that's still open. If nothing is open, mint a
          // brand-new empty session so `activeSession` never resolves
          // to an archived/empty-but-displayed state.
          const open = sessions.find((s) => !s.archived);
          if (open) {
            nextActiveId = open.id;
          } else {
            const fresh = createNewSession(sessions.length + 1);
            nextActiveId = fresh.id;
            sessions.unshift(fresh);
          }
        }
        return { sessions, activeSessionId: nextActiveId };
      });
    },
    reopenSession: (sessionId) => {
      set((state) => ({
        // Reopening is an explicit "I'm working on this again" — bump
        // lastActivityAt so it floats to the top of the history list.
        sessions: state.sessions.map((session) =>
          session.id === sessionId
            ? { ...session, archived: undefined, lastActivityAt: Date.now() }
            : session,
        ),
      }));
    },
    setActiveSession: (sessionId) => set({ activeSessionId: sessionId }),
    setSessionMode: (sessionId, mode) =>
      set((state) => ({
        sessions: updateSessionState(state.sessions, sessionId, { mode }),
      })),
    setSessionFeatureToggle: (sessionId, toggleId, enabled) =>
      set((state) => ({
        sessions: updateSessions(state.sessions, sessionId, (session) => {
          const current: FeatureToggleMap = { ...(session.featureToggles ?? {}) };
          if (enabled) {
            current[toggleId] = true;
          } else {
            // Drop the key so the on-disk shape stays compact — a session
            // with every toggle off shouldn't carry an empty `{}` either.
            delete current[toggleId];
          }
          return {
            ...session,
            featureToggles: Object.keys(current).length > 0 ? current : undefined,
          };
        }),
      })),
    getSession: (sessionId) => get().sessions.find((session) => session.id === sessionId),
    updateSession: (sessionId, updater) =>
      set((state) => ({
        sessions: updateSessions(state.sessions, sessionId, updater),
      })),
    setSessionTodoSnapshot: (sessionId, toolCallId, items) =>
      set((state) => ({
        todoSnapshotBySession: {
          ...state.todoSnapshotBySession,
          [sessionId]: {
            items,
            toolCallId,
            updatedAt: Date.now(),
          },
        },
      })),
    clearSessionTodoSnapshot: (sessionId) =>
      set((state) => {
        if (!(sessionId in state.todoSnapshotBySession)) return state;
        const { [sessionId]: _drop, ...rest } = state.todoSnapshotBySession;
        return { todoSnapshotBySession: rest };
      }),
  };
};
