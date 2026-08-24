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

/**
 * Imperative save hook owned by an editor whose authoritative document model
 * cannot be reconstructed from the generic text store (currently Word and
 * Excel). `true` means the write completed, while `false` keeps the tab/window
 * open so a failed save can never turn into silent data loss.
 */
export type DocumentSaveHandler = () => Promise<boolean>;

interface EditorHandleState {
  commands: EditorCommands | null;
  capabilities: EditorCapabilities;
  documentSaveHandlers: Map<string, DocumentSaveHandler>;
  setCommands: (commands: EditorCommands | null) => void;
  setCapabilities: (capabilities: Partial<EditorCapabilities>) => void;
  registerDocumentSaveHandler: (path: string, handler: DocumentSaveHandler) => void;
  unregisterDocumentSaveHandler: (path: string, handler?: DocumentSaveHandler) => void;
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
  documentSaveHandlers: new Map(),
  setCommands: (commands) => set({ commands }),
  setCapabilities: (capabilities) =>
    set((state) => ({ capabilities: { ...state.capabilities, ...capabilities } })),
  registerDocumentSaveHandler: (path, handler) =>
    set((state) => {
      const handlers = new Map(state.documentSaveHandlers);
      handlers.set(path, handler);
      return { documentSaveHandlers: handlers };
    }),
  unregisterDocumentSaveHandler: (path, handler) =>
    set((state) => {
      const registered = state.documentSaveHandlers.get(path);
      // A cleanup from an older render must not unregister the replacement
      // handler installed by a newer render of the same editor.
      if (!registered || (handler && registered !== handler)) return {};
      const handlers = new Map(state.documentSaveHandlers);
      handlers.delete(path);
      return { documentSaveHandlers: handlers };
    }),
}));

/** Convenience accessor for code paths that don't need a subscription. */
export function getEditorCommands(): EditorCommands {
  return useEditorHandleStore.getState().commands ?? emptyCommands;
}

/** Convenience accessor for the capability flags. */
export function getEditorCapabilities(): EditorCapabilities {
  return useEditorHandleStore.getState().capabilities;
}

/** Resolve the live editor-owned save hook for a path, if one is mounted. */
export function getDocumentSaveHandler(path: string): DocumentSaveHandler | null {
  return useEditorHandleStore.getState().documentSaveHandlers.get(path) ?? null;
}
