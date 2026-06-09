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
