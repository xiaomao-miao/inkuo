import { useCallback, useRef } from 'react';
import { useAIPanelStore } from '../../store';
import type { StreamPayload } from './streamTypes';
import type { PendingToolArgEntry } from './toolCallStreamActions';
import { TIMING } from '../../constants/timing';
import { parsePartialJsonObject } from '../../utils/json';
import {
  applyPendingToolArgs,
  applyToolCallStartToState,
} from './toolCallStreamActions';

function parseToolArgs(rawArgs: string) {
  const partial = parsePartialJsonObject(rawArgs);
  const streamingContent = [partial.content, partial.new_text, partial.json_content]
    .find((value): value is string => typeof value === 'string');

  let parsedArgs = partial;
  try {
    if (rawArgs) {
      const parsed = JSON.parse(rawArgs);
      parsedArgs = parsed && typeof parsed === 'object' && !Array.isArray(parsed)
        ? parsed as Record<string, unknown>
        : partial;
    }
  } catch {
    // Fall back to the partially parsed object while streaming.
  }

  return {
    parsedArgs,
    streamingContent,
  };
}

/** Per-session pending tool call state. */
type SessionPending = {
  /** tool_call_id → entry */
  byId: Record<string, PendingToolArgEntry>;
  /** insertion order of tool_call_ids */
  order: string[];
  /** flush timer for this session */
  flushTimer: ReturnType<typeof setTimeout> | null;
};

export function useToolCallStreaming() {
  // Keyed by sessionId so that switching sessions doesn't interfere with
  // pending tool calls in the newly active session.
  const sessionPendingRef = useRef<Record<string, SessionPending>>({});

  const getOrCreateSessionPending = (sessionId: string): SessionPending => {
    if (!sessionPendingRef.current[sessionId]) {
      sessionPendingRef.current[sessionId] = {
        byId: {},
        order: [],
        flushTimer: null,
      };
    }
    return sessionPendingRef.current[sessionId];
  };

  const flushSession = useCallback((sessionId: string) => {
    const pending = sessionPendingRef.current[sessionId];
    if (!pending || pending.order.length === 0) return;

    const entries = pending.order
      .map((id) => pending.byId[id])
      .filter((e): e is PendingToolArgEntry => !!e);

    pending.byId = {};
    pending.order = [];
    pending.flushTimer = null;

    useAIPanelStore.setState((state) =>
      applyPendingToolArgs(state, entries)
    );
  }, []);

  const scheduleSessionFlush = useCallback((sessionId: string) => {
    const pending = sessionPendingRef.current[sessionId];
    if (!pending || pending.flushTimer !== null) return;
    pending.flushTimer = setTimeout(() => {
      pending.flushTimer = null;
      flushSession(sessionId);
    }, TIMING.STREAM_FLUSH_INTERVAL_MS);
  }, [flushSession]);

  const handleToolCallStart = useCallback((payload: StreamPayload) => {
    const { session_id, message_id, tool_call_id, tool_name, tool_args } = payload;
    if (!tool_call_id || !tool_name) return;

    const rawArgs = tool_args ?? '';
    const { parsedArgs, streamingContent } = parseToolArgs(rawArgs);

    useAIPanelStore.setState((state) =>
      applyToolCallStartToState({
        state,
        sessionId: session_id,
        messageId: message_id,
        toolCallId: tool_call_id,
        toolName: tool_name,
        parsedArgs,
        rawArgs,
        streamingContent,
        startTime: Date.now(),
      })
    );
  }, []);

  const handleToolCallArgsDelta = useCallback((payload: StreamPayload) => {
    const { session_id, message_id, tool_call_id, tool_args } = payload;
    if (!tool_call_id) return;

    const rawArgs = tool_args ?? '';
    const { parsedArgs, streamingContent } = parseToolArgs(rawArgs);

    const pending = getOrCreateSessionPending(session_id);
    const previous = pending.byId[tool_call_id];

    if (!previous || previous.rawArgs !== rawArgs) {
      pending.byId[tool_call_id] = {
        sessionId: session_id,
        messageId: message_id,
        tool_call_id,
        rawArgs,
        parsedArgs,
        streamingContent,
      };
      if (!previous) pending.order.push(tool_call_id);
    }
    scheduleSessionFlush(session_id);
  }, [scheduleSessionFlush]);

  /** Flush and clear pending state for a specific session (e.g. on stream end or cancel). */
  const resetSession = useCallback((sessionId: string) => {
    const pending = sessionPendingRef.current[sessionId];
    if (!pending) return;
    if (pending.flushTimer !== null) {
      clearTimeout(pending.flushTimer);
      pending.flushTimer = null;
    }
    pending.byId = {};
    pending.order = [];
  }, []);

  /** Flush all pending updates for a specific session. */
  const flushToolArgs = useCallback((sessionId: string) => {
    flushSession(sessionId);
  }, [flushSession]);

  return {
    flushToolArgs,
    handleToolCallStart,
    handleToolCallArgsDelta,
    resetSession,
  };
}
