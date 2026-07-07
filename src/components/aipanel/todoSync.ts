//! Bridges between the `update_todo` tool's streamed OutputItem and the
//! AIPanelStore's `todoSnapshotBySession` map.
//!
//! Why a dedicated module: the call site (`streamEventHandlers.handleToolResult`)
//! is already busy, and the normalisation rules for `TodoItem` (status
//! defaults, fallback ids) and the v2 action state machine deserve a
//! single home so the same logic runs whether the snapshot comes from a
//! live tool_result or from replaying persisted messages after a restart.

import type { TodoAction, TodoItem, TodoStatus } from '../../types';
import { useAIPanelStore } from '../../store';

const VALID_STATUSES: ReadonlyArray<TodoStatus> = ['pending', 'in_progress', 'completed'];

function normaliseStatus(value: unknown): TodoStatus {
  if (typeof value !== 'string') return 'pending';
  return (VALID_STATUSES as readonly string[]).includes(value)
    ? (value as TodoStatus)
    : 'pending';
}

/**
 * Detect whether `raw` looks like a v1 (snapshot) item — an object with
 * a `content` string and (optionally) a `status`/`id` field — or a v2
 * item, which is just a plain string. Returns `'v1'`, `'v2'`, or
 * `'invalid'`.
 */
type ItemShape = 'v1' | 'v2' | 'invalid';

function detectItemShape(raw: unknown): ItemShape {
  if (typeof raw === 'string') {
    return raw.trim().length > 0 ? 'v2' : 'invalid';
  }
  if (raw && typeof raw === 'object') {
    const obj = raw as Record<string, unknown>;
    return typeof obj.content === 'string' && obj.content.trim().length > 0
      ? 'v1'
      : 'invalid';
  }
  return 'invalid';
}

/**
 * v1 normaliser: object with `content`, optional `id`, optional `status`.
 * Used when replaying old snapshots from persisted messages.
 */
function normaliseV1Item(raw: unknown, fallbackId: string): TodoItem | null {
  if (!raw || typeof raw !== 'object') return null;
  const obj = raw as Record<string, unknown>;
  const content = typeof obj.content === 'string' ? obj.content.trim() : '';
  if (content.length === 0) return null;
  const idRaw = typeof obj.id === 'string' ? obj.id.trim() : '';
  return {
    id: idRaw.length > 0 ? idRaw : fallbackId,
    content,
    status: normaliseStatus(obj.status),
  };
}

/**
 * Coerce the raw `items` from a tool call into a uniform `TodoItem[]`,
 * handling both v1 (snapshot) and v2 (string array) shapes. v1 items
 * retain their model-supplied `status`; v2 items default to `pending`.
 *
 * Empty / mixed-but-all-invalid inputs collapse to `[]`.
 */
function normaliseItems(rawItems: unknown): TodoItem[] {
  if (!Array.isArray(rawItems)) return [];
  const result: TodoItem[] = [];
  for (let i = 0; i < rawItems.length; i += 1) {
    const raw = rawItems[i];
    const shape = detectItemShape(raw);
    if (shape === 'invalid') continue;
    if (shape === 'v2') {
      // v2: raw is a string. `id` is purely a row index for React keys;
      // the actual status is decided by `applyTodoAction` from the
      // action the model called, not from the input payload.
      result.push({
        id: String(i + 1),
        content: (raw as string).trim(),
        status: 'pending',
      });
    } else {
      const fallbackId = String(i + 1);
      const item = normaliseV1Item(raw, fallbackId);
      if (item) result.push(item);
    }
  }
  return result;
}

/**
 * Pure state-machine step. Given the previous snapshot (or `null` if
 * none), an action, and the items the model just sent, return the next
 * snapshot's items.
 *
 *   - `set` with `newItems` — replace the list. The first row becomes
 *     `in_progress` (you've started it), the rest are `pending`. If
 *     `newItems` is empty, the snapshot is cleared.
 *
 *   - `set` with no `newItems` (legacy v1) — replace the list as
 *     written, preserving model-supplied statuses. The first row that
 *     is not `completed` gets promoted to `in_progress` so the panel
 *     always has exactly one "now" row. (Without this promotion a
 *     model that publishes a fresh list with all `pending` statuses
 *     would render with no current task.)
 *
 *   - `advance` — atomic "just finished current step". Find the
 *     `in_progress` row and flip it to `completed`, then promote the
 *     first remaining `pending` row to `in_progress`. No-op if every
 *     row is already `completed` (or if there's no `in_progress`).
 *
 *   - `complete_current` — flip the current `in_progress` to
 *     `completed` without promoting. No-op if no `in_progress`.
 *
 * The output always has stable, sequential `id`s (`"1"`, `"2"`, …)
 * so React can `key` rows across re-renders regardless of what the
 * model wrote.
 */
export function applyTodoAction(
  prev: TodoItem[] | null,
  action: TodoAction,
  rawNewItems: unknown,
): TodoItem[] {
  // Coerce the raw input into a uniform v2-style array of strings-or-
  // items-with-status. We split into two arrays so we can tell whether
  // the call was a v2 (`set` with strings) or a v1 (`set` with status
  // objects) — the v1 path needs to preserve the model's statuses
  // verbatim, while the v2 path always starts from `pending`.
  const isV2 = Array.isArray(rawNewItems) && rawNewItems.every((r) => detectItemShape(r) === 'v2' || detectItemShape(r) === 'invalid');

  if (action === 'set') {
    const incoming = normaliseItems(rawNewItems);
    if (isV2) {
      // v2 path: every row is `pending`, then promote the first to
      // `in_progress` so the panel always has exactly one current task.
      const next: TodoItem[] = incoming.map((item, i) => ({
        id: String(i + 1),
        content: item.content,
        status: i === 0 ? 'in_progress' : 'pending',
      }));
      return next;
    }
    // v1 path: preserve model-supplied statuses, but make sure there's
    // exactly one `in_progress` row (the first non-`completed` one).
    // Without this guard, a model that publishes a fresh list with all
    // `pending` statuses would render with no current task and the
    // panel would freeze on the header count.
    const next: TodoItem[] = incoming.map((item, i) => ({
      id: String(i + 1),
      content: item.content,
      status: item.status,
    }));
    const firstOpen = next.findIndex((it) => it.status !== 'completed');
    if (firstOpen !== -1) {
      next[firstOpen] = { ...next[firstOpen], status: 'in_progress' };
    }
    return next;
  }

  if (action === 'advance' || action === 'complete_current') {
    if (!prev || prev.length === 0) return prev ?? [];
    const currentIdx = prev.findIndex((it) => it.status === 'in_progress');
    if (currentIdx === -1) {
      // Nothing in progress. If the model called `advance` it probably
      // expected something to be in progress; promote the first
      // `pending` row so the panel has somewhere to land. Then
      // complete-current / advance from there. This makes the tool
      // robust to small mistakes in the model's mental model.
      const firstPending = prev.findIndex((it) => it.status === 'pending');
      if (firstPending === -1) {
        // Everything's already done. `advance` is a no-op.
        return prev.map((it) => ({ ...it }));
      }
      if (action === 'advance') {
        // Treat as "start the first pending step, mark it done, and
        // promote the next one" — useful when the model forgot to call
        // `set` first.
        const secondPending = prev.findIndex(
          (it, i) => i > firstPending && it.status === 'pending',
        );
        return prev.map((it, i) => {
          if (i === firstPending) {
            return { ...it, status: 'completed' };
          }
          if (i === secondPending) {
            return { ...it, status: 'in_progress' };
          }
          return { ...it };
        });
      }
      // complete_current with no in_progress: no-op.
      return prev.map((it) => ({ ...it }));
    }

    // Normal path: there's a row in progress.
    if (action === 'complete_current') {
      const next = prev.map((it, i) =>
        i === currentIdx ? { ...it, status: 'completed' as const } : { ...it },
      );
      return next;
    }

    // `advance`: complete the current, then promote the first
    // remaining `pending` to `in_progress`.
    const nextPendingIdx = prev.findIndex(
      (it, i) => i > currentIdx && it.status === 'pending',
    );
    return prev.map((it, i) => {
      if (i === currentIdx) return { ...it, status: 'completed' as const };
      if (i === nextPendingIdx) return { ...it, status: 'in_progress' as const };
      return { ...it };
    });
  }

  // Unknown action — defensive no-op.
  return prev ?? [];
}

/**
 * Parse a tool-call's `arguments` into the (action, rawItems) pair the
 * state machine needs. Accepts the v2 `{action, items}` shape and the
 * v1 `{items}` shape (treated as `action='set'`). Returns `null` when
 * the action is missing or unrecognised — the caller should treat that
 * as "not the todo tool".
 */
function parseActionArgs(args: unknown): { action: TodoAction; items: unknown } | null {
  if (!args || typeof args !== 'object') return null;
  const obj = args as Record<string, unknown>;
  const items = obj.items;
  const rawAction = obj.action;
  if (typeof rawAction === 'string') {
    if (rawAction === 'set' || rawAction === 'advance' || rawAction === 'complete_current') {
      return { action: rawAction, items };
    }
    // Unknown action — don't crash, just bail. The model wrote
    // something we don't understand; the panel keeps its current
    // snapshot rather than corrupting it.
    return null;
  }
  // v1 fallback: no `action` field means the call is a snapshot replace.
  // Only treat as v1 if at least one item looks like a v1 object (i.e.
  // has a `content` property). If `items` is a string array, that's
  // still a v2-style payload and we default to `set` for it.
  if (Array.isArray(items)) {
    return { action: 'set', items };
  }
  return null;
}

/**
 * Walk the message's outputItems backwards, find the matching
 * `tool_call_start` whose toolCallId equals `toolCallId`, and return
 * its parsed (action, rawItems). Returns `null` when the matching
 * item is missing or wasn't named `update_todo`.
 */
function findUpdateTodoArgs(
  sessionId: string,
  messageId: string,
  toolCallId: string,
): { action: TodoAction; items: unknown } | null {
  const message = useAIPanelStore
    .getState()
    .sessions.find((s) => s.id === sessionId)
    ?.messages.find((m) => m.id === messageId);
  if (!message) return null;
  for (let i = message.outputItems.length - 1; i >= 0; i -= 1) {
    const item = message.outputItems[i];
    if (
      item.type === 'tool_call_start' &&
      item.toolCallId === toolCallId &&
      item.toolName === 'update_todo'
    ) {
      return parseActionArgs(item.arguments);
    }
  }
  return null;
}

/**
 * Called from `streamEventHandlers.handleToolResult` after the tool has
 * finished executing. Reads the (action, rawItems) from the matching
 * tool_call_start OutputItem, runs them through the state machine, and
 * writes the next snapshot to the AIPanelStore. Idempotent and no-op
 * when the tool wasn't `update_todo` or the OutputItem can't be found.
 */
export function syncTodoSnapshotFromToolCall(
  sessionId: string,
  messageId: string,
  toolCallId: string,
): void {
  const parsed = findUpdateTodoArgs(sessionId, messageId, toolCallId);
  if (parsed === null) return;

  const store = useAIPanelStore.getState();
  const prevSnapshot = store.todoSnapshotBySession[sessionId] ?? null;
  const prevItems = prevSnapshot ? prevSnapshot.items : null;

  const nextItems = applyTodoAction(prevItems, parsed.action, parsed.items);
  store.setSessionTodoSnapshot(sessionId, toolCallId, nextItems);
}