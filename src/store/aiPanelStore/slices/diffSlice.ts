//! Diff-acceptance slice of the AI panel store.
//!
//! Owns the current/pending diffs, the message-attached diff state, and
//! the per-hunk accept/reject actions. The slice is parameterised on the
//! editor-level `DiffApplicationActions` so the actual apply/reject path
//! stays in `editorStore` — this file only orchestrates the state
//! transitions and persistence.

import type {
  AIPanelState,
  AIPanelStateCreator,
  DiffApplicationActions,
} from '../../aiPanelStore.types';
import {
  setMessageDiffState,
  updatePendingDiffHunks,
  updatePendingDiffState,
  updateSessionState,
  updateSessions,
} from '../../aiPanelReducers';

export const createDiffSlice = (
  applyDiffActions: DiffApplicationActions,
): AIPanelStateCreator<Pick<AIPanelState, 'setCurrentDiff' | 'setMessageDiff' | 'setPendingDiff' | 'setDiffFromToolResult' | 'acceptHunk' | 'rejectHunk' | 'acceptAllHunks' | 'rejectAllHunks'>> => (set) => ({
  setCurrentDiff: (sessionId, diff) =>
    set((state) => ({
      sessions: updateSessionState(state.sessions, sessionId, { currentDiff: diff }),
    })),
  setMessageDiff: (sessionId, messageId, diff) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) =>
        setMessageDiffState(session, messageId, diff)
      ),
    })),
  setPendingDiff: (sessionId, diff) =>
    set((state) => ({
      sessions: updatePendingDiffState(state.sessions, sessionId, diff),
    })),
  setDiffFromToolResult: (sessionId, diff) =>
    set((state) => ({
      sessions: updatePendingDiffState(state.sessions, sessionId, diff),
    })),
  acceptHunk: (sessionId, hunkId) =>
    set((state) => {
      const session = state.sessions.find((s) => s.id === sessionId);
      const diff = session?.pendingDiff;
      if (!diff) return state;

      const hunk = diff.hunks.find((h) => h.id === hunkId);
      if (!hunk) return state;

      if (diff.filePath) {
        applyDiffActions.applyHunk(diff.filePath, hunkId);
      }

      const remainingHunks = diff.hunks.filter((h) => h.id !== hunkId);
      return {
        sessions: updateSessions(state.sessions, sessionId, (session) => ({
          ...session,
          pendingDiff: remainingHunks.length > 0
            ? { ...session.pendingDiff!, hunks: remainingHunks }
            : null,
        })),
      };
    }),
  rejectHunk: (sessionId, hunkId) =>
    set((state) => {
      const diff = state.sessions.find((s) => s.id === sessionId)?.pendingDiff;
      if (diff?.filePath) applyDiffActions.rejectHunk(diff.filePath, hunkId);
      return {
        sessions: updateSessions(state.sessions, sessionId, (session) =>
          updatePendingDiffHunks(session, hunkId)
        ),
      };
    }),
  acceptAllHunks: (sessionId) =>
    set((state) => {
      const session = state.sessions.find((s) => s.id === sessionId);
      const diff = session?.pendingDiff;
      if (diff?.filePath) {
        applyDiffActions.applyAllHunks(diff.filePath);
      }
      return {
        sessions: updateSessions(state.sessions, sessionId, (session) => ({
          ...session,
          pendingDiff: null,
        })),
      };
    }),
  rejectAllHunks: (sessionId) =>
    set((state) => {
      const diff = state.sessions.find((s) => s.id === sessionId)?.pendingDiff;
      if (diff?.filePath) applyDiffActions.rejectAllHunks(diff.filePath);
      return {
        sessions: updateSessions(state.sessions, sessionId, (session) => ({
          ...session,
          pendingDiff: null,
        })),
      };
    }),
});
