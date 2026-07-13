import { useCallback, useEffect, useRef } from 'react';
import { useSidebarStore } from '../store';
import type { FileEntry } from '../types';
import { useWorkspaceFileWatcher } from './useWorkspaceFileWatcher';
import { useDebouncedCallback } from './useDebouncedCallback';
import { loadDirectoryChildren } from '../services/workspace';
import { reportError } from '../utils/errors';
import { getRelativePath, isPathInside, normalizeDirPath } from '../utils/path';

interface FileChangeEvent {
  type: string;
  data: { path: string };
}

interface UseWorkspaceTreeResult {
  workspacePath: string | null;
  expandedDirs: Set<string>;
  selectedFile: string | null;
  isLoading: boolean;
  loadingDirs: Set<string>;
  openTabs: ReturnType<typeof useSidebarStore.getState>['openTabs'];

  /** Synchronous read from the cache. Returns `[]` for unknown paths so the
   *  tree never has to special-case "no data yet". */
  getChildren: (dirPath: string) => FileEntry[];

  /**
   * Ensure `dirPath` has a cache entry. Cheap if it does; otherwise fetches
   * from the backend and stores the result. The promise resolves once the
   * fetch completes — even on error, so callers can `await` without leaking
   * unhandled rejection warnings.
   */
  ensureLoaded: (dirPath: string) => Promise<void>;

  /**
   * Toggle the expanded state of a directory. If the directory is being
   * expanded for the first time, kick off `ensureLoaded` so the row
   * actually has something to render.
   */
  onDirectoryClick: (entry: FileEntry) => void;

  /**
   * Re-read `dirPath` from the backend and replace the cache entry. Used
   * by mutation call sites (create, rename, paste, delete) to keep the
   * tree honest without dropping subdirectory entries.
   */
  refreshDirectory: (dirPath: string) => Promise<void>;
}

export function useWorkspaceTree(): UseWorkspaceTreeResult {
  const workspacePath = useSidebarStore((state) => state.workspacePath);
  const expandedDirs = useSidebarStore((state) => state.expandedDirs);
  const selectedFile = useSidebarStore((state) => state.selectedFile);
  const isLoading = useSidebarStore((state) => state.isLoading);
  const loadingDirs = useSidebarStore((state) => state.loadingDirs);
  const openTabs = useSidebarStore((state) => state.openTabs);

  const toggleDir = useSidebarStore((state) => state.toggleDir);
  const setDirLoading = useSidebarStore((state) => state.setDirLoading);
  const setCachedChildren = useSidebarStore((state) => state.setCachedChildren);
  const getCachedChildren = useSidebarStore((state) => state.getCachedChildren);
  const hasCachedChildren = useSidebarStore((state) => state.hasCachedChildren);
  const evictCachedChildren = useSidebarStore((state) => state.evictCachedChildren);

  const normalizedWorkspacePath = workspacePath
    ? normalizeDirPath(workspacePath)
    : null;

  /**
   * Fetch the children of `dirPath` from the backend, store them, and
   * toggle the `loadingDirs` flag so the row can show a spinner.
   *
   * This is the **only** function in the file that talks to the backend.
   * Every cache update flows through here, which means there is exactly
   * one place to reason about ordering, error handling, and the
   * "evict-on-error" fallback.
   *
   * Normalisation is performed here (not at the callsite) so every cache
   * key is automatically consistent regardless of which OS-style path the
   * caller passes in.
   */
  const fetchAndCache = useCallback(
    async (rawDirPath: string): Promise<void> => {
      const dirPath = normalizeDirPath(rawDirPath);
      if (!dirPath) return;

      setDirLoading(dirPath, true);
      try {
        const children = await loadDirectoryChildren(dirPath);
        setCachedChildren(dirPath, children);
      } catch (err) {
        reportError('workspace-tree-fetch', err);
        // Drop the stale entry so the tree stops rendering an out-of-date
        // list. The next click on the row will trigger another fetch.
        evictCachedChildren(dirPath);
      } finally {
        setDirLoading(dirPath, false);
      }
    },
    [setDirLoading, setCachedChildren, evictCachedChildren],
  );

  /**
   * Debounce file-watcher events so a burst of changes (e.g. an editor
   * saving a batch of files) triggers a single refetch per directory
   * instead of one per event.
   */
  const debouncedFetch = useDebouncedCallback(fetchAndCache, 250);

  // Per-directory lock to coalesce overlapping refresh requests. If a
  // refresh for `parentPath` is already in flight (or just resolved), we
  // skip until the lock is released. Without this, a quick `Created` →
  // `Modified` pair for the same file could cancel each other out and
  // leave a partially-applied cache entry.
  const inflightRef = useRef<Set<string>>(new Set());

  const refreshDirectory = useCallback(
    async (parentPath: string) => {
      const dirPath = normalizeDirPath(parentPath);
      if (!dirPath || inflightRef.current.has(dirPath)) return;
      inflightRef.current.add(dirPath);
      try {
        await fetchAndCache(dirPath);
      } finally {
        inflightRef.current.delete(dirPath);
      }
    },
    [fetchAndCache],
  );

  const ensureLoaded = useCallback(
    async (dirPath: string) => {
      if (hasCachedChildren(dirPath)) return;
      await refreshDirectory(dirPath);
    },
    [hasCachedChildren, refreshDirectory],
  );

  /**
   * First-time population of the root directory cache.
   *
   * Triggers once when the workspace is opened (either via persist-restore
   * or via `setWorkspacePath` from the picker). Only depends on the
   * normalised workspace path so it does NOT re-fire on every expand /
   * collapse — that was the bug that produced the tree-wide flicker.
   *
   * The `getCachedChildren` guard means a `switchWorkspace` that has
   * already populated the root (via `applyWorkspaceDirectoryLoad`) won't
   * pay for a redundant fetch.
   */
  useEffect(() => {
    if (!normalizedWorkspacePath) return;
    if (hasCachedChildren(normalizedWorkspacePath)) return;
    void refreshDirectory(normalizedWorkspacePath);
  }, [normalizedWorkspacePath, hasCachedChildren, refreshDirectory]);

  /**
   * Watcher → refresh. Every `file-change` event resolves to the parent
   * directory of the changed path, which is exactly the cache entry we
   * need to invalidate. We debounce to coalesce bursts.
   */
  const handleFileChange = useCallback(
    (event: FileChangeEvent) => {
      if (!normalizedWorkspacePath) return;

      const changedPath = event.data?.path;
      if (!changedPath || !isPathInside(normalizedWorkspacePath, changedPath)) return;

      if (
        event.type === 'Created' ||
        event.type === 'Deleted' ||
        event.type === 'Modified'
      ) {
        const parentPath = getParentDirPath(changedPath, normalizedWorkspacePath);
        if (parentPath) {
          void debouncedFetch(parentPath);
        }
      }
    },
    [normalizedWorkspacePath, debouncedFetch],
  );

  useWorkspaceFileWatcher(normalizedWorkspacePath, handleFileChange);

  /**
   * Click handler for a directory row in the tree. Toggles the expanded
   * state and lazily fetches children on first expansion.
   */
  const onDirectoryClick = useCallback(
    (entry: FileEntry) => {
      if (!entry.is_dir) return;
      const wasExpanded = useSidebarStore.getState().isDirExpanded(entry.path);
      toggleDir(entry.path);
      if (!wasExpanded) {
        void ensureLoaded(entry.path);
      }
    },
    [toggleDir, ensureLoaded],
  );

  const getChildren = useCallback(
    (dirPath: string): FileEntry[] => {
      return getCachedChildren(normalizeDirPath(dirPath));
    },
    [getCachedChildren],
  );

  return {
    workspacePath,
    expandedDirs,
    selectedFile,
    isLoading,
    loadingDirs,
    openTabs,
    getChildren,
    ensureLoaded,
    onDirectoryClick,
    refreshDirectory,
  };
}

/**
 * Return the directory that contains `filePath`, rooted at `workspaceRoot`.
 * Both inputs are normalised first so the math is separator-agnostic.
 *
 *   getParentDirPath('/root', '/root/a.md')           === '/root'
 *   getParentDirPath('/root', '/root/sub/b.md')      === '/root/sub'
 *   getParentDirPath('/root', '/root/sub/nested/c')  === '/root/sub/nested'
 */
function getParentDirPath(filePath: string, workspaceRoot: string): string | null {
  const normalizedRoot = normalizeDirPath(workspaceRoot);
  if (!normalizedRoot) return null;

  const relativePath = getRelativePath(normalizedRoot, filePath);
  if (!relativePath) return normalizedRoot;

  const segments = relativePath.split('/').filter(Boolean);
  if (segments.length <= 1) return normalizedRoot;

  return segments
    .slice(0, -1)
    .reduce((acc, segment) => `${acc}/${segment}`, normalizedRoot);
}