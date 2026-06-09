import type { StateCreator } from 'zustand';
import type { DiffHunk, Document } from '../types';
import {
  applyRemainingHunks,
  applySelectedHunk,
  rejectSelectedHunk,
} from './editorDiffState';

export interface DocumentMetadata {
  document: Document | null;
  content: string;
  mtime: number;
  isDirty: boolean;
  selection: { from: number; to: number } | null;
}

export interface DocumentDiffState {
  hunks: DiffHunk[];
  originalText: string;
  originalOffset: number;
  activeHunkIndex: number;
  isActive: boolean;
}

export interface DocumentOfficeState {
  docxBuffer: number[] | null;
  excelData: ExcelWorkbook | null;
  bufferVersion: number;
}

export interface ExcelWorkbook {
  sheets: Sheet[];
}

export interface Sheet {
  name: string;
  data: string[][];
}

export interface DocumentState {
  metadata: DocumentMetadata;
  diff: DocumentDiffState;
  office: DocumentOfficeState;
}

export interface EditorState {
  documentContents: Record<string, DocumentState>;
  isPreviewMode: Record<string, boolean>;

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
  setExcelData: (path: string, workbook: ExcelWorkbook) => void;
  clearDocxBuffer: (path: string) => void;
  clearExcelData: (path: string) => void;
  invalidateOfficeBuffer: (path: string) => void;
  togglePreviewMode: (path: string) => void;
}

type DocumentSlice = Pick<
  EditorState,
  'documentContents' | 'setDocumentContent' | 'setContent' | 'setSelection' | 'markSaved' | 'updateTabDirty' | 'removeDocumentContent'
>;

type DiffSlice = Pick<
  EditorState,
  'setDiffHunks' | 'setActiveHunkIndex' | 'setIsDiffMode' | 'applyHunk' | 'rejectHunk' | 'applyAllHunks' | 'rejectAllHunks' | 'clearDiff'
>;

type OfficeSlice = Pick<
  EditorState,
  'setDocxBuffer' | 'setExcelData' | 'clearDocxBuffer' | 'clearExcelData' | 'invalidateOfficeBuffer'
>;

type PreviewSlice = Pick<EditorState, 'isPreviewMode' | 'togglePreviewMode'>;

type EditorStoreCreator<TSlice> = StateCreator<EditorState, [], [], TSlice>;

function createDefaultDocumentMetadata(overrides?: Partial<DocumentMetadata>): DocumentMetadata {
  return {
    document: null,
    content: '',
    mtime: 0,
    isDirty: false,
    selection: null,
    ...overrides,
  };
}

function normalizeDocumentMetadata(metadata?: Partial<DocumentMetadata>): DocumentMetadata {
  return createDefaultDocumentMetadata(metadata);
}

function createDefaultDocumentDiffState(overrides?: Partial<DocumentDiffState>): DocumentDiffState {
  return {
    hunks: [],
    originalText: '',
    originalOffset: 0,
    activeHunkIndex: 0,
    isActive: false,
    ...overrides,
  };
}

function normalizeDocumentDiffState(diff?: Partial<DocumentDiffState>): DocumentDiffState {
  return createDefaultDocumentDiffState(diff);
}

function createDefaultDocumentOfficeState(overrides?: Partial<DocumentOfficeState>): DocumentOfficeState {
  return {
    docxBuffer: null,
    excelData: null,
    bufferVersion: 0,
    ...overrides,
  };
}

function normalizeDocumentOfficeState(office?: Partial<DocumentOfficeState>): DocumentOfficeState {
  return createDefaultDocumentOfficeState(office);
}

export function createDefaultDocumentState(overrides?: {
  metadata?: Partial<DocumentMetadata>;
  diff?: Partial<DocumentDiffState>;
  office?: Partial<DocumentOfficeState>;
}): DocumentState {
  return {
    metadata: createDefaultDocumentMetadata(overrides?.metadata),
    diff: createDefaultDocumentDiffState(overrides?.diff),
    office: createDefaultDocumentOfficeState(overrides?.office),
  };
}

export function normalizeDocumentState(documentState?: Partial<DocumentState> | null): DocumentState {
  return {
    metadata: normalizeDocumentMetadata(documentState?.metadata),
    diff: normalizeDocumentDiffState(documentState?.diff),
    office: normalizeDocumentOfficeState(documentState?.office),
  };
}

function updateDocumentSection<TKey extends keyof DocumentState>(
  state: Pick<EditorState, 'documentContents'>,
  path: string,
  section: TKey,
  update: Partial<DocumentState[TKey]>,
): Pick<EditorState, 'documentContents'> | Pick<EditorState, never> {
  const current = state.documentContents[path];
  if (!current) return {};

  return {
    documentContents: {
      ...state.documentContents,
      [path]: {
        ...current,
        [section]: {
          ...current[section],
          ...update,
        },
      },
    },
  };
}

export function setOrCreateDocumentContent(
  state: Pick<EditorState, 'documentContents'>,
  path: string,
  updater: (current: DocumentState | undefined) => DocumentState,
): Pick<EditorState, 'documentContents'> {
  return {
    documentContents: {
      ...state.documentContents,
      [path]: updater(state.documentContents[path]),
    },
  };
}

function toLegacyDiffContext(documentState: DocumentState) {
  return {
    content: documentState.metadata.content,
    diffHunks: documentState.diff.hunks,
    diffOriginalText: documentState.diff.originalText,
    diffOriginalOffset: documentState.diff.originalOffset,
  };
}

export const createDocumentSlice: EditorStoreCreator<DocumentSlice> = (set) => ({
  documentContents: {},
  setDocumentContent: (path, doc, content, mtime = 0) =>
    set((state) => ({
      documentContents: {
        ...state.documentContents,
        [path]: createDefaultDocumentState({
          metadata: { document: doc, content, mtime },
          office: {
            docxBuffer: state.documentContents[path]?.office.docxBuffer ?? null,
            excelData: state.documentContents[path]?.office.excelData ?? null,
            bufferVersion: state.documentContents[path]?.office.bufferVersion ?? 0,
          },
        }),
      },
    })),
  setContent: (path, content) =>
    set((state) => updateDocumentSection(state, path, 'metadata', { content, isDirty: true })),
  setSelection: (path, selection) =>
    set((state) => updateDocumentSection(state, path, 'metadata', { selection })),
  markSaved: (path) =>
    set((state) => updateDocumentSection(state, path, 'metadata', { isDirty: false })),
  updateTabDirty: (path, isDirty) =>
    set((state) => updateDocumentSection(state, path, 'metadata', { isDirty })),
  removeDocumentContent: (path) =>
    set((state) => {
      const { [path]: removedDocument, ...rest } = state.documentContents;
      void removedDocument;
      return { documentContents: rest };
    }),
});

export const createDiffSlice: EditorStoreCreator<DiffSlice> = (set) => ({
  setDiffHunks: (path, hunks, originalText, originalOffset) =>
    set((state) =>
      updateDocumentSection(state, path, 'diff', {
        hunks,
        originalText,
        originalOffset,
        isActive: hunks.length > 0,
      })
    ),
  setActiveHunkIndex: (path, index) =>
    set((state) => updateDocumentSection(state, path, 'diff', { activeHunkIndex: index })),
  setIsDiffMode: (path, isDiff) =>
    set((state) => updateDocumentSection(state, path, 'diff', { isActive: isDiff })),
  applyHunk: (path, hunkId) =>
    set((state) => {
      const current = state.documentContents[path];
      if (!current) return {};

      const resolvedDiff = applySelectedHunk(toLegacyDiffContext(current), hunkId);
      if (!resolvedDiff) return {};

      return {
        documentContents: {
          ...state.documentContents,
          [path]: {
            ...current,
            metadata: {
              ...current.metadata,
              content: resolvedDiff.content,
              isDirty: resolvedDiff.isDirty ?? current.metadata.isDirty,
            },
            diff: {
              ...current.diff,
              hunks: resolvedDiff.diffHunks,
              originalText: resolvedDiff.diffOriginalText,
              originalOffset: resolvedDiff.diffOriginalOffset,
              isActive: resolvedDiff.isDiffMode,
            },
          },
        },
      };
    }),
  rejectHunk: (path, hunkId) =>
    set((state) => {
      const current = state.documentContents[path];
      if (!current) return {};

      const resolvedDiff = rejectSelectedHunk(toLegacyDiffContext(current), hunkId);
      return {
        documentContents: {
          ...state.documentContents,
          [path]: {
            ...current,
            diff: {
              ...current.diff,
              hunks: resolvedDiff.diffHunks,
              originalText: resolvedDiff.diffOriginalText,
              originalOffset: resolvedDiff.diffOriginalOffset,
              isActive: resolvedDiff.isDiffMode,
            },
          },
        },
      };
    }),
  applyAllHunks: (path) =>
    set((state) => {
      const current = state.documentContents[path];
      if (!current || current.diff.hunks.length === 0) return {};

      const resolvedDiff = applyRemainingHunks(toLegacyDiffContext(current));
      return {
        documentContents: {
          ...state.documentContents,
          [path]: {
            ...current,
            metadata: {
              ...current.metadata,
              content: resolvedDiff.content,
              isDirty: resolvedDiff.isDirty ?? current.metadata.isDirty,
            },
            diff: {
              ...current.diff,
              hunks: resolvedDiff.diffHunks,
              originalText: resolvedDiff.diffOriginalText,
              originalOffset: resolvedDiff.diffOriginalOffset,
              isActive: resolvedDiff.isDiffMode,
            },
          },
        },
      };
    }),
  rejectAllHunks: (path) =>
    set((state) =>
      updateDocumentSection(state, path, 'diff', {
        hunks: [],
        originalText: '',
        originalOffset: 0,
        isActive: false,
      })
    ),
  clearDiff: (path) =>
    set((state) =>
      updateDocumentSection(state, path, 'diff', {
        hunks: [],
        originalText: '',
        originalOffset: 0,
        isActive: false,
        activeHunkIndex: 0,
      })
    ),
});

export const createOfficeSlice: EditorStoreCreator<OfficeSlice> = (set) => ({
  setDocxBuffer: (path, buffer) =>
    set((state) =>
      setOrCreateDocumentContent(state, path, (current) =>
        current
          ? {
            ...current,
            office: { ...current.office, docxBuffer: buffer },
          }
          : createDefaultDocumentState({ office: { docxBuffer: buffer } })
      )
    ),
  setExcelData: (path, workbook) =>
    set((state) =>
      setOrCreateDocumentContent(state, path, (current) =>
        current
          ? {
            ...current,
            office: { ...current.office, excelData: workbook },
          }
          : createDefaultDocumentState({ office: { excelData: workbook } })
      )
    ),
  clearDocxBuffer: (path) =>
    set((state) => updateDocumentSection(state, path, 'office', { docxBuffer: null })),
  clearExcelData: (path) =>
    set((state) => updateDocumentSection(state, path, 'office', { excelData: null })),
  invalidateOfficeBuffer: (path) =>
    set((state) => {
      const current = state.documentContents[path];
      if (!current) return {};

      return updateDocumentSection(state, path, 'office', {
        bufferVersion: (current.office.bufferVersion ?? 0) + 1,
      });
    }),
});

export const createPreviewSlice: EditorStoreCreator<PreviewSlice> = (set) => ({
  isPreviewMode: {},
  togglePreviewMode: (path) =>
    set((state) => ({
      isPreviewMode: {
        ...state.isPreviewMode,
        [path]: !state.isPreviewMode[path],
      },
    })),
});
