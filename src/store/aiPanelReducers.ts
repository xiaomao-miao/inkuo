import type {
  ActiveToolCall,
  ChatMessage,
  ChatSession,
  CurrentDiff,
  OutputItem,
} from '../types';

export type OutputItemMatchKey = { toolCallId: string } | { contentContains: string };

export function createSessionTitle(index: number) {
  return `对话 ${index}`;
}

export function createNewSession(index: number): ChatSession {
  const now = Date.now();
  return {
    id: crypto.randomUUID(),
    title: createSessionTitle(index),
    createdAt: now,
    mode: 'ask',
    messages: [],
    isStreaming: false,
    currentDiff: null,
    activeToolCalls: [],
    pendingDiff: null,
  };
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

export function patchMessageOutputItems(
  message: ChatMessage,
  matchKey: OutputItemMatchKey,
  patch: Partial<OutputItem>,
): ChatMessage {
  const outputItems = message.outputItems.map((item) => {
    const matchesByToolCallId =
      'toolCallId' in matchKey &&
      'toolCallId' in item &&
      item.toolCallId === matchKey.toolCallId;
    const matchesByContent =
      'contentContains' in matchKey &&
      'content' in item &&
      typeof item.content === 'string' &&
      item.content.includes(matchKey.contentContains);

    return matchesByToolCallId || matchesByContent
      ? ({ ...item, ...patch } as OutputItem)
      : item;
  });

  return { ...message, outputItems };
}

export function setMessageDiffState(
  session: ChatSession,
  messageId: string,
  diff: CurrentDiff | null,
): ChatSession {
  return updateMessages(session, messageId, (message) => ({
    ...message,
    diff: diff ?? undefined,
  }));
}

export function setMessageOutputItems(
  session: ChatSession,
  messageId: string,
  outputItems: OutputItem[],
): ChatSession {
  return updateMessages(session, messageId, (message) => ({ ...message, outputItems }));
}

export function addMessageOutputItem(
  session: ChatSession,
  messageId: string,
  outputItem: OutputItem,
): ChatSession {
  return updateMessages(session, messageId, (message) => ({
    ...message,
    outputItems: [...message.outputItems, outputItem],
  }));
}

export function patchMessageOutputState(
  session: ChatSession,
  messageId: string,
  matchKey: OutputItemMatchKey,
  patch: Partial<OutputItem>,
): ChatSession {
  return updateMessages(session, messageId, (message) =>
    patchMessageOutputItems(message, matchKey, patch)
  );
}

export function updatePendingDiffHunks(
  session: ChatSession,
  hunkId: string,
): ChatSession {
  if (!session.pendingDiff) return session;
  const remainingHunks = session.pendingDiff.hunks.filter((hunk) => hunk.id !== hunkId);
  return {
    ...session,
    pendingDiff:
      remainingHunks.length > 0
        ? { ...session.pendingDiff, hunks: remainingHunks }
        : null,
  };
}
