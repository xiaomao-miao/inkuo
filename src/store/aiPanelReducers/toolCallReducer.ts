// Tool-call array helpers — used by the tool-call slice and the stream
// reducer to mutate `ChatSession.activeToolCalls`. Split out from the
// original monolithic `aiPanelReducers.ts` so that session-level
// metadata and tool-call bookkeeping don't share a file.

import type { ActiveToolCall, ChatSession } from '../../types';
import { updateSessionState, updateSessions, updateMessages } from './sessionReducer';

export function appendSessionToolCall(
  sessions: ChatSession[],
  sessionId: string,
  toolCall: ActiveToolCall,
): ChatSession[] {
  return updateSessions(sessions, sessionId, (session) => ({
    ...session,
    activeToolCalls: [...session.activeToolCalls, toolCall],
  }));
}

export function removeSessionToolCall(
  sessions: ChatSession[],
  sessionId: string,
  toolCallId: string,
): ChatSession[] {
  return updateSessions(sessions, sessionId, (session) => ({
    ...session,
    activeToolCalls: session.activeToolCalls.filter((toolCall) => toolCall.id !== toolCallId),
  }));
}

export function clearSessionToolCalls(
  sessions: ChatSession[],
  sessionId: string,
): ChatSession[] {
  return updateSessionState(sessions, sessionId, { activeToolCalls: [] });
}

export function updateToolCalls(
  session: ChatSession,
  toolCallId: string,
  updater: (toolCall: ActiveToolCall) => ActiveToolCall,
): ChatSession {
  return {
    ...session,
    activeToolCalls: session.activeToolCalls.map((toolCall) =>
      toolCall.id === toolCallId ? updater(toolCall) : toolCall
    ),
  };
}

// Re-export `updateMessages` so the slice files can keep importing
// the reducers they need from a single module if they prefer.
export { updateMessages };