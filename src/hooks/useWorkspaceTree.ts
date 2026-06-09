import { useCallback, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useSidebarStore } from '../store';
import type { FileEntry } from '../types';
import { useDebouncedCallback } from './useDebouncedCallback';
import { useWorkspaceFileWatcher } from './useWorkspaceFileWatcher';
import { applyWorkspaceDirectoryLoad, openWorkspaceDirectory } from '../services/workspace';
import { reportError } from '../utils/errors';

interface FileChangeEvent {
  type: string;
  data: { path: string };
}

interface UseWorkspaceTreeResult {
  workspacePath: string | null;
  files: FileEntry[];
  expandedDirs: Set<string>;
  selectedFile: string | null;
  isLoading: boolean;
  openTabs: ReturnType<typeof useSidebarStore.getState>['openTabs'];
  isCollapsed: boolean;
  setIsCollapsed: React.Dispatch<React.SetStateAction<boolean>>;
  openWorkspace: () => Promise<void>;
  refreshWorkspace: () => Promise<void>;
  handleFileClick: (entry: FileEntry) => Promise<void>;
}

export function useWorkspaceTree(): UseWorkspaceTreeResult {
  const workspacePath = useSidebarStore((state) => state.workspacePath);
  const files = useSidebarStore((state) => state.files);
  const expandedDirs = useSidebarStore((state) => state.expandedDirs);
  const selectedFile = useSidebarStore((state) => state.selectedFile);
  const isLoading = useSidebarStore((state) => state.isLoading);
  const openTabs = useSidebarStore((state) => state.openTabs);
  const setWorkspacePath = useSidebarStore((state) => state.setWorkspacePath);
  const setFiles = useSidebarStore((state) => state.setFiles);
  const toggleDir = useSidebarStore((state) => state.toggleDir);
  const setIsLoading = useSidebarStore((state) => state.setIsLoading);
  const openWorkspaceFile = useSidebarStore((state) => state.openWorkspaceFile);
  const addFileEntry = useSidebarStore((state) => state.addFileEntry);
  const removeFileEntry = useSidebarStore((state) => state.removeFileEntry);
  const removeDescendants = useSidebarStore((state) => state.removeDescendants);
  const isDirExpanded = useSidebarStore((state) => state.isDirExpanded);

  const [isCollapsed, setIsCollapsed] = useState(false);

  const workspaceRootPath = useMemo(() => workspacePath ?? null, [workspacePath]);

  const loadDirectory = useCallback(async (path: string, mergeWithExisting = true) => {
    setIsLoading(true);
    try {
      await applyWorkspaceDirectoryLoad(path, { mergeWithExisting });
    } catch (err) {
      reportError('workspace-tree-load-directory', err);
    } finally {
      setIsLoading(false);
    }
  }, [setIsLoading]);

  const refreshWorkspace = useCallback(async () => {
    if (!workspaceRootPath) return;
    await loadDirectory(workspaceRootPath);
  }, [loadDirectory, workspaceRootPath]);

  const handleFileCreated = useCallback(async (changedPath: string) => {
    if (!workspaceRootPath) return;

    try {
      const entries = await invoke<FileEntry[]>('list_directory', {
        path: workspaceRootPath,
      });
      const entry = entries.find((candidate) => candidate.path === changedPath);
      if (!entry) return;

      const parentPath = entry.path.substring(workspaceRootPath.length + 1).split('/').slice(0, -1).join('/');
      const parentDir = parentPath ? `${workspaceRootPath}/${parentPath}` : workspaceRootPath;

      if (isDirExpanded(parentDir)) {
        const refreshedChildren = await invoke<FileEntry[]>('list_directory', { path: parentDir });
        setFiles((prev) => {
          const cleaned = prev.filter((file) => !file.path.startsWith(`${parentDir}/`) || file.path === parentDir);
          return [...cleaned, ...refreshedChildren];
        });
        return;
      }

      addFileEntry(entry);
    } catch (err) {
      reportError('workspace-tree-handle-file-created', err);
    }
  }, [addFileEntry, isDirExpanded, setFiles, workspaceRootPath]);

  const handleFileDeleted = useCallback((deletedPath: string) => {
    removeFileEntry(deletedPath);
  }, [removeFileEntry]);

  const handleFileModified = useCallback(() => {
    setFiles((prev) => [...prev]);
  }, [setFiles]);

  const debouncedFullRefresh = useDebouncedCallback(() => {
    if (workspaceRootPath) {
      loadDirectory(workspaceRootPath, false);
    }
  }, 500);

  const handleFileChange = useCallback((event: FileChangeEvent) => {
    if (!workspaceRootPath) return;

    const changedPath = event.data?.path;
    if (!changedPath || !changedPath.startsWith(workspaceRootPath)) return;

    switch (event.type) {
      case 'Created':
        void handleFileCreated(changedPath);
        break;
      case 'Deleted':
        handleFileDeleted(changedPath);
        break;
      case 'Modified':
        handleFileModified();
        break;
      default:
        debouncedFullRefresh();
        break;
    }
  }, [debouncedFullRefresh, handleFileCreated, handleFileDeleted, handleFileModified, workspaceRootPath]);

  useWorkspaceFileWatcher(workspaceRootPath, handleFileChange);

  const openWorkspace = useCallback(async () => {
    try {
      const selected = await openWorkspaceDirectory();
      if (!selected) return;

      setWorkspacePath(selected);
      await loadDirectory(selected, false);
    } catch (err) {
      reportError('workspace-tree-open-workspace', err);
    }
  }, [loadDirectory, setWorkspacePath]);

  const handleFileClick = useCallback(async (entry: FileEntry) => {
    if (entry.is_dir) {
      const wasExpanded = isDirExpanded(entry.path);

      if (wasExpanded) {
        removeDescendants(entry.path);
      } else {
        try {
          const childEntries = await invoke<FileEntry[]>('list_directory', { path: entry.path });
          setFiles((prevFiles) => [...prevFiles, ...childEntries]);
        } catch (err) {
          reportError('workspace-tree-load-children', err);
        }
      }

      toggleDir(entry.path);
      return;
    }

    openWorkspaceFile(entry.path, { name: entry.name });
  }, [isDirExpanded, openWorkspaceFile, removeDescendants, setFiles, toggleDir]);

  return {
    workspacePath,
    files,
    expandedDirs,
    selectedFile,
    isLoading,
    openTabs,
    isCollapsed,
    setIsCollapsed,
    openWorkspace,
    refreshWorkspace,
    handleFileClick,
  };
}
