import { listen } from '@tauri-apps/api/event';
import { useEffect, useRef } from 'react';
import {
  useAIPanelStore,
  type ChatMode,
} from '../../store';
import type { StreamPayload } from './streamTypes';
import { dispatchStreamEvent } from './streamEventDispatcher';
import { useTextStreaming } from './useTextStreaming';
import { useToolCallStreaming } from './useToolCallStreaming';

interface UseAgentStreamArgs {
  mode: ChatMode;
}

export function useAgentStream({ mode }: UseAgentStreamArgs) {
  const clearToolCalls = useAIPanelStore((state) => state.clearToolCalls);
  const setPendingDiff = useAIPanelStore((state) => state.setPendingDiff);

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
          await dispatchStreamEvent({
            payload: event.payload,
            currentMode: modeRef.current,
            clearToolCalls,
            flushAllPending: () => flushAllPendingRef.current(),
            streamingContentRef,
            appendTextDelta,
            handleToolCallStart,
            handleToolCallArgsDelta,
            setPendingDiff,
          });
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
  }, [appendTextDelta, clearToolCalls, handleToolCallArgsDelta, handleToolCallStart, resetTextStreaming, resetToolCallStreaming, setPendingDiff]);
}
