//! Sub-agent activity slice of the AI panel store.
//!
//! Records and clears `SubagentActivity` entries attached to specific
//! messages inside a session. The slice is intentionally narrow —
//! sub-agent activity is read by the message renderer, not directly by
//! the user — so it never crosses the persistence boundary.

import type { AIPanelState } from '../../aiPanelStore.types';
import type { AIPanelStateCreator, SubagentActivitySlice } from '../../aiPanelStore.types';
import { TIMING } from '../../../constants/timing';

/**
 * Targeted slice updater: mutates only the subagentActivities array of
 * a specific message inside a specific session, without copying the
 * entire session chain.
 *
 * The key optimization is incremental copying:
 *   - Only copies `subagentActivities` array when items change
 *   - Only copies `messages` array when the target message changes
 *   - Only copies `sessions` array when the target session changes
 *
 * This matters because sub-agent streams can generate thousands of delta
 * events per second. The old implementation always copied the full
 * session chain, leading to O(n×m) object allocations and GC pressure
 * where n = delta count and m = session/message tree depth.
 */
function updateSubagentActivitiesInState(
  state: AIPanelState,
  sessionId: string,
  parentMessageId: string,
  mutator: (activities: unknown[]) => unknown[],
): AIPanelState {
  const sessions = state.sessions;
  const sessionIdx = sessions.findIndex((s) => s.id === sessionId);
  if (sessionIdx < 0) return state;
  const session = sessions[sessionIdx];
  const msgIdx = session.messages.findIndex((m) => m.id === parentMessageId);
  if (msgIdx < 0) return state;

  const message = session.messages[msgIdx];
  const prevActivities = message.subagentActivities ?? [];
  const nextActivities = mutator(prevActivities);

  // Only copy arrays that actually changed
  if (nextActivities === prevActivities) return state;

  const nextMessage = { ...message, subagentActivities: nextActivities as typeof message.subagentActivities };
  const nextMessages = [...session.messages.slice(0, msgIdx), nextMessage, ...session.messages.slice(msgIdx + 1)];
  const nextSession = { ...session, messages: nextMessages };
  const nextSessions = [...sessions.slice(0, sessionIdx), nextSession, ...sessions.slice(sessionIdx + 1)];
  return { ...state, sessions: nextSessions };
}

export const createSubagentSlice: AIPanelStateCreator<SubagentActivitySlice> = (set) => ({
  addSubagentActivity: (sessionId, messageId, activity) =>
    set((state) => updateSubagentActivitiesInState(
      state, sessionId, messageId,
      (activities) => [...activities, activity],
    )),

  addOutputToSubagentActivity: (sessionId, parentMessageId, subagentId, outputItem) =>
    set((state) => updateSubagentActivitiesInState(
      state, sessionId, parentMessageId,
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (activities) => (activities as any[]).map((a: any) =>
        a.id === subagentId
          ? { ...a, outputItems: [...a.outputItems, outputItem] }
          : a,
      ),
    )),

  appendOutputDeltaToSubagentActivity: (
    sessionId: string,
    parentMessageId: string,
    subagentId: string,
    delta: { content: string; type: 'text' | 'reasoning' },
  ) =>
    set((state) => {
      const sessions = state.sessions;
      const sessionIdx = sessions.findIndex((s) => s.id === sessionId);
      if (sessionIdx < 0) return state;
      const session = sessions[sessionIdx];
      const msgIdx = session.messages.findIndex((m) => m.id === parentMessageId);
      if (msgIdx < 0) return state;

      const message = session.messages[msgIdx];
      const prevActivities = message.subagentActivities ?? [];
      const activityIdx = prevActivities.findIndex((a: unknown) => (a as { id?: string }).id === subagentId);
      if (activityIdx < 0) return state;

      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const activity = prevActivities[activityIdx] as any;
      const items = activity.outputItems;
      const last = items[items.length - 1];

      // Apply truncation if content exceeds threshold — keeps the DOM bounded
      // for long-running sub-agents without discarding data (the full content
      // stays in memory for export/debugging purposes).
      const threshold = TIMING.MESSAGE_TRUNCATE_THRESHOLD_CHARS;
      const keep = TIMING.MESSAGE_TRUNCATE_KEEP_TAIL_CHARS;

      if (last && last.type === delta.type) {
        const mergedContent = last.content + delta.content;
        let finalItem: typeof last;

        if (mergedContent.length <= threshold) {
          finalItem = { ...last, content: mergedContent };
        } else {
          const trim = mergedContent.length - keep;
          finalItem = {
            ...last,
            content: mergedContent.slice(trim),
            truncatedPrefix: (last.truncatedPrefix ?? '') + mergedContent.slice(0, trim),
          };
        }

        const nextItems = [...items.slice(0, -1), finalItem];
        if (nextItems === items) return state;
        const nextActivity = { ...activity, outputItems: nextItems };
        const nextActivities = [...prevActivities.slice(0, activityIdx), nextActivity, ...prevActivities.slice(activityIdx + 1)];
        const nextMessage = { ...message, subagentActivities: nextActivities as typeof message.subagentActivities };
        const nextMessages = [...session.messages.slice(0, msgIdx), nextMessage, ...session.messages.slice(msgIdx + 1)];
        const nextSession = { ...session, messages: nextMessages };
        const nextSessions = [...sessions.slice(0, sessionIdx), nextSession, ...sessions.slice(sessionIdx + 1)];
        return { ...state, sessions: nextSessions };
      }

      // No matching trailing item — append a fresh one
      const fresh = delta.type === 'text'
        ? { type: 'text' as const, content: delta.content, isPendingMarkdown: false }
        : { type: 'reasoning' as const, content: delta.content, isPendingMarkdown: false };
      const nextItems = [...items, fresh];
      const nextActivity = { ...activity, outputItems: nextItems };
      const nextActivities = [...prevActivities.slice(0, activityIdx), nextActivity, ...prevActivities.slice(activityIdx + 1)];
      const nextMessage = { ...message, subagentActivities: nextActivities as typeof message.subagentActivities };
      const nextMessages = [...session.messages.slice(0, msgIdx), nextMessage, ...session.messages.slice(msgIdx + 1)];
      const nextSession = { ...session, messages: nextMessages };
      const nextSessions = [...sessions.slice(0, sessionIdx), nextSession, ...sessions.slice(sessionIdx + 1)];
      return { ...state, sessions: nextSessions };
    }),

  completeSubagentActivity: (sessionId, parentMessageId, subagentId, status, summary, error) =>
    set((state) => updateSubagentActivitiesInState(
      state, sessionId, parentMessageId,
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (activities) => (activities as any[]).map((a: any) =>
        a.id === subagentId
          ? { ...a, status, summary, error, expanded: false }
          : a,
      ),
    )),

  toggleSubagentActivityExpanded: (sessionId, parentMessageId, subagentId) =>
    set((state) => updateSubagentActivitiesInState(
      state, sessionId, parentMessageId,
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (activities) => (activities as any[]).map((a: any) =>
        a.id === subagentId ? { ...a, expanded: !a.expanded } : a,
      ),
    )),
});
