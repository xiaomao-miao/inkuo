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
    lastActivityAt: now,
    mode: 'ask',
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

/**
 * Splice `prefix` back in front of the visible content for a message's
 * trailing text OutputItem (or the message's `content` field if the message
 * has no outputItems), and clear `truncatedPrefix` on the message / item.
 *
 * If `keepTail` is provided and the visible content is longer than
 * `keepTail`, only the trailing `keepTail` chars stay rendered — the rest
 * is folded back into `truncatedPrefix` so the DOM stays bounded.
 */
export function spliceMessagePrefix(
  message: ChatMessage,
  prefix: string,
  keepTail?: number,
): ChatMessage {
  if (!prefix) return message;
  const items = message.outputItems;
  const lastItem = items[items.length - 1];

  if (lastItem && lastItem.type === 'text') {
    const restored = prefix + lastItem.content;
    let content = restored;
    let leftover = '';
    if (typeof keepTail === 'number' && content.length > keepTail) {
      const headLen = content.length - keepTail;
      leftover = content.slice(0, headLen);
      content = content.slice(headLen);
    }
    const updatedItem = {
      ...lastItem,
      content,
      truncatedPrefix: leftover || undefined,
    };
    return { ...message, outputItems: [...items.slice(0, -1), updatedItem] };
  }

  // No text OutputItem — fall back to the legacy `content` field.
  const restored = prefix + (message.content || '');
  let content = restored;
  let leftover = '';
  if (typeof keepTail === 'number' && content.length > keepTail) {
    const headLen = content.length - keepTail;
    leftover = content.slice(0, headLen);
    content = content.slice(headLen);
  }
  return {
    ...message,
    content,
    truncatedPrefix: leftover || undefined,
  };
}

/**
 * Move the head of the message's visible content into `truncatedPrefix` so
 * the DOM shrinks. Used by the lazy-load affordance to collapse the message
 * back to its tail window.
 */
export function collapseMessageHead(
  message: ChatMessage,
  keepTail: number,
): ChatMessage {
  const items = message.outputItems;
  const lastItem = items[items.length - 1];

  if (lastItem && lastItem.type === 'text') {
    const full = lastItem.content;
    if (full.length <= keepTail) return message;
    const trim = full.length - keepTail;
    const nextPrefix = (lastItem.truncatedPrefix ?? '') + full.slice(0, trim);
    return {
      ...message,
      outputItems: [
        ...items.slice(0, -1),
        { ...lastItem, content: full.slice(trim), truncatedPrefix: nextPrefix },
      ],
    };
  }

  const full = message.content || '';
  if (full.length <= keepTail) return message;
  const trim = full.length - keepTail;
  return {
    ...message,
    content: full.slice(trim),
    truncatedPrefix: (message.truncatedPrefix ?? '') + full.slice(0, trim),
  };
}

/**
 * Mark the oldest messages in a session as collapsed so the renderer can
 * swap them for a single placeholder card. Returns the session unchanged
 * when no collapse is needed.
 *
 * Strategy: keep the last `keepTail` (default = SESSION_VIRTUALIZE_THRESHOLD)
 * messages fully rendered; everything earlier is flagged with
 * `collapsed: true`. The full data (content, outputItems, toolCalls) is
 * NOT mutated, so restoring later is just an object-shape flag flip.
 */
export function collapseOldSessionMessages(
  session: ChatSession,
  keepTail: number,
): ChatSession {
  const messages = session.messages;
  if (messages.length <= keepTail) return session;
  const collapseCount = messages.length - keepTail;
  let touched = false;
  const next = messages.map((message, idx) => {
    if (idx >= collapseCount) return message;
    if (message.collapsed) return message;
    touched = true;
    return { ...message, collapsed: true as const };
  });
  if (!touched) return session;
  return { ...session, messages: next };
}

/**
 * Un-collapse the oldest `revealCount` previously-collapsed messages so
 * they render again. Used by the placeholder's "load earlier" affordance.
 *
 * `revealCount` defaults to `SESSION_VIRTUALIZE_EXPAND_BATCH`. We never
 * cross into the always-live tail — collapsed messages are always older
 * than the live window.
 */
export function expandCollapsedSessionMessages(
  session: ChatSession,
  revealCount: number,
): ChatSession {
  const messages = session.messages;
  let touched = false;
  let revealedSoFar = 0;
  const next = messages.map((message) => {
    if (!message.collapsed) return message;
    if (revealedSoFar >= revealCount) return message;
    revealedSoFar += 1;
    touched = true;
    const { collapsed: _collapsed, ...rest } = message;
    void _collapsed;
    return { ...rest } as ChatMessage;
  });
  if (!touched) return session;
  return { ...session, messages: next };
}

/**
 * Hard-collapse every currently-expanded history placeholder. Called when
 * the user starts a new turn (sends a message) so the live DOM stays
 * bounded while the new stream renders. This is the "新问题触发时立即
 * 卸载旧消息" behavior the user explicitly requested.
 */
export function hardCollapseSessionHistory(session: ChatSession): ChatSession {
  const messages = session.messages;
  let touched = false;
  const next = messages.map((message) => {
    if (!message.collapsed) return message;
    touched = true;
    return { ...message, collapsed: true as const };
  });
  if (!touched) return session;
  return { ...session, messages: next };
}
