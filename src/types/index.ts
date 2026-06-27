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
  | 'error'
  | 'tool_call_start'
  | 'tool_result'
  | 'done';

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

export type ChatMode = 'ask' | 'plan' | 'agent' | 'knowledge';

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

export type OutputItem =
  | { type: 'text'; content: string; isPendingMarkdown?: boolean }
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
  | { type: 'tool_error'; toolCallId: string; error: string };

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
  outputItems: OutputItem[];
  toolCalls?: MessageToolCall[];
  toolResults?: MessageToolResult[];
  toolCallId?: string;
  toolResult?: MessageToolResult;
  diff?: CurrentDiff;
  searchResults?: SearchResult[];
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
  messages: ChatMessage[];
  isStreaming: boolean;
  currentDiff: CurrentDiff | null;
  activeToolCalls: ActiveToolCall[];
  pendingDiff: CurrentDiff | null;
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
