import React, { useCallback, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useInlineCompleteStore, useNotificationStore } from '../../store';
import { reportError } from '../../utils/errors';
import type {
  InlineCompletionRequest,
  InlineCompletionResponse,
  CompletionItem,
} from '../../types/inline-complete';
import { InlineCompleteContext, type InlineCompleteContextValue } from './inlineCompleteContext';

interface InlineCompleteProviderProps {
  children: React.ReactNode;
}

export function InlineCompleteProvider({ children }: InlineCompleteProviderProps) {
  const abortControllerRef = useRef<AbortController | null>(null);
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const requestSeqRef = useRef(0);
  const latestRequestRef = useRef<{ seq: number; filePath?: string; cursorPosition: number } | null>(null);
  const pushNotification = useNotificationStore((state) => state.pushNotification);

  const {
    enabled,
    currentCompletion,
    isLoading,
    error,
    debounceMs,
    setEnabled,
    setCompletion,
    setLoading,
    setError,
    clearCompletion,
  } = useInlineCompleteStore();

  const cancelPendingRequest = useCallback(() => {
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
      abortControllerRef.current = null;
    }
    if (debounceTimerRef.current) {
      clearTimeout(debounceTimerRef.current);
      debounceTimerRef.current = null;
    }
  }, []);

  const invalidateRequests = useCallback(() => {
    requestSeqRef.current += 1;
    latestRequestRef.current = null;
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
      abortControllerRef.current = null;
    }
    if (debounceTimerRef.current) {
      clearTimeout(debounceTimerRef.current);
      debounceTimerRef.current = null;
    }
  }, []);

  const triggerCompletion = useCallback(async (params: {
    document: string;
    cursorPosition: number;
    language: string;
    filePath?: string;
    snippet?: { text: string; start_offset: number };
  }) => {
    const seq = requestSeqRef.current + 1;
    requestSeqRef.current = seq;
    latestRequestRef.current = { seq, filePath: params.filePath, cursorPosition: params.cursorPosition };

    const hasCurrentCompletion = !!useInlineCompleteStore.getState().currentCompletion;

    if (!enabled) {
      return;
    }

    if (hasCurrentCompletion) {
      clearCompletion();
    }

    cancelPendingRequest();

    setLoading(true);
    setError(null);

    abortControllerRef.current = new AbortController();

    debounceTimerRef.current = setTimeout(async () => {
      try {
        const request: InlineCompletionRequest = {
          document: params.document,
          cursor_position: params.cursorPosition,
          language: params.language,
          file_path: params.filePath,
          snippet: params.snippet,
        };

        const response = await invoke<InlineCompletionResponse>(
          'ai_inline_complete',
          { request }
        );

        const latest = latestRequestRef.current;
        if (!latest || latest.seq !== seq) {
          return;
        }
        if (latest.filePath !== params.filePath || latest.cursorPosition !== params.cursorPosition) {
          return;
        }

        const currentState = useInlineCompleteStore.getState();
        if (currentState.currentCompletion) {
          return;
        }

        if (response.completions.length > 0) {
          setCompletion(response.completions[0], params.cursorPosition);
        } else {
          clearCompletion();
        }
      } catch (err) {
        if (err instanceof Error && err.name === 'AbortError') {
          return;
        }
        const message = reportError('inline-complete-request', err);
        setError(message);
        pushNotification({ kind: 'error', title: 'AI 补全失败', message });
      } finally {
        setLoading(false);
      }
    }, debounceMs);
  }, [enabled, debounceMs, cancelPendingRequest, setLoading, setError, setCompletion, clearCompletion, pushNotification]);

  const acceptCompletion = useCallback((): CompletionItem | null => {
    const completion = useInlineCompleteStore.getState().currentCompletion;
    if (completion) {
      clearCompletion();
      return completion;
    }
    return null;
  }, [clearCompletion]);

  const dismissCompletion = useCallback(() => {
    invalidateRequests();
    clearCompletion();
  }, [invalidateRequests, clearCompletion]);

  useEffect(() => {
    return () => {
      invalidateRequests();
    };
  }, [invalidateRequests]);

  const value: InlineCompleteContextValue = {
    isEnabled: enabled,
    currentCompletion,
    isLoading,
    error,
    triggerCompletion,
    acceptCompletion,
    dismissCompletion,
    setEnabled,
  };

  return (
    <InlineCompleteContext.Provider value={value}>
      {children}
    </InlineCompleteContext.Provider>
  );
}
