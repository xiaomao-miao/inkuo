//! Barrel module for the AI panel store's per-feature slices.
//!
//! Each slice lives in its own file (see `uiSlice.ts`, `sessionSlice.ts`,
//! …) and exports a single `createXxxSlice` factory. The root
//! `aiPanelStore.ts` composes them in the same order they appear in this
//! re-export, which is also the order persisted + IPC surfaces see them
//! in.

export { createDiffSlice } from './diffSlice';
export { createMessageSlice } from './messageSlice';
export { createSessionSlice } from './sessionSlice';
export { createSubagentSlice } from './subagentSlice';
export { createToolCallSlice } from './toolCallSlice';
export { createUiSlice } from './uiSlice';
