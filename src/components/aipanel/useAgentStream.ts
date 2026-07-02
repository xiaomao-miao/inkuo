import { listen } from '@tauri-apps/api/event';
import { useEffect, useRef } from 'react';
import {
  useAIPanelStore,
  type ChatMode,
} from '../../store';
import type { StreamPayload } from './streamTypes';
import { dispatchStreamEvent } from './streamEventDispatcher';
import { useTextStreaming } from './useTextStreaming';
import { useReasoningStreaming } from './useReasoningStreaming';
import { useToolCallStreaming } from './useToolCallStreaming';
import { isTauriRuntime } from '../../utils/tauri';

interface UseAgentStreamArgs {
  mode: ChatMode;
}

export function useAgentStream({ mode }: UseAgentStreamArgs) {
  const clearToolCalls = useAIPanelStore((state) => state.clearToolCalls);
  const setPendingDiff = useAIPanelStore((state) => state.setPendingDiff);

  const unlistenRef = useRef<(() => void) | null>(null);
  const modeRef = useRef(mode);

  const {
    streamingContentRef,
    flushTextDeltas,
    appendTextDelta,
  } = useTextStreaming();

  const {
    flushReasoningDeltas,
    appendReasoningDelta,
  } = useReasoningStreaming();

  const {
    flushToolArgs,
    handleToolCallStart,
    handleToolCallArgsDelta,
  } = useToolCallStreaming();

  useEffect(() => {
    modeRef.current = mode;
  }, [mode]);

  // Hold a stable reference to the latest flush callbacks so the Tauri listener
  // (registered once) always invokes the most recent functions.
  const flushAllPendingRef = useRef<(sessionId: string) => void>(() => {});

  useEffect(() => {
    flushAllPendingRef.current = (sessionId) => {
      flushReasoningDeltas();
      flushTextDeltas();
      flushToolArgs(sessionId);
    };
  }, [flushReasoningDeltas, flushTextDeltas, flushToolArgs]);

  // Register the Tauri listener exactly once per mount. Callbacks read from
  // refs that are updated above, so they always see the latest closures
  // without re-registering on every render.
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let unlisten: (() => void) | null = null;

    (async () => {
      unlisten = await listen<StreamPayload>('ai://stream', (event) => {
        if (disposed) return;
        void dispatchStreamEvent({
          payload: event.payload,
          currentMode: modeRef.current,
          clearToolCalls,
          flushAllPending: (sessionId) => flushAllPendingRef.current(sessionId),
          streamingContentRef,
          appendTextDelta,
          appendReasoningDelta,
          handleToolCallStart,
          handleToolCallArgsDelta,
          setPendingDiff,
        });
      });
      if (disposed) {
        unlisten();
        unlisten = null;
      } else {
        unlistenRef.current = unlisten;
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
      unlisten = null;
      unlistenRef.current = null;
    };
    // Register once. Inner callbacks read from refs to stay up-to-date.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}
