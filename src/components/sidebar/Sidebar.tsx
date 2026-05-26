import React, { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
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
import styles from './Sidebar.module.css';

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
    openTab,
  } = useSidebarStore();

  const [searchQuery, setSearchQuery] = useState('');
  const [isCollapsed, setIsCollapsed] = useState(false);

  const openWorkspace = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: '选择工作区文件夹',
      });
      
      if (selected) {
        setWorkspacePath(selected);
        loadDirectory(selected);
      }
    } catch (err) {
      console.error('Failed to open workspace:', err);
    }
  };

  const loadDirectory = async (path: string) => {
    setIsLoading(true);
    try {
      const entries = await invoke<FileEntry[]>('list_directory', { path });
      setFiles(entries);
    } catch (err) {
      console.error('Failed to load directory:', err);
    } finally {
      setIsLoading(false);
    }
  };

  const handleFileClick = async (entry: FileEntry) => {
    if (entry.is_dir) {
      const wasExpanded = expandedDirs.has(entry.path);
      toggleDir(entry.path);
      
      if (!wasExpanded) {
        // Loading children for the first time
        try {
          const childEntries = await invoke<FileEntry[]>('list_directory', { path: entry.path });
          // Add child entries to the file list, keeping the parent folder
          setFiles([...files, ...childEntries]);
        } catch (err) {
          console.error('Failed to load directory:', err);
        }
      }
    } else {
      // Open file in new tab
      openTab({
        id: entry.path,
        path: entry.path,
        name: entry.name,
        isDirty: false,
      });
    }
  };

  const renderFileTree = (entries: FileEntry[], depth: number = 0): React.ReactNode => {
    // Filter root level entries (entries that are direct children of workspace root)
    const rootEntries = entries.filter(e => {
      if (!workspacePath) return true;
      const relativePath = e.path.replace(workspacePath + '/', '');
      return !relativePath.includes('/');
    });
    
    const filteredEntries = searchQuery
      ? entries.filter(e => e.name.toLowerCase().includes(searchQuery.toLowerCase()))
      : rootEntries;

    const sortedEntries = [...filteredEntries].sort((a, b) => {
      if (a.is_dir && !b.is_dir) return -1;
      if (!a.is_dir && b.is_dir) return 1;
      return a.name.localeCompare(b.name);
    });

    return sortedEntries.map(entry => {
      const isExpanded = expandedDirs.has(entry.path);
      const isSelected = selectedFile === entry.path;
      const isOpen = openTabs.some(t => t.path === entry.path);

      // Get relative path depth
      const relativePath = entry.path.replace(workspacePath || '', '');
      const pathDepth = (relativePath.match(/\//g) || []).length;

      // Get children for this directory
      const children = files.filter(f => f.path.startsWith(entry.path + '/'));
      
      return (
        <div key={entry.path} className={styles.treeItem}>
          <div
            className={`${styles.fileItem} ${isSelected ? styles.selected : ''}`}
            onClick={() => handleFileClick(entry)}
            data-depth={Math.min(pathDepth, 4)}
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
          </div>
          {entry.is_dir && isExpanded && (
            <div className={styles.children}>
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
              renderFileTree(files)
            )}
          </div>
        </>
      )}
    </aside>
  );
};
