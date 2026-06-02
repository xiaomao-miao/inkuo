import { create } from 'zustand';
import { persist } from 'zustand/middleware';

interface DocumentState {
  document: Document | null;
  content: string;
  isDirty: boolean;
  selection: { from: number; to: number } | null;
  diffHunks: DiffHunk[];
  activeHunkIndex: number;
  isDiffMode: boolean;
  docxBuffer: number[] | null;
  excelData: string[][] | null;
  officeBufferVersion: number;
}

export interface DiffHunk {
  id: string;
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  changes: DiffChange[];
}

export interface DiffChange {
  tag: 'delete' | 'insert' | 'equal';
  old_line: number | null;
  new_line: number | null;
  content: string;
}

interface EditorState {
  documentContents: Record<string, DocumentState>;

  setDocumentContent: (path: string, doc: Document, content: string) => void;
  setContent: (path: string, content: string) => void;
  setSelection: (path: string, selection: { from: number; to: number } | null) => void;
  setDiffHunks: (path: string, hunks: any[]) => void;
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
}

export const useEditorStore = create<EditorState>()(
  persist(
    (set) => ({
      documentContents: {},

      setDocumentContent: (path, doc, content) => set((state) => ({
        documentContents: {
          ...state.documentContents,
          [path]: {
            document: doc,
            content: content,
            isDirty: false,
            selection: null,
            diffHunks: [] as any[],
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
              diffHunks: [] as any[],
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
              diffHunks: [] as any[],
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
              diffHunks: [] as any[],
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
    }),
    {
      name: 'inkuo-editor',
      partialize: (state) => ({
        documentContents: Object.fromEntries(
          Object.entries(state.documentContents).map(([path, doc]) => [
            path,
            {
              document: doc.document,
              content: doc.content,
              isDirty: doc.isDirty,
              selection: doc.selection,
              diffHunks: doc.diffHunks,
              activeHunkIndex: doc.activeHunkIndex,
              isDiffMode: doc.isDiffMode,
              docxBuffer: doc.docxBuffer,
              excelData: doc.excelData,
              officeBufferVersion: doc.officeBufferVersion ?? 0,
            }
          ])
        ),
      }),
    }
  )
);
