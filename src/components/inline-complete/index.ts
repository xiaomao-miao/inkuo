// Inline Complete Module - Public API

export { InlineCompleteProvider, useInlineComplete } from './InlineCompleteProvider';
export { InlineCompleteStatus } from './GhostTextOverlay';
export { inlineCompletionDecoration } from './InlineCompletionDecoration';
export {
  scheduleWordInlineCompletion,
  clearWordTimers,
  clearWordTimersForEditor,
  markAccepted,
} from './useWordInlineCompleteTrigger';
export { createWordInlineCompletePlugin, showWordInlineCompletion, clearWordInlineCompletion } from './wordInlineCompletePlugin';
export type { InlineStyle } from '../../types/inline-complete';
