import { handleStreamDone, handleStreamError, handleToolResult } from './streamEventHandlers';
import type { MutableRefObject } from 'react';
import { useAIPanelStore } from '../../store';
import type { ChatMode } from '../../store';
import type { StreamPayload } from './streamTypes';
import type { OutputItem } from '../../types';
import { TIMING } from '../../constants/timing';

/** Tracks which sub-agent activity a given sub-message_id belongs to */
const subagentActivityMap = new Map<string, { parentMessageId: string; subagentId: string }>();

/** Accumulates text/reasoning deltas per sub-agent */
const subagentBuffers = new Map<string, { text: string; reasoning: string }>();

function getOrCreateBuffer(subagentId: string): { text: string; reasoning: string } {
  let buf = subagentBuffers.get(subagentId);
  if (!buf) {
    buf = { text: '', reasoning: '' };
    subagentBuffers.set(subagentId, buf);
  }
  return buf;
}

/**
 * Flush accumulated text/reasoning buffers into output items by *appending*
 * to the trailing matching item of the sub-agent. This mirrors the
 * main-stream behavior so users see progressive updates inside a single
 * OutputItem rather than a new item every flush tick.
 */
function flushSubagentBuffer(
  sessionId: string,
  parentMessageId: string,
  subagentId: string,
  appendDelta: (
    sessionId: string,
    parentMessageId: string,
    subagentId: string,
    delta: { content: string; type: 'text' | 'reasoning' },
  ) => void,
): void {
  const buf = subagentBuffers.get(subagentId);
  if (!buf) return;

  if (buf.text.length > 0) {
    appendDelta(sessionId, parentMessageId, subagentId, {
      type: 'text',
      content: buf.text,
    });
    buf.text = '';
  }

  if (buf.reasoning.length > 0) {
    appendDelta(sessionId, parentMessageId, subagentId, {
      type: 'reasoning',
      content: buf.reasoning,
    });
    buf.reasoning = '';
  }
}

/** Per-message sticky buffer category. */
type BufferCategory = 'text' | 'reasoning' | 'tool';

/**
 * Adaptive flush timer for sub-agent buffers, mirroring the main stream's
 * behavior so users see progressive updates instead of waiting for the first
 * tool-call boundary.
 */
function getTotalPendingChars(): number {
  let total = 0;
  for (const buf of subagentBuffers.values()) {
    total += buf.text.length + buf.reasoning.length;
  }
  return total;
}

function computeSubagentFlushIntervalMs(bufferLen: number): number {
  if (bufferLen <= TIMING.STREAM_FLUSH_INTERVAL_MIN_MS * 12) {
    return TIMING.STREAM_FLUSH_INTERVAL_MIN_MS;
  }
  const span = TIMING.MAX_BUFFER_CHARS_BEFORE_FORCE_FLUSH - 200;
  if (span <= 0) return TIMING.STREAM_FLUSH_INTERVAL_MAX_MS;
  const over = Math.min(bufferLen, TIMING.MAX_BUFFER_CHARS_BEFORE_FORCE_FLUSH) - 200;
  const ratio = over / span;
  return Math.round(
    TIMING.STREAM_FLUSH_INTERVAL_MIN_MS +
      ratio * (TIMING.STREAM_FLUSH_INTERVAL_MAX_MS - TIMING.STREAM_FLUSH_INTERVAL_MIN_MS),
  );
}

let subagentFlushTimer: ReturnType<typeof setTimeout> | null = null;

interface SubagentFlushCallback {
  sessionId: string;
  parentMessageId: string;
  subagentId: string;
  appendDelta: (
    sessionId: string,
    parentMessageId: string,
    subagentId: string,
    delta: { content: string; type: 'text' | 'reasoning' },
  ) => void;
}

/** Pending flush callbacks, keyed by subagent id, so we know what to flush
 *  when the timer fires. */
const pendingFlushCallbacks = new Map<string, SubagentFlushCallback>();

function scheduleSubagentFlush(cb: SubagentFlushCallback) {
  pendingFlushCallbacks.set(cb.subagentId, cb);

  const bufferLen = getTotalPendingChars();
  if (bufferLen >= TIMING.MAX_BUFFER_CHARS_BEFORE_FORCE_FLUSH) {
    if (subagentFlushTimer !== null) {
      clearTimeout(subagentFlushTimer);
      subagentFlushTimer = null;
    }
    queueMicrotask(() => flushAllSubagentBuffers());
    return;
  }

  if (subagentFlushTimer !== null) return;

  const interval = computeSubagentFlushIntervalMs(bufferLen);
  subagentFlushTimer = setTimeout(flushAllSubagentBuffers, interval);
}

function flushAllSubagentBuffers() {
  if (subagentFlushTimer !== null) {
    clearTimeout(subagentFlushTimer);
    subagentFlushTimer = null;
  }

  // Iterate over a copy so that flushSubagentBuffer (which doesn't mutate
  // the map) is safe — but the callback map CAN shrink via clear().
  for (const cb of Array.from(pendingFlushCallbacks.values())) {
    flushSubagentBuffer(cb.sessionId, cb.parentMessageId, cb.subagentId, cb.appendDelta);
  }
  pendingFlushCallbacks.clear();
}

function stopSubagentTimerWhenIdle(): void {
  if (pendingFlushCallbacks.size !== 0 || subagentFlushTimer === null) return;
  clearTimeout(subagentFlushTimer);
  subagentFlushTimer = null;
}

/** Flush and release every module-level entry owned by one parent stream. */
function cleanupDispatcherMessage(messageId: string): void {
  lastCategoryByMessage.delete(messageId);
  for (const [subMessageId, info] of Array.from(subagentActivityMap.entries())) {
    if (subMessageId !== messageId && info.parentMessageId !== messageId) continue;
    const callback = pendingFlushCallbacks.get(info.subagentId);
    if (callback) {
      flushSubagentBuffer(
        callback.sessionId,
        callback.parentMessageId,
        callback.subagentId,
        callback.appendDelta,
      );
    }
    pendingFlushCallbacks.delete(info.subagentId);
    subagentBuffers.delete(info.subagentId);
    lastCategoryByMessage.delete(subMessageId);
    subagentActivityMap.delete(subMessageId);
  }
  stopSubagentTimerWhenIdle();
}

interface StreamEventDispatcherArgs {
  payload: StreamPayload;
  currentMode: ChatMode;
  clearToolCalls: (sessionId: string) => void;
  flushAllPending: (sessionId: string) => void;
  streamingContentRef: MutableRefObject<Record<string, string>>;
  appendTextDelta: (messageId: string, content: string) => void;
  appendReasoningDelta: (messageId: string, content: string) => void;
  handleToolCallStart: (payload: StreamPayload) => void;
  handleToolCallArgsDelta: (payload: StreamPayload) => void;
  setPendingDiff: (sessionId: string, diff: import('../../types').CurrentDiff | null) => void;
  addSubagentActivity: (
    sessionId: string,
    messageId: string,
    activity: import('../../types').SubagentActivity,
  ) => void;
  addOutputToSubagentActivity: (
    sessionId: string,
    parentMessageId: string,
    subagentId: string,
    outputItem: OutputItem,
  ) => void;
  appendOutputDeltaToSubagentActivity: (
    sessionId: string,
    parentMessageId: string,
    subagentId: string,
    delta: { content: string; type: 'text' | 'reasoning' },
  ) => void;
  completeSubagentActivity: (
    sessionId: string,
    parentMessageId: string,
    subagentId: string,
    status: 'completed' | 'error',
    summary?: string,
    error?: string,
  ) => void;
}

/**
 * Check if a message_id belongs to a sub-agent (prefixed with "sub:")
 */
function getSubagentInfo(messageId: string): { isSubagent: boolean; parentMessageId: string; subagentId: string } | null {
  const info = subagentActivityMap.get(messageId);
  if (info) {
    return { isSubagent: true, ...info };
  }
  return null;
}

/**
 * Event-type categories that own a single buffer in `useTextStreaming` /
 * `useReasoningStreaming`. Whenever the category of an incoming event
 * differs from the previous one for the same message we must flush the
 * previous buffer FIRST, otherwise the visible content can land out of
 * order. Concretely:
 *
 *   text.delta "晚上好"
 *     → textBuffer = "晚上好"
 *   reasoning.delta "用户说..."
 *     → reasoningBuffer = "用户说..."
 *     → timer fires, reasoning flushes → store.lastItem is still text
 *       "晚上好" (correct), reasoning item appended.
 *     → meanwhile, text.delta "" arrives, no-op.
 *
 * The bug shows up when the order is:
 *   text.delta "晚上好"     ← in textBuffer, NOT flushed yet
 *   reasoning.delta "..."   ← in reasoningBuffer, NOT flushed yet
 *   reasoning.delta "..."   ← still in reasoningBuffer
 *   timer fires reasoning   → store.lastItem is still empty (no text item
 *                              exists yet!) → reasoning item appended at
 *                              the END, BEFORE "晚上好" lands in store.
 *   timer fires text        → store.lastItem is reasoning → completed=true,
 *                              new text item "晚上好" appended AFTER
 *                              reasoning. Now user sees:
 *                              [reasoning "..." collapsed] [text "晚上好"]
 *
 * To prevent this we eagerly flush ALL pending buffers whenever the
 * category changes (and before tool_call_start which also forces a visible
 * boundary). This guarantees that the relative order of `text` and
 * `reasoning` items in the store always matches the order of the deltas
 * arriving on the wire.
 */

function bufferCategoryFor(eventType: string): BufferCategory {
  if (eventType === 'reasoning') return 'reasoning';
  if (eventType === 'tool_call_start' || eventType === 'tool_call_args_delta' || eventType === 'tool_result') {
    return 'tool';
  }
  // text, done, error, anything else
  return 'text';
}

export async function dispatchStreamEvent({
  payload,
  currentMode,
  clearToolCalls,
  flushAllPending,
  streamingContentRef,
  appendTextDelta,
  appendReasoningDelta,
  handleToolCallStart,
  handleToolCallArgsDelta,
  setPendingDiff,
  addSubagentActivity,
  addOutputToSubagentActivity,
  appendOutputDeltaToSubagentActivity,
  completeSubagentActivity,
}: StreamEventDispatcherArgs) {
  const { session_id, message_id, event_type, content, done, final_content, summary, tool_args, error } = payload;

  if (!payload || !session_id || !message_id) return;

  // Handle subagent_start event
  if (event_type === 'subagent_start') {
    const subMessageId = final_content || '';
    const expert = summary || '';
    const label = tool_args || expert;
    const task = content || '';

    // Create a unique subagent ID
    const subagentId = subMessageId;

    // Register the mapping so subsequent events can be routed
    subagentActivityMap.set(subMessageId, { parentMessageId: message_id, subagentId });

    // Create the nested activity in the store
    addSubagentActivity(session_id, message_id, {
      id: subagentId,
      parentToolCallId: payload.tool_call_id || undefined,
      expert,
      label,
      task,
      status: 'running',
      expanded: true,
      outputItems: [],
    });

    return;
  }

  // Handle subagent_end event
  if (event_type === 'subagent_end') {
    const subMessageId = content || '';
    const info = subagentActivityMap.get(subMessageId);
    if (info) {
      // Flush any remaining buffers before completing
      flushSubagentBuffer(session_id, info.parentMessageId, info.subagentId, appendOutputDeltaToSubagentActivity);
      pendingFlushCallbacks.delete(info.subagentId);
      completeSubagentActivity(session_id, info.parentMessageId, info.subagentId, 'completed');
      // Clean up
      subagentBuffers.delete(info.subagentId);
      subagentActivityMap.delete(subMessageId);
      lastCategoryByMessage.delete(subMessageId);
      stopSubagentTimerWhenIdle();
    }
    return;
  }

  // Check if this message belongs to a sub-agent
  const subagentInfo = getSubagentInfo(message_id);

  // For sub-agent messages, we need to route them to the nested activity
  if (subagentInfo) {
    const flushCallback: SubagentFlushCallback = {
      sessionId: session_id,
      parentMessageId: subagentInfo.parentMessageId,
      subagentId: subagentInfo.subagentId,
      appendDelta: appendOutputDeltaToSubagentActivity,
    };

    // Track category so we can flush the buffer on category switch
    const lastSubagentCategory = lastCategoryByMessage.get(message_id) ?? null;

    // Handle sub-agent stream events
    if (event_type === 'text' && typeof content === 'string' && content.length > 0) {
      // Preserve wire order when providers alternate reasoning and final text.
      // Both categories have separate buffers, so letting them coexist until
      // the timer fires would always flush text first even when reasoning
      // arrived first.
      if (lastSubagentCategory !== null && lastSubagentCategory !== 'text') {
        flushSubagentBuffer(
          session_id,
          subagentInfo.parentMessageId,
          subagentInfo.subagentId,
          appendOutputDeltaToSubagentActivity,
        );
        pendingFlushCallbacks.delete(subagentInfo.subagentId);
        stopSubagentTimerWhenIdle();
      }
      // Accumulate text into buffer and schedule an adaptive flush so
      // users see progressive updates instead of waiting for the first
      // tool-call boundary.
      const buf = getOrCreateBuffer(subagentInfo.subagentId);
      buf.text += content;
      lastCategoryByMessage.set(message_id, 'text');
      scheduleSubagentFlush(flushCallback);
      return;
    }

    if (event_type === 'reasoning' && typeof content === 'string' && content.length > 0) {
      if (lastSubagentCategory !== null && lastSubagentCategory !== 'reasoning') {
        flushSubagentBuffer(
          session_id,
          subagentInfo.parentMessageId,
          subagentInfo.subagentId,
          appendOutputDeltaToSubagentActivity,
        );
        pendingFlushCallbacks.delete(subagentInfo.subagentId);
        stopSubagentTimerWhenIdle();
      }
      // Accumulate reasoning into buffer and schedule an adaptive flush.
      const buf = getOrCreateBuffer(subagentInfo.subagentId);
      buf.reasoning += content;
      lastCategoryByMessage.set(message_id, 'reasoning');
      scheduleSubagentFlush(flushCallback);
      return;
    }

    // For tool events, flush text/reasoning buffers first
    if (event_type === 'tool_call_start' || event_type === 'tool_call_args_delta' || event_type === 'tool_result') {
      if (lastSubagentCategory !== null && lastSubagentCategory !== 'tool') {
        flushSubagentBuffer(session_id, subagentInfo.parentMessageId, subagentInfo.subagentId, appendOutputDeltaToSubagentActivity);
        // Drop the pending callback so the timer doesn't double-flush.
        pendingFlushCallbacks.delete(subagentInfo.subagentId);
      }
    }

    if (event_type === 'tool_call_start') {
      const outputItem: OutputItem = {
        type: 'tool_call_start',
        toolCallId: payload.tool_call_id || '',
        toolName: payload.tool_name || '',
        arguments: {},
        rawArguments: payload.tool_args,
        isExecuting: true,
        startedAt: Date.now(),
      };
      addOutputToSubagentActivity(session_id, subagentInfo.parentMessageId, subagentInfo.subagentId, outputItem);
      lastCategoryByMessage.set(message_id, 'tool');
      return;
    }

    if (event_type === 'tool_result') {
      const isError = !!error;
      const outputItem: OutputItem = {
        type: 'tool_result',
        toolCallId: payload.tool_call_id || '',
        status: isError ? 'error' : 'success',
        result: isError ? (error || '') : (content || ''),
        duration: undefined,
      };
      addOutputToSubagentActivity(session_id, subagentInfo.parentMessageId, subagentInfo.subagentId, outputItem);
      lastCategoryByMessage.set(message_id, 'tool');
      return;
    }

    if (event_type === 'error') {
      // Flush any pending buffers first
      flushSubagentBuffer(session_id, subagentInfo.parentMessageId, subagentInfo.subagentId, appendOutputDeltaToSubagentActivity);
      pendingFlushCallbacks.delete(subagentInfo.subagentId);
      // Mark the sub-agent as errored
      completeSubagentActivity(session_id, subagentInfo.parentMessageId, subagentInfo.subagentId, 'error', undefined, error);
      // Clean up
      subagentBuffers.delete(subagentInfo.subagentId);
      subagentActivityMap.delete(message_id);
      lastCategoryByMessage.delete(message_id);
      stopSubagentTimerWhenIdle();
      return;
    }

    // done events for sub-agent - flush all buffers and complete
    if (event_type === 'done') {
      flushSubagentBuffer(session_id, subagentInfo.parentMessageId, subagentInfo.subagentId, appendOutputDeltaToSubagentActivity);
      pendingFlushCallbacks.delete(subagentInfo.subagentId);
      completeSubagentActivity(session_id, subagentInfo.parentMessageId, subagentInfo.subagentId, 'completed', final_content);
      // Clean up
      subagentBuffers.delete(subagentInfo.subagentId);
      subagentActivityMap.delete(message_id);
      lastCategoryByMessage.delete(message_id);
      stopSubagentTimerWhenIdle();
      return;
    }

    // For any other sub-agent event type, skip processing
    return;
  }

  // Track the last category we routed so we can flush on category change.
  // The module-level cache below keeps this sticky across events for the
  // same message without forcing the dispatcher to read the store on every
  // event.
  const incomingCategory = bufferCategoryFor(event_type);
  const lastCategory = lastCategoryByMessage.get(message_id) ?? null;
  if (lastCategory !== null && lastCategory !== incomingCategory) {
    // The category flipped — flush everything that came before so the
    // previous content lands in the store before we start appending the
    // new category.
    flushAllPending(session_id);
  }
  // `done` and `error` don't change the category on their own, but they
  // always need everything flushed (otherwise the very last characters of
  // a text/reasoning stream stay stuck in the buffer and never reach the
  // store).
  if (event_type === 'done' || event_type === 'error') {
    flushAllPending(session_id);
  }

  if (event_type === 'error') {
    cleanupDispatcherMessage(message_id);
    handleStreamError({
      payload,
      flushAllPending: () => flushAllPending(session_id),
      streamingContentRef,
    });
    return;
  }

  if (event_type === 'tool_call_start') {
    // Right before the new tool is appended, prune the trailing compact
    // tool (if any) so a tight `list_dir → read_file` sequence collapses
    // into a single inline line on the user's screen. We only do this on
    // the main-message path — sub-agent activity streams keep every tool
    // visible because the user is reading a sub-task's full trace there.
    useAIPanelStore.getState().pruneTrailingCompactTool(session_id, message_id);
    handleToolCallStart(payload);
    lastCategoryByMessage.set(message_id, 'tool');
    return;
  }

  if (event_type === 'tool_call_args_delta') {
    handleToolCallArgsDelta(payload);
    lastCategoryByMessage.set(message_id, 'tool');
    return;
  }

  if (event_type === 'tool_paused') {
    // The agent loop got the model to emit an `ask_user` tool call and
    // parked the session in `runtime::ask_pending`. The full question
    // schema rides on this event so the frontend can render the
    // AskUserCard without an extra round-trip. We push the entry into
    // the store keyed by `session_id:message_id` so the renderer can
    // find it next to the `tool_call_start` output item.
    if (payload.request_id && payload.tool_call_id && payload.questions) {
      const { setPendingAsk, patchOutputItem } = useAIPanelStore.getState();
      setPendingAsk(session_id, message_id, {
        sessionId: session_id,
        messageId: message_id,
        requestId: payload.request_id,
        toolCallId: payload.tool_call_id,
        questions: payload.questions,
      });
      patchOutputItem(
        session_id,
        message_id,
        { toolCallId: payload.tool_call_id },
        { interactionState: 'pending' },
      );
    }
    return;
  }

  if (event_type === 'stream_paused') {
    // Terminal event emitted by `ai_agent_stream` when the loop parked
    // itself in response to `ask_user`. Flip `isStreaming` so the chat
    // input unlocks; the AskUserCard still owns the cursor while the
    // user is making their choice.
    useAIPanelStore.getState().setIsStreaming(session_id, false);
    lastCategoryByMessage.set(message_id, 'text');
    return;
  }

  if (event_type === 'tool_result') {
    // Flush buffered args BEFORE applying the result so the outputItem
    // has complete arguments at the moment it is marked done.
    handleToolResult(payload, () => flushAllPending(session_id));
    // tool_result transitions back to text for any subsequent deltas.
    lastCategoryByMessage.set(message_id, 'text');
    return;
  }

  if (event_type === 'reasoning') {
    if (typeof content === 'string' && content.length > 0) {
      appendReasoningDelta(message_id, content);
    }
    lastCategoryByMessage.set(message_id, 'reasoning');
    return;
  }

  if (typeof content === 'string' && content.length > 0) {
    appendTextDelta(message_id, content);
  }
  lastCategoryByMessage.set(message_id, 'text');

  if (done || event_type === 'done') {
    cleanupDispatcherMessage(message_id);
    await handleStreamDone({
      payload,
      currentMode,
      clearToolCalls,
      setPendingDiff,
      flushAllPending: () => flushAllPending(session_id),
      streamingContentRef,
    });
    // After the stream ends we can forget about this message's category —
    // a future stream will start fresh and re-enter the flush branch.
    lastCategoryByMessage.delete(message_id);
  }
}

/**
 * Per-message sticky buffer category.
 *
 * Module-level because the dispatcher is invoked from a single Tauri
 * listener that can outlive any particular hook instance. A `Map` keeps
 * state isolation between concurrent streams on different messages.
 */
const lastCategoryByMessage = new Map<string, BufferCategory>();

/**
 * Drop all module-level dispatch state.
 *
 * Call this when the AI panel unmounts (or on HMR / hot reload) so the
 * next mount starts from a clean slate. Without this, leftover entries
 * from a previous mount can misroute the first events of a fresh stream
 * — most visibly, a stale `lastCategory` from an aborted prior stream
 * can cause the dispatcher's first event to skip its flush-on-category-
 * change branch.
 *
 * Safe to call multiple times; idempotent.
 */
export function resetStreamDispatcherState(): void {
  subagentActivityMap.clear();
  subagentBuffers.clear();
  lastCategoryByMessage.clear();
  pendingFlushCallbacks.clear();
  if (subagentFlushTimer !== null) {
    clearTimeout(subagentFlushTimer);
    subagentFlushTimer = null;
  }
}
