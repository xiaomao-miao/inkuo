import { create } from 'zustand';
import type { FileEntry } from '../types';
import type { OpenTab } from './sidebarStore';

export type ContextMenuKind = 'workspace' | 'entry' | 'tab' | 'selection' | 'docx' | 'editor';

export interface ContextMenuTarget {
  kind: ContextMenuKind;
  /** Absolute path for an entry; for the workspace this is the root path. */
  path: string;
  /** Viewport coordinates of the right-click. */
  x: number;
  y: number;
  /** Required for entry targets; unused for workspace-root targets. */
  entry?: FileEntry;
  /** Required for tab targets; identifies which tab was right-clicked. */
  tab?: OpenTab;
  /**
   * Required for selection targets. The plain-text contents of the
   * browser `Selection` at the moment of right-click. We snapshot it
   * eagerly so the menu can render even if the user collapses the
   * live selection (e.g. by clicking an item) before the menu closes.
   */
  selectionText?: string;
  /**
   * Required for docx targets. Imperative actions bound to the
   * docx editor's ProseMirror view (undo, redo, cut, copy, paste,
   * select-all). The host (`OfficeViewer`) snapshots these at the
   * moment of right-click so the menu can dispatch them even if the
   * editor is unmounted by the time the user clicks an item.
   */
  docxCommands?: DocxCommands;
  /**
   * Required for editor targets (the markdown / code / text editor).
   * Imperative actions that route to the live CodeMirror view, plus
   * the file path so the menu can build per-file AI actions. Same
   * snapshot-at-right-click rationale as `docxCommands` — the user
   * may pick a menu row long after the right-click.
   */
  editorCommands?: EditorCommands;
}

/**
 * Imperative wrappers over the live PM `EditorView` for the docx
 * editor. Each entry is a `() => void` so the menu renderer doesn't
 * need to know about ProseMirror. A no-op stub is used when the
 * command cannot run (e.g. nothing to undo).
 */
export interface DocxCommands {
  undo: () => void;
  redo: () => void;
  cut: () => void;
  copy: () => void;
  paste: () => void;
  selectAll: () => void;
  /**
   * Open the editor's built-in find dialog. Equivalent to pressing
   * Ctrl+F when the editor has focus. Implemented by focusing the
   * editor and dispatching a synthetic keydown event so the
   * editor's own keymap (mounted in capture phase) picks it up
   * regardless of focus routing.
   */
  find: () => void;
  /**
   * Open the editor's built-in replace dialog (Ctrl+H). Same
   * focus + synthetic-event approach as `find`.
   */
  replace: () => void;
  /** Snapshot of the editor's capability flags at right-click time.
   *  Used to disable items that wouldn't have any effect (e.g. "Undo"
   *  when there's no history). The snapshot is cheap because ProseMirror
   *  exposes these as plain booleans. The menu re-renders on `target`
   *  change so the flags stay consistent with the click moment. */
  canUndo: boolean;
  canRedo: boolean;
  hasSelection: boolean;
  hasClipboard: boolean;
}

/**
 * Imperative wrappers over the live CodeMirror `EditorView` for the
 * markdown / code / text editor. Each entry is a `() => void` so the
 * menu renderer doesn't need to know about CodeMirror. A no-op stub
 * is used when the command cannot run (e.g. nothing to copy).
 *
 * Mirrors the `DocxCommands` shape used by the docx editor so the
 * menu renderer can handle both editor kinds uniformly.
 */
export interface EditorCommands {
  cut: () => void;
  copy: () => void;
  paste: () => void;
  selectAll: () => void;
  /** Open the editor's built-in find dialog (Ctrl+F). */
  find: () => void;
  /** Open the editor's built-in replace dialog (Ctrl+H). */
  replace: () => void;
  /** Read the current document text — used by file-level AI actions
   *  ("用 AI 处理此文件" inside the editor body). Cheap to call. */
  readContent: () => string;
}

interface ContextMenuState {
  target: ContextMenuTarget | null;
  open: (target: ContextMenuTarget) => void;
  close: () => void;
}

/**
 * Single-instance context menu state. The FileTree pushes a target on
 * `onContextMenu`, the ContextMenu component reads and clears it.
 */
export const useContextMenuStore = create<ContextMenuState>((set) => ({
  target: null,
  open: (target) => set({ target }),
  close: () => set({ target: null }),
}));
