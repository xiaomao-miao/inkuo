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

const EDITOR_STORAGE_VERSION = 2;
const EDITOR_STORAGE_VERSION_KEY = 'inkuo-editor-version';

function createEditorState(...args: Parameters<typeof createDocumentSlice>): EditorState {
  return {
    ...createDocumentSlice(...args),
    ...createDiffSlice(...args),
    ...createOfficeSlice(...args),
    ...createPreviewSlice(...args),
  };
}

export const useEditorStore = create<EditorState>()(
  persist(
    (...args) => createEditorState(...args),
    {
      name: 'inkuo-editor',
      onRehydrateStorage: () => (state) => {
        if (state) {
          const version = localStorage.getItem(EDITOR_STORAGE_VERSION_KEY);
          if (version !== String(EDITOR_STORAGE_VERSION)) {
            localStorage.setItem(EDITOR_STORAGE_VERSION_KEY, String(EDITOR_STORAGE_VERSION));
          }

          state.documentContents = Object.fromEntries(
            Object.entries(state.documentContents).map(([path, documentState]) => [
              path,
              normalizeDocumentState(documentState),
            ])
          );

          if (version !== String(EDITOR_STORAGE_VERSION)) {
            state.documentContents = {};
          }
        }
      },
      partialize: (state) => ({
        documentContents: state.documentContents,
        isPreviewMode: state.isPreviewMode,
      }),
    }
  )
);

export type { DocumentState, EditorState } from './editorStore.slices';
