import { useCallback, useRef } from 'react';
import { useAIPanelStore } from '../../store';
import { applyStreamingTextDeltas } from './textStreamActions';
import { TIMING } from '../../constants/timing';

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

/**
 * Compute a flush interval that grows with buffer pressure.
 *
 * We use a 2-piece linear ramp:
 *   bufferLen <= MIN_BUFFER_CHARS_FOR_SLOWDOWN           → MIN_MS
 *   bufferLen >= MAX_BUFFER_CHARS_BEFORE_FORCE_FLUSH      → MAX_MS
 *   in between                                         → linear interpolation
 *
 * The cap at MAX_MS ensures the user still sees progress while a flood of
 * deltas is in flight; without it, a runaway buffer could stretch the
 * interval to a point where the UI looks frozen.
 */
function computeFlushIntervalMs(bufferLen: number): number {
  const {
    STREAM_FLUSH_INTERVAL_MIN_MS,
    STREAM_FLUSH_INTERVAL_MAX_MS,
    MIN_BUFFER_CHARS_FOR_SLOWDOWN,
    MAX_BUFFER_CHARS_BEFORE_FORCE_FLUSH,
  } = TIMING;

  if (bufferLen <= MIN_BUFFER_CHARS_FOR_SLOWDOWN) {
    return STREAM_FLUSH_INTERVAL_MIN_MS;
  }

  const span = MAX_BUFFER_CHARS_BEFORE_FORCE_FLUSH - MIN_BUFFER_CHARS_FOR_SLOWDOWN;
  if (span <= 0) return STREAM_FLUSH_INTERVAL_MAX_MS;

  const over = Math.min(bufferLen, MAX_BUFFER_CHARS_BEFORE_FORCE_FLUSH) - MIN_BUFFER_CHARS_FOR_SLOWDOWN;
  const ratio = over / span;
  return Math.round(
    STREAM_FLUSH_INTERVAL_MIN_MS +
      ratio * (STREAM_FLUSH_INTERVAL_MAX_MS - STREAM_FLUSH_INTERVAL_MIN_MS),
  );
}

/** Per-session pending text state. */
type SessionTextPending = {
  deltas: Record<string, string>;
  flushTimer: ReturnType<typeof setTimeout> | null;
};

export function useTextStreaming() {
  // Accumulated content per messageId (flat: messageId → text)
  // This shape is required by handleStreamDone / handleStreamError (they access by messageId).
  const streamingContentRef = useRef<Record<string, string>>({});

  // Per-session pending deltas
  const sessionPendingRef = useRef<Record<string, SessionTextPending>>({});
  const flushTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const getOrCreateSession = (_sessionId: string): SessionTextPending => {
    if (!sessionPendingRef.current[_sessionId]) {
      sessionPendingRef.current[_sessionId] = { deltas: {}, flushTimer: null };
    }
    return sessionPendingRef.current[_sessionId];
  };

  /**
   * Truncate the *visible* text in a text OutputItem so the DOM stays bounded.
   *
   * The first `trim` characters of the visible content are moved into
   * `truncatedPrefix` on the item. `streamingContentRef` keeps the full
   * accumulated string so we can restore the head when the user expands the
   * message.
   */
  const truncateTextItem = useCallback((item: { type: 'text'; content: string; isPendingMarkdown?: boolean; truncatedPrefix?: string }, keepTail: number) => {
    const full = item.content;
    if (full.length <= keepTail) return item;
    const trim = full.length - keepTail;
    const newPrefix = (item.truncatedPrefix ?? '') + full.slice(0, trim);
    return { ...item, content: full.slice(trim), truncatedPrefix: newPrefix };
  }, []);

  const flushTextDeltas = useCallback(() => {
    const allPending = sessionPendingRef.current;
    const hasAny = Object.values(allPending).some((p) => Object.keys(p.deltas).length > 0);
    if (!hasAny) return;

    const pending = Object.values(allPending);
    const textDeltas: Array<[string, string]> = [];
    for (const p of pending) {
      for (const [msgId, delta] of Object.entries(p.deltas)) {
        if (delta) textDeltas.push([msgId, delta]);
      }
    }

    // Reset all pending deltas
    for (const p of Object.values(allPending)) {
      p.deltas = {};
    }
    if (flushTimeoutRef.current !== null) {
      clearTimeout(flushTimeoutRef.current);
      flushTimeoutRef.current = null;
    }

    if (textDeltas.length > 0) {
      useAIPanelStore.setState((state) => {
        const deltaMap = new Map(textDeltas);
        return applyStreamingTextDeltas(
          state,
          deltaMap,
          (text) => text !== stripOpenTrailingTableBlock(text),
          (item) => {
            const threshold = TIMING.MESSAGE_TRUNCATE_THRESHOLD_CHARS;
            const keep = TIMING.MESSAGE_TRUNCATE_KEEP_TAIL_CHARS;
            if (item.content.length <= threshold) return item;
            return truncateTextItem(item, keep);
          },
        );
      });
    }
  }, [truncateTextItem]);

  const scheduleFlush = useCallback(() => {
    // Total pending chars across all sessions — drives the adaptive interval.
    let bufferLen = 0;
    for (const pending of Object.values(sessionPendingRef.current)) {
      for (const delta of Object.values(pending.deltas)) {
        bufferLen += delta.length;
      }
    }

    // Force-flush if the buffer is dangerously large. This prevents the
    // adaptive interval from growing past a safe point and keeps the UI
    // responsive even under a flood of deltas.
    if (bufferLen >= TIMING.MAX_BUFFER_CHARS_BEFORE_FORCE_FLUSH) {
      if (flushTimeoutRef.current !== null) {
        clearTimeout(flushTimeoutRef.current);
        flushTimeoutRef.current = null;
      }
      // Defer one tick so we don't recursively flush inside the listener.
      queueMicrotask(() => flushTextDeltas());
      return;
    }

    if (flushTimeoutRef.current !== null) {
      // Timer already armed. Keep the earliest deadline so we never stretch
      // an already-scheduled interval past the requested next one.
      return;
    }

    const interval = computeFlushIntervalMs(bufferLen);
    flushTimeoutRef.current = setTimeout(flushTextDeltas, interval);
  }, [flushTextDeltas]);

  const appendTextDelta = useCallback((messageId: string, content: string) => {
    const normalizedDelta = normalizeStreamChunk(content);
    if (normalizedDelta.length === 0) return;

    const pending = getOrCreateSession('current');
    pending.deltas[messageId] = (pending.deltas[messageId] || '') + normalizedDelta;

    // Keep streamingContentRef flat (messageId → accumulated text)
    streamingContentRef.current[messageId] =
      (streamingContentRef.current[messageId] || '') + normalizedDelta;

    scheduleFlush();
  }, [scheduleFlush]);

  /** Get accumulated content for a message (for streaming preview). */
  const getStreamingContent = useCallback((messageId: string): string => {
    return streamingContentRef.current[messageId] || '';
  }, []);

  /** Clear pending state for a specific session. */
  const resetSession = useCallback((sessionId: string) => {
    delete sessionPendingRef.current[sessionId];
  }, []);

  /** Clear all pending text state. */
  const resetTextStreaming = useCallback(() => {
    if (flushTimeoutRef.current !== null) {
      clearTimeout(flushTimeoutRef.current);
      flushTimeoutRef.current = null;
    }
    sessionPendingRef.current = {};
    streamingContentRef.current = {};
  }, []);

  return {
    streamingContentRef,
    flushTextDeltas,
    appendTextDelta,
    resetTextStreaming,
    resetSession,
    getStreamingContent,
  };
}
