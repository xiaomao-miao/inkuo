import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { FileEntry, ActiveToolCall, KnowledgeBase, BuildProgress } from '../types';

let getExpandedDirsSnapshot: (() => Set<string>) | null = null;

function isSamePathOrDescendant(path: string, targetPath: string): boolean {
  return path === targetPath || path.startsWith(`${targetPath}/`);
}

function mergeFileEntries(existingEntries: FileEntry[], incomingEntries: FileEntry[]): FileEntry[] {
  const mergedEntries = new Map<string, FileEntry>();

  for (const entry of existingEntries) {
    mergedEntries.set(entry.path, entry);
  }

  for (const entry of incomingEntries) {
    mergedEntries.set(entry.path, entry);
  }

  return Array.from(mergedEntries.values());
}

export interface OpenTab {
  id: string;
  path: string;
  name: string;
  isDirty: boolean;
  isSettings?: boolean;
}

export type { KnowledgeBase, BuildProgress };

interface PersistedSidebarState {
  workspacePath: string | null;
  openTabs: OpenTab[];
  activeTabId: string | null;
  selectedFile: string | null;
  expandedDirs: string[];
}

function normalizeWorkspaceFilePath(path: string, workspacePath: string | null): string {
  if (!workspacePath || !path.startsWith(workspacePath)) {
    return path;
  }

  const normalized = path.slice(workspacePath.length);
  return normalized.startsWith('/') ? normalized.slice(1) : normalized;
}

function resolveWorkspaceFileEntry(path: string, files: FileEntry[], workspacePath: string | null): FileEntry | null {
  const normalizedPath = normalizeWorkspaceFilePath(path, workspacePath);

  return files.find((file) => {
    if (file.is_dir) return false;
    if (file.path === path) return true;
    if (file.path === normalizedPath) return true;
    if (workspacePath && `${workspacePath}/${file.path}` === path) return true;
    return false;
  }) ?? null;
}

function updateOpenTabDirtyState(openTabs: OpenTab[], path: string, isDirty: boolean): OpenTab[] {
  return openTabs.map((tab) =>
    tab.path === path ? { ...tab, isDirty } : tab
  );
}

export const SETTINGS_TAB_ID = '__settings__';

interface SidebarState {
  workspacePath: string | null;
  files: FileEntry[];
  expandedDirs: Set<string>;
  selectedFile: string | null;
  isLoading: boolean;
  openTabs: OpenTab[];
  activeTabId: string | null;

  knowledgeBase?: KnowledgeBase;
  buildProgress?: BuildProgress;
  knowledgeToolCall?: ActiveToolCall;

  hasRestoredFromPersist: boolean;

  setWorkspacePath: (path: string) => void;
  setFiles: (files: FileEntry[] | ((prev: FileEntry[]) => FileEntry[])) => void;
  toggleDir: (path: string) => void;
  setSelectedFile: (path: string | null) => void;
  setIsLoading: (loading: boolean) => void;
  openTab: (tab: OpenTab) => void;
  openWorkspaceFile: (path: string, options?: { name?: string }) => void;
  closeTab: (tabId: string) => void;
  setActiveTab: (tabId: string) => void;
  setOpenTabDirty: (path: string, isDirty: boolean) => void;
  addFileEntry: (entry: FileEntry) => void;
  addFileEntries: (entries: FileEntry[]) => void;
  removeFileEntry: (path: string) => void;
  removeDescendants: (parentPath: string) => void;
  isDirExpanded: (path: string) => boolean;

  setKnowledgeBase: (kb: KnowledgeBase | undefined) => void;
  setBuildProgress: (progress: BuildProgress | undefined) => void;
  setKnowledgeToolCall: (toolCall: ActiveToolCall | undefined) => void;
}

export const useSidebarStore = create<SidebarState>()(
  persist<SidebarState, [], [], PersistedSidebarState>(
    (set, get) => {
      getExpandedDirsSnapshot = () => get().expandedDirs;

      return {
        workspacePath: null,
        files: [],
        expandedDirs: new Set(),
        selectedFile: null,
        isLoading: false,
        openTabs: [],
        activeTabId: null,

        knowledgeBase: undefined,
        buildProgress: undefined,
        knowledgeToolCall: undefined,
        hasRestoredFromPersist: false,

        setWorkspacePath: (path) => set({ workspacePath: path }),
        setFiles: (files) => set((state) => ({
          files: typeof files === 'function' ? files(state.files) : files,
        })),
        toggleDir: (path) => set((state) => {
          const newExpanded = new Set(state.expandedDirs);
          if (newExpanded.has(path)) {
            newExpanded.delete(path);
          } else {
            newExpanded.add(path);
          }
          return { expandedDirs: newExpanded };
        }),
        setSelectedFile: (path) => set({ selectedFile: path }),
        setIsLoading: (loading) => set({ isLoading: loading }),
        openTab: (tab) => set((state) => {
          const existing = state.openTabs.find((t) => t.path === tab.path);
          if (existing) {
            return { activeTabId: existing.id, selectedFile: tab.path };
          }
          const newTabs = [...state.openTabs, { ...tab, isDirty: tab.isDirty ?? false }];
          const newSelectedFile = tab.isSettings ? null : tab.path;
          return {
            openTabs: newTabs,
            activeTabId: tab.id,
            selectedFile: newSelectedFile,
          };
        }),
        openWorkspaceFile: (path, options) => set((state) => {
          const resolvedEntry = resolveWorkspaceFileEntry(path, state.files, state.workspacePath);
          const resolvedPath = resolvedEntry?.path ?? path;
          const existing = state.openTabs.find((t) => t.path === resolvedPath);

          if (existing) {
            return { activeTabId: existing.id, selectedFile: resolvedPath };
          }

          const tabName = options?.name ?? resolvedEntry?.name ?? resolvedPath.split('/').pop() ?? '未命名文档';
          const newTab: OpenTab = {
            id: resolvedPath,
            path: resolvedPath,
            name: tabName,
            isDirty: false,
          };

          return {
            openTabs: [...state.openTabs, newTab],
            activeTabId: newTab.id,
            selectedFile: resolvedPath,
          };
        }),
        closeTab: (tabId) => set((state) => {
          const newTabs = state.openTabs.filter((t) => t.id !== tabId);
          let newActiveId = state.activeTabId;
          if (state.activeTabId === tabId) {
            const closedIndex = state.openTabs.findIndex((t) => t.id === tabId);
            newActiveId = newTabs.length > 0
              ? newTabs[Math.min(closedIndex, newTabs.length - 1)].id
              : null;
          }

          return {
            openTabs: newTabs,
            activeTabId: newActiveId,
            selectedFile: newActiveId ? (newTabs.find((t) => t.id === newActiveId)?.path || null) : null,
          };
        }),
        setActiveTab: (tabId) => set((state) => {
          const tab = state.openTabs.find((t) => t.id === tabId);
          const newSelectedFile = tab?.isSettings ? null : (tab?.path || state.selectedFile);
          return {
            activeTabId: tabId,
            selectedFile: newSelectedFile,
          };
        }),
        setOpenTabDirty: (path, isDirty) => set((state) => ({
          openTabs: updateOpenTabDirtyState(state.openTabs, path, isDirty),
        })),
        addFileEntry: (entry) => set((state) => ({
          files: mergeFileEntries(state.files, [entry]),
        })),
        addFileEntries: (entries) => set((state) => ({
          files: mergeFileEntries(state.files, entries),
        })),
        removeFileEntry: (path) => set((state) => ({
          files: state.files.filter((f) => !isSamePathOrDescendant(f.path, path)),
        })),
        removeDescendants: (parentPath) => set((state) => ({
          files: state.files.filter((f) => !f.path.startsWith(parentPath + '/')),
        })),
        isDirExpanded: (path) => getExpandedDirsSnapshot?.().has(path) ?? false,

        setKnowledgeBase: (kb: KnowledgeBase | undefined) => set({ knowledgeBase: kb }),
        setBuildProgress: (progress: BuildProgress | undefined) => set({ buildProgress: progress }),
        setKnowledgeToolCall: (toolCall: ActiveToolCall | undefined) => set({ knowledgeToolCall: toolCall }),
      };
    },
    {
      name: 'inkuo-sidebar',
      partialize: (state): PersistedSidebarState => ({
        workspacePath: state.workspacePath,
        openTabs: state.openTabs,
        activeTabId: state.activeTabId,
        selectedFile: state.selectedFile,
        expandedDirs: Array.from(state.expandedDirs),
      }),
      merge: (persisted, current): SidebarState => {
        const persistedState = persisted as PersistedSidebarState | undefined;
        return {
          ...current,
          ...persistedState,
          expandedDirs: new Set(persistedState?.expandedDirs ?? []),
          hasRestoredFromPersist: true,
        };
      },
    },
  ),
);
