import { listen } from '@tauri-apps/api/event';
import { useEffect, useRef } from 'react';
import {
  useAIPanelStore,
  type ChatMode,
} from '../../store';
import type { StreamPayload } from './streamTypes';
import { handleStreamDone, handleStreamError, handleToolResult } from './streamEventHandlers';
import { useTextStreaming } from './useTextStreaming';
import { useToolCallStreaming } from './useToolCallStreaming';

interface UseAgentStreamArgs {
  mode: ChatMode;
}

export function useAgentStream({ mode }: UseAgentStreamArgs) {
  const clearToolCalls = useAIPanelStore((state) => state.clearToolCalls);
  const setMessageDiff = useAIPanelStore((state) => state.setMessageDiff);

  const unlistenRef = useRef<(() => void) | null>(null);
  const isSettingUpRef = useRef(false);
  const modeRef = useRef(mode);

  const {
    streamingContentRef,
    flushTextDeltas,
    appendTextDelta,
    resetTextStreaming,
  } = useTextStreaming();

  const {
    flushToolArgs,
    handleToolCallStart,
    handleToolCallArgsDelta,
    resetToolCallStreaming,
  } = useToolCallStreaming();

  const flushAllPendingRef = useRef<() => void>(() => {});

  useEffect(() => {
    modeRef.current = mode;
  }, [mode]);

  useEffect(() => {
    flushAllPendingRef.current = () => {
      flushTextDeltas();
      flushToolArgs();
    };
  }, [flushTextDeltas, flushToolArgs]);

  useEffect(() => {
    const setupListener = async () => {
      if (unlistenRef.current || isSettingUpRef.current) return;
      isSettingUpRef.current = true;

      try {
        unlistenRef.current = await listen<StreamPayload>('ai://stream', async (event) => {
          const payload = event.payload;
          const { session_id, message_id, event_type, content, done } = payload;

          if (!payload || !session_id || !message_id) return;

          if (event_type === 'error') {
            handleStreamError({
              payload,
              currentMode: modeRef.current,
              flushAllPending: () => flushAllPendingRef.current(),
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
              currentMode: modeRef.current,
              clearToolCalls,
              setMessageDiff,
              flushAllPending: () => flushAllPendingRef.current(),
              streamingContentRef,
            });
          }
        });
      } finally {
        isSettingUpRef.current = false;
      }
    };

    setupListener();

    return () => {
      unlistenRef.current?.();
      unlistenRef.current = null;
      resetTextStreaming();
      resetToolCallStreaming();
    };
  }, [appendTextDelta, clearToolCalls, handleToolCallArgsDelta, handleToolCallStart, resetTextStreaming, resetToolCallStreaming, setMessageDiff]);
}
