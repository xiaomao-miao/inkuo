import { useCallback, useRef } from 'react';
import { useAIPanelStore } from '../../store';

export function useTextStreaming() {
  const streamingContentRef = useRef<Record<string, string>>({});
  const pendingTextDeltasRef = useRef<Record<string, string>>({});
  const flushTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingFlushRef = useRef<Set<string>>(new Set());

  const flushTextDeltas = useCallback(() => {
    const deltas = pendingTextDeltasRef.current;
    const toFlush = [...pendingFlushRef.current];
    if (toFlush.length === 0) return;

    pendingTextDeltasRef.current = {};
    pendingFlushRef.current = new Set();
    flushTimeoutRef.current = null;

    useAIPanelStore.setState((state) => {
      const deltaMap = new Map(toFlush.map((id) => [id, deltas[id]]));

      return {
        sessions: state.sessions.map((session) => {
          const sessionMessageIds = toFlush.filter((id) => session.messages.some((message) => message.id === id));
          if (sessionMessageIds.length === 0) return session;

          const updatedMessages = session.messages.map((message) => {
            const delta = deltaMap.get(message.id);
            if (!delta) return message;

            const items = message.outputItems;
            const lastItem = items[items.length - 1];
            if (lastItem && lastItem.type === 'text') {
              const updated = { ...lastItem, content: lastItem.content + delta };
              return { ...message, outputItems: [...items.slice(0, -1), updated] };
            }

            return {
              ...message,
              outputItems: [...items, { type: 'text' as const, content: delta, isPendingMarkdown: true }],
            };
          });

          return { ...session, messages: updatedMessages };
        }),
      };
    });
  }, []);

  const scheduleTextFlush = useCallback(() => {
    if (flushTimeoutRef.current !== null) return;
    flushTimeoutRef.current = setTimeout(flushTextDeltas, 16);
  }, [flushTextDeltas]);

  const appendTextDelta = useCallback((messageId: string, content: string) => {
    const currentAccumulated = streamingContentRef.current[messageId] || '';
    streamingContentRef.current[messageId] = currentAccumulated + content;

    pendingTextDeltasRef.current[messageId] =
      (pendingTextDeltasRef.current[messageId] || '') + content;
    pendingFlushRef.current.add(messageId);

    scheduleTextFlush();
  }, [scheduleTextFlush]);

  const resetTextStreaming = useCallback(() => {
    if (flushTimeoutRef.current !== null) {
      clearTimeout(flushTimeoutRef.current);
      flushTimeoutRef.current = null;
    }
    pendingTextDeltasRef.current = {};
    pendingFlushRef.current = new Set();
    streamingContentRef.current = {};
  }, []);

  return {
    streamingContentRef,
    flushTextDeltas,
    appendTextDelta,
    resetTextStreaming,
  };
}
