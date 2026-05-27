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

/** Stream-specific diff types for UI display */
export interface StreamDiffChange {
  tag: 'delete' | 'insert' | 'equal';
  old_line: number | null;
  new_line: number | null;
  content: string;
}

export interface StreamDiffHunk {
  id: string;
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  changes: StreamDiffChange[];
}

export interface StreamDiffSummary {
  file_name: string;
  added_lines: number;
  deleted_lines: number;
  hunks: StreamDiffHunk[];
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
// File types
// ============================================================================

export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  is_markdown: boolean;
}

// ============================================================================
// Settings types
// ============================================================================

export interface Settings {
  theme: ThemeType;
  accent_color: string;
  editor_font_size: number;
  editor_font_family: string;
  ai_provider: AIProviderType;
  ai_model: string;
  ai_api_key: string | null;
  ai_base_url: string | null;
  ai_temperature: number;
  ai_max_tokens: number | null;
}

export type ThemeType = 'cursor-dark' | 'cursor-light' | 'high-contrast-dark' | 'high-contrast-light';
export type AIProviderType = 'openai' | 'ollama' | 'official';

// ============================================================================
// Search types
// ============================================================================

export interface SearchResult {
  chunks: SearchChunk[];
  total: number;
}

export interface SearchChunk {
  chunk: EmbeddingChunk;
  score: number;
  citation: Citation;
}

export interface EmbeddingChunk {
  chunk_id: string;
  doc_id: string;
  range: { block_ids: string[]; start_line: number; end_line: number };
  text: string;
  embedding: number[];
  updated_at: string;
}

export interface Citation {
  source_doc: string;
  source_path: string;
  range: string;
  snippet: string;
  hash: string;
}
