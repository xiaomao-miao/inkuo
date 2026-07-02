import { useCallback, useRef } from 'react';
import { useAIPanelStore } from '../../store';
import { applyStreamingReasoningDeltas } from './reasoningStreamActions';
import { TIMING } from '../../constants/timing';

function normalizeStreamChunk(chunk: string): string {
  return chunk.replace(/\r\n?/g, '\n');
}

type ReasoningPending = {
  deltas: Record<string, string>;
  flushTimer: ReturnType<typeof setTimeout> | null;
};

/**
 * Compute a flush interval that grows with the reasoning buffer pressure.
 *
 * Reasoning blocks tend to be much larger than the visible answer, and
 * refreshing the DOM on every chunk during a long chain-of-thought is
 * extremely expensive. We bias toward longer intervals here so the
 * reasoning block streams smoothly without thrashing React.
 */
function computeReasoningFlushIntervalMs(bufferLen: number): number {
  const {
    REASONING_FLUSH_INTERVAL_MIN_MS,
    REASONING_FLUSH_INTERVAL_MAX_MS,
    REASONING_MIN_BUFFER_CHARS_FOR_SLOWDOWN,
    REASONING_MAX_BUFFER_CHARS_BEFORE_FORCE_FLUSH,
  } = TIMING;

  if (bufferLen <= REASONING_MIN_BUFFER_CHARS_FOR_SLOWDOWN) {
    return REASONING_FLUSH_INTERVAL_MIN_MS;
  }
  const span = REASONING_MAX_BUFFER_CHARS_BEFORE_FORCE_FLUSH - REASONING_MIN_BUFFER_CHARS_FOR_SLOWDOWN;
  if (span <= 0) return REASONING_FLUSH_INTERVAL_MAX_MS;
  const over = Math.min(bufferLen, REASONING_MAX_BUFFER_CHARS_BEFORE_FORCE_FLUSH) - REASONING_MIN_BUFFER_CHARS_FOR_SLOWDOWN;
  const ratio = over / span;
  return Math.round(
    REASONING_FLUSH_INTERVAL_MIN_MS +
      ratio * (REASONING_FLUSH_INTERVAL_MAX_MS - REASONING_FLUSH_INTERVAL_MIN_MS),
  );
}

export function useReasoningStreaming() {
  const sessionPendingRef = useRef<ReasoningPending>({ deltas: {}, flushTimer: null });
  const flushTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const flushReasoningDeltas = useCallback(() => {
    const pending = sessionPendingRef.current;
    const toFlush: Array<[string, string]> = [];
    for (const [messageId, delta] of Object.entries(pending.deltas)) {
      if (delta) toFlush.push([messageId, delta]);
    }
    if (toFlush.length === 0) return;
    pending.deltas = {};
    if (flushTimeoutRef.current !== null) {
      clearTimeout(flushTimeoutRef.current);
      flushTimeoutRef.current = null;
    }

    useAIPanelStore.setState((state) => {
      const deltaMap = new Map(toFlush);
      return applyStreamingReasoningDeltas(state, deltaMap, (item) => {
        const threshold = TIMING.MESSAGE_TRUNCATE_THRESHOLD_CHARS;
        const keep = TIMING.MESSAGE_TRUNCATE_KEEP_TAIL_CHARS;
        if (item.content.length <= threshold) return item;
        const full = item.content;
        const trim = full.length - keep;
        const newPrefix = (item.truncatedPrefix ?? '') + full.slice(0, trim);
        return { ...item, content: full.slice(trim), truncatedPrefix: newPrefix };
      });
    });
  }, []);

  const scheduleFlush = useCallback(() => {
    let bufferLen = 0;
    for (const delta of Object.values(sessionPendingRef.current.deltas)) {
      bufferLen += delta.length;
    }

    if (bufferLen >= TIMING.REASONING_MAX_BUFFER_CHARS_BEFORE_FORCE_FLUSH) {
      if (flushTimeoutRef.current !== null) {
        clearTimeout(flushTimeoutRef.current);
        flushTimeoutRef.current = null;
      }
      queueMicrotask(() => flushReasoningDeltas());
      return;
    }

    if (flushTimeoutRef.current !== null) return;
    const interval = computeReasoningFlushIntervalMs(bufferLen);
    flushTimeoutRef.current = setTimeout(flushReasoningDeltas, interval);
  }, [flushReasoningDeltas]);

  const appendReasoningDelta = useCallback((messageId: string, content: string) => {
    const normalizedDelta = normalizeStreamChunk(content);
    if (normalizedDelta.length === 0) return;
    sessionPendingRef.current.deltas[messageId] =
      (sessionPendingRef.current.deltas[messageId] || '') + normalizedDelta;
    scheduleFlush();
  }, [scheduleFlush]);

  const resetReasoningStreaming = useCallback(() => {
    if (flushTimeoutRef.current !== null) {
      clearTimeout(flushTimeoutRef.current);
      flushTimeoutRef.current = null;
    }
    sessionPendingRef.current = { deltas: {}, flushTimer: null };
  }, []);

  return {
    flushReasoningDeltas,
    appendReasoningDelta,
    resetReasoningStreaming,
  };
}