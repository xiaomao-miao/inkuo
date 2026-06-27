import { create } from 'zustand';
import type { FileEntry } from '../types';

export type ContextMenuKind = 'workspace' | 'entry';

export interface ContextMenuTarget {
  kind: ContextMenuKind;
  /** Absolute path for an entry; for the workspace this is the root path. */
  path: string;
  /** Viewport coordinates of the right-click. */
  x: number;
  y: number;
  /** Required for entry targets; unused for workspace-root targets. */
  entry?: FileEntry;
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
