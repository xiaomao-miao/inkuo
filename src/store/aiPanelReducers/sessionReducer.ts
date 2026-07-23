// Session-level reducer helpers — creating new sessions, mutating the
// session array, and updating individual messages within a session.
// Split out from the original monolithic `aiPanelReducers.ts` so that
// the lifecycle / metadata and the message-shaping logic live in
// different files.

import type { ChatMessage, ChatSession, CurrentDiff } from '../../types';
import { DEFAULT_CHAT_MODE } from '../../constants/chatModes';

export function createSessionTitle(index: number) {
  return `对话 ${index}`;
}

export function createNewSession(index: number): ChatSession {
  const now = Date.now();
  return {
    id: crypto.randomUUID(),
    title: createSessionTitle(index),
    createdAt: now,
    lastActivityAt: now,
    mode: DEFAULT_CHAT_MODE,
    featureToggles: {},
    messages: [],
    isStreaming: false,
    currentDiff: null,
    activeToolCalls: [],
    pendingDiff: null,
  };
}

/**
 * Returns the session with `lastActivityAt` set to `now`. Used by the
 * store to bubble a session to the top of the history sidebar's
 * sort-by-recency ordering whenever the conversation meaningfully
 * progresses (new message, stream finished, reopened from history).
 */
export function touchSession(session: ChatSession): ChatSession {
  return { ...session, lastActivityAt: Date.now() };
}

export function updateSessions(
  sessions: ChatSession[],
  sessionId: string,
  updater: (session: ChatSession) => ChatSession,
): ChatSession[] {
  return sessions.map((session) =>
    session.id === sessionId ? updater(session) : session
  );
}

export function updateSessionState(
  sessions: ChatSession[],
  sessionId: string,
  patch: Partial<ChatSession>,
): ChatSession[] {
  return updateSessions(sessions, sessionId, (session) => ({
    ...session,
    ...patch,
  }));
}

export function appendSessionMessage(
  sessions: ChatSession[],
  sessionId: string,
  message: ChatMessage,
): ChatSession[] {
  return updateSessions(sessions, sessionId, (session) => ({
    ...session,
    messages: [...session.messages, message],
  }));
}

export function updateSessionMessage(
  sessions: ChatSession[],
  sessionId: string,
  messageId: string,
  updater: (message: ChatMessage) => ChatMessage,
): ChatSession[] {
  return updateSessions(sessions, sessionId, (session) => updateMessages(session, messageId, updater));
}

export function finishSessionMessageStreaming(
  sessions: ChatSession[],
  sessionId: string,
  messageId: string,
  content: string,
): ChatSession[] {
  return updateSessions(sessions, sessionId, (session) => ({
    ...updateMessages(session, messageId, (message) => ({
      ...message,
      content,
    })),
    isStreaming: false,
  }));
}

export function updatePendingDiffState(
  sessions: ChatSession[],
  sessionId: string,
  pendingDiff: CurrentDiff | null,
): ChatSession[] {
  return updateSessionState(sessions, sessionId, { pendingDiff });
}

export function updateMessages(
  session: ChatSession,
  messageId: string,
  updater: (message: ChatMessage) => ChatMessage,
): ChatSession {
  return {
    ...session,
    messages: session.messages.map((message) =>
      message.id === messageId ? updater(message) : message
    ),
  };
}

export function clearSessionConversation(session: ChatSession): ChatSession {
  return {
    ...session,
    messages: [],
    currentDiff: null,
    pendingDiff: null,
    activeToolCalls: [],
  };
}

export function trimSessionMessagesAfter(session: ChatSession, messageId: string): ChatSession {
  const index = session.messages.findIndex((message) => message.id === messageId);
  if (index === -1) return session;
  return {
    ...session,
    messages: session.messages.slice(0, index + 1),
  };
}