// Output-item reducer helpers — the heart of the AI panel's message
// rendering. This module owns:
//
//   - Patch / append / find logic against `message.outputItems`.
//   - Streaming-plan conversion (text item → plan item) and delta
//     ingestion for the plan stream.
//   - Lazy-load window management: collapsing the visible content
//     into `truncatedPrefix` and splicing it back when the user
//     expands a message.
//   - Session-history collapse / expand for the placeholder card in
//     `ChatView.tsx`.
//
// Split out from the original monolithic `aiPanelReducers.ts` so that
// the streaming, plan-handling, and history-collapse logic are easy to
// reason about and unit-test independently.

import type {
  ChatMessage,
  ChatSession,
  CurrentDiff,
  OutputItem,
} from '../../types';
import { parseStreamingPlan } from '../../utils/planStream';
import { updateMessages } from './sessionReducer';

export type OutputItemMatchKey = { toolCallId: string } | { contentContains: string };

export function patchMessageOutputItems(
  message: ChatMessage,
  matchKey: OutputItemMatchKey,
  patch: Partial<OutputItem>,
): ChatMessage {
  const outputItems = message.outputItems.map((item) => {
    const matchesByToolCallId =
      'toolCallId' in matchKey &&
      'toolCallId' in item &&
      item.toolCallId === matchKey.toolCallId;
    const matchesByContent =
      'contentContains' in matchKey &&
      'content' in item &&
      typeof item.content === 'string' &&
      item.content.includes(matchKey.contentContains);

    return matchesByToolCallId || matchesByContent
      ? ({ ...item, ...patch } as OutputItem)
      : item;
  });

  return { ...message, outputItems };
}

export function setMessageDiffState(
  session: ChatSession,
  messageId: string,
  diff: CurrentDiff | null,
): ChatSession {
  return updateMessages(session, messageId, (message) => ({
    ...message,
    diff: diff ?? undefined,
  }));
}

export function setMessageOutputItems(
  session: ChatSession,
  messageId: string,
  outputItems: OutputItem[],
): ChatSession {
  return updateMessages(session, messageId, (message) => ({ ...message, outputItems }));
}

export function addMessageOutputItem(
  session: ChatSession,
  messageId: string,
  outputItem: OutputItem,
): ChatSession {
  return updateMessages(session, messageId, (message) => ({
    ...message,
    outputItems: [...message.outputItems, outputItem],
  }));
}

export function patchMessageOutputState(
  session: ChatSession,
  messageId: string,
  matchKey: OutputItemMatchKey,
  patch: Partial<OutputItem>,
): ChatSession {
  return updateMessages(session, messageId, (message) =>
    patchMessageOutputItems(message, matchKey, patch)
  );
}

/**
 * Append `delta` to the trailing plan OutputItem of `messageId` and
 * recompute the parsed `plan` / `parseError` fields.
 *
 * If the message has no trailing plan item yet, this is a no-op — the
 * caller is expected to first create the plan item via `addMessageOutputItem`
 * (see `useTextStreaming` which does both: detect ```plan in accumulated
 * text → create plan item → route subsequent deltas here).
 *
 * Returns the session unchanged when no plan item exists.
 */
export function appendPlanDeltaToMessage(
  session: ChatSession,
  messageId: string,
  delta: string,
): ChatSession {
  if (!delta) return session;
  return updateMessages(session, messageId, (message) => {
    const items = message.outputItems;
    let lastIdx = items.length - 1;
    while (lastIdx >= 0 && items[lastIdx].type !== 'plan') {
      lastIdx -= 1;
    }
    if (lastIdx < 0) return message;
    const last = items[lastIdx];
    if (last.type !== 'plan') return message;

    const rawText = last.rawText + delta;
    const parsed = parseStreamingPlan(rawText);
    const nextItem: OutputItem = {
      type: 'plan',
      rawText,
      plan: parsed.plan,
      ...(parsed.parseError ? { parseError: parsed.parseError } : { parseError: undefined }),
      isStreaming: last.isStreaming ?? true,
    };
    const nextItems = items.slice();
    nextItems[lastIdx] = nextItem;
    return { ...message, outputItems: nextItems };
  });
}

/**
 * Convert the trailing text OutputItem (if any) of `messageId` into a plan
 * OutputItem seeded with `rawText`. Used when the streaming text buffer
 * first crosses the ```plan threshold — we keep the already-streamed
 * Markdown inside `rawText` so the PlanCard can render the details.
 *
 * If there is no trailing text item, this is a no-op (the caller should
 * still add a fresh plan item).
 */
export function convertTrailingTextToPlanItem(
  session: ChatSession,
  messageId: string,
  rawText: string,
): ChatSession {
  return updateMessages(session, messageId, (message) => {
    const items = message.outputItems;
    const lastIdx = items.length - 1;
    if (lastIdx < 0) return message;
    const last = items[lastIdx];
    if (last.type !== 'text') return message;
    const planItem: OutputItem = {
      type: 'plan',
      rawText: last.content + rawText,
      plan: null,
      isStreaming: true,
    };
    const nextItems = items.slice();
    nextItems[lastIdx] = planItem;
    return { ...message, outputItems: nextItems };
  });
}

export function updatePendingDiffHunks(
  session: ChatSession,
  hunkId: string,
): ChatSession {
  if (!session.pendingDiff) return session;
  const remainingHunks = session.pendingDiff.hunks.filter((hunk) => hunk.id !== hunkId);
  return {
    ...session,
    pendingDiff:
      remainingHunks.length > 0
        ? { ...session.pendingDiff, hunks: remainingHunks }
        : null,
  };
}

/**
 * Splice `prefix` back in front of the visible content for a message's
 * trailing text OutputItem (or the message's `content` field if the message
 * has no outputItems), and clear `truncatedPrefix` on the message / item.
 *
 * If `keepTail` is provided and the visible content is longer than
 * `keepTail`, only the trailing `keepTail` chars stay rendered — the rest
 * is folded back into `truncatedPrefix` so the DOM stays bounded.
 */
export function spliceMessagePrefix(
  message: ChatMessage,
  prefix: string,
  keepTail?: number,
): ChatMessage {
  if (!prefix) return message;
  const items = message.outputItems;
  const lastItem = items[items.length - 1];

  if (lastItem && lastItem.type === 'text') {
    const restored = prefix + lastItem.content;
    let content = restored;
    let leftover = '';
    if (typeof keepTail === 'number' && content.length > keepTail) {
      const headLen = content.length - keepTail;
      leftover = content.slice(0, headLen);
      content = content.slice(headLen);
    }
    const updatedItem = {
      ...lastItem,
      content,
      truncatedPrefix: leftover || undefined,
    };
    return { ...message, outputItems: [...items.slice(0, -1), updatedItem] };
  }

  // No text OutputItem — fall back to the legacy `content` field.
  const restored = prefix + (message.content || '');
  let content = restored;
  let leftover = '';
  if (typeof keepTail === 'number' && content.length > keepTail) {
    const headLen = content.length - keepTail;
    leftover = content.slice(0, headLen);
    content = content.slice(headLen);
  }
  return {
    ...message,
    content,
    truncatedPrefix: leftover || undefined,
  };
}

/**
 * Move the head of the message's visible content into `truncatedPrefix` so
 * the DOM shrinks. Used by the lazy-load affordance to collapse the message
 * back to its tail window.
 */
export function collapseMessageHead(
  message: ChatMessage,
  keepTail: number,
): ChatMessage {
  const items = message.outputItems;
  const lastItem = items[items.length - 1];

  if (lastItem && lastItem.type === 'text') {
    const full = lastItem.content;
    if (full.length <= keepTail) return message;
    const trim = full.length - keepTail;
    const nextPrefix = (lastItem.truncatedPrefix ?? '') + full.slice(0, trim);
    return {
      ...message,
      outputItems: [
        ...items.slice(0, -1),
        { ...lastItem, content: full.slice(trim), truncatedPrefix: nextPrefix },
      ],
    };
  }

  const full = message.content || '';
  if (full.length <= keepTail) return message;
  const trim = full.length - keepTail;
  return {
    ...message,
    content: full.slice(trim),
    truncatedPrefix: (message.truncatedPrefix ?? '') + full.slice(0, trim),
  };
}

/**
 * Mark the oldest messages in a session as collapsed so the renderer can
 * swap them for a single placeholder card. Returns the session unchanged
 * when no collapse is needed.
 *
 * Strategy: keep the last `keepTail` messages fully rendered; everything
 * earlier is flagged with `collapsed: true`. The full data (content,
 * outputItems, toolCalls) is NOT mutated, so restoring later is just an
 * object-shape flag flip.
 */
export function collapseOldSessionMessages(
  session: ChatSession,
  keepTail: number,
): ChatSession {
  const messages = session.messages;
  if (messages.length <= keepTail) return session;
  const collapseCount = messages.length - keepTail;
  let touched = false;
  const next = messages.map((message, idx) => {
    if (idx >= collapseCount) return message;
    if (message.collapsed) return message;
    touched = true;
    return { ...message, collapsed: true as const };
  });
  if (!touched) return session;
  return { ...session, messages: next };
}

/**
 * Un-collapse the oldest `revealCount` previously-collapsed messages so
 * they render again. Used by the placeholder's "load earlier" affordance.
 *
 * We never cross into the always-live tail — collapsed messages are
 * always older than the live window.
 */
export function expandCollapsedSessionMessages(
  session: ChatSession,
  revealCount: number,
): ChatSession {
  const messages = session.messages;
  let touched = false;
  let revealedSoFar = 0;
  const next = messages.map((message) => {
    if (!message.collapsed) return message;
    if (revealedSoFar >= revealCount) return message;
    revealedSoFar += 1;
    touched = true;
    const { collapsed: _collapsed, ...rest } = message;
    void _collapsed;
    return { ...rest } as ChatMessage;
  });
  if (!touched) return session;
  return { ...session, messages: next };
}

/**
 * Hard-collapse every currently-expanded history placeholder. Called when
 * the user starts a new turn (sends a message) so the live DOM stays
 * bounded while the new stream renders. This is the "新问题触发时立即
 * 卸载旧消息" behavior the user explicitly requested.
 */
export function hardCollapseSessionHistory(session: ChatSession): ChatSession {
  const messages = session.messages;
  let touched = false;
  const next = messages.map((message) => {
    if (!message.collapsed) return message;
    touched = true;
    return { ...message, collapsed: true as const };
  });
  if (!touched) return session;
  return { ...session, messages: next };
}