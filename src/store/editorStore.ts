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
  const typedState = (persistedState ?? {}) as Partial<Pick<EditorState, 'documentContents' | 'isPreviewMode'>>;

  return {
    documentContents: {},
    isPreviewMode: version === EDITOR_STORAGE_VERSION ? (typedState.isPreviewMode ?? {}) : (typedState.isPreviewMode ?? {}),
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
  applyAllHunks: (path) => {
    useEditorStore.getState().applyAllHunks(path);
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

export type { DocumentState, EditorState, ExcelWorkbook, Sheet } from './editorStore.slices';
