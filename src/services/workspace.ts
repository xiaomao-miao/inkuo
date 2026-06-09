import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { useSidebarStore } from '../store';
import type { FileEntry } from '../types';

export async function loadWorkspaceDirectory(path: string, options?: {
  mergeWithExisting?: boolean;
  existingFiles?: FileEntry[];
  expandedDirs?: Set<string>;
}): Promise<FileEntry[]> {
  const entries = await invoke<FileEntry[]>('list_directory', { path });

  if (!options?.mergeWithExisting || !options.existingFiles?.length) {
    return entries;
  }

  const expandedDirs = options.expandedDirs ?? new Set<string>();
  const childrenToKeep = options.existingFiles.filter((file) =>
    [...expandedDirs].some((expanded) => file.path.startsWith(`${expanded}/`))
  );

  const seen = new Set<string>();
  const uniqueEntries: FileEntry[] = [];
  for (const entry of [...childrenToKeep, ...entries]) {
    if (!seen.has(entry.path)) {
      seen.add(entry.path);
      uniqueEntries.push(entry);
    }
  }
  return uniqueEntries;
}

export async function applyWorkspaceDirectoryLoad(path: string, options?: {
  mergeWithExisting?: boolean;
}): Promise<FileEntry[]> {
  const store = useSidebarStore.getState();
  const entries = await loadWorkspaceDirectory(path, {
    mergeWithExisting: options?.mergeWithExisting ?? true,
    existingFiles: store.files,
    expandedDirs: store.expandedDirs,
  });
  store.setFiles(entries);
  return entries;
}

export async function openWorkspaceDirectory(): Promise<string | null> {
  const selected = await open({
    directory: true,
    multiple: false,
    title: '选择工作区文件夹',
  });

  return selected ?? null;
}
