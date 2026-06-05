import { useCallback, useRef } from 'react';
import { useAIPanelStore } from '../../store';

function normalizeStreamChunk(chunk: string): string {
  return chunk.replace(/\r\n?/g, '\n');
}

function stripOpenTrailingTableBlock(text: string): string {
  const normalized = normalizeStreamChunk(text);
  const lines = normalized.split('\n');

  let lastTableStart = -1;
  for (let i = 0; i < lines.length - 1; i += 1) {
    const current = lines[i]?.trim() ?? '';
    const next = lines[i + 1]?.trim() ?? '';
    const looksLikeHeader = /\|/.test(current);
    const looksLikeDivider = /^\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)+\|?$/.test(next);

    if (looksLikeHeader && looksLikeDivider) {
      lastTableStart = i;
    }
  }

  if (lastTableStart === -1) {
    return normalized;
  }

  const tailLines = lines.slice(lastTableStart);
  const tailHasBlankLine = tailLines.some((line, index) => index > 1 && line.trim() === '');
  const tailEndsWithPipeRow = tailLines.length > 0 && /\|/.test(tailLines[tailLines.length - 1] ?? '');

  if (tailHasBlankLine || !tailEndsWithPipeRow) {
    return normalized;
  }

  return lines.slice(0, lastTableStart).join('\n').trimEnd();
}

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
              const nextContent = lastItem.content + delta;
              const updated = {
                ...lastItem,
                content: nextContent,
                isPendingMarkdown: nextContent !== stripOpenTrailingTableBlock(nextContent),
              };
              return { ...message, outputItems: [...items.slice(0, -1), updated] };
            }

            return {
              ...message,
              outputItems: [...items, {
                type: 'text' as const,
                content: delta,
                isPendingMarkdown: delta !== stripOpenTrailingTableBlock(delta),
              }],
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
    const normalizedDelta = normalizeStreamChunk(content);
    const currentAccumulated = streamingContentRef.current[messageId] || '';
    streamingContentRef.current[messageId] = currentAccumulated + normalizedDelta;

    pendingTextDeltasRef.current[messageId] =
      (pendingTextDeltasRef.current[messageId] || '') + normalizedDelta;
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
