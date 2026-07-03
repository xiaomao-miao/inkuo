import { handleStreamDone, handleStreamError, handleToolResult } from './streamEventHandlers';
import type { MutableRefObject } from 'react';
import type { ChatMode } from '../../store';
import type { StreamPayload } from './streamTypes';
import type { OutputItem } from '../../types';

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
 * Flush accumulated text/reasoning buffers into output items.
 * Called when switching categories (e.g., from text to tool call) or on completion.
 */
function flushSubagentBuffer(
  sessionId: string,
  parentMessageId: string,
  subagentId: string,
  addOutput: (sessionId: string, parentMessageId: string, subagentId: string, item: OutputItem) => void,
): void {
  const buf = subagentBuffers.get(subagentId);
  if (!buf) return;

  if (buf.text.length > 0) {
    addOutput(sessionId, parentMessageId, subagentId, {
      type: 'text',
      content: buf.text,
      isPendingMarkdown: false,
    });
    buf.text = '';
  }

  if (buf.reasoning.length > 0) {
    addOutput(sessionId, parentMessageId, subagentId, {
      type: 'reasoning',
      content: buf.reasoning,
      isPendingMarkdown: false,
    });
    buf.reasoning = '';
  }
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
type BufferCategory = 'text' | 'reasoning' | 'tool';

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
      flushSubagentBuffer(session_id, info.parentMessageId, info.subagentId, addOutputToSubagentActivity);
      completeSubagentActivity(session_id, info.parentMessageId, info.subagentId, 'completed');
      // Clean up
      subagentBuffers.delete(info.subagentId);
      subagentActivityMap.delete(subMessageId);
    }
    return;
  }

  // Check if this message belongs to a sub-agent
  const subagentInfo = getSubagentInfo(message_id);

  // For sub-agent messages, we need to route them to the nested activity
  if (subagentInfo) {
    // Track category so we can flush the buffer on category switch
    const lastSubagentCategory = lastCategoryByMessage.get(message_id) ?? null;

    // Handle sub-agent stream events
    if (event_type === 'text' && typeof content === 'string' && content.length > 0) {
      // Accumulate text into buffer
      const buf = getOrCreateBuffer(subagentInfo.subagentId);
      buf.text += content;
      lastCategoryByMessage.set(message_id, 'text');
      return;
    }

    if (event_type === 'reasoning' && typeof content === 'string' && content.length > 0) {
      // Accumulate reasoning into buffer
      const buf = getOrCreateBuffer(subagentInfo.subagentId);
      buf.reasoning += content;
      lastCategoryByMessage.set(message_id, 'reasoning');
      return;
    }

    // For tool events, flush text/reasoning buffers first
    if (event_type === 'tool_call_start' || event_type === 'tool_call_args_delta' || event_type === 'tool_result') {
      if (lastSubagentCategory !== null && lastSubagentCategory !== 'tool') {
        flushSubagentBuffer(session_id, subagentInfo.parentMessageId, subagentInfo.subagentId, addOutputToSubagentActivity);
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
      flushSubagentBuffer(session_id, subagentInfo.parentMessageId, subagentInfo.subagentId, addOutputToSubagentActivity);
      // Mark the sub-agent as errored
      completeSubagentActivity(session_id, subagentInfo.parentMessageId, subagentInfo.subagentId, 'error', undefined, error);
      // Clean up
      subagentBuffers.delete(subagentInfo.subagentId);
      subagentActivityMap.delete(message_id);
      lastCategoryByMessage.delete(message_id);
      return;
    }

    // done events for sub-agent - flush all buffers and complete
    if (event_type === 'done') {
      flushSubagentBuffer(session_id, subagentInfo.parentMessageId, subagentInfo.subagentId, addOutputToSubagentActivity);
      completeSubagentActivity(session_id, subagentInfo.parentMessageId, subagentInfo.subagentId, 'completed', final_content);
      // Clean up
      subagentBuffers.delete(subagentInfo.subagentId);
      lastCategoryByMessage.delete(message_id);
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
    handleStreamError({
      payload,
      currentMode,
      flushAllPending: () => flushAllPending(session_id),
      streamingContentRef,
    });
    lastCategoryByMessage.set(message_id, 'text');
    return;
  }

  if (event_type === 'tool_call_start') {
    handleToolCallStart(payload);
    lastCategoryByMessage.set(message_id, 'tool');
    return;
  }

  if (event_type === 'tool_call_args_delta') {
    handleToolCallArgsDelta(payload);
    lastCategoryByMessage.set(message_id, 'tool');
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

  if (done) {
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