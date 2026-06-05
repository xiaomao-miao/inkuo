import type { AIPanelState } from '../../store/aiPanelStore.types';

export function applyStreamingTextDeltas(
  state: AIPanelState,
  deltaMap: Map<string, string>,
  getPendingMarkdown: (content: string) => boolean,
): AIPanelState {
  return {
    ...state,
    sessions: state.sessions.map((session) => {
      const sessionMessageIds = [...deltaMap.keys()].filter((id) =>
        session.messages.some((message) => message.id === id)
      );
      if (sessionMessageIds.length === 0) return session;

      return {
        ...session,
        messages: session.messages.map((message) => {
          const delta = deltaMap.get(message.id);
          if (!delta) return message;

          const items = message.outputItems;
          const lastItem = items[items.length - 1];
          if (lastItem && lastItem.type === 'text') {
            const nextContent = lastItem.content + delta;
            const updated = {
              ...lastItem,
              content: nextContent,
              isPendingMarkdown: getPendingMarkdown(nextContent),
            };
            return { ...message, outputItems: [...items.slice(0, -1), updated] };
          }

          return {
            ...message,
            outputItems: [
              ...items,
              {
                type: 'text' as const,
                content: delta,
                isPendingMarkdown: getPendingMarkdown(delta),
              },
            ],
          };
        }),
      };
    }),
  };
}
