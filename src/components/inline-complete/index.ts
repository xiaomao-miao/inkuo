// Inline Complete Module - Public API

export { InlineCompleteProvider } from './InlineCompleteProvider';
export { useInlineComplete } from './useInlineComplete';
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
