import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import {
  createDiffSlice,
  createDocumentSlice,
  createOfficeSlice,
  createPreviewSlice,
  type EditorState,
} from './editorStore.slices';
import type { DiffApplicationActions } from './aiPanelStore.types';

const EDITOR_STORAGE_VERSION = 3;

function migrateEditorState(
  persistedState: unknown,
  version: number,
): Pick<EditorState, 'documentContents' | 'isPreviewMode'> {
  const typedState = (persistedState ?? {}) as Partial<
    Pick<EditorState, 'documentContents' | 'isPreviewMode'>
  >;

  // `documentContents` is intentionally wiped on every migration — it caches
  // heavy document state that is reloaded from disk on demand. `isPreviewMode`
  // is a small UI preference and is preserved across migrations of the same
  // version; only a downgrade (or unknown future version) discards it so we
  // don't resurrect stale entries against a newer schema.
  const preservePreview = version === EDITOR_STORAGE_VERSION;

  return {
    documentContents: {},
    isPreviewMode: preservePreview ? (typedState.isPreviewMode ?? {}) : {},
  };
}

function createEditorState(...args: Parameters<typeof createDocumentSlice>): EditorState {
  return {
    ...createDocumentSlice(...args),
    ...createDiffSlice(...args),
    ...createOfficeSlice(...args),
    ...createPreviewSlice(...args),
  };
}

export const editorDiffActions: DiffApplicationActions = {
  applyHunk: (path, hunkId) => {
    useEditorStore.getState().applyHunk(path, hunkId);
  },
  rejectHunk: (path, hunkId) => {
    useEditorStore.getState().rejectHunk(path, hunkId);
  },
  applyAllHunks: (path) => {
    useEditorStore.getState().applyAllHunks(path);
  },
  rejectAllHunks: (path) => {
    useEditorStore.getState().rejectAllHunks(path);
  },
};

export const useEditorStore = create<EditorState>()(
  persist(
    (...args) => createEditorState(...args),
    {
      name: 'inkuo-editor',
      version: EDITOR_STORAGE_VERSION,
      migrate: migrateEditorState,
      partialize: (state) => ({
        documentContents: {},
        isPreviewMode: state.isPreviewMode,
      }),
    }
  )
);

export type {
  DocumentState, EditorState,
  FortuneSheetWorkbook, Sheet, CellData, Cell, Config,
} from './editorStore.slices';
