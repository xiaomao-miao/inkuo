// Shared helpers for the three stream reducer files (`textStreamActions`,
// `reasoningStreamActions`, `toolCallStreamActions`). The big "walk
// sessions and apply a per-message update" pattern is repeated three
// times with the same boilerplate; this file extracts it so the
// reducers can stay focused on the per-type update logic.
//
// Functions exported here are intentionally tiny and pure — they never
// touch `state` directly, they only describe how to walk it.

import type { ChatSession } from '../../types';
import type { AIPanelState } from '../../store/aiPanelStore.types';

/**
 * Return only the sessions that contain at least one messageId from
 * `deltaMap`. Cheap O(sessions * deltaMap) pre-filter so the inner
 * reducer can assume every key in `deltaMap` belongs to the current
 * session — no per-message array scan.
 */
export function filterSessionsWithMessages(
  sessions: ChatSession[],
  deltaMap: ReadonlyMap<string, unknown>,
): ChatSession[] {
  if (deltaMap.size === 0) return [];
  return sessions.filter((session) =>
    session.messages.some((message) => deltaMap.has(message.id))
  );
}

/**
 * Map over `sessions` and replace each session with the result of
 * `updater(session)`. Sessions for which `updater` returns the same
 * reference are kept as-is (no new object allocated) so React/zustand
 * selectors can rely on referential equality to bail out of re-renders.
 *
 * `shouldUpdate` is an optional pre-filter that returns `false` to
 * skip the `updater` entirely; `updater` is then guaranteed to be
 * called with the original session reference, which makes the early-
 * return path obvious in tests.
 */
export function mapSessionIfRelevant<S extends AIPanelState | { sessions: ChatSession[] }>(
  state: S,
  predicate: (session: ChatSession) => boolean,
  updater: (session: ChatSession) => ChatSession,
): S {
  let anyChanged = false;
  const next = state.sessions.map((session) => {
    if (!predicate(session)) return session;
    const updated = updater(session);
    if (updated !== session) anyChanged = true;
    return updated;
  });
  return anyChanged ? ({ ...state, sessions: next } as S) : state;
}
