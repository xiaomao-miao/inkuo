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

  /**
   * Stream flush interval adapts to buffer pressure:
   *   - bufferLen <= MIN_BUFFER_CHARS_FOR_SLOWDOWN: STREAM_FLUSH_INTERVAL_MS
   *   - bufferLen >= MAX_BUFFER_CHARS_BEFORE_FORCE_FLUSH: force flush immediately
   * The interval grows linearly from MIN to MAX as the buffer fills.
   */
  /** Minimum flush interval (ms) — used when the buffer is small. */
  STREAM_FLUSH_INTERVAL_MIN_MS: 16,
  /** Maximum flush interval (ms) — used when the buffer is large. */
  STREAM_FLUSH_INTERVAL_MAX_MS: 80,
  /** Buffer size at which we start stretching the flush interval. */
  MIN_BUFFER_CHARS_FOR_SLOWDOWN: 200,
  /** Buffer size above which we force-flush regardless of timer. */
  MAX_BUFFER_CHARS_BEFORE_FORCE_FLUSH: 1600,
  /** Per-message soft cap (chars). Past this we drop the head on flush. */
  MESSAGE_TRUNCATE_THRESHOLD_CHARS: 16000,
  /** How much head to keep once truncation kicks in. */
  MESSAGE_TRUNCATE_KEEP_TAIL_CHARS: 8000,

  /**
   * Reasoning-block flush tunables. Reasoning content is typically much
   * larger than the final answer and is rendered collapsed by default, so
   * we tolerate a larger buffer (less frequent React re-renders) and only
   * force-flush under heavier pressure.
   */
  REASONING_FLUSH_INTERVAL_MIN_MS: 32,
  REASONING_FLUSH_INTERVAL_MAX_MS: 120,
  REASONING_MIN_BUFFER_CHARS_FOR_SLOWDOWN: 400,
  REASONING_MAX_BUFFER_CHARS_BEFORE_FORCE_FLUSH: 3200,

  /**
   * Pixel distance from the top of the scroll container at which
   * truncated-prefix auto-expand kicks in. Smaller = user has to be
   * closer to the very top before content is restored.
   */
  TRUNCATED_PREFIX_AUTOEXPAND_SCROLL_PX: 64,

  /**
   * List-level virtualization tunables.
   *
   * When the session has more than `SESSION_VIRTUALIZE_THRESHOLD`
   * messages, the older ones are replaced in the DOM by a single
   * "collapsed history" placeholder card. The full message data
   * remains in the store, so a click on the placeholder can restore
   * them at any time. This prevents React from re-rendering dozens of
   * (potentially heavy) markdown bodies on every streaming token and
   * keeps the message list DOM bounded regardless of session length.
   */
  SESSION_VIRTUALIZE_THRESHOLD: 50,
  /**
   * Number of older messages to reveal when the user expands a collapsed
   * history placeholder. The newest messages are always shown; this is
   * how many EXTRA older ones get added back to the live DOM per click.
   */
  SESSION_VIRTUALIZE_EXPAND_BATCH: 50,
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
