// AI panel domain types — ChatMessage / ChatSession / ChatMode and the
// structured-output pieces (OutputItem, Todo, etc.) used by `AIPanel.tsx`
// and its sub-components.

import type { DiffHunk } from './diff';
import type { StreamDiffSummary } from './diff';
import type { MessageRole, ToolCallStatus } from './agent';
import type { SearchResult } from './knowledge';

/** Chat mode. Only "agent" remains after the ask/plan removal. */
export type ChatMode = 'agent';

/**
 * Per-message feature toggles. Each toggle can be flipped on independently
 * before sending a turn. They live on `ChatSession.featureToggles` so they
 * survive across messages in the same conversation but reset when the user
 * starts a new chat (we don't persist them across restarts either — the
 * toolbar's collapsed state is the only thing that survives).
 *
 * Adding a new toggle:
 *   1. Add the id below.
 *   2. Add a prompt fragment + tool gating in the backend (see
 *      `kb_strict.md` and the `feature_toggles` module).
 *   3. Register an entry in `ChatInput.tsx`'s `TOGGLES` list — it owns
 *      the composer's inline toggle rows.
 */
export type FeatureToggleId = 'kb_strict' | 'web_search' | 'sandbox';

export interface FeatureToggleDescriptor {
  id: FeatureToggleId;
  label: string;
  /** Short helper text shown under the toggle in the expanded toolbar. */
  hint: string;
  /** True if the toggle is currently unavailable in the active mode
   * (e.g. KB strict is meaningless in plan mode). The toolbar greys it
   * out and prevents enabling it. */
  unavailable?: boolean;
  unavailableReason?: string;
}

/** Image input accepted by the Rust multimodal adapter. Exactly one of
 * `path` and `dataBase64` should be supplied. Workspace-relative paths are
 * preferred for document preview screenshots because the backend resolves
 * and validates them against the active workspace. */
export interface ImageAttachmentInput {
  path?: string;
  dataBase64?: string;
  mimeType?: 'image/png' | 'image/jpeg' | 'image/webp' | 'image/gif';
  detail?: 'auto' | 'low' | 'high';
  name?: string;
}

/** Per-turn feature flags. Missing key == off. */
export type FeatureToggleMap = Partial<Record<FeatureToggleId, boolean>>;

export interface MessageToolCall {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
}

export interface MessageToolResult {
  toolCallId: string;
  result: string;
  isError: boolean;
  duration?: number;
  diffSummary?: StreamDiffSummary;
}

/**
 * One row in the AI's task checklist. Mirrors the `TodoItem` Rust type
 * emitted by the `update_todo` tool. `status` is normalised on read
 * (unknown values fall back to `'pending'`) so a typo doesn't crash the
 * panel.
 */
export type TodoStatus = 'pending' | 'in_progress' | 'completed';

export interface TodoItem {
  id: string;
  content: string;
  status: TodoStatus;
}

/**
 * The freshest published todo list per session. Derived state — see
 * `selectActiveTodoSnapshot` in `useAIPanelStore`. Empty list means the
 * model cleared the panel (or never published one).
 */
export interface TodoSnapshot {
  items: TodoItem[];
  /**
   * Id of the `update_todo` tool call that produced this snapshot. Lets
   * the UI debug a stale panel by jumping to the source message.
   */
  toolCallId: string;
  /** Wall-clock millis when the snapshot landed. */
  updatedAt: number;
}

/**
 * The three actions the `update_todo` tool understands in v2.
 *
 *   - `set` — publish a fresh list. Used at the *start* of a multi-step
 *     task. `items` is a list of one-line strings; the frontend numbers
 *     them and renders the first one as `in_progress`, the rest as
 *     `pending`.
 *
 *   - `advance` — atomic "I just finished the current step, move on".
 *     Flips the current `in_progress` row to `completed` and the first
 *     remaining `pending` row to `in_progress`. This is the workhorse
 *     call — produced once per finished step.
 *
 *   - `complete_current` — flip the current `in_progress` row to
 *     `completed` without promoting the next one. Rare; `advance`
 *     covers the common path.
 */
export type TodoAction = 'set' | 'advance' | 'complete_current';

export type OutputItem =
  | { type: 'text'; content: string; isPendingMarkdown?: boolean; truncatedPrefix?: string }
  | {
      type: 'reasoning';
      content: string;
      isPendingMarkdown?: boolean;
      /** Truncated characters held back from the visible content (lazy load). */
      truncatedPrefix?: string;
      /**
       * Stable id used to track per-block UI state (e.g. which blocks the
       * user has explicitly expanded). Optional for legacy items persisted
       * from earlier sessions — the renderer falls back to the item's
       * array index in that case.
       */
      reasoningId?: string;
      /** Wall-clock start used to keep elapsed time stable across remounts. */
      startedAt?: number;
      /** Frozen elapsed time once the block reaches a terminal state. */
      durationMs?: number;
      /**
       * `true` once the assistant has begun emitting final-answer content,
       * marking the reasoning block as complete and eligible for auto-collapse.
       */
      completed?: boolean;
    }
  | {
      type: 'tool_call_start';
      toolCallId: string;
      toolName: string;
      arguments: Record<string, unknown>;
      rawArguments?: string;
      streamingContent?: string;
      isExecuting?: boolean;
      result?: string;
      status?: 'success' | 'error';
      duration?: number;
      diffSummary?: StreamDiffSummary;
      /** Wall-clock start for terminal reconciliation if a result is lost. */
      startedAt?: number;
    }
  | {
      type: 'tool_result';
      toolCallId: string;
      status: 'success' | 'error';
      result: string;
      duration?: number;
      diffSummary?: StreamDiffSummary;
    }
  | { type: 'tool_error'; toolCallId: string; error: string }
  | {
      type: 'subagent_block';
      /**
       * Sub-message id used to thread the nested conversation block into
       * the parent's message list. Each `subagent_start` event mints one.
       */
      subMessageId: string;
      /** Display label for the block header (e.g. "Word 文档专家"). */
      label: string;
      /** Sub-agent's expert name (e.g. "office_word_expert"). */
      expert: string;
      /** Task text passed to the sub-agent. Shown when the block is collapsed. */
      task: string;
      /** Cached rendered body for the sub-agent — keeps the block cheap to open. */
      children: import('./agent').AgentMessage[];
      /** True while the sub-agent is still streaming. */
      isStreaming: boolean;
      /** True once the block has been collapsed (rendered as a one-line summary). */
      collapsed: boolean;
    };

export interface CurrentDiff {
  originalText: string;
  newText: string;
  hunks: DiffHunk[];
  summary: string;
  filePath?: string;
}

export interface ActiveToolCall {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
  status: ToolCallStatus;
  result?: string;
  error?: string;
  startTime: number;
  duration?: number;
  diffSummary?: StreamDiffSummary;
}

export interface ChatMessage {
  id: string;
  role: MessageRole;
  timestamp: number;
  content?: string;
  /** Provider-neutral images attached to this user turn. Keeping them on the
   * message lets prior visual context survive history rebuilds and ensures an
   * edited/re-sent turn uses the same source pixels instead of silently
   * becoming text-only. */
  imageAttachments?: ImageAttachmentInput[];
  /**
   * Overflow characters removed from `content` (or from the trailing text
   * OutputItem) to keep the rendered DOM small. Empty string means no
   * truncation has occurred. The full content is reconstructed lazily when
   * the user expands the message.
   */
  truncatedPrefix?: string;
  /**
   * Marks a message as having been collapsed into a history placeholder.
   * The component layer replaces the message DOM with a single compact
   * card; the real data (content / outputItems / toolCalls) is left
   * untouched so it can be restored verbatim by `expandCollapsedHistory`.
   *
   * This is a session-wide list-level virtualization signal — distinct
   * from `truncatedPrefix`, which is a per-message single-string tail
   * truncation that fires during streaming.
   */
  collapsed?: true;
  /**
   * Set of reasoning-block ids the user has explicitly expanded. Stored
   * per-message because a single message can contain multiple reasoning
   * blocks (one per `reasoning` event), each with independent collapse
   * state.
   */
  expandedReasoningIds?: string[];
  outputItems: OutputItem[];
  toolCalls?: MessageToolCall[];
  toolResults?: MessageToolResult[];
  toolCallId?: string;
  toolResult?: MessageToolResult;
  diff?: CurrentDiff;
  searchResults?: SearchResult[];
  /**
   * Nested sub-agent activity blocks. Rendered as collapsible sections
   * under the delegate_to card.
   */
  subagentActivities?: SubagentActivity[];
}

/** Represents a sub-agent's nested activity block */
export interface SubagentActivity {
  /** Unique ID for this sub-agent run */
  id: string;
  /** The expert name (e.g., "office_word_expert") */
  expert: string;
  /** Display label (e.g., "Word Document Expert") */
  label: string;
  /** The task given to the sub-agent */
  task: string;
  /** Current status */
  status: 'running' | 'completed' | 'error';
  /** Final summary when completed */
  summary?: string;
  /** Error message if failed */
  error?: string;
  /** Whether the activity block is expanded */
  expanded?: boolean;
  /** Nested output items from the sub-agent */
  outputItems: OutputItem[];
}

export interface ChatSession {
  id: string;
  title: string;
  createdAt: number;
  mode: ChatMode;
  /**
   * Per-session feature toggle state (e.g. "strict KB mode"). Lives on
   * the session so it survives across messages in the same conversation;
   * not persisted across restarts — the toolbar's collapsed state is the
   * only thing that survives (see `AIPanelUiSlice`).
   */
  featureToggles?: FeatureToggleMap;
  messages: ChatMessage[];
  isStreaming: boolean;
  currentDiff: CurrentDiff | null;
  activeToolCalls: ActiveToolCall[];
  pendingDiff: CurrentDiff | null;
  /**
   * Soft-deleted / closed marker. Set by `closeSession` so the session
   * drops out of the header chip bar but stays in the array forever and
   * still appears in the HistorySidebar (with a "已关闭" badge).
   *
   * Only `deleteSession` truly removes a session from history. Closing
   * is reversible (the data is intact on disk and across restarts), so
   * users never lose work just by hiding a conversation tab.
   */
  archived?: boolean;
  /**
   * Monotonically increasing wall-clock timestamp updated every time
   * the session is touched (new message added, stream completed,
   * session reopened from history, etc.). The history sidebar sorts
   * by this so an active or just-restored conversation bubbles to the
   * top of its date group. Falls back to `createdAt` if never set.
   */
  lastActivityAt?: number;
}
