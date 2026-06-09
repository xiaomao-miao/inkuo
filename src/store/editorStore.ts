import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import {
  createDiffSlice,
  createDocumentSlice,
  createOfficeSlice,
  createPreviewSlice,
  normalizeDocumentState,
  type EditorState,
} from './editorStore.slices';
import type { DiffApplicationActions } from './aiPanelStore.types';

const EDITOR_STORAGE_VERSION = 2;

function migrateEditorState(
  persistedState: unknown,
  version: number,
): Pick<EditorState, 'documentContents' | 'isPreviewMode'> {
  const typedState = (persistedState ?? {}) as Partial<Pick<EditorState, 'documentContents' | 'isPreviewMode'>>;

  if (version !== EDITOR_STORAGE_VERSION) {
    return {
      documentContents: {},
      isPreviewMode: typedState.isPreviewMode ?? {},
    };
  }

  return {
    documentContents: Object.fromEntries(
      Object.entries(typedState.documentContents ?? {}).map(([path, documentState]) => [
        path,
        normalizeDocumentState(documentState),
      ])
    ),
    isPreviewMode: typedState.isPreviewMode ?? {},
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
        documentContents: state.documentContents,
        isPreviewMode: state.isPreviewMode,
      }),
    }
  )
);

export type { DocumentState, EditorState } from './editorStore.slices';
