//! Sub-agent activity slice of the AI panel store.
//!
//! Records and clears `SubagentActivity` entries attached to specific
//! messages inside a session. The slice is intentionally narrow —
//! sub-agent activity is read by the message renderer, not directly by
//! the user — so it never crosses the persistence boundary.

import type { AIPanelState } from '../../aiPanelStore.types';
import type { AIPanelStateCreator, SubagentActivitySlice } from '../../aiPanelStore.types';
import { TIMING } from '../../../constants/timing';
import type { OutputItem, SubagentActivity } from '../../../types';

export function applySubagentOutputItem(
  items: OutputItem[],
  outputItem: OutputItem,
  now = Date.now(),
): OutputItem[] {
  if (outputItem.type !== 'tool_result') return [...items, outputItem];

  let matchedStart = false;
  const next = items
    .filter((item) => !(item.type === 'tool_result' && item.toolCallId === outputItem.toolCallId))
    .map((item) => {
      if (item.type !== 'tool_call_start' || item.toolCallId !== outputItem.toolCallId) return item;
      matchedStart = true;
      return {
        ...item,
        isExecuting: false,
        status: outputItem.status,
        result: outputItem.result,
        duration: outputItem.duration ?? (item.startedAt ? Math.max(0, now - item.startedAt) : undefined),
        diffSummary: outputItem.diffSummary,
      };
    });
  return matchedStart ? next : [...next, outputItem];
}

export function finalizeSubagentOutputItems(
  items: OutputItem[],
  status: 'completed' | 'error',
  now = Date.now(),
): OutputItem[] {
  let changed = false;
  const next = items.map((item) => {
    if (item.type === 'reasoning' && !item.completed) {
      changed = true;
      return {
        ...item,
        completed: true,
        durationMs: item.durationMs ?? (item.startedAt ? Math.max(0, now - item.startedAt) : undefined),
      };
    }
    if (item.type === 'tool_call_start' && (item.isExecuting || !item.status)) {
      changed = true;
      return {
        ...item,
        isExecuting: false,
        status: status === 'error' ? 'error' as const : 'success' as const,
        duration: item.duration ?? (item.startedAt ? Math.max(0, now - item.startedAt) : undefined),
        result: item.result ?? (status === 'error' ? '子任务在工具返回前结束' : undefined),
      };
    }
    return item;
  });
  return changed ? next : items;
}

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
  mutator: (activities: SubagentActivity[]) => SubagentActivity[],
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

  const nextMessage = { ...message, subagentActivities: nextActivities };
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
      (activities) => activities.map((a) =>
        a.id === subagentId
          ? { ...a, outputItems: applySubagentOutputItem(a.outputItems, outputItem) }
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

      const activity = prevActivities[activityIdx];
      const items = activity.outputItems;
      const originalLast = items[items.length - 1];
      const workingItems = delta.type === 'text' && originalLast?.type === 'reasoning' && !originalLast.completed
        ? [
            ...items.slice(0, -1),
            {
              ...originalLast,
              completed: true,
              durationMs: originalLast.durationMs ?? (
                originalLast.startedAt ? Math.max(0, Date.now() - originalLast.startedAt) : undefined
              ),
            },
          ]
        : items;
      const last = workingItems[workingItems.length - 1];

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

        const nextItems = [...workingItems.slice(0, -1), finalItem];
        const nextActivity = { ...activity, outputItems: nextItems };
        const nextActivities = [...prevActivities.slice(0, activityIdx), nextActivity, ...prevActivities.slice(activityIdx + 1)];
        const nextMessage = { ...message, subagentActivities: nextActivities };
        const nextMessages = [...session.messages.slice(0, msgIdx), nextMessage, ...session.messages.slice(msgIdx + 1)];
        const nextSession = { ...session, messages: nextMessages };
        const nextSessions = [...sessions.slice(0, sessionIdx), nextSession, ...sessions.slice(sessionIdx + 1)];
        return { ...state, sessions: nextSessions };
      }

      // No matching trailing item — append a fresh one
      const fresh = delta.type === 'text'
        ? { type: 'text' as const, content: delta.content, isPendingMarkdown: false }
        : {
            type: 'reasoning' as const,
            content: delta.content,
            isPendingMarkdown: false,
            startedAt: Date.now(),
          };
      const nextItems = [...workingItems, fresh];
      const nextActivity = { ...activity, outputItems: nextItems };
      const nextActivities = [...prevActivities.slice(0, activityIdx), nextActivity, ...prevActivities.slice(activityIdx + 1)];
      const nextMessage = { ...message, subagentActivities: nextActivities };
      const nextMessages = [...session.messages.slice(0, msgIdx), nextMessage, ...session.messages.slice(msgIdx + 1)];
      const nextSession = { ...session, messages: nextMessages };
      const nextSessions = [...sessions.slice(0, sessionIdx), nextSession, ...sessions.slice(sessionIdx + 1)];
      return { ...state, sessions: nextSessions };
    }),

  completeSubagentActivity: (sessionId, parentMessageId, subagentId, status, summary, error) =>
    set((state) => updateSubagentActivitiesInState(
      state, sessionId, parentMessageId,
      (activities) => activities.map((a) =>
        a.id === subagentId
          ? {
              ...a,
              status,
              summary,
              error,
              expanded: false,
              outputItems: finalizeSubagentOutputItems(a.outputItems, status),
            }
          : a,
      ),
    )),

  toggleSubagentActivityExpanded: (sessionId, parentMessageId, subagentId) =>
    set((state) => updateSubagentActivitiesInState(
      state, sessionId, parentMessageId,
      (activities) => activities.map((a) =>
        a.id === subagentId ? { ...a, expanded: !a.expanded } : a,
      ),
    )),
});
