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
  old_range: HunkRange;
  new_range: HunkRange;
  changes: DiffChange[];
  summary: string;
}

export interface HunkRange {
  start_line: number;
  end_line: number;
}

export interface DiffChange {
  tag: 'Delete' | 'Insert' | 'Equal';
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

// File types
export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  is_markdown: boolean;
}

// Settings types
export interface Settings {
  theme: ThemeType;
  accent_color: string;
  editor_font_size: number;
  editor_font_family: string;
  ai_provider: AIProviderType;
  ai_model: string;
  ai_api_key: string | null;
  ai_base_url: string | null;
}

export type ThemeType = 'cursor-dark' | 'cursor-light' | 'high-contrast-dark' | 'high-contrast-light';
export type AIProviderType = 'openai' | 'ollama' | 'official';

// Search types
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
