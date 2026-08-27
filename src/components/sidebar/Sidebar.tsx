import {
  RefreshCw,
  PanelLeftClose,
  PanelLeft,
  Search,
  X,
  File,
  FileText,
  FileCode,
  FileImage,
  FileType,
  FileAudio,
  FileVideo,
  FileArchive,
  Folder,
  FolderOpen as FolderOpenIcon,
  BookMarked,
  Check,
  Minus,
} from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useWorkspaceSearch } from '../../hooks/useWorkspaceSearch';
import { useWorkspaceTree } from '../../hooks/useWorkspaceTree';
import { useContextMenuStore, useSidebarStore } from '../../store';
import { FileTree } from './FileTree';
import { ContextMenu } from './ContextMenu';
import type { FileEntry } from '../../types';
import { getRelativePath, normalizeDirPath } from '../../utils/path';
import { reloadCurrentWorkspace } from '../../services/workspace';
import styles from './Sidebar.module.css';
import { SkeletonGroup, SkeletonListItem } from '../common/Skeleton';

interface SearchResultItemProps {
  entry: FileEntry;
  workspaceRoot: string;
  onClick: (entry: FileEntry) => void;
}

const SearchResultItem = ({ entry, workspaceRoot, onClick }: SearchResultItemProps) => {
  const relativePath = getRelativePath(workspaceRoot, entry.path);
  const depth = relativePath ? relativePath.split('/').length - 1 : 0;

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
        data-type={
          entry.is_dir
            ? 'folder'
            : entry.file_kind === 'image'
              ? 'image'
              : entry.file_kind === 'pdf'
                ? 'pdf'
                : entry.file_kind === 'code' ||
                    entry.file_kind === 'config' ||
                    entry.file_kind === 'data'
                  ? 'code'
                  : entry.file_kind === 'audio'
                    ? 'audio'
                    : entry.file_kind === 'video'
                      ? 'video'
                      : entry.file_kind === 'archive'
                        ? 'archive'
                        : entry.is_markdown
                          ? 'markdown'
                          : 'file'
        }
      >
        {entry.is_dir ? (
          entry.is_dir ? <Folder size={14} /> : <FolderOpenIcon size={14} />
        ) : entry.file_kind === 'image' ? (
          <FileImage size={14} />
        ) : entry.file_kind === 'pdf' ? (
          <FileType size={14} />
        ) : entry.file_kind === 'audio' ? (
          <FileAudio size={14} />
        ) : entry.file_kind === 'video' ? (
          <FileVideo size={14} />
        ) : entry.file_kind === 'archive' ? (
          <FileArchive size={14} />
        ) : entry.file_kind === 'code' ||
            entry.file_kind === 'config' ||
            entry.file_kind === 'data' ? (
          <FileCode size={14} />
        ) : entry.is_markdown ? (
          <FileText size={14} />
        ) : (
          <File size={14} />
        )}
      </span>
      <span className={styles.searchResultName}>{entry.name}</span>
      <span className={styles.searchResultPath}>
        {relativePath ? normalizeDirPath(relativePath).split('/').slice(0, -1).join('/') : ''}
      </span>
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
    directoryErrors,
    openTabs,
    onDirectoryClick,
    getChildren,
    isDirectoryCached,
    refreshDirectory,
  } = useWorkspaceTree();

  const [isCollapsed, setIsCollapsed] = useState(false);

  const openWorkspaceFile = useSidebarStore((s) => s.openWorkspaceFile);

  const handleFileClick = useCallback(
    async (entry: FileEntry) => {
      if (entry.is_dir) {
        onDirectoryClick(entry);
      } else {
        openWorkspaceFile(entry.path, { name: entry.name });
      }
    },
    [onDirectoryClick, openWorkspaceFile],
  );

  const refreshWorkspace = useCallback(async () => {
    await reloadCurrentWorkspace();
  }, []);

  const {
    knowledgeSelectMode,
    knowledgeCheckedPaths,
    setKnowledgeSelectMode,
    checkAllKnowledgePaths,
    clearKnowledgeChecked,
    knowledgeBase,
  } = useSidebarStore();

  const { searchQuery, setSearchQuery, searchResults, isSearching, searchError, clearSearch } =
    useWorkspaceSearch(workspacePath);

  const searchInputRef = useRef<HTMLInputElement>(null);
  const [isSearchFocused, setIsSearchFocused] = useState(false);

  // Exit knowledge select mode on Escape
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && knowledgeSelectMode) {
        // Same as cancel - revert to initial state without syncing
        setKnowledgeSelectMode(false);
        clearKnowledgeChecked();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [knowledgeSelectMode, setKnowledgeSelectMode, clearKnowledgeChecked]);

  // First-time population of the root directory cache lives in
  // `useWorkspaceTree` as a `useEffect([workspaceRootPath])`, NOT here. The
  // previous incarnation of this file did mount its own refresh effect, but
  // its dependency list captured `expandedDirs` (transitively, via
  // `refreshWorkspace`'s `useCallback` deps), which made every
  // expand/collapse re-fire the full clearCache+isLoading+Skeleton swap.

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
      </div>
    );
  }

  const showSearchResults = searchQuery.trim().length > 0;

  return (
    <aside className={styles.sidebar}>
      <div className={styles.header}>
        <span className={styles.title}>
          {knowledgeSelectMode ? '知识库选择' : '资源管理器'}
        </span>
        <div className={styles.headerActions}>
          {!knowledgeSelectMode && (
            <button
              className={`${styles.iconButton} ${knowledgeBase?.members.length ? styles.iconButtonActive : ''}`}
              onClick={() => useSidebarStore.getState().toggleKnowledgeSelectMode()}
              title={knowledgeBase?.members.length ? `已选 ${knowledgeBase.members.length} 个知识库文件` : '知识库模式'}
              disabled={!workspacePath}
            >
              <BookMarked size={14} />
            </button>
          )}
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

      {/* Knowledge select mode toolbar */}
      {knowledgeSelectMode && (
        <KnowledgeSelectToolbar
          checkedPaths={knowledgeCheckedPaths}
          allMembersCount={knowledgeBase?.members.length ?? 0}
          onAdd={checkAllKnowledgePaths}
          onRemove={clearKnowledgeChecked}
          onCancel={() => setKnowledgeSelectMode(false)}
        />
      )}

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

          <div
            className={styles.fileTree}
            onContextMenu={(e) => {
              if (!workspacePath) return;
              // TreeRow calls stopPropagation on its own onContextMenu, so
              // this handler only fires for "empty area" clicks — the
              // .fileTree padding, the gap between rows, or inside an
              // EmptyFolder/Skeleton placeholder. Showing the workspace
              // menu here matches what users expect from IDE file trees.
              e.preventDefault();
              useContextMenuStore.getState().open({
                kind: 'workspace',
                path: workspacePath,
                x: e.clientX,
                y: e.clientY,
              });
            }}
          >
            {isLoading ? (
              <SkeletonGroup className={styles.skeletonContainer}>
                <SkeletonListItem />
                <SkeletonListItem />
                <SkeletonListItem />
                <SkeletonListItem dense />
                <SkeletonListItem dense />
              </SkeletonGroup>
            ) : showSearchResults ? (
              <div className={styles.searchResults}>
                {isSearching ? (
                  <SkeletonGroup className={styles.skeletonContainer}>
                    <SkeletonListItem dense />
                    <SkeletonListItem dense />
                    <SkeletonListItem dense />
                  </SkeletonGroup>
                ) : searchError ? (
                  <div className={styles.emptyFolder} role="alert">{searchError}</div>
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
                isDirectoryCached={isDirectoryCached}
                expandedDirs={expandedDirs}
                loadingDirs={loadingDirs}
                directoryErrors={directoryErrors}
                selectedFile={selectedFile}
                openTabs={openTabs}
                onFileClick={handleFileClick}
                onRetryDirectory={refreshDirectory}
                knowledgeSelectMode={knowledgeSelectMode}
                knowledgeCheckedPaths={knowledgeCheckedPaths}
                knowledgeMembers={knowledgeBase?.members ?? []}
                onKnowledgeCheck={(path, checked) => {
                  useSidebarStore.getState().setKnowledgeChecked(path, checked);
                }}
              />
            )}
          </div>
        </>
      )}

      <ContextMenu />
    </aside>
  );
};

// Knowledge select mode toolbar component
interface KnowledgeSelectToolbarProps {
  checkedPaths: Set<string>;
  allMembersCount: number;
  onAdd: (paths: string[]) => void;
  onRemove: () => void;
  onCancel: () => void;
}

function KnowledgeSelectToolbar({
  checkedPaths,
  allMembersCount,
  onAdd,
  onRemove,
  onCancel,
}: KnowledgeSelectToolbarProps) {
  const checkedCount = checkedPaths.size;

  return (
    <div className={styles.knowledgeToolbar}>
      <span className={styles.knowledgeToolbarCount}>
        已选 {checkedCount} 个
        {allMembersCount > 0 && `（知识库已有 ${allMembersCount} 个）`}
      </span>
      <div className={styles.knowledgeToolbarActions}>
        <button
          className={styles.knowledgeToolbarBtn}
          onClick={() => onAdd(Array.from(checkedPaths))}
          title="全选"
        >
          <Check size={12} />
          <span>全选</span>
        </button>
        <button
          className={styles.knowledgeToolbarBtn}
          onClick={onRemove}
          title="取消全选"
        >
          <Minus size={12} />
          <span>取消</span>
        </button>
        <button
          className={`${styles.knowledgeToolbarBtn} ${styles.knowledgeToolbarBtnCancel}`}
          onClick={onCancel}
          title="退出选择模式（Esc）"
        >
          <X size={12} />
          <span>退出</span>
        </button>
      </div>
    </div>
  );
}
