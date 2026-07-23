// Re-export of `./composer`.
//
// The original 581-line `ChatInput.tsx` has been split into focused
// modules under `./composer/`. Existing callers keep working with
// the single import path:
//
//   import { ChatInput, ComposerToggleRows } from './ChatInput';
//
// New code can import individual sub-components, hooks, or pure
// helpers from `./composer` directly.

export * from './composer';