import React, { createContext, useContext, useCallback, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useInlineCompleteStore } from '../../store';
import type {
  InlineCompletionRequest,
  InlineCompletionResponse,
  CompletionItem,
} from '../../types/inline-complete';

const debugInlineComplete = import.meta.env.DEV
  ? (...args: unknown[]) => console.debug('[InlineComplete]', ...args)
  : undefined;

interface InlineCompleteContextValue {
  isEnabled: boolean;
  currentCompletion: CompletionItem | null;
  isLoading: boolean;
  error: string | null;

  triggerCompletion: (params: {
    document: string;
    cursorPosition: number;
    language: string;
    filePath?: string;
    snippet?: { text: string; start_offset: number };
  }) => Promise<void>;
  acceptCompletion: () => CompletionItem | null;
  dismissCompletion: () => void;
  setEnabled: (enabled: boolean) => void;
}

const InlineCompleteContext = createContext<InlineCompleteContextValue | null>(null);

export function useInlineComplete(): InlineCompleteContextValue {
  const context = useContext(InlineCompleteContext);
  if (!context) {
    throw new Error('useInlineComplete must be used within InlineCompleteProvider');
  }
  return context;
}

interface InlineCompleteProviderProps {
  children: React.ReactNode;
}

export function InlineCompleteProvider({ children }: InlineCompleteProviderProps) {
  const abortControllerRef = useRef<AbortController | null>(null);
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const requestSeqRef = useRef(0);
  const latestRequestRef = useRef<{ seq: number; filePath?: string; cursorPosition: number } | null>(null);

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

    debugInlineComplete?.('triggerCompletion called', {
      enabled,
      isLoading,
      hasCurrentCompletion: !!currentCompletion,
    });

    if (!enabled) {
      debugInlineComplete?.('Not triggered: not enabled');
      return;
    }

    if (currentCompletion) {
      debugInlineComplete?.('Clearing existing completion for new request');
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
        };

        debugInlineComplete?.('Sending request to backend');
        const response = await invoke<InlineCompletionResponse>(
          'ai_inline_complete',
          { request }
        );

        debugInlineComplete?.('Received response', response);

        const latest = latestRequestRef.current;
        if (!latest || latest.seq !== seq) {
          debugInlineComplete?.('Stale response ignored: seq mismatch');
          return;
        }
        if (latest.filePath !== params.filePath || latest.cursorPosition !== params.cursorPosition) {
          debugInlineComplete?.('Stale response ignored: context changed');
          return;
        }

        const currentState = useInlineCompleteStore.getState();
        if (currentState.currentCompletion) {
          debugInlineComplete?.('Stale response ignored: another completion already set');
          return;
        }

        if (response.completions.length > 0) {
          debugInlineComplete?.('Setting completion');
          setCompletion(response.completions[0], params.cursorPosition);
        } else {
          debugInlineComplete?.('No completions returned');
          clearCompletion();
        }
      } catch (err) {
        if (err instanceof Error && err.name === 'AbortError') {
          return;
        }
        console.error('[InlineComplete] Error:', err);
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setLoading(false);
      }
    }, debounceMs);
  }, [enabled, debounceMs, cancelPendingRequest, setLoading, setError, setCompletion, clearCompletion, isLoading, currentCompletion, error]);

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
