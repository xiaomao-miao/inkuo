import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { useSidebarStore, useAIPanelStore } from '../store';
import { createNewSession } from '../store/aiPanelReducers';
import {
  getRelativePath,
  joinPath,
  normalizeDirPath,
} from '../utils/path';
import { reportError } from '../utils/errors';
import type {
  CreateEntryResult,
  FileEntry,
  NewEntryPayload,
  RenamePathResult,
  TodoSnapshot,
} from '../types';
import type { ChatSession, WorkspaceSnapshot } from '../store';

// Single-flight queue for workspace switches. Without this, two calls to
// `switchWorkspace` that fire in rapid succession (e.g. user picks "A",
// then quickly picks "B" before A finishes loading) overlap: the second
// call's `saveCurrentSnapshot(sidebar)` step sees the partially-updated
// state left over by the first call's apply step, and its `loadSnapshot`
// can return a target-file payload that no longer matches the actual UI.
//
// The simplest correct serialization is to chain promises: each caller
// awaits the previous one before starting its own work. Late callers
// observe the most recent state when they eventually run, which matches
// user expectation (the latest click wins).
let switchQueue: Promise<void> = Promise.resolve();

/** Enqueue `task` behind any in-flight workspace switch. */
function withSwitchLock<T>(task: () => Promise<T>): Promise<T> {
  const next = switchQueue.then(task, task);
  // Swallow rejections on the queue itself — they belong to the caller
  // who will await `next` (the original task's promise). Without this the
  // queue could become a permanently-rejected promise and block every
  // subsequent switch.
  switchQueue = next.then(
    () => undefined,
    () => undefined,
  );
  return next;
}

/**
 * Drop todo snapshots whose session id is no longer in `sessions`.
 * Sessions are normally what outlives the snapshot (and vice versa) —
 * but `loadSnapshot` doesn't guarantee the two arrays match perfectly
 * if a partial write from an older app version survives in
 * `workspace_snapshots.json`. A leftover `todoSnapshotBySession[id]`
 * would just dangle harmlessly in the map, but pruning keeps the
 * hydrated store shape consistent with the in-memory invariant.
 */
function pruneTodoSnapshotsToSessions(
  snapshots: Record<string, TodoSnapshot> | undefined,
  sessions: ChatSession[],
): Record<string, TodoSnapshot> {
  if (!snapshots) return {};
  const sessionIds = new Set(sessions.map((s) => s.id));
  const pruned: Record<string, TodoSnapshot> = {};
  for (const [id, snap] of Object.entries(snapshots)) {
    if (sessionIds.has(id)) pruned[id] = snap;
  }
  return pruned;
}

/**
 * Load children entries for a directory from the backend.
 *
 * The path is normalized to use `/` separators before being sent to the
 * backend. Rust's `std::fs::read_dir` accepts either separator on Windows,
 * but normalizing at this boundary guarantees that every cache key and
 * every comparison downstream is separator-agnostic.
 */
export async function loadDirectoryChildren(path: string): Promise<FileEntry[]> {
  return invoke<FileEntry[]>('list_directory', { path: normalizeDirPath(path) });
}

/**
 * Search for files in a directory by name.
 */
export async function searchFiles(path: string, query: string): Promise<FileEntry[]> {
  return invoke<FileEntry[]>('search_directory', { path: normalizeDirPath(path), query });
}

/**
 * Apply a workspace directory load, caching the root entries.
 *
 * `mergeWithExisting` controls whether the directory cache is wiped before
 * loading:
 *   - `false` (default for picker-driven switches): the new root entry is
 *     added on top of whatever is already in the cache. The caller (typically
 *     `switchWorkspace`) has already cleared the cache in its own step, so we
 *     would otherwise do the work twice.
 *   - `true` (default for `reloadCurrentWorkspace`): clear the cache first so
 *     a manual "重新加载工作区" really does rebuild everything from disk.
 *
 * `showSkeleton` toggles the sidebar `isLoading` flag for the duration of
 * the network call, so the UI shows the loading skeleton instead of an
 * empty/broken tree. Off by default — background refreshes triggered by
 * the file watcher shouldn't swap the visible tree for a skeleton.
 */
export async function applyWorkspaceDirectoryLoad(
  path: string,
  options?: { mergeWithExisting?: boolean; showSkeleton?: boolean },
): Promise<FileEntry[]> {
  const store = useSidebarStore.getState();
  const normalizedPath = normalizeDirPath(path);

  if (options?.showSkeleton) {
    store.setIsLoading(true);
  }
  try {
    const children = await loadDirectoryChildren(normalizedPath);

    if (options?.mergeWithExisting !== false) {
      store.clearCache();
    }

    store.setCachedChildren(normalizedPath, children);
    return children;
  } finally {
    if (options?.showSkeleton) {
      store.setIsLoading(false);
    }
  }
}

/**
 * Reload the current workspace from disk.
 *
 * Sole owner of the "整树全清 + 重新加载" flow. Wraps a `clearCache` + root
 * load + restored-expanded-dirs walk with a single `isLoading` envelope so
 * the sidebar shows the loading skeleton for the duration. Callers:
 *   - the Sidebar's "刷新" button (manual refresh)
 *   - the ContextMenu's "重新加载工作区" entry
 *
 * This function is intentionally NOT bound to React's render lifecycle —
 * we used to drive it from a `Sidebar` `useEffect` whose dependencies
 * included a function that captured `expandedDirs`, which made every
 * expand/collapse re-run the whole reload. Centralising it here keeps
 * `refresh-on-demand` and `refresh-on-render` correctly separated.
 */
export async function reloadCurrentWorkspace(): Promise<void> {
  const { workspacePath, expandedDirs, setIsLoading } = useSidebarStore.getState();
  if (!workspacePath) return;

  const normalizedRoot = normalizeDirPath(workspacePath);

  setIsLoading(true);
  try {
    // `applyWorkspaceDirectoryLoad` already owns `clearCache` + root write.
    await applyWorkspaceDirectoryLoad(normalizedRoot, {
      mergeWithExisting: true,
      showSkeleton: false, // we manage isLoading here to keep the gate open
      // across the whole restore walk below.
    });

    await reloadExpandedDirectories(normalizedRoot, Array.from(expandedDirs));
  } catch (err) {
    reportError('reload-current-workspace', err);
  } finally {
    setIsLoading(false);
  }
}

/**
 * Walk every directory the user has expanded in the current workspace and
 * re-read it from the backend, so the freshly-cleared cache is repopulated
 * with what the user previously chose to keep visible.
 *
 * Mirrors the helper that used to live inside `useWorkspaceTree` (kept here
 * so the reactive hook no longer owns the reload flow).
 */
async function reloadExpandedDirectories(
  workspaceRootPath: string,
  expandedDirPaths: string[],
): Promise<void> {
  const normalizedRoot = normalizeDirPath(workspaceRootPath);

  const normalizedExpandedPaths = expandedDirPaths
    .map((path) => normalizeDirPath(path))
    .filter((path) => path && path !== normalizedRoot)
    .sort((left, right) => left.length - right.length);

  const store = useSidebarStore.getState();
  for (const dirPath of normalizedExpandedPaths) {
    const relativePath = getRelativePath(normalizedRoot, dirPath);
    if (!relativePath) continue;

    const segments = relativePath.split('/').filter(Boolean);
    let currentPath = normalizedRoot;
    for (const segment of segments) {
      currentPath = joinPath(currentPath, segment);
      try {
        const children = await loadDirectoryChildren(currentPath);
        store.setCachedChildren(currentPath, children);
      } catch (err) {
        reportError('reload-expanded-directory', err);
      }
    }
  }
}

/**
 * Open a workspace by path, loading its saved snapshot (open tabs + AI sessions)
 * from the Rust-side shared store so the same workspace opened from a different
 * window restores the user's prior history.
 *
 * Order of operations:
 *  1. Save the current workspace's snapshot to the Rust backend (skip if there
 *     is no current workspace, i.e. the user is on the welcome page).
 *  2. Load the target workspace's snapshot from the backend.
 *  3. Apply the snapshot to sidebar tabs and aiPanel sessions; seed a fresh
 *     empty session if the target workspace has no saved history.
 *
 * Note: the Rust backend stores snapshots in a shared JSON file that all
 * webview windows read/write from, so this works across windows even on
 * platforms where localStorage is per-window (Linux).
 *
 * Caller is responsible for also calling `applyWorkspaceDirectoryLoad` to
 * populate the directory cache and trigger file-tree rendering.
 */
export async function switchWorkspace(targetPath: string): Promise<void> {
  // Coalesce rapid double-clicks of the picker so only one switch is in
  // flight at a time. The latest click still wins because each task reads
  // the live sidebar state at the moment it runs (after the previous task
  // has fully applied its changes).
  return withSwitchLock(async () => {
    const sidebar = useSidebarStore.getState();
    const aiPanel = useAIPanelStore.getState();

    // 1. Persist the current workspace's snapshot before leaving it. We do this
    //    best-effort: a save failure must not block the user from switching
    //    workspaces. The most common cause is "no current workspace yet" (e.g.
    //    we are on the welcome page), which is not an error.
    if (sidebar.workspacePath) {
      try {
        await saveCurrentSnapshot(
          sidebar.workspacePath,
          sidebar.openTabs,
          sidebar.activeTabId,
          aiPanel.sessions,
          aiPanel.activeSessionId,
          aiPanel.todoSnapshotBySession,
        );
      } catch (err) {
        console.warn('Failed to save current workspace snapshot before switch:', err);
      }
    }

    // 2. Load the target workspace's snapshot from the shared store.
    const snapshot = await loadSnapshot(targetPath);

    // 3. Apply the snapshot to the live stores through proper actions so we
    //    keep the React subscription lifecycle consistent.
    if (snapshot && snapshot.openTabs.length > 0) {
      sidebar.setWorkspacePath(targetPath);
      sidebar.replaceTabs(snapshot.openTabs, snapshot.activeTabId);
      const targetSessions = snapshot.aiSessions.length > 0
        ? snapshot.aiSessions
        : [createNewSession(1)];
      // If the persisted activeSessionId points at a session that's no
      // longer in the list (or has been archived), fall back to the
      // first non-archived session so the user isn't dropped onto a
      // blank/closed conversation.
      const stillActive = snapshot.activeSessionId
        ? targetSessions.some((s) => s.id === snapshot.activeSessionId)
        : false;
      const resolvedActiveId = stillActive
        ? snapshot.activeSessionId!
        : targetSessions.find((s) => !s.archived)?.id ?? targetSessions[0].id;
      useAIPanelStore.setState({
        sessions: targetSessions,
        activeSessionId: resolvedActiveId,
        // Restore the in-flight todo chip. Pruned to only sessions that
        // survived the round-trip above so stale ids can't leak back in
        // if `aiSessions` ever drifts from `todoSnapshotBySession`.
        todoSnapshotBySession: pruneTodoSnapshotsToSessions(
          snapshot.todoSnapshotBySession,
          targetSessions,
        ),
      });
    } else {
      // Fresh workspace — reset everything to empty/default.
      sidebar.setWorkspacePath(targetPath);
      sidebar.replaceTabs([], null);
      const fresh = createNewSession(1);
      useAIPanelStore.setState({
        sessions: [fresh],
        activeSessionId: fresh.id,
        todoSnapshotBySession: {},
      });
    }
  });
}

/**
 * Persist the current workspace's tabs + AI sessions as its snapshot.
 * Exposed for callers that want to flush state to disk outside of a
 * workspace switch (e.g. on window close).
 *
 * `todoSnapshotBySession` is persisted alongside the sessions so the
 * `update_todo` chip can repopulate after a restart. We deliberately
 * don't prune sessions here — `loadSnapshot` handles that defensively
 * in case a snapshot ever references a session id that no longer
 * exists (e.g. partial write from an older app version).
 */
export async function saveCurrentSnapshot(
  workspacePath: string,
  openTabs: WorkspaceSnapshot['openTabs'],
  activeTabId: WorkspaceSnapshot['activeTabId'],
  aiSessions: WorkspaceSnapshot['aiSessions'],
  activeSessionId: WorkspaceSnapshot['activeSessionId'],
  todoSnapshotBySession?: WorkspaceSnapshot['todoSnapshotBySession'],
): Promise<void> {
  const snapshot: WorkspaceSnapshot = {
    openTabs,
    activeTabId,
    aiSessions,
    activeSessionId,
    todoSnapshotBySession,
  };
  await invoke('save_workspace_snapshot', {
    path: workspacePath,
    snapshot: snapshot as unknown as Record<string, unknown>,
  });
}

/**
 * Load the saved snapshot for `path`, or `null` if there is none or the load
 * failed. Logs (does not throw) on read errors so callers can treat a missing
 * snapshot the same as an empty one.
 */
export async function loadSnapshot(path: string): Promise<WorkspaceSnapshot | null> {
  try {
    const raw = await invoke<Record<string, unknown> | null>('load_workspace_snapshot', { path });
    if (!raw) return null;
    return raw as unknown as WorkspaceSnapshot;
  } catch (err) {
    console.warn(`Failed to load workspace snapshot for ${path}:`, err);
    return null;
  }
}

/**
 * Open a directory picker dialog and return the selected path.
 */
export async function openWorkspaceDirectory(): Promise<string | null> {
  const selected = await open({
    directory: true,
    multiple: false,
    title: '选择工作区文件夹',
  });

  return selected ?? null;
}

/**
 * Create a new file or directory under `parent`. The backend applies the
 * extension (if missing) and the optional template content, then emits a
 * `Created` file-change event so the tree refreshes itself.
 */
export async function createFileEntry(
  parent: string,
  name: string,
  payload: NewEntryPayload,
): Promise<CreateEntryResult> {
  return invoke<CreateEntryResult>('create_file_entry', { parent, name, payload });
}

/**
 * Atomically rename/move a file or directory on disk. Emits `Deleted` for the
 * old path and `Created` for the new path so both parent caches refresh.
 */
export async function renamePath(from: string, to: string): Promise<RenamePathResult> {
  return invoke<RenamePathResult>('rename_path', { from, to });
}

/**
 * Delete a file, or a directory when `recursive` is true. Idempotent: deleting
 * a missing path is treated as success.
 */
export async function deletePath(path: string, recursive: boolean): Promise<void> {
  await invoke('delete_path', { path, recursive });
}

/**
 * Copy a file or directory tree.
 */
export async function copyPath(from: string, to: string): Promise<void> {
  await invoke('copy_path', { from, to });
}

/**
 * Move a file or directory (atomic on the same filesystem).
 */
export async function movePath(from: string, to: string): Promise<void> {
  await invoke('move_path', { from, to });
}

/**
 * Lightweight existence check used to disambiguate paste collisions and
 * confirm dialog wording before triggering a backend mutation.
 */
export async function pathExists(path: string): Promise<boolean> {
  return invoke<boolean>('path_exists', { path });
}

/**
 * Open a path with the OS's default associated application.
 */
export async function openWithDefaultApp(path: string): Promise<void> {
  await invoke('open_with_default_app', { path });
}

/**
 * Reveal a path in the platform file manager (Finder / Explorer / xdg-open).
 * Selects the item when the platform supports it.
 */
export async function revealInFileManager(path: string): Promise<void> {
  await invoke('reveal_in_file_manager', { path });
}
