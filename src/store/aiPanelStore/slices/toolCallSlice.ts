//! Tool-call slice of the AI panel store.
//!
//! Manages per-session tool-call records (`addToolCall`, `updateToolCall`,
//! `removeToolCall`, `clearToolCalls`). Reads and writes go straight
//! through the immutable `updateToolCalls` reducer so the rest of the
//! store never holds stale tool-call references.

import type { AIPanelState, AIPanelStateCreator } from '../../aiPanelStore.types';
import {
  appendSessionToolCall,
  clearSessionToolCalls,
  removeSessionToolCall,
  updateSessions,
  updateToolCalls,
} from '../../aiPanelReducers';

export const createToolCallSlice: AIPanelStateCreator<Pick<AIPanelState, 'addToolCall' | 'updateToolCall' | 'removeToolCall' | 'clearToolCalls'>> = (set) => ({
  addToolCall: (sessionId, toolCall) =>
    set((state) => ({
      sessions: appendSessionToolCall(state.sessions, sessionId, toolCall),
    })),
  updateToolCall: (sessionId, toolCallId, update) =>
    set((state) => ({
      sessions: updateSessions(state.sessions, sessionId, (session) =>
        updateToolCalls(session, toolCallId, (toolCall) => ({ ...toolCall, ...update }))
      ),
    })),
  removeToolCall: (sessionId, toolCallId) =>
    set((state) => ({
      sessions: removeSessionToolCall(state.sessions, sessionId, toolCallId),
    })),
  clearToolCalls: (sessionId) =>
    set((state) => ({
      sessions: clearSessionToolCalls(state.sessions, sessionId),
    })),
});
