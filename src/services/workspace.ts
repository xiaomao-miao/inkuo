import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { useSidebarStore } from '../store';
import type { FileEntry } from '../types';

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
