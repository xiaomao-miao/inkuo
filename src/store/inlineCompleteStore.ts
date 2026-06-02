import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { CompletionItem } from '../types/inline-complete';

interface InlineCompleteState {
  enabled: boolean;
  currentCompletion: CompletionItem | null;
  isLoading: boolean;
  error: string | null;
  triggerPosition: number | null;
  debounceMs: number;
  maxLines: number;

  setEnabled: (enabled: boolean) => void;
  setCompletion: (completion: CompletionItem | null, triggerPosition?: number) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  clearCompletion: () => void;
  updateSettings: (settings: Partial<Pick<InlineCompleteState, 'debounceMs' | 'maxLines'>>) => void;
}

export const useInlineCompleteStore = create<InlineCompleteState>()(
  persist(
    (set) => ({
      enabled: true,
      currentCompletion: null,
      isLoading: false,
      error: null,
      triggerPosition: null,
      debounceMs: 700,
      maxLines: 10,

      setEnabled: (enabled) => set({ enabled }),

      setCompletion: (completion, triggerPosition) => set({
        currentCompletion: completion,
        triggerPosition: completion ? (triggerPosition ?? null) : null,
        isLoading: false,
        error: null
      }),

      setLoading: (loading) => set({ isLoading: loading }),

      setError: (error) => set({ error, isLoading: false }),

      clearCompletion: () => set({
        currentCompletion: null,
        triggerPosition: null,
        isLoading: false,
        error: null
      }),

      updateSettings: (settings) => set((state) => ({
        ...state,
        ...settings
      })),
    }),
    {
      name: 'inkuo-inline-complete',
      partialize: (state) => ({
        enabled: state.enabled,
        debounceMs: state.debounceMs,
        maxLines: state.maxLines,
      }),
    }
  )
);
