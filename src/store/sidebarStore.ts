import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type {
  FileEntry,
  ActiveToolCall,
  ChatSession,
  KnowledgeBase,
  BuildProgress,
  NewEntryPayload,
} from '../types';
import { getBaseName, getRelativePath, joinPath, normalizeDirPath } from '../utils/path';

/**
 * Extract parent directory path from a file path.
 *
 * Both inputs are normalized to `/` separators first so the math works
 * on Windows where file paths are stored with `\`. The returned parent is
 * also normalized, ready to be used as a cache key or sent back to the
 * backend.
 */
function getParentPath(filePath: string, workspacePath: string | null): string | null {
  const normalizedRoot = workspacePath ? normalizeDirPath(workspacePath) : null;
  if (!normalizedRoot) return null;

  const relativePath = getRelativePath(normalizedRoot, filePath);
  if (!relativePath) return normalizedRoot;

  const segments = relativePath.split('/').filter(Boolean);
  if (segments.length <= 1) {
    return normalizedRoot;
  }

  return joinPath(normalizedRoot, ...segments.slice(0, -1));
}

export interface OpenTab {
  id: string;
  path: string;
  name: string;
  isDirty: boolean;
  isSettings?: boolean;
  /** Marks this tab as the cloud account / workspace page. Editor.tsx
   * routes it to <CloudPage /> instead of a file editor. */
  isCloud?: boolean;
}

export type { KnowledgeBase, BuildProgress, ChatSession };

/**
 * Per-workspace snapshot of state that the user expects to "come back" when
 * reopening the same workspace from a new window. Open tabs and AI chat
 * sessions are the headline items; everything else (document content cache,
 * directory cache, file selection, expanded dirs) is transient and rebuilt on
 * demand from disk.
 *
 * Stored in a Rust-side JSON file (`workspace_snapshots.json`) that is shared
 * across all webview windows. The sidebarStore keeps a memory cache (`snapshots`)
 * and synchronizes it with the backend via `syncSnapshotFromBackend` /
 * `syncSnapshotToBackend`.
 */
export interface WorkspaceSnapshot {
  openTabs: OpenTab[];
  activeTabId: string | null;
  aiSessions: ChatSession[];
  activeSessionId: string | null;
  /**
   * Latest published `update_todo` snapshot per session. Survives
   * restarts so the chip above the chat input still shows the in-flight
   * task list when the user reopens the workspace. Keyed by
   * `ChatSession.id`. Sessions whose id is no longer present in
   * `aiSessions` are pruned at load time.
   */
  todoSnapshotBySession?: Record<string, import('../types').TodoSnapshot>;
}

interface PersistedSidebarState {
  workspacePath: string | null;
  openTabs: OpenTab[];
  activeTabId: string | null;
  selectedFile: string | null;
  expandedDirs: string[];
  directoryCache: Record<string, FileEntry[]>;
}

/**
 * Look up a `FileEntry` in the cached directory tree by absolute path.
 *
 * The backend returns `FileEntry.path` as an absolute, platform-native
 * path (`E:\文档\file.md` on Windows). The cache is keyed by the
 * normalized form (`E:/文档/...`), so we compare against normalized values
 * on both sides — no more false negatives when the workspace was opened
 * with a different separator style than the cache key.
 */
function resolveWorkspaceFileEntry(
  path: string,
  directoryCache: Map<string, FileEntry[]>,
  workspacePath: string | null,
): FileEntry | null {
  const normalizedRoot = workspacePath ? normalizeDirPath(workspacePath) : null;
  const normalizedTarget = normalizeDirPath(path);
  const relativeTarget = normalizedRoot
    ? getRelativePath(normalizedRoot, normalizedTarget)
    : normalizedTarget;

  for (const children of directoryCache.values()) {
    const found = children.find((file) => {
      if (file.is_dir) return false;
      const filePath = normalizeDirPath(file.path);
      if (filePath === normalizedTarget) return true;
      if (filePath === relativeTarget) return true;
      if (normalizedRoot && joinPath(normalizedRoot, filePath) === normalizedTarget) return true;
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
export const CLOUD_TAB_ID = '__cloud__';

/**
 * Transient UI state for the context menu's "New file / New folder / Rename"
 * flow. The FileTree reads this to swap the affected row for an
 * `<InlineRenameInput />`; `commit` and `cancel` clear it.
 *
 * Not persisted (cleared on reload by design — half-typed names from a
 * previous session would be surprising).
 */
export interface InlineEditState {
  /** Absolute path the edited row will live at once committed. */
  parentPath: string;
  /** Original full path (for `rename`); empty when creating a new entry. */
  originalPath: string | null;
  /** Pre-filled value (existing name for rename; empty for create). */
  initialValue: string;
  /** Whether to apply the `.md` / `.docx` extension automatically. */
  extension?: string;
  /** What we're creating; undefined means rename. */
  createPayload?: NewEntryPayload;
  /** Discriminates `create` vs `rename` so the input can branch its UX. */
  mode: 'create' | 'rename';
}

interface SidebarState {
  workspacePath: string | null;
  /**
   * Lazy directory cache, keyed by normalized absolute path (`/` separator,
   * no trailing slash). The tree reads this synchronously; only the cache
   * writer (the `useWorkspaceTree` hook, the watcher, and the file-mutation
   * call sites) ever writes to it.
   *
   * Invariants:
   *   - A key exists in this map IFF we have read that directory from the
   *     backend at least once. The value is the exact list we got back.
   *   - The map only ever grows by `setCachedChildren(path, list)` and shrinks
   *     by `evictCachedChildren(path)` or `clearCache()`. There is no
   *     prefix-delete or "invalidate" — subdirectory entries are never
   *     affected by parent-directory operations.
   *   - We do NOT cache `loading` state here; the `loadingDirs` Set below
   *     tracks in-flight fetches independently.
   */
  directoryCache: Map<string, FileEntry[]>;
  expandedDirs: Set<string>;
  /** Directories currently being fetched from the backend. */
  loadingDirs: Set<string>;
  selectedFile: string | null;
  /** True only during a full `reloadCurrentWorkspace` (manual refresh). */
  isLoading: boolean;
  openTabs: OpenTab[];
  activeTabId: string | null;

  knowledgeBase?: KnowledgeBase;
  buildProgress?: BuildProgress;
  knowledgeToolCall?: ActiveToolCall;

  knowledgeSelectMode: boolean;
  knowledgeCheckedPaths: Set<string>;

  /** Inline-rename / new-entry state; null when no row is being edited. */
  inlineEdit: InlineEditState | null;

  hasRestoredFromPersist: boolean;

  setWorkspacePath: (path: string) => void;
  getCachedChildren: (dirPath: string) => FileEntry[];
  hasCachedChildren: (dirPath: string) => boolean;
  /**
   * Replace the cached child list for a single directory. This is the
   * ONLY write path for `directoryCache`. The previous entry (if any) is
   * discarded wholesale — partial merges are not supported because we
   * always re-read the full listing from the backend.
   */
  setCachedChildren: (dirPath: string, children: FileEntry[]) => void;
  /**
   * Drop the cached entry for `dirPath` alone. Does NOT touch subdirectory
   * entries. Callers that need a fresh listing should fetch first then call
   * `setCachedChildren`; this method is for the error path where we want
   * the tree to show "no data" rather than hold on to a stale list.
   */
  evictCachedChildren: (dirPath: string) => void;
  /** Drop every cached directory entry. Used only by the manual "刷新"
   *  pipeline before it reloads the tree from disk. */
  clearCache: () => void;

  toggleDir: (path: string) => void;
  setSelectedFile: (path: string | null) => void;
  setIsLoading: (loading: boolean) => void;
  setDirLoading: (path: string, loading: boolean) => void;
  isDirExpanded: (path: string) => boolean;
  isDirLoading: (path: string) => boolean;

  openTab: (tab: OpenTab) => void;
  openWorkspaceFile: (path: string, options?: { name?: string; forceNew?: boolean }) => void;
  closeTab: (tabId: string) => void;
  /**
   * Close a tab by path. If the tab is dirty, returns `false` so the caller
   * can show a confirmation dialog instead of force-closing.
   */
  requestCloseTab: (path: string) => boolean;
  setActiveTab: (tabId: string) => void;
  setOpenTabDirty: (path: string, isDirty: boolean) => void;
  replaceTabs: (openTabs: OpenTab[], activeTabId: string | null) => void;

  setKnowledgeBase: (kb: KnowledgeBase | undefined) => void;
  setBuildProgress: (progress: BuildProgress | undefined) => void;
  setKnowledgeToolCall: (toolCall: ActiveToolCall | undefined) => void;

  toggleKnowledgeSelectMode: () => void;
  setKnowledgeSelectMode: (mode: boolean) => void;
  toggleKnowledgeChecked: (path: string) => void;
  setKnowledgeChecked: (path: string, checked: boolean) => void;
  checkAllKnowledgePaths: (paths: string[]) => void;
  clearKnowledgeChecked: () => void;

  startInlineEdit: (state: InlineEditState) => void;
  cancelInlineEdit: () => void;
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

      knowledgeSelectMode: false,
      knowledgeCheckedPaths: new Set(),

      inlineEdit: null,

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

      /**
       * Drop a single cache entry. The path is normalized so cache-key
       * consistency holds across callsites that may pass Windows-style
       * (`\`) input. No recursive / prefix-delete — sibling and child
       * directories are untouched on purpose.
       */
      evictCachedChildren: (dirPath) =>
        set((state) => {
          const key = normalizeDirPath(dirPath);
          if (!state.directoryCache.has(key)) return {};
          const newCache = new Map(state.directoryCache);
          newCache.delete(key);
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
            const newSelectedFile = tab.isSettings || tab.isCloud ? null : tab.path;
            return { activeTabId: existing.id, selectedFile: newSelectedFile };
          }
          const newTabs = [...state.openTabs, { ...tab, isDirty: tab.isDirty ?? false }];
          const newSelectedFile = tab.isSettings || tab.isCloud ? null : tab.path;
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

          if (existing && !options?.forceNew) {
            return { activeTabId: existing.id, selectedFile: resolvedPath };
          }

          // When forcing a new tab we reuse the same path as the tab id but
          // disambiguate with a suffix so React keys remain unique.
          const newTabId = existing && options?.forceNew
            ? `${resolvedPath}::${Date.now()}`
            : resolvedPath;

          const tabName =
            options?.name ??
            resolvedEntry?.name ??
            getBaseName(resolvedPath) ??
            '未命名文档';
          const newTab: OpenTab = {
            id: newTabId,
            path: resolvedPath,
            name: tabName,
            isDirty: false,
          };

          // Auto-expand parent directories in the file tree. We add every
          // ancestor between the workspace root and the opened file's
          // parent so the row stays visible.
          const newExpandedDirs = new Set(state.expandedDirs);
          const parentPath = getParentPath(resolvedPath, state.workspacePath);
          const normalizedRoot = state.workspacePath
            ? normalizeDirPath(state.workspacePath)
            : null;
          if (parentPath && normalizedRoot) {
            const relativeParent = getRelativePath(normalizedRoot, parentPath);
            const parts = relativeParent.split('/').filter(Boolean);
            let currentPath = normalizedRoot;
            for (const part of parts) {
              currentPath = joinPath(currentPath, part);
              newExpandedDirs.add(currentPath);
            }
          }

          return {
            openTabs: [...state.openTabs, newTab],
            activeTabId: newTab.id,
            selectedFile: resolvedPath,
            expandedDirs: newExpandedDirs,
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
            selectedFile: (() => {
              if (!newActiveId) return null;
              const next = newTabs.find((t) => t.id === newActiveId);
              if (!next) return null;
              if (next.isSettings || next.isCloud) return null;
              return next.path;
            })(),
          };
        }),

      requestCloseTab: (path) => {
        const state = get();
        const tab = state.openTabs.find((t) => t.path === path && !t.isSettings && !t.isCloud);
        if (!tab) return true;
        if (tab.isDirty) return false;
        state.closeTab(tab.id);
        return true;
      },

      setActiveTab: (tabId) =>
        set((state) => {
          const tab = state.openTabs.find((t) => t.id === tabId);
          const newSelectedFile = tab?.isSettings || tab?.isCloud
            ? null
            : tab?.path ?? state.selectedFile;
          return {
            activeTabId: tabId,
            selectedFile: newSelectedFile,
          };
        }),

      /**
       * Replace the entire `openTabs` array and active tab id. Used when
       * restoring a per-workspace snapshot so we don't go through individual
       * `openTab`/`closeTab` actions (which would generate many persisted
       * snapshots of their own).
       */
      replaceTabs: (openTabs, activeTabId) =>
        set((state) => ({
          openTabs,
          activeTabId,
          selectedFile: openTabs.find((t) => t.id === activeTabId)?.path ?? state.selectedFile,
        })),

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

      setKnowledgeBase: (kb) => set({ knowledgeBase: kb }),
      setBuildProgress: (progress) => set({ buildProgress: progress }),
      setKnowledgeToolCall: (toolCall) => set({ knowledgeToolCall: toolCall }),

      toggleKnowledgeSelectMode: () =>
        set((state) => ({
          knowledgeSelectMode: !state.knowledgeSelectMode,
          knowledgeCheckedPaths: state.knowledgeSelectMode ? new Set() : state.knowledgeCheckedPaths,
        })),

      setKnowledgeSelectMode: (mode) =>
        set({ knowledgeSelectMode: mode, knowledgeCheckedPaths: mode ? new Set() : new Set() }),

      toggleKnowledgeChecked: (path) =>
        set((state) => {
          const newChecked = new Set(state.knowledgeCheckedPaths);
          if (newChecked.has(path)) {
            newChecked.delete(path);
          } else {
            newChecked.add(path);
          }
          return { knowledgeCheckedPaths: newChecked };
        }),

      setKnowledgeChecked: (path, checked) =>
        set((state) => {
          const newChecked = new Set(state.knowledgeCheckedPaths);
          if (checked) {
            newChecked.add(path);
          } else {
            newChecked.delete(path);
          }
          return { knowledgeCheckedPaths: newChecked };
        }),

      checkAllKnowledgePaths: (paths) =>
        set({ knowledgeCheckedPaths: new Set(paths) }),

      clearKnowledgeChecked: () =>
        set({ knowledgeCheckedPaths: new Set() }),

      startInlineEdit: (state) => set({ inlineEdit: state }),
      cancelInlineEdit: () => set({ inlineEdit: null }),
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
          // Transient UI state must not survive a reload.
          inlineEdit: null,
          hasRestoredFromPersist: true,
        };
      },
    },
  ),
);
