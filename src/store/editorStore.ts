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
  setDiffHunks: (path: string, hunks: DiffHunk[]) => void;
  setActiveHunkIndex: (path: string, index: number) => void;
  setIsDiffMode: (path: string, isDiff: boolean) => void;
  applyHunk: (path: string, hunkId: string) => void;
  rejectHunk: (path: string, hunkId: string) => void;
  applyAllHunks: (path: string) => void;
  rejectAllHunks: (path: string) => void;
  clearDiff: (path: string) => void;
  markSaved: (path: string) => void;
  updateTabDirty: (path: string, isDirty: boolean) => void;
  getSelection: () => string | null;
  applyDiff: (diff: { originalText: string; newText: string }) => void;
  removeDocumentContent: (path: string) => void;
  setDocxBuffer: (path: string, buffer: number[]) => void;
  setExcelData: (path: string, data: string[][]) => void;
  clearDocxBuffer: (path: string) => void;
  clearExcelData: (path: string) => void;
  invalidateOfficeBuffer: (path: string) => void;
  isPreviewMode: Record<string, boolean>;
  togglePreviewMode: (path: string) => void;
}

export const useEditorStore = create<EditorState>()(
  persist(
    (set) => ({
      documentContents: {},
      isPreviewMode: {},

      setDocumentContent: (path, doc, content, mtime = 0) => set((state) => ({
        documentContents: {
          ...state.documentContents,
          [path]: {
            document: doc,
            content: content,
            mtime: mtime,
            isDirty: false,
            selection: null,
            diffHunks: [] as DiffHunk[],
            activeHunkIndex: 0,
            isDiffMode: false,
            docxBuffer: state.documentContents[path]?.docxBuffer ?? null,
            excelData: state.documentContents[path]?.excelData ?? null,
            officeBufferVersion: state.documentContents[path]?.officeBufferVersion ?? 0,
          }
        }
      })),

      setContent: (path, content) => set((state) => {
        const current = state.documentContents[path];
        if (!current) return state;
        return {
          documentContents: {
            ...state.documentContents,
            [path]: {
              ...current,
              content: content,
              isDirty: true,
            }
          }
        };
      }),

      setSelection: (path, selection) => set((state) => {
        const current = state.documentContents[path];
        if (!current) return state;
        return {
          documentContents: {
            ...state.documentContents,
            [path]: {
              ...current,
              selection,
            }
          }
        };
      }),

      setDiffHunks: (path, hunks) => set((state) => {
        const current = state.documentContents[path];
        if (!current) return state;
        return {
          documentContents: {
            ...state.documentContents,
            [path]: {
              ...current,
              diffHunks: hunks,
              isDiffMode: hunks.length > 0,
            }
          }
        };
      }),

      setActiveHunkIndex: (path, index) => set((state) => {
        const current = state.documentContents[path];
        if (!current) return state;
        return {
          documentContents: {
            ...state.documentContents,
            [path]: {
              ...current,
              activeHunkIndex: index,
            }
          }
        };
      }),

      setIsDiffMode: (path, isDiff) => set((state) => {
        const current = state.documentContents[path];
        if (!current) return state;
        return {
          documentContents: {
            ...state.documentContents,
            [path]: {
              ...current,
              isDiffMode: isDiff,
            }
          }
        };
      }),

      applyHunk: (path, hunkId) => set((state) => {
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
              isDirty: true,
            }
          }
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
            }
          }
        };
      }),

      applyAllHunks: (path) => set((state) => {
        const current = state.documentContents[path];
        if (!current) return state;
        return {
          documentContents: {
            ...state.documentContents,
            [path]: {
              ...current,
              diffHunks: [] as DiffHunk[],
              isDiffMode: false,
              isDirty: true,
            }
          }
        };
      }),

      rejectAllHunks: (path) => set((state) => {
        const current = state.documentContents[path];
        if (!current) return state;
        return {
          documentContents: {
            ...state.documentContents,
            [path]: {
              ...current,
              diffHunks: [] as DiffHunk[],
              isDiffMode: false,
            }
          }
        };
      }),

      clearDiff: (path) => set((state) => {
        const current = state.documentContents[path];
        if (!current) return state;
        return {
          documentContents: {
            ...state.documentContents,
            [path]: {
              ...current,
              diffHunks: [] as DiffHunk[],
              isDiffMode: false,
              activeHunkIndex: 0,
            }
          }
        };
      }),

      markSaved: (path) => set((state) => {
        const current = state.documentContents[path];
        if (!current) return state;
        return {
          documentContents: {
            ...state.documentContents,
            [path]: {
              ...current,
              isDirty: false,
            }
          }
        };
      }),

      updateTabDirty: (path, isDirty) => set((state) => {
        const current = state.documentContents[path];
        if (!current) return state;
        return {
          documentContents: {
            ...state.documentContents,
            [path]: {
              ...current,
              isDirty,
            }
          }
        };
      }),

      getSelection: () => null,

      applyDiff: (diff) => {
        console.log('Applying diff:', diff);
      },

      removeDocumentContent: (path) => set((state) => {
        const { [path]: _, ...rest } = state.documentContents;
        return { documentContents: rest };
      }),

      setDocxBuffer: (path, buffer) => set((state) => {
        const current = state.documentContents[path];
        if (!current) {
          return {
            documentContents: {
              ...state.documentContents,
              [path]: {
                document: null,
                content: '',
                mtime: 0,
                isDirty: false,
                selection: null,
                diffHunks: [],
                activeHunkIndex: 0,
                isDiffMode: false,
                docxBuffer: buffer,
                excelData: null,
                officeBufferVersion: 0,
              },
            }
          };
        }
        return {
          documentContents: {
            ...state.documentContents,
            [path]: { ...current, docxBuffer: buffer, isDirty: true },
          }
        };
      }),

      setExcelData: (path, data) => set((state) => {
        const current = state.documentContents[path];
        if (!current) {
          return {
            documentContents: {
              ...state.documentContents,
              [path]: {
                document: null,
                content: '',
                mtime: 0,
                isDirty: false,
                selection: null,
                diffHunks: [],
                activeHunkIndex: 0,
                isDiffMode: false,
                docxBuffer: null,
                excelData: data,
                officeBufferVersion: 0,
              },
            }
          };
        }
        return {
          documentContents: {
            ...state.documentContents,
            [path]: { ...current, excelData: data, isDirty: true },
          }
        };
      }),

      clearDocxBuffer: (path) => set((state) => {
        const current = state.documentContents[path];
        if (!current) return state;
        return {
          documentContents: {
            ...state.documentContents,
            [path]: { ...current, docxBuffer: null, isDirty: false },
          }
        };
      }),

      clearExcelData: (path) => set((state) => {
        const current = state.documentContents[path];
        if (!current) return state;
        return {
          documentContents: {
            ...state.documentContents,
            [path]: { ...current, excelData: null, isDirty: false },
          }
        };
      }),

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
          }
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
