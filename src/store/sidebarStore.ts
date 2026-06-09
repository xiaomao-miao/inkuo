import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { FileEntry, ActiveToolCall, KnowledgeBase, BuildProgress } from '../types';

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
  /** Cached directory contents: parentPath -> children entries */
  directoryCache: Record<string, FileEntry[]>;
}

function normalizeWorkspaceFilePath(path: string, workspacePath: string | null): string {
  if (!workspacePath || !path.startsWith(workspacePath)) {
    return path;
  }
  const normalized = path.slice(workspacePath.length);
  return normalized.startsWith('/') ? normalized.slice(1) : normalized;
}

function resolveWorkspaceFileEntry(
  path: string,
  directoryCache: Map<string, FileEntry[]>,
  workspacePath: string | null,
): FileEntry | null {
  const normalizedPath = normalizeWorkspaceFilePath(path, workspacePath);

  for (const children of directoryCache.values()) {
    const found = children.find((file) => {
      if (file.is_dir) return false;
      if (file.path === path) return true;
      if (file.path === normalizedPath) return true;
      if (workspacePath && `${workspacePath}/${file.path}` === path) return true;
      return false;
    });
    if (found) return found;
  }
  return null;
}

function updateOpenTabDirtyState(openTabs: OpenTab[], path: string, isDirty: boolean): OpenTab[] {
  return openTabs.map((tab) => (tab.path === path ? { ...tab, isDirty } : tab));
}

export const SETTINGS_TAB_ID = '__settings__';

interface SidebarState {
  workspacePath: string | null;
  /** Cached directory contents: parentPath -> children entries */
  directoryCache: Map<string, FileEntry[]>;
  /** Currently expanded directory paths */
  expandedDirs: Set<string>;
  /** Directories currently being loaded */
  loadingDirs: Set<string>;
  selectedFile: string | null;
  isLoading: boolean;
  openTabs: OpenTab[];
  activeTabId: string | null;

  knowledgeBase?: KnowledgeBase;
  buildProgress?: BuildProgress;
  knowledgeToolCall?: ActiveToolCall;

  hasRestoredFromPersist: boolean;

  setWorkspacePath: (path: string) => void;
  /** Get entries cached for a specific directory */
  getCachedChildren: (dirPath: string) => FileEntry[];
  /** Check if a directory's children are cached */
  hasCachedChildren: (dirPath: string) => boolean;
  /** Cache children for a directory */
  setCachedChildren: (dirPath: string, children: FileEntry[]) => void;
  /** Remove cached children for a directory (and all its descendants) */
  invalidateCache: (dirPath: string) => void;
  /** Clear all cache */
  clearCache: () => void;

  toggleDir: (path: string) => void;
  setSelectedFile: (path: string | null) => void;
  setIsLoading: (loading: boolean) => void;
  setDirLoading: (path: string, loading: boolean) => void;
  isDirExpanded: (path: string) => boolean;
  isDirLoading: (path: string) => boolean;

  openTab: (tab: OpenTab) => void;
  openWorkspaceFile: (path: string, options?: { name?: string }) => void;
  closeTab: (tabId: string) => void;
  setActiveTab: (tabId: string) => void;
  setOpenTabDirty: (path: string, isDirty: boolean) => void;

  setKnowledgeBase: (kb: KnowledgeBase | undefined) => void;
  setBuildProgress: (progress: BuildProgress | undefined) => void;
  setKnowledgeToolCall: (toolCall: ActiveToolCall | undefined) => void;
}

export const useSidebarStore = create<SidebarState>()(
  persist<SidebarState, [], [], PersistedSidebarState>(
    (set, get) => ({
      workspacePath: null,
      directoryCache: new Map(),
      expandedDirs: new Set(),
      loadingDirs: new Set(),
      selectedFile: null,
      isLoading: false,
      openTabs: [],
      activeTabId: null,

      knowledgeBase: undefined,
      buildProgress: undefined,
      knowledgeToolCall: undefined,
      hasRestoredFromPersist: false,

      setWorkspacePath: (path) => set({ workspacePath: path }),

      getCachedChildren: (dirPath) => get().directoryCache.get(dirPath) ?? [],
      hasCachedChildren: (dirPath) => get().directoryCache.has(dirPath),
      setCachedChildren: (dirPath, children) =>
        set((state) => {
          const newCache = new Map(state.directoryCache);
          newCache.set(dirPath, children);
          return { directoryCache: newCache };
        }),

      invalidateCache: (dirPath) =>
        set((state) => {
          const newCache = new Map(state.directoryCache);
          const prefix = `${dirPath}/`;
          for (const key of newCache.keys()) {
            if (key === dirPath || key.startsWith(prefix)) {
              newCache.delete(key);
            }
          }
          return { directoryCache: newCache };
        }),

      clearCache: () => set({ directoryCache: new Map() }),

      toggleDir: (path) =>
        set((state) => {
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
      setDirLoading: (path, loading) =>
        set((state) => {
          const newLoading = new Set(state.loadingDirs);
          if (loading) {
            newLoading.add(path);
          } else {
            newLoading.delete(path);
          }
          return { loadingDirs: newLoading };
        }),

      isDirExpanded: (path) => get().expandedDirs.has(path),
      isDirLoading: (path) => get().loadingDirs.has(path),

      openTab: (tab) =>
        set((state) => {
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

      openWorkspaceFile: (path, options) =>
        set((state) => {
          const resolvedEntry = resolveWorkspaceFileEntry(
            path,
            state.directoryCache,
            state.workspacePath,
          );
          const resolvedPath = resolvedEntry?.path ?? path;
          const existing = state.openTabs.find((t) => t.path === resolvedPath);

          if (existing) {
            return { activeTabId: existing.id, selectedFile: resolvedPath };
          }

          const tabName =
            options?.name ??
            resolvedEntry?.name ??
            resolvedPath.split('/').pop() ??
            '未命名文档';
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

      closeTab: (tabId) =>
        set((state) => {
          const newTabs = state.openTabs.filter((t) => t.id !== tabId);
          let newActiveId = state.activeTabId;
          if (state.activeTabId === tabId) {
            const closedIndex = state.openTabs.findIndex((t) => t.id === tabId);
            newActiveId =
              newTabs.length > 0
                ? newTabs[Math.min(closedIndex, newTabs.length - 1)].id
                : null;
          }

          return {
            openTabs: newTabs,
            activeTabId: newActiveId,
            selectedFile: newActiveId
              ? newTabs.find((t) => t.id === newActiveId)?.path || null
              : null,
          };
        }),

      setActiveTab: (tabId) =>
        set((state) => {
          const tab = state.openTabs.find((t) => t.id === tabId);
          const newSelectedFile = tab?.isSettings
            ? null
            : tab?.path ?? state.selectedFile;
          return {
            activeTabId: tabId,
            selectedFile: newSelectedFile,
          };
        }),

      setOpenTabDirty: (path, isDirty) =>
        set((state) => {
          const currentTab = state.openTabs.find((tab) => tab.path === path);
          if (!currentTab || currentTab.isDirty === isDirty) {
            return {};
          }

          return {
            openTabs: updateOpenTabDirtyState(state.openTabs, path, isDirty),
          };
        }),

      setKnowledgeBase: (kb: KnowledgeBase | undefined) => set({ knowledgeBase: kb }),
      setBuildProgress: (progress: BuildProgress | undefined) => set({ buildProgress: progress }),
      setKnowledgeToolCall: (toolCall: ActiveToolCall | undefined) =>
        set({ knowledgeToolCall: toolCall }),
    }),
    {
      name: 'inkuo-sidebar',
      partialize: (state): PersistedSidebarState => ({
        workspacePath: state.workspacePath,
        openTabs: state.openTabs,
        activeTabId: state.activeTabId,
        selectedFile: state.selectedFile,
        expandedDirs: Array.from(state.expandedDirs),
        directoryCache: Object.fromEntries(state.directoryCache),
      }),
      merge: (persisted, current): SidebarState => {
        const persistedState = persisted as PersistedSidebarState | undefined;
        const cacheEntries = persistedState?.directoryCache
          ? Object.entries(persistedState.directoryCache)
          : [];
        return {
          ...current,
          ...persistedState,
          expandedDirs: new Set(persistedState?.expandedDirs ?? []),
          directoryCache: new Map(cacheEntries),
          hasRestoredFromPersist: true,
        };
      },
    },
  ),
);
