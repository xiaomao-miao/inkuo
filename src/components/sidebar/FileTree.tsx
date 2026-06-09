import {
  ChevronDown,
  ChevronRight,
  File,
  FileText,
  Folder,
  FolderOpen as FolderOpenIcon,
  Loader2,
} from 'lucide-react';
import type { OpenTab } from '../../store';
import type { FileEntry } from '../../types';
import styles from './Sidebar.module.css';

interface FileTreeProps {
  workspaceRoot: string;
  getChildren: (dirPath: string) => FileEntry[];
  expandedDirs: Set<string>;
  loadingDirs: Set<string>;
  selectedFile: string | null;
  openTabs: OpenTab[];
  onFileClick: (entry: FileEntry) => void | Promise<void>;
}

export const FileTree = ({
  workspaceRoot,
  getChildren,
  expandedDirs,
  loadingDirs,
  selectedFile,
  openTabs,
  onFileClick,
}: FileTreeProps) => {
  const rootChildren = getChildren(workspaceRoot);

  return (
    <div role="tree" aria-label="文件树">
      {rootChildren.length === 0 ? (
        <div className={styles.emptyFolder}>空文件夹</div>
      ) : (
        <FileTreeNode
          entry={{ name: workspaceRoot.split('/').pop() ?? '', path: workspaceRoot, is_dir: true, is_markdown: false }}
          getChildren={getChildren}
          expandedDirs={expandedDirs}
          loadingDirs={loadingDirs}
          selectedFile={selectedFile}
          openTabs={openTabs}
          onFileClick={onFileClick}
          depth={0}
          isRoot
        />
      )}
    </div>
  );
};

interface FileTreeNodeProps {
  entry: FileEntry;
  getChildren: (dirPath: string) => FileEntry[];
  expandedDirs: Set<string>;
  loadingDirs: Set<string>;
  selectedFile: string | null;
  openTabs: OpenTab[];
  onFileClick: (entry: FileEntry) => void | Promise<void>;
  depth: number;
  isRoot?: boolean;
}

const FileTreeNode = ({
  entry,
  getChildren,
  expandedDirs,
  loadingDirs,
  selectedFile,
  openTabs,
  onFileClick,
  depth,
  isRoot,
}: FileTreeNodeProps) => {
  const isExpanded = expandedDirs.has(entry.path);
  const isLoading = loadingDirs.has(entry.path);
  const isSelected = selectedFile === entry.path;
  const isOpen = openTabs.some((tab) => tab.path === entry.path);
  const children = getChildren(entry.path);

  const handleClick = () => {
    void onFileClick(entry);
  };

  if (!entry.is_dir) {
    return (
      <div
        key={entry.path}
        role="treeitem"
        aria-selected={isSelected}
        aria-level={depth + 1}
        className={styles.treeItem}
      >
        <button
          type="button"
          className={`${styles.fileItem} ${isSelected ? styles.selected : ''}`}
          onClick={handleClick}
          data-depth={Math.min(depth, 4)}
        >
          <span className={styles.chevronPlaceholder} />
          <span
            className={`${styles.icon} ${isSelected ? styles.iconActive : ''}`}
            data-type={entry.is_markdown ? 'markdown' : 'file'}
          >
            {entry.is_markdown ? <FileText size={14} /> : <File size={14} />}
          </span>
          <span className={styles.fileName}>{entry.name}</span>
          {isOpen && (
            <span className={`${styles.openIndicator} ${styles.openIndicatorActive}`}>
              ●
            </span>
          )}
        </button>
      </div>
    );
  }

  if (isRoot) {
    return (
      <>
        {children.length === 0 && !isLoading ? (
          <div className={styles.emptyFolder}>空文件夹</div>
        ) : (
          children.map((child) => (
            <FileTreeNode
              key={child.path}
              entry={child}
              getChildren={getChildren}
              expandedDirs={expandedDirs}
              loadingDirs={loadingDirs}
              selectedFile={selectedFile}
              openTabs={openTabs}
              onFileClick={onFileClick}
              depth={0}
            />
          ))
        )}
      </>
    );
  }

  return (
    <div
      key={entry.path}
      role="treeitem"
      aria-expanded={isExpanded}
      aria-selected={isSelected}
      aria-level={depth + 1}
      className={styles.treeItem}
    >
      <button
        type="button"
        className={`${styles.fileItem} ${isSelected ? styles.selected : ''}`}
        onClick={handleClick}
        data-depth={Math.min(depth, 4)}
      >
        <span className={styles.chevron}>
          {isLoading ? (
            <Loader2 size={14} className={styles.spin} />
          ) : isExpanded ? (
            <ChevronDown size={14} />
          ) : (
            <ChevronRight size={14} />
          )}
        </span>
        <span
          className={styles.icon}
          data-type={isExpanded ? 'folder-open' : 'folder'}
        >
          {isExpanded ? <FolderOpenIcon size={14} /> : <Folder size={14} />}
        </span>
        <span className={styles.fileName}>{entry.name}</span>
      </button>

      {isExpanded && (
        <div role="group" className={styles.children}>
          {isLoading ? (
            <div className={styles.loadingChildren}>
              <Loader2 size={12} className={styles.spin} />
              <span>加载中...</span>
            </div>
          ) : children.length === 0 ? (
            <div className={styles.emptyFolder}>空文件夹</div>
          ) : (
            children.map((child) => (
              <FileTreeNode
                key={child.path}
                entry={child}
                getChildren={getChildren}
                expandedDirs={expandedDirs}
                loadingDirs={loadingDirs}
                selectedFile={selectedFile}
                openTabs={openTabs}
                onFileClick={onFileClick}
                depth={depth + 1}
              />
            ))
          )}
        </div>
      )}
    </div>
  );
};
