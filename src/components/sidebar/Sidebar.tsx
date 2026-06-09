import React, { useState, useEffect, useCallback, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
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
import { useWorkspaceFileWatcher } from '../../hooks/useWorkspaceFileWatcher';
import { applyWorkspaceDirectoryLoad, openWorkspaceDirectory } from '../../services/workspace';
import styles from './Sidebar.module.css';

// Returns matched entries together with their ancestor folders so search results remain navigable.
function getEntriesWithParents(matchingEntries: FileEntry[], allEntries: FileEntry[]): FileEntry[] {
  const resultSet = new Set<string>();
  
  for (const entry of matchingEntries) {
    resultSet.add(entry.path);
    
    const parts = entry.path.split('/');
    for (let i = 1; i < parts.length; i++) {
      const parentPath = parts.slice(0, i).join('/');
      resultSet.add(parentPath);
    }
  }
  
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
    isDirExpanded,
  } = useSidebarStore();

  const [searchQuery, setSearchQuery] = useState('');
  const [isCollapsed, setIsCollapsed] = useState(false);

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

  const workspaceRootPath = useMemo(() => workspacePath ?? null, [workspacePath]);

  useEffect(() => {
    if (workspaceRootPath) {
      loadDirectory(workspaceRootPath);
    }
  }, [workspaceRootPath, loadDirectory]);

  // --- Incremental file update handlers ---

  const handleFileCreated = useCallback(async (changedPath: string) => {
    if (!workspaceRootPath) return;

    try {
      const entries = await invoke<FileEntry[]>('list_directory', {
        path: workspaceRootPath,
      });
      const entry = entries.find(e => e.path === changedPath);
      if (!entry) return;

      const parentPath = entry.path.substring(
        workspaceRootPath.length + 1
      ).split('/').slice(0, -1).join('/');
      const parentDir = parentPath
        ? `${workspaceRootPath}/${parentPath}`
        : workspaceRootPath;

      const needsChildRefresh = isDirExpanded(parentDir);

      if (needsChildRefresh) {
        const refreshedChildren = await invoke<FileEntry[]>('list_directory', {
          path: parentDir,
        });
        setFiles(prev => {
          const cleaned = prev.filter(
            f => !f.path.startsWith(parentDir + '/') || f.path === parentDir
          );
          return [...cleaned, ...refreshedChildren];
        });
      } else {
        addFileEntry(entry);
      }
    } catch (err) {
      console.error('[FileWatcher] Failed to handle file creation:', err);
    }
  }, [workspaceRootPath, addFileEntry, isDirExpanded, setFiles]);

  const handleFileDeleted = useCallback((deletedPath: string) => {
    removeFileEntry(deletedPath);
  }, [removeFileEntry]);

  const handleFileModified = useCallback((_modifiedPath: string) => {
    setFiles(prev => [...prev]);
  }, [setFiles]);

  // Falls back to a full tree refresh when incremental updates are insufficient.
  const debouncedFullRefresh = useDebouncedCallback(() => {
    if (workspaceRootPath) {
      loadDirectory(workspaceRootPath, false);
    }
  }, 500);

  const handleFileChange = useCallback((event: { type: string; data: { path: string } }) => {
    if (!workspaceRootPath) return;

    const { type, data } = event;
    const changedPath = data?.path;
    if (!changedPath || !changedPath.startsWith(workspaceRootPath)) return;

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
        debouncedFullRefresh();
        break;
    }
  }, [workspaceRootPath, handleFileCreated, handleFileDeleted, handleFileModified, debouncedFullRefresh]);

  useWorkspaceFileWatcher(workspaceRootPath, handleFileChange);

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

  const handleFileClick = async (entry: FileEntry) => {
    if (entry.is_dir) {
      const wasExpanded = isDirExpanded(entry.path);

      if (wasExpanded) {
        const folderPath = entry.path + '/';
        setFiles(prevFiles => prevFiles.filter(f => !f.path.startsWith(folderPath)));
      } else {
        try {
          const childEntries = await invoke<FileEntry[]>('list_directory', { path: entry.path });
          setFiles(prevFiles => [...prevFiles, ...childEntries]);
        } catch (err) {
          console.error('Failed to load directory:', err);
        }
      }

      toggleDir(entry.path);
    } else {
      openWorkspaceFile(entry.path, { name: entry.name });
    }
  };

  const renderFileTree = (entries: FileEntry[], depth: number = 0): React.ReactNode => {
    const rootEntries = depth === 0 ? entries.filter(e => {
      if (!workspacePath) return true;
      const relativePath = e.path.replace(workspacePath + '/', '');
      return !relativePath.includes('/');
    }) : entries;
    
    const filteredEntries = searchQuery
      ? entries.filter(e => e.name.toLowerCase().includes(searchQuery.toLowerCase()))
      : rootEntries;

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
