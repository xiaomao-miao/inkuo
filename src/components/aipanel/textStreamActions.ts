import type { AIPanelState } from '../../store/aiPanelStore.types';
import { filterSessionsWithMessages } from './streamReducerHelpers';

type TextItem = {
  type: 'text';
  content: string;
  isPendingMarkdown?: boolean;
  truncatedPrefix?: string;
};

function isTextItem(item: unknown): item is TextItem {
  return !!item && typeof item === 'object' && (item as { type?: string }).type === 'text';
}

/**
 * Apply a batch of text deltas to the AI panel store.
 *
 * For each messageId in `deltaMap`, the delta is appended to the trailing
 * text OutputItem of that message (or a new text item is appended if the
 * message's last OutputItem is not text). Each updated text item is then
 * passed through `postProcessTextItem` so callers can apply additional
 * transformations — currently used by `useTextStreaming` to truncate the
 * head of overgrown messages and keep the rendered DOM bounded.
 *
 * Side effect: if the message's last OutputItem is a reasoning block, the
 * first non-reasoning text delta marks it as `completed`, which signals
 * the UI that the reasoning is done streaming and the block is eligible
 * for auto-collapse.
 */
export function applyStreamingTextDeltas(
  state: AIPanelState,
  deltaMap: Map<string, string>,
  getPendingMarkdown: (content: string) => boolean,
  postProcessTextItem?: (item: TextItem) => TextItem,
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

          // If a non-reasoning text delta is being applied after a
          // reasoning block, mark the reasoning block as completed so the
          // UI knows it's safe to auto-collapse. We splice the completed
          // flag in here rather than at flush-all time so a reasoning
          // block whose final `reasoning` event never arrives (e.g. abrupt
          // stream end) still gets finalised.
          let workingItems = items;
          if (lastItem && lastItem.type === 'reasoning' && !lastItem.completed) {
            workingItems = [
              ...items.slice(0, -1),
              { ...lastItem, completed: true },
            ];
          }

          const lastForText = workingItems[workingItems.length - 1];
          if (lastForText && isTextItem(lastForText)) {
            const next: TextItem = {
              ...lastForText,
              content: lastForText.content + delta,
              isPendingMarkdown: getPendingMarkdown(lastForText.content + delta),
            };
            const updated = postProcessTextItem ? postProcessTextItem(next) : next;
            // Preserves `collapsed` because we spread `...message` — collapsed
            // placeholders never receive deltas in practice (the streaming
            // reducer scopes by messageId, not by index), but if a stale
            // event ever targets one, the collapse flag survives so the
            // virtualized placeholder keeps rendering the compact card.
            return { ...message, outputItems: [...workingItems.slice(0, -1), updated] };
          }

          const initial: TextItem = {
            type: 'text' as const,
            content: delta,
            isPendingMarkdown: getPendingMarkdown(delta),
          };
          const updated = postProcessTextItem ? postProcessTextItem(initial) : initial;
          return { ...message, outputItems: [...workingItems, updated] };
        }),
      };
    }),
  };
}