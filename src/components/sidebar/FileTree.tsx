import {
  ChevronDown,
  ChevronRight,
  File,
  FileText,
  Folder,
  FolderOpen as FolderOpenIcon,
  Loader2,
  BookMarked,
  FolderX,
} from 'lucide-react';
import type { MouseEvent as ReactMouseEvent } from 'react';
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
  /** Whether knowledge base selection mode is active */
  knowledgeSelectMode?: boolean;
  /** Paths checked in knowledge selection mode */
  knowledgeCheckedPaths?: Set<string>;
  /** Paths that are already knowledge base members (relative to workspace) */
  knowledgeMembers?: string[];
  /** Callback when a path is checked/unchecked */
  onKnowledgeCheck?: (path: string, checked: boolean) => void;
}

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
  const rootChildren = getChildren(workspaceRoot);

  const handleRootContextMenu = (e: ReactMouseEvent<HTMLDivElement>) => {
    // Only fire when the click landed on the empty area of the tree itself,
    // not on a child row (which stops propagation in its own handler).
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
      {rootChildren.length === 0 ? (
        <div className={styles.emptyFolder}><FolderX size={12} /> 空文件夹</div>
      ) : (
        <FileTreeNode
          entry={{
            name: workspaceRoot.split('/').pop() ?? '',
            path: workspaceRoot,
            is_dir: true,
            is_markdown: false,
          }}
          getChildren={getChildren}
          expandedDirs={expandedDirs}
          loadingDirs={loadingDirs}
          selectedFile={selectedFile}
          openTabs={openTabs}
          onFileClick={onFileClick}
          depth={0}
          isRoot
          knowledgeSelectMode={knowledgeSelectMode}
          knowledgeCheckedPaths={knowledgeCheckedPaths}
          knowledgeMembers={knowledgeMembers}
          onKnowledgeCheck={onKnowledgeCheck}
          workspaceRoot={workspaceRoot}
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
  knowledgeSelectMode?: boolean;
  knowledgeCheckedPaths?: Set<string>;
  knowledgeMembers?: string[];
  onKnowledgeCheck?: (path: string, checked: boolean) => void;
  workspaceRoot?: string;
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
  knowledgeSelectMode = false,
  knowledgeCheckedPaths = new Set(),
  knowledgeMembers = [],
  onKnowledgeCheck,
  workspaceRoot = '',
}: FileTreeNodeProps) => {
  const isExpanded = expandedDirs.has(entry.path);
  const isLoading = loadingDirs.has(entry.path);
  const isSelected = selectedFile === entry.path;
  const isOpen = openTabs.some((tab) => tab.path === entry.path);
  const children = getChildren(entry.path);

  // Inline-edit slot: either renaming an existing entry, or creating a new
  // one as a child of this node (for directories) / as a sibling (for the
  // workspace root, which is handled via parent path).
  const inlineEdit = useSidebarStore((s) => s.inlineEdit);
  const inlineEditForThisEntry =
    inlineEdit &&
    ((inlineEdit.mode === 'rename' && inlineEdit.originalPath === entry.path) ||
      (inlineEdit.mode === 'create' &&
        !isRoot &&
        inlineEdit.parentPath === entry.path &&
        !entry.is_dir));

  // For directory rows in 'create' mode, render the input as a synthetic
  // first-child instead of the directory's own row.
  const inlineEditAsChild =
    inlineEdit &&
    inlineEdit.mode === 'create' &&
    !isRoot &&
    entry.is_dir &&
    inlineEdit.parentPath === entry.path;

  // Compute relative path for knowledge base matching.
  // Knowledge-base members are stored as workspace-relative paths by the
  // Rust backend (see `DocScanner::strip_prefix`). Both sides need to be
  // normalized so the comparison is correct on Windows, where `entry.path`
  // is `E:\文档\file.md` and a stored member would also have `\`.
  const relativePath = workspaceRoot
    ? getRelativePath(workspaceRoot, entry.path)
    : normalizeDirPath(entry.path);
  const isKnowledgeMember = knowledgeMembers.includes(relativePath);
  const isChecked = knowledgeCheckedPaths.has(relativePath);
  const showCheckbox = knowledgeSelectMode && !entry.is_dir;

  const handleClick = () => {
    void onFileClick(entry);
  };

  const handleCheckboxChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    e.stopPropagation();
    onKnowledgeCheck?.(relativePath, e.target.checked);
  };

  const handleContextMenu = (e: ReactMouseEvent<HTMLElement>) => {
    if (knowledgeSelectMode) return; // selection mode has its own UX
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

  // If this specific row is in rename mode, render the input in place of
  // the static row. We still keep it inside the treeItem wrapper so the
  // surrounding chrome (checkbox, indentation) stays consistent.
  if (inlineEditForThisEntry && inlineEdit) {
    return (
      <div
        key={entry.path}
        role="treeitem"
        aria-selected
        aria-level={depth + 1}
        className={styles.treeItem}
      >
        <InlineRenameInput state={inlineEdit} depth={depth} />
      </div>
    );
  }

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
          onContextMenu={handleContextMenu}
          data-depth={Math.min(depth, 4)}
        >
          {showCheckbox && (
            <input
              type="checkbox"
              className={styles.knowledgeCheckbox}
              checked={isChecked}
              onChange={handleCheckboxChange}
              onClick={(e) => e.stopPropagation()}
            />
          )}
          {!showCheckbox && <span className={styles.chevronPlaceholder} />}
          <span
            className={`${styles.icon} ${isSelected ? styles.iconActive : ''}`}
            data-type={entry.is_markdown ? 'markdown' : 'file'}
          >
            {entry.is_markdown ? <FileText size={14} /> : <File size={14} />}
          </span>
          <span className={styles.fileName}>{entry.name}</span>
          {isKnowledgeMember && !showCheckbox && (
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
      </div>
    );
  }

  if (isRoot) {
    // Workspace-root: also render inline-edit row as a virtual child if the
    // user picked "New file / New folder" at the root.
    return (
      <>
        {inlineEdit && inlineEdit.mode === 'create' && inlineEdit.parentPath === entry.path ? (
          <InlineRenameInput
            key={`__inline-create__`}
            state={inlineEdit}
            depth={0}
          />
        ) : null}
        {children.length === 0 && !isLoading && !inlineEditAsChild ? (
          <div className={styles.emptyFolder}><FolderX size={12} /> 空文件夹</div>
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
              knowledgeSelectMode={knowledgeSelectMode}
              knowledgeCheckedPaths={knowledgeCheckedPaths}
              knowledgeMembers={knowledgeMembers}
              onKnowledgeCheck={onKnowledgeCheck}
              workspaceRoot={workspaceRoot}
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
        onContextMenu={handleContextMenu}
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
          ) : (
            <>
              {inlineEditAsChild && inlineEdit ? (
                <InlineRenameInput
                  key="__inline-create__"
                  state={inlineEdit}
                  depth={depth + 1}
                />
              ) : null}
              {children.length === 0 && !inlineEditAsChild ? (
                <div className={styles.emptyFolder}><FolderX size={12} /> 空文件夹</div>
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
                    knowledgeSelectMode={knowledgeSelectMode}
                    knowledgeCheckedPaths={knowledgeCheckedPaths}
                    knowledgeMembers={knowledgeMembers}
                    onKnowledgeCheck={onKnowledgeCheck}
                    workspaceRoot={workspaceRoot}
                  />
                ))
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
};
