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

export function useToolCallStreaming() {
  const pendingToolArgsRef = useRef<Record<string, PendingToolArgEntry>>({});
  const pendingToolArgsOrderRef = useRef<string[]>([]);
  const flushToolArgsTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const flushToolArgs = useCallback(() => {
    const pending = pendingToolArgsRef.current;
    const order = pendingToolArgsOrderRef.current;
    if (order.length === 0) return;

    pendingToolArgsRef.current = {};
    pendingToolArgsOrderRef.current = [];
    flushToolArgsTimeoutRef.current = null;

    useAIPanelStore.setState((state) =>
      applyPendingToolArgs(state, order.map((id) => pending[id]).filter((entry): entry is PendingToolArgEntry => !!entry))
    );
  }, []);

  const scheduleToolArgsFlush = useCallback(() => {
    if (flushToolArgsTimeoutRef.current !== null) return;
    flushToolArgsTimeoutRef.current = setTimeout(flushToolArgs, TIMING.STREAM_FLUSH_INTERVAL_MS);
  }, [flushToolArgs]);

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

    const previous = pendingToolArgsRef.current[tool_call_id];
    if (!previous || previous.rawArgs !== rawArgs) {
      pendingToolArgsRef.current[tool_call_id] = {
        sessionId: session_id,
        messageId: message_id,
        rawArgs,
        parsedArgs,
        streamingContent,
      };
      if (!previous) pendingToolArgsOrderRef.current.push(tool_call_id);
    }
    scheduleToolArgsFlush();
  }, [scheduleToolArgsFlush]);

  const resetToolCallStreaming = useCallback(() => {
    if (flushToolArgsTimeoutRef.current !== null) {
      clearTimeout(flushToolArgsTimeoutRef.current);
      flushToolArgsTimeoutRef.current = null;
    }
    pendingToolArgsRef.current = {};
    pendingToolArgsOrderRef.current = [];
  }, []);

  return {
    flushToolArgs,
    handleToolCallStart,
    handleToolCallArgsDelta,
    resetToolCallStreaming,
  };
}
