// Inline Completion Provider
// Context provider that manages inline completion state

import React, { createContext, useContext, useCallback, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useInlineCompleteStore } from '../../store';
import type {
  InlineCompletionRequest,
  InlineCompletionResponse,
  CompletionItem,
} from '../../types/inline-complete';

interface InlineCompleteContextValue {
  // State
  isEnabled: boolean;
  currentCompletion: CompletionItem | null;
  isLoading: boolean;
  error: string | null;

  // Actions
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

  // Cancel any pending request
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

  // Trigger completion request
  const triggerCompletion = useCallback(async (params: {
    document: string;
    cursorPosition: number;
    language: string;
    filePath?: string;
  }) => {
    const seq = requestSeqRef.current + 1;
    requestSeqRef.current = seq;
    latestRequestRef.current = { seq, filePath: params.filePath, cursorPosition: params.cursorPosition };

    if (import.meta.env.DEV) {
      console.log('[InlineComplete] triggerCompletion called, enabled:', enabled, 'isLoading:', isLoading, 'currentCompletion:', !!currentCompletion);
    }

    if (!enabled) {
      if (import.meta.env.DEV) {
        console.log('[InlineComplete] Not triggered: not enabled');
      }
      return;
    }

    // Clear any existing completion when new input happens
    if (currentCompletion) {
      if (import.meta.env.DEV) {
        console.log('[InlineComplete] Clearing existing completion for new request');
      }
      clearCompletion();
    }

    // Cancel previous timer/in-flight request (but do NOT invalidate this request)
    cancelPendingRequest();

    // Set loading state
    setLoading(true);
    setError(null);

    // Create new abort controller
    abortControllerRef.current = new AbortController();

    // Debounce the actual request
    debounceTimerRef.current = setTimeout(async () => {
      try {
        const request: InlineCompletionRequest = {
          document: params.document,
          cursor_position: params.cursorPosition,
          language: params.language,
          file_path: params.filePath,
          // pass snippet when available (Cursor-like)
          snippet: (params as any).snippet,
        };

        if (import.meta.env.DEV) {
          console.log('[InlineComplete] Sending request to backend');
        }
        const response = await invoke<InlineCompletionResponse>(
          'ai_inline_complete',
          { request }
        );

        if (import.meta.env.DEV) {
          console.log('[InlineComplete] Received response:', response);
        }

        // Drop stale/outdated responses (Cursor-like)
        const latest = latestRequestRef.current;
        if (!latest || latest.seq !== seq) {
          if (import.meta.env.DEV) {
            console.log('[InlineComplete] Stale response (seq mismatch), ignoring');
          }
          return;
        }
        if (latest.filePath !== params.filePath || latest.cursorPosition !== params.cursorPosition) {
          if (import.meta.env.DEV) {
            console.log('[InlineComplete] Stale response (context changed), ignoring');
          }
          return;
        }

        // If some other completion was already set while waiting, ignore
        const currentState = useInlineCompleteStore.getState();
        if (currentState.currentCompletion) {
          if (import.meta.env.DEV) {
            console.log('[InlineComplete] Another completion was set, ignoring response');
          }
          return;
        }

        if (response.completions.length > 0) {
          if (import.meta.env.DEV) {
            console.log('[InlineComplete] Setting completion:', response.completions[0]);
          }
          // Pass the trigger position so we can detect cursor movement
          setCompletion(response.completions[0], params.cursorPosition);
        } else {
          if (import.meta.env.DEV) {
            console.log('[InlineComplete] No completions returned');
          }
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

  // Accept the current completion
  const acceptCompletion = useCallback((): CompletionItem | null => {
    const completion = useInlineCompleteStore.getState().currentCompletion;
    if (completion) {
      clearCompletion();
      return completion;
    }
    return null;
  }, [clearCompletion]);

  // Dismiss the current completion
  const dismissCompletion = useCallback(() => {
    invalidateRequests();
    clearCompletion();
  }, [invalidateRequests, clearCompletion]);

  // Cleanup on unmount
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
