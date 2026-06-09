import {
  ChevronDown,
  ChevronRight,
  File,
  FileText,
  Folder,
  FolderOpen as FolderOpenIcon,
} from 'lucide-react';
import type { OpenTab } from '../../store';
import type { FileEntry } from '../../types';
import styles from './Sidebar.module.css';

interface FileTreeProps {
  entries: FileEntry[];
  expandedDirs: Set<string>;
  selectedFile: string | null;
  openTabs: OpenTab[];
  onFileClick: (entry: FileEntry) => void | Promise<void>;
}

function getChildEntries(parent: FileEntry, entries: FileEntry[]) {
  return entries.filter((candidate) => {
    const relativePath = candidate.path.slice(parent.path.length + 1);
    return relativePath.length > 0 && !relativePath.includes('/');
  });
}

export const FileTree = ({
  entries,
  expandedDirs,
  selectedFile,
  openTabs,
  onFileClick,
}: FileTreeProps) => {
  const renderEntries = (visibleEntries: FileEntry[], depth = 0): React.ReactNode => {
    return visibleEntries.map((entry) => {
      const isExpanded = expandedDirs.has(entry.path);
      const isSelected = selectedFile === entry.path;
      const isOpen = openTabs.some((tab) => tab.path === entry.path);
      const children = entry.is_dir ? getChildEntries(entry, entries) : [];

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
            onClick={() => void onFileClick(entry)}
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
                <span
                  className={`${styles.icon} ${isSelected ? styles.iconActive : ''}`}
                  data-type={entry.is_markdown ? 'markdown' : 'file'}
                >
                  {entry.is_markdown ? <FileText size={14} /> : <File size={14} />}
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
                renderEntries(children, depth + 1)
              ) : (
                <div className={styles.emptyFolder}>空文件夹</div>
              )}
            </div>
          )}
        </div>
      );
    });
  };

  return <>{renderEntries(entries)}</>;
};
