/**
 * Chat mode constants — single source of truth for the cycle order and
 * the canonical default.
 *
 * Previously the order array `['ask', 'plan', 'agent']` was inlined in
 * `useChatSessionActions.cycleMode`, and the default mode was repeated
 * in `createNewSession` and `useAIPanelController`. Centralising them
 * here keeps the cycle order consistent across the UI (chat header,
 * cycle button, default-mode selectors).
 */
import type { ChatMode } from '../types';

export const CHAT_MODES = ['ask', 'plan', 'agent'] as const satisfies readonly ChatMode[];

/** Mode the panel boots into for a brand-new session. */
export const DEFAULT_CHAT_MODE: ChatMode = 'ask';

/** Human-readable label for the chat header / mode chip. */
export const CHAT_MODE_LABEL: Record<ChatMode, string> = {
  ask: 'Ask',
  plan: 'Plan',
  agent: 'Agent',
};

/** One-line hint shown next to the mode chip in the composer. */
export const CHAT_MODE_HINT: Record<ChatMode, string> = {
  ask: '只回答（不修改文件）',
  plan: '只输出计划（不修改文件）',
  agent: 'Full Agent（可调用工具读写文件）',
};

/**
 * Advance to the next mode in the cycle. Used by the chat header's
 * cycle button (and any future keyboard shortcut for it).
 *
 * Pure function so it's trivially testable.
 */
export function nextChatMode(current: ChatMode): ChatMode {
  const idx = CHAT_MODES.indexOf(current);
  // `idx` is always valid because `ChatMode` is a strict subset of
  // `CHAT_MODES`, but we guard with a fallback to keep TypeScript
  // happy and to make a defensive runtime choice if the union ever
  // gains a value that isn't in the cycle.
  if (idx < 0) return CHAT_MODES[0];
  return CHAT_MODES[(idx + 1) % CHAT_MODES.length];
}
