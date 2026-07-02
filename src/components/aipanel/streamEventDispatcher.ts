import { handleStreamDone, handleStreamError, handleToolResult } from './streamEventHandlers';
import type { MutableRefObject } from 'react';
import type { ChatMode } from '../../store';
import type { StreamPayload } from './streamTypes';

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
}: StreamEventDispatcherArgs) {
  const { session_id, message_id, event_type, content, done } = payload;

  if (!payload || !session_id || !message_id) return;

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