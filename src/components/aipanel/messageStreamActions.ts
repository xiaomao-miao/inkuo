import type {
  CurrentDiff,
  SearchResult,
} from '../../types';
import { updateMessages } from '../../store/aiPanelReducers';
import type { AIPanelState } from '../../store/aiPanelStore.types';

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
            ...updateMessages(session, messageId, (message) => ({
              ...message,
              content: finalContent,
            })),
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
            ...updateMessages(session, messageId, (message) => ({
              ...message,
              content: error,
            })),
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
