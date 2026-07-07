// Document types
export interface Document {
  id: string;
  path: string;
  doc_type: DocumentType;
  title: string;
  blocks: Block[];
  updated_at: string;
  hash: string;
}

export type DocumentType = 'Markdown' | 'Word' | 'Excel' | 'PlainText';

export interface Block {
  id: string;
  kind: BlockKind;
  range: Range;
  text: string;
  metadata: Record<string, unknown>;
}

export type BlockKind =
  | 'Paragraph'
  | { Heading: number }
  | { CodeBlock: string | null }
  | { List: boolean }
  | 'ListItem'
  | 'Table'
  | 'TableRow'
  | 'TableCell'
  | 'Blockquote'
  | 'HorizontalRule';

export interface Range {
  start_line: number;
  start_col: number;
  end_line: number;
  end_col: number;
}

// Diff types
export interface DiffResult {
  hunks: DiffHunk[];
  summary: DiffSummary;
}

export interface DiffHunk {
  id: string;
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  /** Character offset in the original text where this hunk starts */
  old_offset: number;
  /** Character offset in the modified text where this hunk starts */
  new_offset: number;
  changes: DiffChange[];
}

export interface DiffChange {
  tag: 'delete' | 'insert' | 'equal';
  old_line: number | null;
  new_line: number | null;
  content: string;
}

export interface DiffSummary {
  added_lines: number;
  deleted_lines: number;
  unchanged_lines: number;
  description: string;
}

export interface StreamDiffSummary {
  file_name: string;
  added_lines: number;
  deleted_lines: number;
  hunks: DiffHunk[];
}

// Stream types
export interface OfficeFileModifiedPayload {
  path: string;
  format: string;
}

export interface StreamPayload {
  session_id: string;
  message_id: string;
  event_type: StreamEventType | 'tool_call_args_delta';
  content?: string;
  summary?: string;
  tool_call_id?: string;
  tool_name?: string;
  tool_args?: string;
  final_content?: string;
  error?: string;
  search_results?: KnowledgeSearchResult[];
  done: boolean;
  file_path?: string;
  original_content?: string;
  new_content?: string;
  diff_summary?: StreamDiffSummary;
  office_file_modified?: OfficeFileModifiedPayload;
}

/** Payload for subagent_start event */
export interface SubagentStartPayload {
  session_id: string;
  parent_message_id: string;
  sub_message_id: string;
  expert: string;
  label: string;
  task: string;
}

// AI types
export interface AIEditRequest {
  instruction: string;
  original_text: string;
  scope: EditScope;
  context: ContextItem[];
}

export type EditScope = 'Selection' | 'Paragraph' | 'Section' | 'Document';

export interface ContextItem {
  title: string;
  path: string;
  range: string;
  excerpt: string;
}

export interface AIEditResponse {
  summary: string;
  content: string;
  rules_applied: string[];
}

// ============================================================================
// Agent & Tool Calling Types
// ============================================================================

/** Tool definition following OpenAI function calling format */
export interface ToolDefinition {
  type: 'function';
  function: ToolFunction;
}

export interface ToolFunction {
  name: string;
  description: string;
  parameters: ToolParameters;
}

export interface ToolParameters {
  type: 'object';
  properties: Record<string, ToolParameter>;
  required: string[];
  additionalProperties?: boolean;
}

export interface ToolParameter {
  type: string;
  description?: string;
  default?: unknown;
}

/** Tool call request from AI */
export interface ToolCall {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
}

/** Tool execution status */
export type ToolCallStatus = 'pending' | 'executing' | 'success' | 'error';

/** Tool call with execution result */
export interface ToolCallResult {
  toolCallId: string;
  toolName: string;
  arguments: Record<string, unknown>;
  status: ToolCallStatus;
  result?: string;
  error?: string;
  duration?: number; // Execution time in ms
}

/** Message role including tool role */
export type MessageRole = 'user' | 'assistant' | 'system' | 'tool';

/** Agent message in conversation */
export interface AgentMessage {
  id: string;
  role: MessageRole;
  content: string;
  timestamp: number;
  toolCalls?: ToolCall[];
  toolCallId?: string; // If role is 'tool', this is the associated call ID
}

/** Stream event from backend */
export interface StreamEvent {
  session_id: string;
  message_id: string;
  event_type: StreamEventType;
  content?: string;
  summary?: string;
  tool_call_id?: string;
  tool_name?: string;
  tool_args?: string;
  final_content?: string;
  error?: string;
  done: boolean;
}

/** Stream event types */
export type StreamEventType =
  | 'text'
  | 'reasoning'
  | 'error'
  | 'tool_call_start'
  | 'tool_result'
  | 'done'
  | 'subagent_start'
  | 'subagent_end';

/** Agent session configuration */
export interface AgentConfig {
  maxIterations: number;
  autoExecute: boolean; // Execute tools automatically without confirmation
  workspacePath?: string;
}

/** Agent mode */
export type AgentMode = 'ask' | 'plan' | 'agent';

/** Agent status */
export type AgentStatus = 'idle' | 'thinking' | 'executing' | 'error';

// ============================================================================
// AI panel types
// ============================================================================

/** Chat mode. The legacy dedicated "knowledge" mode is gone —
 * knowledge-base behavior is now layered on via per-message feature
 * toggles (see `FeatureToggle` / `featureTogglesSlice`). */
export type ChatMode = 'ask' | 'plan' | 'agent';

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
export type FeatureToggleId = 'kb_strict' | 'web_search';

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

export type PlanFileIntent = 'read' | 'create' | 'modify' | 'delete' | 'rename';
export type PlanRisk = 'low' | 'medium' | 'high';

export interface PlanFileTouch {
  path: string;
  intent: PlanFileIntent;
  reason: string;
}

export interface PlanOutput {
  plan_summary: string;
  files_to_touch: PlanFileTouch[];
  risk: PlanRisk;
  risk_reason?: string;
  needs_confirmation: boolean;
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
      type: 'plan';
      /**
       * Model's raw output text (Markdown prose + ```plan JSON block).
       * Used for fallback display when plan parsing fails.
       */
      rawText: string;
      /**
       * Parsed structured plan data. `null` while still collecting
       * or if JSON parsing failed.
       */
      plan: PlanOutput | null;
      /** Set when JSON.parse threw after the ```plan block was closed. */
      parseError?: string;
      /** True while the model is still streaming the plan output. */
      isStreaming?: boolean;
      /**
       * Plan id (filename stem) under `<workspace>/.inkuo/plans/<planFileId>.md`
       * once the plan has been persisted. `undefined` means not yet saved.
       * On apply / cancel / session-close the frontend asks Rust to delete
       * this file if present.
       */
      planFileId?: string;
      /** Absolute path to the persisted plan md on disk, if known. */
      planFilePath?: string;
    };

export interface SearchResult {
  chunkId: string;
  documentId: string;
  content: string;
  score: number;
  documentTitle: string;
  filePath: string;
  startLine?: number;
  endLine?: number;
}

export interface KnowledgeSearchResult {
  chunk_id: string;
  document_id: string;
  content: string;
  score: number;
  document_title: string;
  file_path: string;
  start_line?: number;
  end_line?: number;
}

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
  status: 'pending' | 'executing' | 'success' | 'error';
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

export interface KnowledgeBase {
  workspaceId: string;
  documentCount: number;
  chunkCount: number;
  lastUpdated: number;
  /** Explicitly selected member file paths (relative to workspace) */
  members: string[];
}

export interface BuildProgress {
  phase: 'scanning' | 'chunking' | 'embedding' | 'storing' | 'done';
  current: number;
  total: number;
  currentFile?: string;
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

// ============================================================================
// File types
// ============================================================================

export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  is_markdown: boolean;
}

/** Payload for the `create_file_entry` Rust command. */
export type NewEntryPayload =
  | { kind: 'file'; extension: string; template?: string }
  | { kind: 'directory' };

export interface CreateEntryResult {
  path: string;
}

export interface RenamePathResult {
  from: string;
  to: string;
}

/** Built-in template presets for new-file creation in the context menu. */
export interface NewFileTemplate {
  id: string;
  label: string;
  extension: string;
  template: string;
}

export const NEW_FILE_TEMPLATES: readonly NewFileTemplate[] = [
  { id: 'md', label: 'Markdown', extension: 'md', template: '# 无标题\n\n开始书写…\n' },
  { id: 'txt', label: '纯文本', extension: 'txt', template: '' },
  { id: 'docx', label: 'Word 文档', extension: 'docx', template: '' },
  { id: 'xlsx', label: 'Excel 工作簿', extension: 'xlsx', template: '' },
] as const;

// ============================================================================
// Settings types
// ============================================================================

export type AIProviderType = 'openai' | 'ollama' | 'deepseek' | 'official';

/** API configuration for a single model provider */
export interface APIConfig {
  id: string;                    // Unique identifier
  name: string;                 // Display name (e.g., "DeepSeek V3", "GPT-4")
  provider: AIProviderType;     // Provider type
  baseUrl: string;              // API base URL
  apiKey: string | null;        // API key (encrypted in storage)
  model: string;                // Model name
  isDefault: boolean;            // Whether this is the default API
  enabled: boolean;              // Whether this API is enabled
  temperature: number;          // Default temperature for this API
  maxTokens: number | null;      // Default max tokens for this API
}

export interface Settings {
  theme: ThemeType;
  accent_color: string;
  editor_font_size: number;
  editor_font_family: string;
  editor_word_wrap: boolean;
  editor_line_numbers: boolean;
  apiConfigs: APIConfig[];
  activeApiConfigId: string | null;
  embedding_model: EmbeddingModelType;
  embedding_model_path: string | null;
  chunk_size: number;
  chunk_overlap: number;
  snapshot: {
    maxCount: number;
    autoBaseline: boolean;
  };
  /**
   * Hard cap on the Agent's tool-calling loop. Roughly the upper bound on
   * how many "round trips" between the LLM and the tool registry the main
   * Agent session will perform before giving up with a `MaxIterationsReached`
   * error. 1–200, default 50 (matches the Rust default).
   */
  agent_max_iterations: number;
}

/** Supported embedding models */
export type EmbeddingModelType =
  | 'BAAI/bge-small-zh-v1.5'
  | 'BAAI/bge-base-zh-v1.5'
  | 'BAAI/bge-large-zh-v1.5';

/** Embedding model info for display */
export interface EmbeddingModelInfo {
  id: EmbeddingModelType;
  name: string;
  dimensions: number;
  size: string;
  description: string;
}

export type ThemeType = 'cursor-dark' | 'cursor-light' | 'high-contrast-dark' | 'high-contrast-light';
