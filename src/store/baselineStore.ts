/**
 * Baseline snapshot store.
 *
 * When the user sends an instruction in the AI panel, we capture a
 * workspace file-content snapshot (the "baseline") so that re-sending an
 * earlier user message can roll the workspace back to its pre-instruction
 * state.
 *
 * The map is keyed by the user-message id that initiated the agent run.
 * Once that run completes successfully the entry is removed (via
 * `consumeBaseline`). If the run fails or is stopped, the entry is left
 * in place so the user can re-edit and retry.
 */

import { create } from 'zustand';
import { persist } from 'zustand/middleware';

interface BaselineState {
  /** userMessageId -> baseline snapshot id */
  baselines: Record<string, string>;

  /** Persist a freshly created baseline for `userMessageId`. */
  recordBaseline: (userMessageId: string, snapshotId: string) => void;

  /**
   * Look up the baseline for `userMessageId` and remove it from the store.
   * Returns `undefined` if no baseline exists.
   */
  consumeBaseline: (userMessageId: string) => string | undefined;

  /** Look up a baseline without removing it. */
  peekBaseline: (userMessageId: string) => string | undefined;

  /** Remove a baseline entry without consuming it. */
  clearBaseline: (userMessageId: string) => void;

  /** Wipe everything (used when switching workspaces). */
  reset: () => void;
}

export const useBaselineStore = create<BaselineState>()(
  persist(
    (set, get) => ({
      baselines: {},

      recordBaseline: (userMessageId, snapshotId) =>
        set((state) => ({
          baselines: {
            ...state.baselines,
            [userMessageId]: snapshotId,
          },
        })),

      consumeBaseline: (userMessageId) => {
        const existing = get().baselines[userMessageId];
        if (!existing) return undefined;
        set((state) => {
          const { [userMessageId]: _drop, ...rest } = state.baselines;
          return { baselines: rest };
        });
        return existing;
      },

      peekBaseline: (userMessageId) => get().baselines[userMessageId],

      clearBaseline: (userMessageId) =>
        set((state) => {
          if (!(userMessageId in state.baselines)) return state;
          const { [userMessageId]: _drop, ...rest } = state.baselines;
          return { baselines: rest };
        }),

      reset: () => set({ baselines: {} }),
    }),
    {
      name: 'inkuo-baselines',
      version: 1,
      partialize: (state) => ({ baselines: state.baselines }),
    }
  )
);
