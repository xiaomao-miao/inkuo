import type {
  CurrentDiff,
  SearchResult,
} from '../../types';
import { updateMessages } from '../../store/aiPanelReducers';
import type { AIPanelState } from '../../store/aiPanelStore.types';

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

export function applyMessageSearchResults(
  state: AIPanelState,
  sessionId: string,
  messageId: string,
  results: SearchResult[],
): AIPanelState {
  return {
    ...state,
    sessions: state.sessions.map((session) =>
      session.id === sessionId
        ? updateMessages(session, messageId, (message) => ({
            ...message,
            searchResults: results,
          }))
        : session
    ),
  };
}

export function finalizeStreamingMessage(
  state: AIPanelState,
  sessionId: string,
  messageId: string,
  finalContent: string,
): AIPanelState {
  return {
    ...state,
    sessions: state.sessions.map((session) =>
      session.id === sessionId
        ? {
            ...updateMessages(session, messageId, (message) => {
              const reasoningFix = finalizeUncompletedReasoning(message);
              return {
                ...message,
                content: finalContent,
                ...(reasoningFix ?? {}),
              };
            }),
            isStreaming: false,
          }
        : session
    ),
  };
}

export function applyStreamingError(
  state: AIPanelState,
  sessionId: string,
  messageId: string,
  error: string,
): AIPanelState {
  return {
    ...state,
    sessions: state.sessions.map((session) =>
      session.id === sessionId
        ? {
            ...updateMessages(session, messageId, (message) => {
              const reasoningFix = finalizeUncompletedReasoning(message);
              return {
                ...message,
                content: error,
                ...(reasoningFix ?? {}),
              };
            }),
            isStreaming: false,
          }
        : session
    ),
  };
}

export function applyMessageDiff(
  state: AIPanelState,
  sessionId: string,
  messageId: string,
  diff: CurrentDiff | null,
): AIPanelState {
  return {
    ...state,
    sessions: state.sessions.map((session) =>
      session.id === sessionId
        ? updateMessages(session, messageId, (message) => ({
            ...message,
            diff: diff ?? undefined,
          }))
        : session
    ),
  };
}
