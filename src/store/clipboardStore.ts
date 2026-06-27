import { create } from 'zustand';

export type ClipboardMode = 'cut' | 'copy';

export interface ClipboardState {
  mode: ClipboardMode | null;
  paths: string[];
  setClipboard: (mode: ClipboardMode, paths: string[]) => void;
  clear: () => void;
}

/**
 * In-memory clipboard for file-tree cut/copy/paste.
 *
 * Survives navigation within the app (unlike the OS clipboard) so a user can
 * right-click a file, choose "Cut", navigate, and paste into another folder
 * without round-tripping through the system clipboard.
 */
export const useClipboardStore = create<ClipboardState>((set) => ({
  mode: null,
  paths: [],

  setClipboard: (mode, paths) => {
    if (paths.length === 0) {
      set({ mode: null, paths: [] });
      return;
    }
    set({ mode, paths: [...paths] });
  },

  clear: () => set({ mode: null, paths: [] }),
}));
