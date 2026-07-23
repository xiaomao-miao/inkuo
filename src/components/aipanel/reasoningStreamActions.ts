import type { AIPanelState } from '../../store/aiPanelStore.types';
import { filterSessionsWithMessages } from './streamReducerHelpers';

type ReasoningItem = {
  type: 'reasoning';
  content: string;
  isPendingMarkdown?: boolean;
  truncatedPrefix?: string;
  /**
   * Stable id used to track per-block UI state. Assigned at creation
   * time (first delta for this block) so subsequent flushes on the same
   * block reuse the same id and the user's "expanded/collapsed" choice
   * survives across re-renders.
   */
  reasoningId?: string;
  completed?: boolean;
};

function isReasoningItem(item: unknown): item is ReasoningItem {
  return !!item && typeof item === 'object' && (item as { type?: string }).type === 'reasoning';
}

/**
 * Apply a batch of reasoning deltas to the AI panel store.
 *
 * Reasoning deltas are routed to the trailing reasoning OutputItem of the
 * matching message (or a new one is appended). The `postProcess` hook lets
 * callers trim the head of overgrown blocks — the same trick we use for
 * `text`, but tuned for reasoning which is typically much longer and can
 * dominate the DOM.
 */
export function applyStreamingReasoningDeltas(
  state: AIPanelState,
  deltaMap: Map<string, string>,
  postProcess?: (item: ReasoningItem) => ReasoningItem,
): AIPanelState {
  const relevantSessions = filterSessionsWithMessages(state.sessions, deltaMap);
  if (relevantSessions.length === 0) return state;

  return {
    ...state,
    sessions: state.sessions.map((session) => {
      if (!relevantSessions.includes(session)) return session;
      return {
        ...session,
        messages: session.messages.map((message) => {
          const delta = deltaMap.get(message.id);
          if (!delta) return message;

          const items = message.outputItems;
          const lastItem = items[items.length - 1];

          if (lastItem && isReasoningItem(lastItem)) {
            const next: ReasoningItem = {
              ...lastItem,
              content: lastItem.content + delta,
            };
            const updated = postProcess ? postProcess(next) : next;
            return { ...message, outputItems: [...items.slice(0, -1), updated] };
          }

          // Starting a new reasoning block — assign a fresh stable id so
          // the UI can track this block's collapse state independently of
          // any siblings in the same message.
          const initial: ReasoningItem = {
            type: 'reasoning' as const,
            content: delta,
            reasoningId: `reasoning-${crypto.randomUUID()}`,
          };
          const updated = postProcess ? postProcess(initial) : initial;
          return { ...message, outputItems: [...items, updated] };
        }),
      };
    }),
  };
}

/**
 * Mark the trailing reasoning OutputItem (if any) of the given message as
 * completed. Completed blocks become eligible for auto-collapse in the UI.
 */
export function completeReasoningItem(
  state: AIPanelState,
  sessionId: string,
  messageId: string,
): AIPanelState {
  return {
    ...state,
    sessions: state.sessions.map((session) =>
      session.id === sessionId
        ? {
            ...session,
            messages: session.messages.map((message) => {
              if (message.id !== messageId) return message;
              const items = message.outputItems;
              const lastIdx = items.length - 1;
              if (lastIdx < 0) return message;
              const last = items[lastIdx];
              if (!last || last.type !== 'reasoning') return message;
              const updated = { ...last, completed: true };
              return { ...message, outputItems: [...items.slice(0, lastIdx), updated] };
            }),
          }
        : session
    ),
  };
}