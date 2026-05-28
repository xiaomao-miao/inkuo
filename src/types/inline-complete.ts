// Inline completion types

/** Request for inline completion */
export interface InlineCompletionRequest {
  /** Current document content (either full document, or a snippet around cursor) */
  document: string;
  /** Cursor position.
   * - If `snippet` is not provided: character offset from start of full document.
   * - If `snippet` is provided: character offset within `snippet`.
   */
  cursor_position: number;
  /** Programming language (e.g., 'rust', 'typescript') */
  language: string;
  /** Optional file path for context */
  file_path?: string;

  /** Optional snippet payload to avoid sending full document. */
  snippet?: {
    /** Snippet text around cursor */
    text: string;
    /** Character offset of snippet start in the full document */
    start_offset: number;
  };
}

/** A single completion item */
export interface CompletionItem {
  /** Unique identifier */
  id: string;
  /** The completion text to insert */
  text: string;
  /** Display text (may be truncated for UI) */
  display_text: string;
  /** Confidence score (0.0 - 1.0) */
  score: number;
  /** Range info (optional) */
  range?: CompletionRange;
}

/** Range for the completion */
export interface CompletionRange {
  from: number;
  to: number;
}

/** Response from inline completion request */
export interface InlineCompletionResponse {
  /** List of completion items */
  completions: CompletionItem[];
  /** Model used for completion */
  model: string;
  /** Usage statistics */
  usage?: TokenUsage;
}

/** Token usage info */
export interface TokenUsage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
}

/** Inline completion state */
export interface InlineCompletionState {
  /** Whether inline completion is enabled */
  enabled: boolean;
  /** Current completion being displayed */
  current: CompletionItem | null;
  /** Loading state */
  is_loading: boolean;
  /** Error message if any */
  error: string | null;
}

/** Inline completion store state */
export interface InlineCompleteStoreState {
  // Feature toggle
  enabled: boolean;

  // Current completion state
  currentCompletion: CompletionItem | null;
  isLoading: boolean;
  error: string | null;

  // Settings
  debounceMs: number;
  maxLines: number;

  // Actions
  setEnabled: (enabled: boolean) => void;
  setCompletion: (completion: CompletionItem | null) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  clearCompletion: () => void;
}

/** Language detection helper */
export function detectLanguage(filePath?: string): string {
  if (!filePath) return 'markdown';

  const ext = filePath.split('.').pop()?.toLowerCase() || '';

  const languageMap: Record<string, string> = {
    // Programming languages
    ts: 'typescript',
    tsx: 'typescript',
    js: 'javascript',
    jsx: 'javascript',
    py: 'python',
    rb: 'ruby',
    rs: 'rust',
    go: 'go',
    java: 'java',
    kt: 'kotlin',
    swift: 'swift',
    cpp: 'cpp',
    c: 'c',
    h: 'c',
    hpp: 'cpp',
    cs: 'csharp',
    php: 'php',
    scala: 'scala',
    r: 'r',
    // Web
    html: 'html',
    css: 'css',
    scss: 'scss',
    sass: 'sass',
    less: 'less',
    json: 'json',
    yaml: 'yaml',
    yml: 'yaml',
    xml: 'xml',
    // Shell
    sh: 'bash',
    bash: 'bash',
    zsh: 'bash',
    fish: 'bash',
    // Documents
    md: 'markdown',
    markdown: 'markdown',
    txt: 'text',
  };

  return languageMap[ext] || 'text';
}
