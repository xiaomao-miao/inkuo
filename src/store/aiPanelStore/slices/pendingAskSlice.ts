//! Pending-ask slice of the AI panel store.
//!
//! Owns the `pendingAskByMessage` keyed map plus the `setPendingAsk` /
//! `clearPendingAsk` actions. The Rust agent loop emits a `tool_paused`
//! stream event when it called `ask_user`; the dispatcher pushes the
//! question schema here. `AskUserCard` reads it, asks the user, and
//! clears the entry on submit/cancel.
//!
//! Keying on `session_id:message_id` rather than just `session_id` lets
//! the (rare) race where two `ask_user` calls land in different
//! messages of the same session coexist. In practice a single run
//! only has one pause at a time, but the cost is two map keys vs one
//! and the benefit is no key collisions across history replays.

import type {
  AIPanelState,
  AIPanelStateCreator,
} from '../../aiPanelStore.types';

export const createPendingAskSlice: AIPanelStateCreator<
  Pick<
    AIPanelState,
    'pendingAskByMessage' | 'setPendingAsk' | 'clearPendingAsk'
  >
> = (set) => ({
  pendingAskByMessage: {},
  setPendingAsk: (sessionId, messageId, entry) =>
    set((state) => ({
      pendingAskByMessage: {
        ...state.pendingAskByMessage,
        [pendingAskKey(sessionId, messageId)]: entry,
      },
    })),
  clearPendingAsk: (sessionId, messageId) =>
    set((state) => {
      const key = pendingAskKey(sessionId, messageId);
      if (!(key in state.pendingAskByMessage)) return state;
      const { [key]: _drop, ...rest } = state.pendingAskByMessage;
      return { pendingAskByMessage: rest };
    }),
});

/** Stable composite key for the pending-ask map. The `:` separator
 * is safe because session / message ids produced by `crypto.randomUUID`
 * never contain `:`. */
export function pendingAskKey(sessionId: string, messageId: string): string {
  return `${sessionId}:${messageId}`;
}
