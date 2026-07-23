import type {
  ChatSession,
  CurrentDiff,
  SearchResult,
} from '../../types';
import { updateMessages } from '../../store/aiPanelReducers';
import type { AIPanelState } from '../../store/aiPanelStore.types';
import { mapSessionIfRelevant } from './streamReducerHelpers';

/**
 * Walk the message's outputItems and flip any reasoning items that are
 * still streaming (`completed` unset / false) to `completed: true`.
 *
 * Without this, a stream that ends mid-think — either because the model
 * only emits `reasoning_content` (no follow-up `content`) or because the
 * user cancelled while reasoning was still flowing — leaves the reasoning
 * block stuck on "正在思考…" forever, since the only completion path
 * (a non-reasoning text delta following it) never fires.
 */
function finalizeUncompletedReasoning<T extends { outputItems: { type: string; completed?: boolean }[] }>(
  message: T,
): { outputItems: T['outputItems'] } | null {
  let changed = false;
  const next = message.outputItems.map((item) => {
    if (item.type === 'reasoning' && !item.completed) {
      changed = true;
      return { ...item, completed: true };
    }
    return item;
  });
  return changed ? { outputItems: next as T['outputItems'] } : null;
}

function withSessionByMessageId(
  state: AIPanelState,
  sessionId: string,
  messageId: string,
  mutate: (session: ChatSession) => ChatSession,
): AIPanelState {
  return mapSessionIfRelevant(state, (s) => s.id === sessionId, (session) => {
    if (!session.messages.some((message) => message.id === messageId)) {
      return session;
    }
    return mutate(session);
  });
}

export function applyMessageSearchResults(
  state: AIPanelState,
  sessionId: string,
  messageId: string,
  results: SearchResult[],
): AIPanelState {
  return withSessionByMessageId(state, sessionId, messageId, (session) =>
    updateMessages(session, messageId, (message) => ({
      ...message,
      searchResults: results,
    }))
  );
}

export function finalizeStreamingMessage(
  state: AIPanelState,
  sessionId: string,
  messageId: string,
  finalContent: string,
): AIPanelState {
  return withSessionByMessageId(state, sessionId, messageId, (session) => {
    const updated = updateMessages(session, messageId, (message) => {
      const reasoningFix = finalizeUncompletedReasoning(message);
      return {
        ...message,
        content: finalContent,
        ...(reasoningFix ?? {}),
      };
    });
    return { ...updated, isStreaming: false };
  });
}

export function applyStreamingError(
  state: AIPanelState,
  sessionId: string,
  messageId: string,
  error: string,
): AIPanelState {
  return withSessionByMessageId(state, sessionId, messageId, (session) => {
    const updated = updateMessages(session, messageId, (message) => {
      const reasoningFix = finalizeUncompletedReasoning(message);
      return {
        ...message,
        content: error,
        ...(reasoningFix ?? {}),
      };
    });
    return { ...updated, isStreaming: false };
  });
}

export function applyMessageDiff(
  state: AIPanelState,
  sessionId: string,
  messageId: string,
  diff: CurrentDiff | null,
): AIPanelState {
  return withSessionByMessageId(state, sessionId, messageId, (session) =>
    updateMessages(session, messageId, (message) => ({
      ...message,
      diff: diff ?? undefined,
    }))
  );
}
