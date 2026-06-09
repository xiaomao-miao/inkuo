import { FolderOpen, RefreshCw, PanelLeftClose, PanelLeft, Search } from 'lucide-react';
import { useEffect } from 'react';
import { useWorkspaceSearch } from '../../hooks/useWorkspaceSearch';
import { useWorkspaceTree } from '../../hooks/useWorkspaceTree';
import { FileTree } from './FileTree';
import styles from './Sidebar.module.css';

export const Sidebar = () => {
  const {
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
  } = useWorkspaceTree();

  const { searchQuery, setSearchQuery, filteredFiles } = useWorkspaceSearch(files, workspacePath);

  useEffect(() => {
    if (workspacePath) {
      void refreshWorkspace();
    }
  }, [refreshWorkspace, workspacePath]);

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
            <Search size={14} className={styles.searchIcon} />
            <input
              type="text"
              placeholder="搜索文件..."
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              className={styles.searchInput}
            />
          </div>

          <div className={styles.fileTree}>
            {isLoading ? (
              <div className={styles.loading}>加载中...</div>
            ) : (
              <div role="tree" aria-label="文件树">
                <FileTree
                  entries={filteredFiles}
                  expandedDirs={expandedDirs}
                  selectedFile={selectedFile}
                  openTabs={openTabs}
                  onFileClick={handleFileClick}
                />
              </div>
            )}
          </div>
        </>
      )}
    </aside>
  );
};
