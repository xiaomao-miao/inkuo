import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { useSidebarStore } from '../store';
import type {
  CreateEntryResult,
  FileEntry,
  NewEntryPayload,
  RenamePathResult,
} from '../types';

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
