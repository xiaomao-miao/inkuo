//! Shared imperative editor handle for menu/toolbar components.
//!
//! `Editor.tsx` builds the `EditorCommands` closure against the live
//! CodeMirror `EditorView` and snapshots it into `useContextMenuStore` for
//! the right-click menu. The top-bar menu lives in a different component
//! tree (TitleBar) and can't read the context menu's snapshot directly,
//! so this store exposes the same shape on a stable module-level singleton.
//!
//! Publishers (the live editor) call `setCommands(...)` on every render
//! that owns the editor; consumers (TitleBar) read the latest snapshot via
//! `getCommands()` and dispatch through the menu.

import { create } from 'zustand';
import type { EditorCommands } from './contextMenuStore';

export type { EditorCommands } from './contextMenuStore';

export interface EditorCapabilities {
  /** CodeMirror has something to undo. Toggled by the editor via the
   *  history field's `undoDepth`/`redoDepth` probe. */
  canUndo: boolean;
  /** CodeMirror has something to redo. */
  canRedo: boolean;
  /** A non-empty browser selection is currently active. */
  hasSelection: boolean;
}

interface EditorHandleState {
  commands: EditorCommands | null;
  capabilities: EditorCapabilities;
  setCommands: (commands: EditorCommands | null) => void;
  setCapabilities: (capabilities: Partial<EditorCapabilities>) => void;
}

const noop = () => {
  /* no-op */
};

const emptyCommands: EditorCommands = {
  cut: noop,
  copy: noop,
  paste: noop,
  selectAll: noop,
  find: noop,
  replace: noop,
  readContent: () => '',
  undo: noop,
  redo: noop,
};

export const useEditorHandleStore = create<EditorHandleState>((set) => ({
  commands: null,
  capabilities: { canUndo: false, canRedo: false, hasSelection: false },
  setCommands: (commands) => set({ commands }),
  setCapabilities: (capabilities) =>
    set((state) => ({ capabilities: { ...state.capabilities, ...capabilities } })),
}));

/** Convenience accessor for code paths that don't need a subscription. */
export function getEditorCommands(): EditorCommands {
  return useEditorHandleStore.getState().commands ?? emptyCommands;
}

/** Convenience accessor for the capability flags. */
export function getEditorCapabilities(): EditorCapabilities {
  return useEditorHandleStore.getState().capabilities;
}
