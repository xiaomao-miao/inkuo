import {
  FolderOpen,
  RefreshCw,
  PanelLeftClose,
  PanelLeft,
  Search,
  X,
  File,
  FileText,
  Folder,
  FolderOpen as FolderOpenIcon,
} from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { useWorkspaceSearch } from '../../hooks/useWorkspaceSearch';
import { useWorkspaceTree } from '../../hooks/useWorkspaceTree';
import { FileTree } from './FileTree';
import type { FileEntry } from '../../types';
import styles from './Sidebar.module.css';

interface SearchResultItemProps {
  entry: FileEntry;
  workspaceRoot: string;
  onClick: (entry: FileEntry) => void;
}

const SearchResultItem = ({ entry, workspaceRoot, onClick }: SearchResultItemProps) => {
  const relativePath = entry.path.slice(workspaceRoot.length + 1);
  const depth = relativePath.split('/').length - 1;

  const handleClick = () => {
    onClick(entry);
  };

  return (
    <button
      className={styles.searchResultItem}
      onClick={handleClick}
      style={{ paddingLeft: `${12 + depth * 12}px` }}
    >
      <span className={styles.chevronPlaceholder} />
      <span
        className={styles.icon}
        data-type={entry.is_dir ? 'folder' : entry.is_markdown ? 'markdown' : 'file'}
      >
        {entry.is_dir ? (
          entry.is_dir ? <Folder size={14} /> : <FolderOpenIcon size={14} />
        ) : entry.is_markdown ? (
          <FileText size={14} />
        ) : (
          <File size={14} />
        )}
      </span>
      <span className={styles.searchResultName}>{entry.name}</span>
      <span className={styles.searchResultPath}>{relativePath.split('/').slice(0, -1).join('/')}</span>
    </button>
  );
};

export const Sidebar = () => {
  const {
    workspacePath,
    expandedDirs,
    selectedFile,
    isLoading,
    loadingDirs,
    openTabs,
    isCollapsed,
    setIsCollapsed,
    openWorkspace,
    refreshWorkspace,
    handleFileClick,
    getChildren,
  } = useWorkspaceTree();

  const { searchQuery, setSearchQuery, searchResults, isSearching, clearSearch } =
    useWorkspaceSearch(workspacePath);

  const searchInputRef = useRef<HTMLInputElement>(null);
  const [isSearchFocused, setIsSearchFocused] = useState(false);

  useEffect(() => {
    if (workspacePath) {
      void refreshWorkspace();
    }
  }, [refreshWorkspace, workspacePath]);

  const handleSearchKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') {
      clearSearch();
      searchInputRef.current?.blur();
    } else if (e.key === 'Enter' && searchResults.length > 0) {
      handleFileClick(searchResults[0]);
      clearSearch();
    }
  };

  const handleSearchResultClick = async (entry: FileEntry) => {
    if (entry.is_dir) {
      handleFileClick(entry);
    } else {
      handleFileClick(entry);
    }
    clearSearch();
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
          onClick={() => void openWorkspace()}
          title="打开工作区"
        >
          <FolderOpen size={18} />
        </button>
      </div>
    );
  }

  const showSearchResults = searchQuery.trim().length > 0;

  return (
    <aside className={styles.sidebar}>
      <div className={styles.header}>
        <span className={styles.title}>资源管理器</span>
        <div className={styles.headerActions}>
          <button
            className={styles.iconButton}
            onClick={() => void refreshWorkspace()}
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
        <button className={styles.openWorkspace} onClick={() => void openWorkspace()}>
          <FolderOpen size={14} />
          <span>{workspacePath ? '更改工作区' : '打开文件夹'}</span>
        </button>
      </div>

      {workspacePath && (
        <>
          <div className={styles.searchBox}>
            <Search
              size={14}
              className={`${styles.searchIcon} ${isSearchFocused ? styles.searchIconActive : ''}`}
            />
            <input
              ref={searchInputRef}
              type="text"
              placeholder="搜索文件..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyDown={handleSearchKeyDown}
              onFocus={() => setIsSearchFocused(true)}
              onBlur={() => setIsSearchFocused(false)}
              className={styles.searchInput}
            />
            {searchQuery && (
              <button
                className={styles.searchClear}
                onClick={clearSearch}
                title="清除搜索"
              >
                <X size={12} />
              </button>
            )}
          </div>

          <div className={styles.fileTree}>
            {isLoading ? (
              <div className={styles.loading}>加载中...</div>
            ) : showSearchResults ? (
              <div className={styles.searchResults}>
                {isSearching ? (
                  <div className={styles.loading}>搜索中...</div>
                ) : searchResults.length === 0 ? (
                  <div className={styles.emptyFolder}>未找到匹配文件</div>
                ) : (
                  <>
                    <div className={styles.searchResultsHeader}>
                      找到 {searchResults.length} 个结果
                    </div>
                    {searchResults.map((entry) => (
                      <SearchResultItem
                        key={entry.path}
                        entry={entry}
                        workspaceRoot={workspacePath}
                        onClick={handleSearchResultClick}
                      />
                    ))}
                  </>
                )}
              </div>
            ) : (
              <FileTree
                workspaceRoot={workspacePath}
                getChildren={getChildren}
                expandedDirs={expandedDirs}
                loadingDirs={loadingDirs}
                selectedFile={selectedFile}
                openTabs={openTabs}
                onFileClick={handleFileClick}
              />
            )}
          </div>
        </>
      )}
    </aside>
  );
};
