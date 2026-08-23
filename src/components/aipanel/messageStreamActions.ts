import type {
  ChatSession,
  CurrentDiff,
  OutputItem,
  SearchResult,
  SubagentActivity,
} from '../../types';
import { updateMessages } from '../../store/aiPanelReducers';
import type { AIPanelState } from '../../store/aiPanelStore.types';
import { mapSessionIfRelevant } from './streamReducerHelpers';

/**
 * Walk the message's outputItems and flip any reasoning items that are
 * still streaming (`completed` unset / false) to `completed: true`.
 *
 * Without this, a stream that ends mid-think — either because the model
 * only emits `reasoning_content` (no follow-up `content`) or because the
 * user cancelled while reasoning was still flowing — leaves the reasoning
 * block stuck on "正在思考…" forever, since the only completion path
 * (a non-reasoning text delta following it) never fires.
 */
type TerminalKind = 'success' | 'error';

/**
 * Put every visible progress row into a terminal state. A tool result can be
 * lost during cancellation, backend failure, or an older panel unmount; the
 * enclosing stream terminal event is still authoritative and must stop every
 * spinner and freeze every elapsed timer.
 */
export function finalizeTerminalOutputItems(
  outputItems: OutputItem[],
  terminal: TerminalKind,
  now: number,
  toolStarts: ReadonlyMap<string, number> = new Map(),
): OutputItem[] {
  let changed = false;
  const next = outputItems.map((item) => {
    if (item.type === 'reasoning' && (
      !item.completed || (item.startedAt !== undefined && item.durationMs === undefined)
    )) {
      changed = true;
      return {
        ...item,
        completed: true,
        durationMs: item.durationMs ?? (item.startedAt ? Math.max(0, now - item.startedAt) : undefined),
      };
    }
    if (item.type === 'tool_call_start' && (item.isExecuting || !item.status)) {
      const startedAt = item.startedAt ?? toolStarts.get(item.toolCallId);
      changed = true;
      return {
        ...item,
        isExecuting: false,
        status: terminal,
        duration: item.duration ?? (startedAt ? Math.max(0, now - startedAt) : undefined),
        result: item.result ?? (terminal === 'error' ? '任务在工具返回前结束' : undefined),
      };
    }
    return item;
  });
  return changed ? next : outputItems;
}

function finalizeSubagentActivities(
  activities: SubagentActivity[] | undefined,
  terminal: TerminalKind,
  now: number,
): SubagentActivity[] | undefined {
  if (!activities) return undefined;
  let changed = false;
  const next = activities.map((activity) => {
    const outputItems = finalizeTerminalOutputItems(activity.outputItems, terminal, now);
    if (activity.status !== 'running' && outputItems === activity.outputItems) return activity;
    changed = true;
    return {
      ...activity,
      status: activity.status === 'running'
        ? terminal === 'error' ? 'error' as const : 'completed' as const
        : activity.status,
      error: activity.status === 'running' && terminal === 'error'
        ? activity.error ?? '父任务已结束'
        : activity.error,
      expanded: false,
      outputItems,
    };
  });
  return changed ? next : activities;
}

function finalizeMessageProgress(
  message: ChatSession['messages'][number],
  session: ChatSession,
  terminal: TerminalKind,
  now: number,
) {
  const toolStarts = new Map(session.activeToolCalls.map((tool) => [tool.id, tool.startTime]));
  return {
    outputItems: finalizeTerminalOutputItems(message.outputItems, terminal, now, toolStarts),
    subagentActivities: finalizeSubagentActivities(message.subagentActivities, terminal, now),
  };
}

function withSessionByMessageId(
  state: AIPanelState,
  sessionId: string,
  messageId: string,
  mutate: (session: ChatSession) => ChatSession,
): AIPanelState {
  return mapSessionIfRelevant(state, (s) => s.id === sessionId, (session) => {
    if (!session.messages.some((message) => message.id === messageId)) {
      return session;
    }
    return mutate(session);
  });
}

export function applyMessageSearchResults(
  state: AIPanelState,
  sessionId: string,
  messageId: string,
  results: SearchResult[],
): AIPanelState {
  return withSessionByMessageId(state, sessionId, messageId, (session) =>
    updateMessages(session, messageId, (message) => ({
      ...message,
      searchResults: results,
    }))
  );
}

export function finalizeStreamingMessage(
  state: AIPanelState,
  sessionId: string,
  messageId: string,
  finalContent: string,
): AIPanelState {
  return withSessionByMessageId(state, sessionId, messageId, (session) => {
    const now = Date.now();
    const updated = updateMessages(session, messageId, (message) => {
      const progress = finalizeMessageProgress(message, session, 'success', now);
      return {
        ...message,
        content: finalContent,
        ...progress,
      };
    });
    return { ...updated, isStreaming: false, activeToolCalls: [] };
  });
}

export function applyStreamingError(
  state: AIPanelState,
  sessionId: string,
  messageId: string,
  error: string,
): AIPanelState {
  return withSessionByMessageId(state, sessionId, messageId, (session) => {
    const now = Date.now();
    const updated = updateMessages(session, messageId, (message) => {
      const progress = finalizeMessageProgress(message, session, 'error', now);
      return {
        ...message,
        content: error,
        ...progress,
      };
    });
    return { ...updated, isStreaming: false, activeToolCalls: [] };
  });
}

export function applyMessageDiff(
  state: AIPanelState,
  sessionId: string,
  messageId: string,
  diff: CurrentDiff | null,
): AIPanelState {
  return withSessionByMessageId(state, sessionId, messageId, (session) =>
    updateMessages(session, messageId, (message) => ({
      ...message,
      diff: diff ?? undefined,
    }))
  );
}
