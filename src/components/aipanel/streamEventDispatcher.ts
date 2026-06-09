import { handleStreamDone, handleStreamError, handleToolResult } from './streamEventHandlers';
import type { MutableRefObject } from 'react';
import type { ChatMode } from '../../store';
import type { StreamPayload } from './streamTypes';

interface StreamEventDispatcherArgs {
  payload: StreamPayload;
  currentMode: ChatMode;
  clearToolCalls: (sessionId: string) => void;
  flushAllPending: () => void;
  streamingContentRef: MutableRefObject<Record<string, string>>;
  appendTextDelta: (messageId: string, content: string) => void;
  handleToolCallStart: (payload: StreamPayload) => void;
  handleToolCallArgsDelta: (payload: StreamPayload) => void;
  setMessageDiff: (sessionId: string, messageId: string, diff: import('../../types').CurrentDiff | null) => void;
}

export async function dispatchStreamEvent({
  payload,
  currentMode,
  clearToolCalls,
  flushAllPending,
  streamingContentRef,
  appendTextDelta,
  handleToolCallStart,
  handleToolCallArgsDelta,
  setMessageDiff,
}: StreamEventDispatcherArgs) {
  const { session_id, message_id, event_type, content, done } = payload;

  if (!payload || !session_id || !message_id) return;

  if (event_type === 'error') {
    handleStreamError({
      payload,
      currentMode,
      flushAllPending,
      streamingContentRef,
    });
    return;
  }

  if (event_type === 'tool_call_start') {
    handleToolCallStart(payload);
    return;
  }

  if (event_type === 'tool_call_args_delta') {
    handleToolCallArgsDelta(payload);
    return;
  }

  if (event_type === 'tool_result') {
    handleToolResult(payload);
    return;
  }

  if (typeof content === 'string' && content.length > 0) {
    appendTextDelta(message_id, content);
  }

  if (done) {
    await handleStreamDone({
      payload,
      currentMode,
      clearToolCalls,
      setMessageDiff,
      flushAllPending,
      streamingContentRef,
    });
  }
}
