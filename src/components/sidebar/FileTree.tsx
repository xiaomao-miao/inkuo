import {
  ChevronDown,
  ChevronRight,
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
  Loader2,
  BookMarked,
  FolderX,
} from 'lucide-react';
import type { MouseEvent as ReactMouseEvent, ReactNode } from 'react';
import {
  useContextMenuStore,
  useSidebarStore,
  type OpenTab,
} from '../../store';
import type { FileEntry } from '../../types';
import { InlineRenameInput } from './InlineRenameInput';
import { getRelativePath, normalizeDirPath } from '../../utils/path';
import styles from './Sidebar.module.css';

interface FileTreeProps {
  workspaceRoot: string;
  getChildren: (dirPath: string) => FileEntry[];
  expandedDirs: Set<string>;
  loadingDirs: Set<string>;
  selectedFile: string | null;
  openTabs: OpenTab[];
  onFileClick: (entry: FileEntry) => void | Promise<void>;
  knowledgeSelectMode?: boolean;
  knowledgeCheckedPaths?: Set<string>;
  knowledgeMembers?: string[];
  onKnowledgeCheck?: (path: string, checked: boolean) => void;
}

/**
 * Root of the workspace file tree.
 *
 * The tree is fully driven by the directory cache:
 *   - `getChildren(path)` returns the cached child list for `path`
 *     (or `[]` if we have never read that directory).
 *   - The tree itself never fetches — `useWorkspaceTree` is the only
 *     component allowed to talk to the backend.
 *
 * Three rendering paths:
 *   1. Root cache is unknown      → show a single "loading…" placeholder
 *      until the hook finishes its initial fetch.
 *   2. Root cache is known but empty → show "空文件夹".
 *   3. Root cache has entries     → recurse into `TreeRow` for each child.
 */
export const FileTree = ({
  workspaceRoot,
  getChildren,
  expandedDirs,
  loadingDirs,
  selectedFile,
  openTabs,
  onFileClick,
  knowledgeSelectMode = false,
  knowledgeCheckedPaths = new Set(),
  knowledgeMembers = [],
  onKnowledgeCheck,
}: FileTreeProps) => {
  const isRootLoading = loadingDirs.has(normalizeDirPath(workspaceRoot));
  const rootChildren = getChildren(workspaceRoot);
  const rootKnown = rootChildren.length > 0 || !isRootLoading;
  // We only know "this folder is empty" once we've actually loaded it.
  // Before the first fetch lands we must not flash an "空文件夹" message.
  const rootIsEmpty = rootKnown && rootChildren.length === 0 && !isRootLoading;

  const inlineEdit = useSidebarStore((s) => s.inlineEdit);
  const isInlineCreateAtRoot =
    inlineEdit?.mode === 'create' && inlineEdit.parentPath === workspaceRoot;

  const handleRootContextMenu = (e: ReactMouseEvent<HTMLDivElement>) => {
    if (e.target !== e.currentTarget) return;
    e.preventDefault();
    useContextMenuStore.getState().open({
      kind: 'workspace',
      path: workspaceRoot,
      x: e.clientX,
      y: e.clientY,
    });
  };

  return (
    <div
      role="tree"
      aria-label="文件树"
      onContextMenu={handleRootContextMenu}
    >
      {isRootLoading && rootChildren.length === 0 ? (
        <RootLoading />
      ) : isInlineCreateAtRoot ? (
        <InlineRenameInput state={inlineEdit} depth={0} />
      ) : rootIsEmpty ? (
        <EmptyFolder />
      ) : (
        rootChildren.map((child) => (
          <TreeRow
            key={child.path}
            entry={child}
            getChildren={getChildren}
            expandedDirs={expandedDirs}
            loadingDirs={loadingDirs}
            selectedFile={selectedFile}
            openTabs={openTabs}
            onFileClick={onFileClick}
            depth={0}
            workspaceRoot={workspaceRoot}
            knowledgeSelectMode={knowledgeSelectMode}
            knowledgeCheckedPaths={knowledgeCheckedPaths}
            knowledgeMembers={knowledgeMembers}
            onKnowledgeCheck={onKnowledgeCheck}
          />
        ))
      )}
    </div>
  );
};

interface TreeRowProps {
  entry: FileEntry;
  getChildren: (dirPath: string) => FileEntry[];
  expandedDirs: Set<string>;
  loadingDirs: Set<string>;
  selectedFile: string | null;
  openTabs: OpenTab[];
  onFileClick: (entry: FileEntry) => void | Promise<void>;
  depth: number;
  workspaceRoot: string;
  knowledgeSelectMode: boolean;
  knowledgeCheckedPaths: Set<string>;
  knowledgeMembers: string[];
  onKnowledgeCheck?: (path: string, checked: boolean) => void;
}

/**
 * One row in the tree — either a file or a directory. The shape of the
 * rendered output differs (file vs chevron + children), but the metadata
 * (selection, knowledge-base membership, inline-edit slot, context menu)
 * is identical, so it lives here in one place.
 */
const TreeRow = ({
  entry,
  getChildren,
  expandedDirs,
  loadingDirs,
  selectedFile,
  openTabs,
  onFileClick,
  depth,
  workspaceRoot,
  knowledgeSelectMode,
  knowledgeCheckedPaths,
  knowledgeMembers,
  onKnowledgeCheck,
}: TreeRowProps) => {
  const isDir = entry.is_dir;
  const isExpanded = isDir && expandedDirs.has(entry.path);
  const isLoading = isDir && loadingDirs.has(entry.path);
  const isSelected = !isDir && selectedFile === entry.path;
  const isOpen = !isDir && openTabs.some((tab) => tab.path === entry.path);

  const children = isDir ? getChildren(entry.path) : [];

  const inlineEdit = useSidebarStore((s) => s.inlineEdit);
  const isRenamingThis =
    inlineEdit?.mode === 'rename' && inlineEdit.originalPath === entry.path;
  const isCreatingAsChild =
    inlineEdit?.mode === 'create' &&
    isDir &&
    inlineEdit.parentPath === entry.path;

  const relativePath = getRelativePath(workspaceRoot, entry.path);
  const isKnowledgeMember = knowledgeMembers.includes(relativePath);
  const isChecked = knowledgeCheckedPaths.has(relativePath);
  const showCheckbox = knowledgeSelectMode && !isDir;

  const handleClick = () => {
    void onFileClick(entry);
  };
  const handleContextMenu = (e: ReactMouseEvent<HTMLElement>) => {
    if (knowledgeSelectMode) return;
    e.preventDefault();
    e.stopPropagation();
    useContextMenuStore.getState().open({
      kind: 'entry',
      path: entry.path,
      x: e.clientX,
      y: e.clientY,
      entry,
    });
  };
  const handleCheckboxChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    e.stopPropagation();
    onKnowledgeCheck?.(relativePath, e.target.checked);
  };

  // Inline-rename replaces this row entirely. We still wrap it in the
  // treeItem container so indentation matches the row it stands in for.
  if (isRenamingThis && inlineEdit) {
    return (
      <div
        role="treeitem"
        aria-selected
        aria-level={depth + 1}
        className={styles.treeItem}
      >
        <InlineRenameInput state={inlineEdit} depth={depth} />
      </div>
    );
  }

  const headerButton = (
    <button
      type="button"
      className={`${styles.fileItem} ${isSelected ? styles.selected : ''}`}
      onClick={handleClick}
      onContextMenu={handleContextMenu}
      data-depth={Math.min(depth, 4)}
    >
      {showCheckbox ? (
        <input
          type="checkbox"
          className={styles.knowledgeCheckbox}
          checked={isChecked}
          onChange={handleCheckboxChange}
          onClick={(e) => e.stopPropagation()}
        />
      ) : isDir ? (
        <span className={styles.chevron}>
          {isLoading ? (
            <Loader2 size={14} className={styles.spin} />
          ) : isExpanded ? (
            <ChevronDown size={14} />
          ) : (
            <ChevronRight size={14} />
          )}
        </span>
      ) : (
        <span className={styles.chevronPlaceholder} />
      )}
      <span
        className={`${styles.icon} ${isSelected ? styles.iconActive : ''}`}
        data-type={iconType(entry)}
      >
        {iconFor(entry, isExpanded)}
      </span>
      <span className={styles.fileName}>{entry.name}</span>
      {!showCheckbox && isKnowledgeMember && (
        <span className={styles.knowledgeBadge}>
          <BookMarked size={12} />
        </span>
      )}
      {isOpen && (
        <span className={`${styles.openIndicator} ${styles.openIndicatorActive}`}>
          ●
        </span>
      )}
    </button>
  );

  if (!isDir) {
    return (
      <div
        role="treeitem"
        aria-selected={isSelected}
        aria-level={depth + 1}
        className={styles.treeItem}
      >
        {headerButton}
      </div>
    );
  }

  return (
    <div
      role="treeitem"
      aria-expanded={isExpanded}
      aria-selected={isSelected}
      aria-level={depth + 1}
      className={styles.treeItem}
    >
      {headerButton}
      {isExpanded && (
        <div role="group" className={styles.children}>
          {renderChildren({
            children,
            isLoading: !!isLoading,
            depth,
            getChildren,
            expandedDirs,
            loadingDirs,
            selectedFile,
            openTabs,
            onFileClick,
            workspaceRoot,
            knowledgeSelectMode,
            knowledgeCheckedPaths,
            knowledgeMembers,
            onKnowledgeCheck,
            inlineCreateSlot: isCreatingAsChild ? inlineEdit : null,
          })}
        </div>
      )}
    </div>
  );
};

interface RenderChildrenArgs {
  children: FileEntry[];
  isLoading: boolean;
  depth: number;
  inlineCreateSlot: ReturnType<typeof useSidebarStore.getState>['inlineEdit'];
  // Re-forwarded props — collapsed into one bag so the JSX stays readable.
  getChildren: TreeRowProps['getChildren'];
  expandedDirs: TreeRowProps['expandedDirs'];
  loadingDirs: TreeRowProps['loadingDirs'];
  selectedFile: TreeRowProps['selectedFile'];
  openTabs: TreeRowProps['openTabs'];
  onFileClick: TreeRowProps['onFileClick'];
  workspaceRoot: TreeRowProps['workspaceRoot'];
  knowledgeSelectMode: TreeRowProps['knowledgeSelectMode'];
  knowledgeCheckedPaths: TreeRowProps['knowledgeCheckedPaths'];
  knowledgeMembers: TreeRowProps['knowledgeMembers'];
  onKnowledgeCheck: TreeRowProps['onKnowledgeCheck'];
}

function renderChildren({
  children,
  isLoading,
  depth,
  inlineCreateSlot,
  ...rest
}: RenderChildrenArgs): ReactNode {
  if (isLoading && children.length === 0) {
    return (
      <div className={styles.loadingChildren}>
        <Loader2 size={12} className={styles.spin} />
        <span>加载中...</span>
      </div>
    );
  }

  if (children.length === 0 && !inlineCreateSlot) {
    return <EmptyFolder />;
  }

  const rows: ReactNode[] = [];
  if (inlineCreateSlot) {
    rows.push(
      <InlineRenameInput
        key="__inline-create__"
        state={inlineCreateSlot}
        depth={depth + 1}
      />,
    );
  }
  for (const child of children) {
    rows.push(
      <TreeRow
        key={child.path}
        entry={child}
        depth={depth + 1}
        {...rest}
      />,
    );
  }
  return rows;
}

function iconType(entry: FileEntry): string {
  if (entry.is_dir) return 'folder';
  if (entry.file_kind === 'markdown' || entry.file_kind === 'text') return 'markdown';
  if (entry.file_kind === 'image') return 'image';
  if (entry.file_kind === 'pdf') return 'pdf';
  if (entry.file_kind === 'code') return 'code';
  if (entry.file_kind === 'config') return 'config';
  if (entry.file_kind === 'data') return 'data';
  if (entry.file_kind === 'audio') return 'audio';
  if (entry.file_kind === 'video') return 'video';
  if (entry.file_kind === 'archive') return 'archive';
  return 'file';
}

function iconFor(entry: FileEntry, isExpanded: boolean): ReactNode {
  if (entry.is_dir) {
    return isExpanded ? <FolderOpenIcon size={14} /> : <Folder size={14} />;
  }
  switch (entry.file_kind) {
    case 'markdown':
    case 'text':
      return <FileText size={14} />;
    case 'image':
      return <FileImage size={14} />;
    case 'pdf':
      return <FileType size={14} />;
    case 'code':
    case 'config':
    case 'data':
      return <FileCode size={14} />;
    case 'audio':
      return <FileAudio size={14} />;
    case 'video':
      return <FileVideo size={14} />;
    case 'archive':
      return <FileArchive size={14} />;
    default:
      return <File size={14} />;
  }
}

const RootLoading = () => (
  <div className={styles.emptyFolder}>
    <Loader2 size={12} className={styles.spin} />
    <span>加载中…</span>
  </div>
);

const EmptyFolder = () => (
  <div className={styles.emptyFolder}>
    <FolderX size={12} />
    <span>空文件夹</span>
  </div>
);