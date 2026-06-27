import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { useSidebarStore, useAIPanelStore } from '../store';
import { createNewSession } from '../store/aiPanelReducers';
import type {
  CreateEntryResult,
  FileEntry,
  NewEntryPayload,
  RenamePathResult,
} from '../types';
import type { WorkspaceSnapshot } from '../store/sidebarStore';

/**
 * Load children entries for a directory from the backend.
 */
export async function loadDirectoryChildren(path: string): Promise<FileEntry[]> {
  return invoke<FileEntry[]>('list_directory', { path });
}

/**
 * Search for files in a directory by name.
 */
export async function searchFiles(path: string, query: string): Promise<FileEntry[]> {
  return invoke<FileEntry[]>('search_directory', { path, query });
}

/**
 * Apply a workspace directory load, caching the root entries.
 */
export async function applyWorkspaceDirectoryLoad(
  path: string,
  options?: { mergeWithExisting?: boolean },
): Promise<FileEntry[]> {
  const store = useSidebarStore.getState();
  const children = await loadDirectoryChildren(path);

  if (options?.mergeWithExisting !== false) {
    store.clearCache();
  }

  store.setCachedChildren(path, children);
  return children;
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
  const sidebar = useSidebarStore.getState();
  const aiPanel = useAIPanelStore.getState();

  // 1. Persist the current workspace's snapshot before leaving it. We do this
  //    best-effort: a save failure must not block the user from switching
  //    workspaces. The most common cause is "no current workspace yet" (e.g.
  //    we are on the welcome page), which is not an error.
  if (sidebar.workspacePath) {
    try {
      await saveCurrentSnapshot(sidebar.workspacePath, sidebar.openTabs, sidebar.activeTabId, aiPanel.sessions, aiPanel.activeSessionId);
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
    useAIPanelStore.setState({
      sessions: targetSessions,
      activeSessionId: snapshot.activeSessionId ?? targetSessions[0].id,
    });
  } else {
    // Fresh workspace — reset everything to empty/default.
    sidebar.setWorkspacePath(targetPath);
    sidebar.replaceTabs([], null);
    const fresh = createNewSession(1);
    useAIPanelStore.setState({
      sessions: [fresh],
      activeSessionId: fresh.id,
    });
  }
}

/**
 * Persist the current workspace's tabs + AI sessions as its snapshot.
 * Exposed for callers that want to flush state to disk outside of a
 * workspace switch (e.g. on window close).
 */
export async function saveCurrentSnapshot(
  workspacePath: string,
  openTabs: WorkspaceSnapshot['openTabs'],
  activeTabId: WorkspaceSnapshot['activeTabId'],
  aiSessions: WorkspaceSnapshot['aiSessions'],
  activeSessionId: WorkspaceSnapshot['activeSessionId'],
): Promise<void> {
  const snapshot: WorkspaceSnapshot = { openTabs, activeTabId, aiSessions, activeSessionId };
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
async function loadSnapshot(path: string): Promise<WorkspaceSnapshot | null> {
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
