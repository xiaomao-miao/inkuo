// Shared constants across the application
// Centralizes all magic numbers for maintainability

export const TIMING = {
  /** Delay before clearing tool calls after stream ends (ms) */
  TOOL_CALL_CLEAR_DELAY_MS: 2000,

  /** Throttle between cancel RPC invocations (ms) */
  INLINE_COMPLETION_CANCEL_THROTTLE_MS: 120,

  /** Minimum interval between flushing pending stream deltas (ms) */
  STREAM_FLUSH_INTERVAL_MS: 16,

  /** Delay before hiding DocxEditor's top menu buttons (ms) */
  OFFICE_MENU_HIDE_DELAY_MS: 1000,

  /** Cooldown before re-triggering completion after acceptance (ms) */
  COMPLETION_RETRIGGER_DELAY_MS: 300,

  /** Debounce for menu hide operations (ms) */
  MENU_HIDE_DEBOUNCE_MS: 100,

  /** Long-press threshold for context menu (ms) */
  CONTEXT_MENU_LONG_PRESS_MS: 500,
} as const;

// ============================================================================
// Document snippet bounds for inline completion
// ============================================================================

/** Snippet bounds for CodeMirror (Markdown) editor */
export const CODEMIRROR_SNIPPET_BOUNDS = {
  /** Max characters to include before cursor */
  MAX_BEFORE: 8000,
  /** Max characters to include after cursor */
  MAX_AFTER: 2000,
} as const;

/** Snippet bounds for ProseMirror (Docx) editor */
export const PROSEMIRROR_SNIPPET_BOUNDS = {
  /** Max characters to include before cursor */
  MAX_BEFORE: 6000,
  /** Max characters to include after cursor */
  MAX_AFTER: 1500,
} as const;
