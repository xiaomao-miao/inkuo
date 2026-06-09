import React, { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  FolderOpen,
  ChevronRight,
  ChevronDown,
  FileText,
  File,
  Folder,
  FolderOpen as FolderOpenIcon,
  Search,
  RefreshCw,
  PanelLeftClose,
  PanelLeft
} from 'lucide-react';
import { useSidebarStore } from '../../store';
import type { FileEntry } from '../../types';
import { useDebouncedCallback } from '../../hooks/useDebouncedCallback';
import { applyWorkspaceDirectoryLoad, openWorkspaceDirectory } from '../../services/workspace';
import styles from './Sidebar.module.css';

// Helper function to get all parent folders for search results
function getEntriesWithParents(matchingEntries: FileEntry[], allEntries: FileEntry[]): FileEntry[] {
  const resultSet = new Set<string>();
  
  for (const entry of matchingEntries) {
    // Add the matching entry itself
    resultSet.add(entry.path);
    
    // Add all parent folders
    const parts = entry.path.split('/');
    for (let i = 1; i < parts.length; i++) {
      const parentPath = parts.slice(0, i).join('/');
      resultSet.add(parentPath);
    }
  }
  
  // Return all entries that are either matching or are parents of matching entries
  return allEntries.filter(e => resultSet.has(e.path));
}

export const Sidebar: React.FC = () => {
  const {
    workspacePath,
    files,
    expandedDirs,
    selectedFile,
    isLoading,
    openTabs,
    setWorkspacePath,
    setFiles,
    toggleDir,
    setIsLoading,
    openWorkspaceFile,
    addFileEntry,
    removeFileEntry,
  } = useSidebarStore();

  const [searchQuery, setSearchQuery] = useState('');
  const [isCollapsed, setIsCollapsed] = useState(false);

  // Load files when workspacePath is set (including on app startup)
  useEffect(() => {
    if (workspacePath) {
      loadDirectory(workspacePath);
    }
  }, [workspacePath]);

  // --- Incremental file update handlers ---

  const handleFileCreated = useCallback(async (changedPath: string) => {
    try {
      const entries = await invoke<FileEntry[]>('list_directory', {
        path: workspacePath,
      });
      const entry = entries.find(e => e.path === changedPath);
      if (!entry) return;

      const parentPath = entry.path.substring(
        workspacePath!.length + 1
      ).split('/').slice(0, -1).join('/');
      const parentDir = parentPath
        ? `${workspacePath}/${parentPath}`
        : workspacePath!;

      // Read expandedDirs fresh from the store to avoid stale closures
      const currentExpandedDirs = useSidebarStore.getState().expandedDirs;
      const needsChildRefresh = currentExpandedDirs.has(parentDir);

      if (needsChildRefresh) {
        // Re-fetch the parent's children to get accurate order + new entry
        const refreshedChildren = await invoke<FileEntry[]>('list_directory', {
          path: parentDir,
        });
        setFiles(prev => {
          // Remove old children of this parent
          const cleaned = prev.filter(
            f => !f.path.startsWith(parentDir + '/') || f.path === parentDir
          );
          return [...cleaned, ...refreshedChildren];
        });
      } else {
        // Parent is collapsed – just store the entry so it's visible when
        // the user expands the parent later
        addFileEntry(entry);
      }
    } catch (err) {
      console.error('[FileWatcher] Failed to handle file creation:', err);
    }
  }, [workspacePath, addFileEntry, setFiles]);

  const handleFileDeleted = useCallback((deletedPath: string) => {
    removeFileEntry(deletedPath);
  }, [removeFileEntry]);

  const handleFileModified = useCallback((_modifiedPath: string) => {
    // For file tree display, modification doesn't require any structural change.
    // The tree order/name stays the same; re-render is sufficient.
    // Force a re-render by touching files (same reference is fine – React
    // compares by identity, but the display only depends on `files` which
    // hasn't changed structurally here).
    setFiles(prev => [...prev]);
  }, [setFiles]);

  // Debounced top-level refresh – used as a safety net when incremental
  // logic cannot handle a situation (e.g. bulk rename outside the workspace).
  const debouncedFullRefresh = useDebouncedCallback(() => {
    const wp = useSidebarStore.getState().workspacePath;
    if (wp) {
      loadDirectory(wp, false);
    }
  }, 500);

  const handleFileChange = useCallback((event: { type: string; data: { path: string } }) => {
    const { type, data } = event;
    const changedPath = data?.path;
    if (!changedPath) return;

    // Ignore paths outside the workspace
    if (!changedPath.startsWith(workspacePath!)) return;

    switch (type) {
      case 'Created':
        handleFileCreated(changedPath);
        break;
      case 'Deleted':
        handleFileDeleted(changedPath);
        break;
      case 'Modified':
        handleFileModified(changedPath);
        break;
      default:
        // Unknown event type – fall back to full refresh
        debouncedFullRefresh();
        break;
    }
  }, [workspacePath, handleFileCreated, handleFileDeleted, handleFileModified, debouncedFullRefresh]);

  // Set up file watcher when workspacePath changes
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;

    const setupWatcher = async () => {
      if (!workspacePath) return;

      try {
        await invoke('watch_directory', { path: workspacePath });

        unlisten = await listen<{ type: string; data: { path: string } }>(
          'file-change',
          (ev) => handleFileChange(ev.payload)
        );
      } catch (err) {
        console.error('Failed to set up file watcher:', err);
      }
    };

    setupWatcher();

    return () => {
      if (unlisten) {
        unlisten();
      }
      invoke('unwatch_directory').catch(console.error);
    };
  }, [workspacePath, handleFileChange]);

  const openWorkspace = async () => {
    try {
      const selected = await openWorkspaceDirectory();

      if (selected) {
        setWorkspacePath(selected);
        await loadDirectory(selected, false);
      }
    } catch (err) {
      console.error('Failed to open workspace:', err);
    }
  };

  const loadDirectory = useCallback(async (path: string, mergeWithExisting: boolean = true) => {
    setIsLoading(true);
    try {
      await applyWorkspaceDirectoryLoad(path, { mergeWithExisting });
    } catch (err) {
      console.error('Failed to load directory:', err);
    } finally {
      setIsLoading(false);
    }
  }, [setIsLoading]);

  // 展开文件夹时添加子项，折叠时移除子项
  const handleFileClick = async (entry: FileEntry) => {
    const currentExpandedDirs = useSidebarStore.getState().expandedDirs;

    if (entry.is_dir) {
      const wasExpanded = currentExpandedDirs.has(entry.path);

      if (wasExpanded) {
        // 折叠：移除该目录的所有子项
        const folderPath = entry.path + '/';
        setFiles(prevFiles => prevFiles.filter(f => !f.path.startsWith(folderPath)));
      } else {
        // 展开：加载并添加子项
        try {
          const childEntries = await invoke<FileEntry[]>('list_directory', { path: entry.path });
          setFiles(prevFiles => [...prevFiles, ...childEntries]);
        } catch (err) {
          console.error('Failed to load directory:', err);
        }
      }

      toggleDir(entry.path);
    } else {
      // Open file in new tab
      openWorkspaceFile(entry.path, { name: entry.name });
    }
  };

  const renderFileTree = (entries: FileEntry[], depth: number = 0): React.ReactNode => {
    // Filter root level entries only when at depth 0
    // For nested calls (depth > 0), we already have the correct subset of entries
    const rootEntries = depth === 0 ? entries.filter(e => {
      if (!workspacePath) return true;
      const relativePath = e.path.replace(workspacePath + '/', '');
      return !relativePath.includes('/');
    }) : entries;
    
    // When searching, show all matching entries with their parent folders
    // When not searching, show only root level entries
    const filteredEntries = searchQuery
      ? entries.filter(e => e.name.toLowerCase().includes(searchQuery.toLowerCase()))
      : rootEntries;

    // When searching, we need to include parent folders of matching items
    const entriesToShow = searchQuery
      ? getEntriesWithParents(filteredEntries, entries)
      : filteredEntries;

    const sortedEntries = [...entriesToShow].sort((a, b) => {
      if (a.is_dir && !b.is_dir) return -1;
      if (!a.is_dir && b.is_dir) return 1;
      return a.name.localeCompare(b.name);
    });

    return sortedEntries.map(entry => {
      const isExpanded = expandedDirs.has(entry.path);
      const isSelected = selectedFile === entry.path;
      const isOpen = openTabs.some(t => t.path === entry.path);

      // Get children for this directory
      const children = files.filter(f => f.path.startsWith(entry.path + '/'));

      return (
        <div
          key={entry.path}
          role="treeitem"
          aria-expanded={entry.is_dir ? isExpanded : undefined}
          aria-selected={isSelected}
          aria-level={depth + 1}
          className={styles.treeItem}
        >
          <button
            type="button"
            className={`${styles.fileItem} ${isSelected ? styles.selected : ''}`}
            onClick={() => handleFileClick(entry)}
            data-depth={Math.min(depth, 4)}
          >
            {entry.is_dir ? (
              <>
                <span className={styles.chevron}>
                  {isExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                </span>
                <span className={styles.icon} data-type={isExpanded ? 'folder-open' : 'folder'}>
                  {isExpanded ? <FolderOpenIcon size={14} /> : <Folder size={14} />}
                </span>
              </>
            ) : (
              <>
                <span className={styles.chevronPlaceholder} />
                <span className={`${styles.icon} ${isSelected ? styles.iconActive : ''}`} data-type={entry.is_markdown ? 'markdown' : 'file'}>
                  {entry.is_markdown ? (
                    <FileText size={14} />
                  ) : (
                    <File size={14} />
                  )}
                </span>
              </>
            )}
            <span className={styles.fileName}>{entry.name}</span>
            {!entry.is_dir && isOpen && (
              <span className={`${styles.openIndicator} ${styles.openIndicatorActive}`}>
                ●
              </span>
            )}
          </button>
          {entry.is_dir && isExpanded && (
            <div role="group" className={styles.children}>
              {children.length > 0 ? (
                renderFileTree(children, depth + 1)
              ) : (
                <div className={styles.emptyFolder}>空文件夹</div>
              )}
            </div>
          )}
        </div>
      );
    });
  };

  if (isCollapsed) {
    return (
      <div className={styles.sidebarCollapsed}>
        <button
          className={styles.iconButton}
          onClick={() => setIsCollapsed(false)}
          title="展开侧边栏"
        >
          <PanelLeft size={18} />
        </button>
        <button
          className={styles.iconButton}
          onClick={openWorkspace}
          title="打开工作区"
        >
          <FolderOpen size={18} />
        </button>
      </div>
    );
  }

  return (
    <aside className={styles.sidebar}>
      <div className={styles.header}>
        <span className={styles.title}>资源管理器</span>
        <div className={styles.headerActions}>
          <button
            className={styles.iconButton}
            onClick={() => workspacePath && loadDirectory(workspacePath)}
            title="刷新"
            disabled={!workspacePath}
          >
            <RefreshCw size={14} />
          </button>
          <button
            className={styles.iconButton}
            onClick={() => setIsCollapsed(true)}
            title="收起侧边栏"
          >
            <PanelLeftClose size={14} />
          </button>
        </div>
      </div>

      <div className={styles.workspaceBar}>
        <button className={styles.openWorkspace} onClick={openWorkspace}>
          <FolderOpen size={14} />
          <span>{workspacePath ? '更改工作区' : '打开文件夹'}</span>
        </button>
      </div>

      {workspacePath && (
        <>
          <div className={styles.searchBox}>
            <Search size={14} className={styles.searchIcon} />
            <input
              type="text"
              placeholder="搜索文件..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className={styles.searchInput}
            />
          </div>

          <div className={styles.fileTree}>
            {isLoading ? (
              <div className={styles.loading}>加载中...</div>
            ) : (
              <div role="tree" aria-label="文件树">
                {renderFileTree(files)}
              </div>
            )}
          </div>
        </>
      )}
    </aside>
  );
};
