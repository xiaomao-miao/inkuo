/**
 * Baseline snapshot store.
 *
 * When the user sends an instruction in the AI panel, we capture a
 * workspace file-content snapshot (the "baseline") so that re-sending an
 * earlier user message can roll the workspace back to its pre-instruction
 * state.
 *
 * The map is keyed by the user-message id that initiated the agent run.
 * Critically, the baseline is **not** cleared on successful completion:
 * the user should always be able to re-edit the same question later and
 * see the model re-approach it from the original pre-instruction state.
 * Entries are dropped only when the underlying snapshot is no longer
 * present (caller invokes `clearBaseline`), the owning session is
 * permanently deleted (caller invokes `clearBaselinesForSession`), or
 * the user switches workspaces (`reset`).
 *
 * The previous behaviour of consuming the baseline on a successful run
 * kept the model from seeing prior answers via `buildConversationHistory`,
 * but it also broke the "re-send a previous question" workflow: by the
 * time the user clicked "edit and resend", the baseline was gone.
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
   *
   * Reserved for callers that intentionally want to invalidate a
   * baseline (e.g. the corresponding snapshot was deleted on disk by
   * the LRU eviction pass). The agent success path MUST NOT call this.
   */
  consumeBaseline: (userMessageId: string) => string | undefined;

  /** Look up a baseline without removing it. */
  peekBaseline: (userMessageId: string) => string | undefined;

  /** Remove a baseline entry without consuming it. */
  clearBaseline: (userMessageId: string) => void;

  /** Drop every baseline whose id is in `messageIds`. Convenience for
   * permanent session deletion. */
  clearBaselinesForSession: (messageIds: string[]) => void;

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

      clearBaselinesForSession: (messageIds) =>
        set((state) => {
          if (messageIds.length === 0) return state;
          const drop = new Set(messageIds);
          let changed = false;
          const next: Record<string, string> = {};
          for (const [id, snapshotId] of Object.entries(state.baselines)) {
            if (drop.has(id)) {
              changed = true;
            } else {
              next[id] = snapshotId;
            }
          }
          return changed ? { baselines: next } : state;
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
