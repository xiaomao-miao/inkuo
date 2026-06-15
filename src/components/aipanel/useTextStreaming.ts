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

  const flushTextDeltas = useCallback(() => {
    const allPending = sessionPendingRef.current;
    const hasAny = Object.values(allPending).some((p) => Object.keys(p.deltas).length > 0);
    if (!hasAny) return;

    const toFlush: Array<[string, string]> = [];
    for (const pending of Object.values(allPending)) {
      for (const [messageId, delta] of Object.entries(pending.deltas)) {
        if (delta) toFlush.push([messageId, delta]);
      }
    }

    if (toFlush.length === 0) return;

    // Reset all pending deltas
    for (const pending of Object.values(allPending)) {
      pending.deltas = {};
    }
    if (flushTimeoutRef.current !== null) {
      clearTimeout(flushTimeoutRef.current);
      flushTimeoutRef.current = null;
    }

    useAIPanelStore.setState((state) => {
      const deltaMap = new Map(toFlush);
      return applyStreamingTextDeltas(
        state,
        deltaMap,
        (text) => text !== stripOpenTrailingTableBlock(text)
      );
    });
  }, []);

  const scheduleFlush = useCallback(() => {
    if (flushTimeoutRef.current !== null) return;
    flushTimeoutRef.current = setTimeout(flushTextDeltas, TIMING.STREAM_FLUSH_INTERVAL_MS);
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
