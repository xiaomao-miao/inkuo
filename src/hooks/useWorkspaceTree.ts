import { useCallback, useEffect, useRef } from 'react';
import { useSidebarStore } from '../store';
import type { FileEntry } from '../types';
import {
  useWorkspaceFileWatcher,
  type DirsChangedPayload,
} from './useWorkspaceFileWatcher';
import { useKeyedDebouncedCallback } from './useDebouncedCallback';
import { loadDirectoryChildren } from '../services/workspace';
import { reportError } from '../utils/errors';
import { isPathInside, normalizeDirPath } from '../utils/path';

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

const DEBOUNCE_MS = 250;

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
   * Per-directory refresh + follow-up queue. Models VS Code's
   * `RunOnceWorker.doRun` semantic: at most one trailing work item per
   * quiet window, none lost.
   *
   *   - `inflightRef` tracks directories whose `list_directory` IPC is
   *     currently in flight. Concurrent calls for the same directory
   *     while one is in flight do NOT drop the call — they mark the
   *     directory dirty in `pendingFollowUpRef` and the just-completed
   *     fetch schedules a single trailing re-read.
   *   - `pendingFollowUpRef` collapses multiple "another event arrived
   *     while we were fetching" notifications for the same directory
   *     into a single follow-up. Once the in-flight fetch finishes, we
   *     kick off exactly one more re-read.
   */
  const inflightRef = useRef<Set<string>>(new Set());
  const pendingFollowUpRef = useRef<Set<string>>(new Set());

  const refreshDirectory = useCallback(
    async (parentPath: string): Promise<void> => {
      const dirPath = normalizeDirPath(parentPath);
      if (!dirPath) return;
      if (inflightRef.current.has(dirPath)) {
        // Don't drop the call — coalesce it into a single trailing
        // re-read. This is the fix for "save-while-fetching" events
        // that the previous inflight-skip silently lost.
        pendingFollowUpRef.current.add(dirPath);
        return;
      }
      inflightRef.current.add(dirPath);
      try {
        await fetchAndCache(dirPath);
      } finally {
        inflightRef.current.delete(dirPath);
        if (pendingFollowUpRef.current.delete(dirPath)) {
          // Schedule the trailing re-read synchronously so the next
          // event that arrives during the follow-up's fetch queues
          // another follow-up, not a direct call.
          void refreshDirectory(dirPath);
        }
      }
    },
    [fetchAndCache],
  );

  /**
   * Keyed debouncer: each directory gets its own 250 ms trailing timer.
   * This is the fix for the second failure mode — a flat debouncer would
   * only keep the last call's args, so a multi-directory burst would
   * leave one parent without a refresh. The keyed debouncer fires one
   * trailing call per directory.
   */
  const keyedRefresh = useKeyedDebouncedCallback<string, typeof refreshDirectory>(
    refreshDirectory,
    (args) => normalizeDirPath(args[0]),
    DEBOUNCE_MS,
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
   * Watcher → refresh. The Rust side already coalesced OS events into a
   * single `dirs-changed` payload, but the payload may list multiple
   * directories that need independent refresh — and a single directory
   * may also be reported multiple times across close-together events.
   * The keyed debouncer handles the former; the per-directory follow-up
   * queue in `refreshDirectory` handles the latter.
   */
  const handleDirsChanged = useCallback(
    (event: DirsChangedPayload) => {
      if (!normalizedWorkspacePath) return;
      for (const rawDir of event.dirs) {
        const dir = normalizeDirPath(rawDir);
        if (!dir) continue;
        if (!isPathInside(normalizedWorkspacePath, dir)) continue;
        keyedRefresh(dir);
      }
    },
    [normalizedWorkspacePath, keyedRefresh],
  );

  useWorkspaceFileWatcher(normalizedWorkspacePath, handleDirsChanged);

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
