import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { DiffHunk, Document } from '../types';

const EDITOR_STORAGE_VERSION = 2;
const EDITOR_STORAGE_VERSION_KEY = 'inkuo-editor-version';

interface DocumentState {
  document: Document | null;
  content: string;
  mtime: number;  // Unix timestamp in ms, used for cache invalidation
  isDirty: boolean;
  selection: { from: number; to: number } | null;
  diffHunks: DiffHunk[];
  /** The original text fragment that was sent to AI for modification */
  diffOriginalText: string;
  /** Character offset in the full content where diffOriginalText starts */
  diffOriginalOffset: number;
  activeHunkIndex: number;
  isDiffMode: boolean;
  docxBuffer: number[] | null;
  excelData: string[][] | null;
  officeBufferVersion: number;
}

interface EditorState {
  documentContents: Record<string, DocumentState>;

  setDocumentContent: (path: string, doc: Document, content: string, mtime?: number) => void;
  setContent: (path: string, content: string) => void;
  setSelection: (path: string, selection: { from: number; to: number } | null) => void;
  setDiffHunks: (path: string, hunks: DiffHunk[], originalText: string, originalOffset: number) => void;
  setActiveHunkIndex: (path: string, index: number) => void;
  setIsDiffMode: (path: string, isDiff: boolean) => void;
  applyHunk: (path: string, hunkId: string) => void;
  rejectHunk: (path: string, hunkId: string) => void;
  applyAllHunks: (path: string) => void;
  rejectAllHunks: (path: string) => void;
  clearDiff: (path: string) => void;
  markSaved: (path: string) => void;
  updateTabDirty: (path: string, isDirty: boolean) => void;
  removeDocumentContent: (path: string) => void;
  setDocxBuffer: (path: string, buffer: number[]) => void;
  setExcelData: (path: string, data: string[][]) => void;
  clearDocxBuffer: (path: string) => void;
  clearExcelData: (path: string) => void;
  invalidateOfficeBuffer: (path: string) => void;
  isPreviewMode: Record<string, boolean>;
  togglePreviewMode: (path: string) => void;
}

// ============================================================================
// Helper Functions
// ============================================================================

/** Apply a single hunk's changes to the original text fragment.
 *  Returns (newText, replacementText) where replacementText is the full
 *  text that should replace the old hunk region. */
function applyHunkToText(
  originalText: string,
  hunk: DiffHunk,
): string {
  // The hunk's new text is reconstructed from its changes.
  // The old region starts at hunk.old_offset in originalText.
  // The new region starts at hunk.new_offset in the AI response.
  const oldContent = hunk.changes
    .filter((c) => c.tag !== 'insert')
    .map((c) => c.content)
    .join('');
  const newContent = hunk.changes
    .filter((c) => c.tag !== 'delete')
    .map((c) => c.content)
    .join('');

  // Find exact old region by matching content
  const oldIdx = originalText.indexOf(oldContent, hunk.old_offset);
  if (oldIdx === -1 || oldIdx !== hunk.old_offset) {
    // Fallback: replace at the recorded offset
    return (
      originalText.slice(0, hunk.old_offset) +
      newContent +
      originalText.slice(hunk.old_offset + oldContent.length)
    );
  }
  return (
    originalText.slice(0, hunk.old_offset) +
    newContent +
    originalText.slice(hunk.old_offset + oldContent.length)
  );
}

/** Apply all remaining hunks to the original text fragment.
 *  Hunks are sorted by offset descending so earlier positions aren't shifted. */
function applyAllHunksToText(originalText: string, hunks: DiffHunk[]): string {
  const sorted = [...hunks].sort((a, b) => b.old_offset - a.old_offset);
  let text = originalText;
  for (const hunk of sorted) {
    text = applyHunkToText(text, { ...hunk, old_offset: hunk.old_offset });
  }
  return text;
}

/** Apply a single hunk's new text to the document at the correct position. */
function applySingleHunk(
  content: string,
  originalText: string,
  diffOffset: number,
  hunk: DiffHunk,
): string {
  // Apply to the local copy of the original text, then splice into content
  const localText = applyHunkToText(originalText, hunk);
  const endOffset = diffOffset + originalText.length;
  return content.slice(0, diffOffset) + localText + content.slice(endOffset);
}

/** Create a default document state for a new document */
function createDefaultDocumentState(overrides?: Partial<DocumentState>): DocumentState {
  return {
    document: null,
    content: '',
    mtime: 0,
    isDirty: false,
    selection: null,
    diffHunks: [],
    diffOriginalText: '',
    diffOriginalOffset: 0,
    activeHunkIndex: 0,
    isDiffMode: false,
    docxBuffer: null,
    excelData: null,
    officeBufferVersion: 0,
    ...overrides,
  };
}

/** Type for partial document state updates */
type DocumentStateUpdate = Partial<DocumentState>;

/** Helper to update a single document in the documentContents map */
function updateDocument(
  state: { documentContents: Record<string, DocumentState> },
  path: string,
  update: DocumentStateUpdate
): Pick<typeof state, 'documentContents'> | typeof state {
  const current = state.documentContents[path];
  if (!current) return state;
  return {
    documentContents: {
      ...state.documentContents,
      [path]: { ...current, ...update },
    },
  };
}

export const useEditorStore = create<EditorState>()(
  persist(
    (set) => ({
      documentContents: {},
      isPreviewMode: {},

      setDocumentContent: (path, doc, content, mtime = 0) => set((state) => ({
        documentContents: {
          ...state.documentContents,
          [path]: createDefaultDocumentState({
            document: doc,
            content,
            mtime,
            docxBuffer: state.documentContents[path]?.docxBuffer ?? null,
            excelData: state.documentContents[path]?.excelData ?? null,
            officeBufferVersion: state.documentContents[path]?.officeBufferVersion ?? 0,
          }),
        },
      })),

      setContent: (path, content) => set((state) =>
        updateDocument(state, path, { content, isDirty: true })
      ),

      setSelection: (path, selection) => set((state) =>
        updateDocument(state, path, { selection })
      ),

      setDiffHunks: (path, hunks, originalText, originalOffset) => set((state) =>
        updateDocument(state, path, {
          diffHunks: hunks,
          diffOriginalText: originalText,
          diffOriginalOffset: originalOffset,
          isDiffMode: hunks.length > 0,
        })
      ),

      setActiveHunkIndex: (path, index) => set((state) =>
        updateDocument(state, path, { activeHunkIndex: index })
      ),

      setIsDiffMode: (path, isDiff) => set((state) =>
        updateDocument(state, path, { isDiffMode: isDiff })
      ),

      applyHunk: (path, hunkId) => set((state) => {
        const current = state.documentContents[path];
        if (!current) return state;
        const hunk = current.diffHunks.find(h => h.id === hunkId);
        if (!hunk) return state;

        // Apply the hunk to content
        const newContent = applySingleHunk(
          current.content,
          current.diffOriginalText,
          current.diffOriginalOffset,
          hunk,
        );

        // Remove the applied hunk from the list
        const newHunks = current.diffHunks.filter(h => h.id !== hunkId);
        return {
          documentContents: {
            ...state.documentContents,
            [path]: {
              ...current,
              content: newContent,
              diffHunks: newHunks,
              diffOriginalText: current.diffOriginalText,
              diffOriginalOffset: current.diffOriginalOffset,
              isDiffMode: newHunks.length > 0,
              isDirty: true,
            },
          },
        };
      }),

      rejectHunk: (path, hunkId) => set((state) => {
        const current = state.documentContents[path];
        if (!current) return state;
        const newHunks = current.diffHunks.filter(h => h.id !== hunkId);
        return {
          documentContents: {
            ...state.documentContents,
            [path]: {
              ...current,
              diffHunks: newHunks,
              isDiffMode: newHunks.length > 0,
            },
          },
        };
      }),

      applyAllHunks: (path) => set((state) => {
        const current = state.documentContents[path];
        if (!current || current.diffHunks.length === 0) return state;

        // Apply all hunks to the original text, then splice into content
        const newOriginalText = applyAllHunksToText(
          current.diffOriginalText,
          current.diffHunks,
        );
        const endOffset = current.diffOriginalOffset + current.diffOriginalText.length;
        const newContent =
          current.content.slice(0, current.diffOriginalOffset) +
          newOriginalText +
          current.content.slice(endOffset);

        return {
          documentContents: {
            ...state.documentContents,
            [path]: {
              ...current,
              content: newContent,
              diffHunks: [],
              diffOriginalText: '',
              diffOriginalOffset: 0,
              isDiffMode: false,
              isDirty: true,
            },
          },
        };
      }),

      rejectAllHunks: (path) => set((state) =>
        updateDocument(state, path, {
          diffHunks: [],
          diffOriginalText: '',
          diffOriginalOffset: 0,
          isDiffMode: false,
        })
      ),

      clearDiff: (path) => set((state) =>
        updateDocument(state, path, {
          diffHunks: [],
          diffOriginalText: '',
          diffOriginalOffset: 0,
          isDiffMode: false,
          activeHunkIndex: 0,
        })
      ),

      markSaved: (path) => set((state) =>
        updateDocument(state, path, { isDirty: false })
      ),

      updateTabDirty: (path, isDirty) => set((state) =>
        updateDocument(state, path, { isDirty })
      ),

      removeDocumentContent: (path) => set((state) => {
        const { [path]: removedDocument, ...rest } = state.documentContents;
        void removedDocument;
        return { documentContents: rest };
      }),

      setDocxBuffer: (path, buffer) => set((state) => {
        const current = state.documentContents[path];
        if (!current) {
          return {
            documentContents: {
              ...state.documentContents,
              [path]: createDefaultDocumentState({ docxBuffer: buffer }),
            },
          };
        }
        return {
          documentContents: {
            ...state.documentContents,
            [path]: { ...current, docxBuffer: buffer },
          },
        };
      }),

      setExcelData: (path, data) => set((state) => {
        const current = state.documentContents[path];
        if (!current) {
          return {
            documentContents: {
              ...state.documentContents,
              [path]: createDefaultDocumentState({ excelData: data }),
            },
          };
        }
        return {
          documentContents: {
            ...state.documentContents,
            [path]: { ...current, excelData: data },
          },
        };
      }),

      clearDocxBuffer: (path) => set((state) =>
        updateDocument(state, path, { docxBuffer: null, isDirty: false })
      ),

      clearExcelData: (path) => set((state) =>
        updateDocument(state, path, { excelData: null, isDirty: false })
      ),

      invalidateOfficeBuffer: (path) => set((state) => {
        const current = state.documentContents[path];
        if (!current) return state;
        return {
          documentContents: {
            ...state.documentContents,
            [path]: {
              ...current,
              officeBufferVersion: (current.officeBufferVersion ?? 0) + 1,
            },
          },
        };
      }),

      togglePreviewMode: (path) => set((state) => ({
        isPreviewMode: {
          ...state.isPreviewMode,
          [path]: !state.isPreviewMode[path],
        },
      })),
    }),
    {
      name: 'inkuo-editor',
      onRehydrateStorage: () => (state) => {
        if (state) {
          // Migrate old format or clear corrupted data
          const version = localStorage.getItem(EDITOR_STORAGE_VERSION_KEY);
          if (version !== String(EDITOR_STORAGE_VERSION)) {
            localStorage.setItem(EDITOR_STORAGE_VERSION_KEY, String(EDITOR_STORAGE_VERSION));
            // Clear old document contents to force reload from disk
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
