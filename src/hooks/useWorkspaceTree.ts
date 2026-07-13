import { useCallback, useEffect, useMemo, useState, useRef } from 'react';
import { useSidebarStore } from '../store';
import type { FileEntry } from '../types';
import { useWorkspaceFileWatcher } from './useWorkspaceFileWatcher';
import { useDebouncedCallback } from './useDebouncedCallback';
import {
  loadDirectoryChildren,
  openWorkspaceDirectory,
  switchWorkspace,
} from '../services/workspace';
import { reportError } from '../utils/errors';
import { getRelativePath, isPathInside, joinPath, normalizeDirPath } from '../utils/path';

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
  isCollapsed: boolean;
  setIsCollapsed: React.Dispatch<React.SetStateAction<boolean>>;
  openWorkspace: () => Promise<void>;
  refreshWorkspace: () => Promise<void>;
  handleFileClick: (entry: FileEntry) => Promise<void>;
  getChildren: (dirPath: string) => FileEntry[];
  isDirLoading: (path: string) => boolean;
  triggerFileRefresh: (parentPath: string) => Promise<void>;
}

async function restoreExpandedDirectories(
  workspaceRootPath: string,
  expandedDirPaths: string[],
  loadChildren: (dirPath: string) => Promise<FileEntry[]>,
): Promise<void> {
  const normalizedRoot = normalizeDirPath(workspaceRootPath);

  // Normalize every persisted path before sorting/comparing so the
  // sort by length + startsWith check works whether the persisted entry
  // used `\` or `/` (Windows app versions persist native paths).
  const normalizedExpandedPaths = expandedDirPaths
    .map((path) => normalizeDirPath(path))
    .filter((path) => path && path !== normalizedRoot)
    .sort((left, right) => left.length - right.length);

  for (const dirPath of normalizedExpandedPaths) {
    const relativePath = getRelativePath(normalizedRoot, dirPath);
    if (!relativePath) continue;

    const segments = relativePath.split('/').filter(Boolean);
    let currentPath = normalizedRoot;

    for (const segment of segments) {
      currentPath = joinPath(currentPath, segment);
      await loadChildren(currentPath);
    }
  }
}

export function useWorkspaceTree(): UseWorkspaceTreeResult {
  const workspacePath = useSidebarStore((state) => state.workspacePath);
  const expandedDirs = useSidebarStore((state) => state.expandedDirs);
  const selectedFile = useSidebarStore((state) => state.selectedFile);
  const isLoading = useSidebarStore((state) => state.isLoading);
  const loadingDirs = useSidebarStore((state) => state.loadingDirs);
  const openTabs = useSidebarStore((state) => state.openTabs);
  const toggleDir = useSidebarStore((state) => state.toggleDir);
  const setIsLoading = useSidebarStore((state) => state.setIsLoading);
  const setDirLoading = useSidebarStore((state) => state.setDirLoading);
  const openWorkspaceFile = useSidebarStore((state) => state.openWorkspaceFile);
  const getCachedChildren = useSidebarStore((state) => state.getCachedChildren);
  const setCachedChildren = useSidebarStore((state) => state.setCachedChildren);
  const hasCachedChildren = useSidebarStore((state) => state.hasCachedChildren);
  const invalidateCache = useSidebarStore((state) => state.invalidateCache);
  const clearCache = useSidebarStore((state) => state.clearCache);
  const isDirExpanded = useSidebarStore((state) => state.isDirExpanded);

  const [isCollapsed, setIsCollapsed] = useState(false);

  const workspaceRootPath = useMemo(
    () => workspacePath ?? null,
    [workspacePath],
  );

  const refreshLockRef = useRef<Set<string>>(new Set());
  /// Outstanding lock-release timers for each parent path. We track them so a
  /// hook unmount (e.g. workspace switch) can cancel pending releases instead
  /// of letting them fire against a (potentially remounted) ref. The set is
  /// the source of truth for `clearTimeout` calls on cleanup.
  const lockReleaseTimersRef = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());

  useEffect(() => {
    const timers = lockReleaseTimersRef.current;
    const lockSet = refreshLockRef.current;
    return () => {
      for (const handle of timers.values()) {
        clearTimeout(handle);
      }
      timers.clear();
      lockSet.clear();
    };
  }, []);

  /**
   * Refresh a specific directory's cache and reload.
   *
   * We always re-read the directory contents (debounced upstream) so that:
   * - expanded directories show fresh children immediately,
   * - collapsed directories have fresh cache for when the user expands them,
   *   avoiding a "stale until manual refresh" UX.
   */
  const triggerFileRefresh = useCallback(
    async (parentPath: string) => {
      invalidateCache(parentPath);

      try {
        const children = await loadDirectoryChildren(parentPath);
        setCachedChildren(parentPath, children);
      } catch (err) {
        reportError('workspace-tree-refresh-children', err);
      }
    },
    [invalidateCache, setCachedChildren],
  );

  /**
   * Debounced file refresh to batch multiple rapid changes.
   */
  const debouncedRefresh = useDebouncedCallback(
    async (parentPath: string) => {
      if (refreshLockRef.current.has(parentPath)) return;

      refreshLockRef.current.add(parentPath);
      try {
        await triggerFileRefresh(parentPath);
      } finally {
        const timers = lockReleaseTimersRef.current;
        const existing = timers.get(parentPath);
        if (existing !== undefined) {
          clearTimeout(existing);
        }
        const handle = setTimeout(() => {
          timers.delete(parentPath);
          refreshLockRef.current.delete(parentPath);
        }, 500);
        timers.set(parentPath, handle);
      }
    },
    300,
  );

  /**
   * Load children for a directory (lazy loading).
   * Uses cache if available, otherwise fetches from backend.
   */
  const loadChildren = useCallback(
    async (dirPath: string) => {
      if (hasCachedChildren(dirPath)) {
        return getCachedChildren(dirPath);
      }

      setDirLoading(dirPath, true);
      try {
        const children = await loadDirectoryChildren(dirPath);
        setCachedChildren(dirPath, children);
        return children;
      } catch (err) {
        reportError('workspace-tree-load-children', err);
        return [];
      } finally {
        setDirLoading(dirPath, false);
      }
    },
    [getCachedChildren, hasCachedChildren, setCachedChildren, setDirLoading],
  );

  /**
   * Get children for a directory (synchronous, from cache).
   */
  const getChildren = useCallback(
    (dirPath: string): FileEntry[] => {
      return getCachedChildren(dirPath);
    },
    [getCachedChildren],
  );

  /**
   * Check if a directory is currently loading.
   */
  const isDirLoading = useCallback(
    (path: string): boolean => {
      return loadingDirs.has(path);
    },
    [loadingDirs],
  );

  /**
   * Refresh the entire workspace by clearing cache and reloading root.
   */
  const refreshWorkspace = useCallback(async () => {
    if (!workspaceRootPath) return;

    setIsLoading(true);
    try {
      clearCache();
      await loadChildren(workspaceRootPath);
      await restoreExpandedDirectories(
        workspaceRootPath,
        Array.from(expandedDirs),
        loadChildren,
      );
    } catch (err) {
      reportError('workspace-tree-refresh', err);
    } finally {
      setIsLoading(false);
    }
  }, [workspaceRootPath, loadChildren, clearCache, expandedDirs, setIsLoading]);

  /**
   * Handle file system changes from the watcher.
   */
  const handleFileChange = useCallback(
    (event: FileChangeEvent) => {
      if (!workspaceRootPath) return;

      const changedPath = event.data?.path;
      if (!changedPath || !isPathInside(workspaceRootPath, changedPath)) return;

      switch (event.type) {
        case 'Created':
        case 'Deleted':
        case 'Modified': {
          const parentPath = getParentPath(changedPath, workspaceRootPath);
          if (parentPath) {
            debouncedRefresh(parentPath);
          }
          break;
        }
        default:
          break;
      }
    },
    [workspaceRootPath, debouncedRefresh],
  );

  useWorkspaceFileWatcher(workspaceRootPath, handleFileChange);

  /**
   * Open a workspace directory.
   */
  const openWorkspace = useCallback(async () => {
    try {
      const selected = await openWorkspaceDirectory();
      if (!selected) return;

      switchWorkspace(selected);
      clearCache();
      await loadChildren(selected);
    } catch (err) {
      reportError('workspace-tree-open-workspace', err);
    }
  }, [loadChildren, clearCache]);

  /**
   * Handle file/folder click.
   */
  const handleFileClick = useCallback(
    async (entry: FileEntry) => {
      if (entry.is_dir) {
        const wasExpanded = isDirExpanded(entry.path);

        if (wasExpanded) {
          toggleDir(entry.path);
        } else {
          toggleDir(entry.path);
          if (!hasCachedChildren(entry.path)) {
            await loadChildren(entry.path);
          }
        }
        return;
      }

      openWorkspaceFile(entry.path, { name: entry.name });
    },
    [
      isDirExpanded,
      toggleDir,
      loadChildren,
      hasCachedChildren,
      openWorkspaceFile,
    ],
  );

  return {
    workspacePath,
    expandedDirs,
    selectedFile,
    isLoading,
    loadingDirs,
    openTabs,
    isCollapsed,
    setIsCollapsed,
    openWorkspace,
    refreshWorkspace,
    handleFileClick,
    getChildren,
    isDirLoading,
    triggerFileRefresh,
  };
}

/**
 * Extract parent directory path from a file path.
 *
 * Both arguments are normalized so this works whether the watcher hands
 * us `E:\文档\sub\file.md` or `E:/文档/sub/file.md`. The returned parent is
 * also normalized, so it can be used as a cache key directly.
 */
function getParentPath(filePath: string, workspaceRoot: string): string | null {
  const normalizedRoot = normalizeDirPath(workspaceRoot);
  if (!normalizedRoot) return null;

  const relativePath = getRelativePath(normalizedRoot, filePath);
  if (!relativePath) return normalizedRoot;

  const segments = relativePath.split('/').filter(Boolean);
  if (segments.length <= 1) {
    return normalizedRoot;
  }

  const parentSegments = segments.slice(0, -1);
  return joinPath(normalizedRoot, ...parentSegments);
}
