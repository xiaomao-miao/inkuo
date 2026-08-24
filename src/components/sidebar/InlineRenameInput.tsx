import {
  type ChangeEvent,
  type KeyboardEvent,
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react';
import { useNotificationStore, useSidebarStore } from '../../store';
import type { InlineEditState } from '../../store';
import {
  createFileEntry,
  loadDirectoryChildren,
  pathExists,
  renamePath,
} from '../../services/workspace';
import { runPathMutationWithOpenTabLifecycle } from '../../services/openTabLifecycle';
import {
  areFilePathsEqual,
  getBaseName,
  joinPath,
  normalizeDirPath,
} from '../../utils/path';
import { reportError } from '../../utils/errors';
import { detectFileKind } from '../../types';
import styles from './InlineRenameInput.module.css';

interface InlineRenameInputProps {
  state: InlineEditState;
  /** Depth in the tree, used to match the row padding of the treeItem it replaces. */
  depth: number;
}

function splitBasename(name: string): { stem: string; ext: string } {
  const dotIdx = name.lastIndexOf('.');
  if (dotIdx <= 0 || dotIdx === name.length - 1) {
    // No extension, or starts with a dot (hidden file), or dot at end.
    return { stem: name, ext: '' };
  }
  return { stem: name.slice(0, dotIdx), ext: name.slice(dotIdx) };
}

function validateName(name: string): string | null {
  if (!name.trim()) return '名称不能为空';
  if (name.includes('/') || name.includes('\\')) return '名称不能包含路径分隔符';
  if (name === '.' || name === '..') return '名称不能为 . 或 ..';
  return null;
}

export const InlineRenameInput = ({ state, depth }: InlineRenameInputProps) => {
  const [value, setValue] = useState(state.initialValue);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const cancelInlineEdit = useSidebarStore((s) => s.cancelInlineEdit);
  const evictCachedChildren = useSidebarStore((s) => s.evictCachedChildren);
  const setCachedChildren = useSidebarStore((s) => s.setCachedChildren);
  const openWorkspaceFile = useSidebarStore((s) => s.openWorkspaceFile);
  const pushNotification = useNotificationStore((s) => s.pushNotification);

  // Focus + select stem on mount.
  useEffect(() => {
    const input = inputRef.current;
    if (!input) return;
    input.focus();
    const { stem, ext } = splitBasename(state.initialValue);
    if (ext) {
      input.setSelectionRange(0, stem.length);
    } else {
      input.select();
    }
  }, [state.initialValue]);

  const submit = useCallback(async () => {
    const trimmed = value.trim();
    const validationError = validateName(trimmed);
    if (validationError) {
      setError(validationError);
      return;
    }
    if (state.mode === 'rename') {
      if (!state.originalPath) {
        cancelInlineEdit();
        return;
      }
      const originalName = getBaseName(state.originalPath);
      if (trimmed === originalName) {
        cancelInlineEdit();
        return;
      }
    } else {
      // create
      if (!state.createPayload) {
        cancelInlineEdit();
        return;
      }
    }
    if (submitting) return;
    setSubmitting(true);

    /**
     * Refetch the parent directory after a successful mutation so the new /
     * renamed entry is visible in the tree without waiting for the (possibly
     * 2-second-poll-interval) backend watcher to deliver its `Created` event.
     *
     * Done *before* `cancelInlineEdit()` so the inline input disappears at the
     * same time the new row materialises, not later.
     */
    const refetchParentAfterMutation = async (parentPath: string) => {
      const normalized = normalizeDirPath(parentPath);
      try {
        const children = await loadDirectoryChildren(normalized);
        setCachedChildren(normalized, children);
      } catch (err) {
        // Drop the (now-stale) cache entry so the tree isn't showing a
        // pre-mutation list. The next expand or watcher event will refetch.
        evictCachedChildren(normalized);
        reportError('inline-refetch-parent', err);
      }
    };

    try {
      if (state.mode === 'rename' && state.originalPath) {
        const originalPath = state.originalPath;
        const parent = state.parentPath;
        const target = joinPath(parent, trimmed);
        // Backend returns TargetExists if collision. Surface as inline error.
        try {
          if (
            !areFilePathsEqual(originalPath, target)
            && await pathExists(target)
          ) {
            setError('同名文件已存在');
            return;
          }
          const renamed = await runPathMutationWithOpenTabLifecycle({
            path: originalPath,
            includeDescendants: state.isDirectory === true,
            mutate: () => renamePath(originalPath, target),
          });
          if (!renamed) {
            cancelInlineEdit();
            return;
          }
        } catch (err) {
          const message = reportError('inline-rename', err);
          if (message.includes('Target') || message.toLowerCase().includes('exist')) {
            setError('同名文件已存在');
            return;
          }
          throw err;
        }
        await refetchParentAfterMutation(parent);
        cancelInlineEdit();
        pushNotification({ kind: 'success', title: '已重命名', message: trimmed });
        return;
      }

      // create mode
      if (state.createPayload) {
        try {
          const result = await createFileEntry(
            state.parentPath,
            trimmed,
            state.createPayload,
          );
          await refetchParentAfterMutation(state.parentPath);
          cancelInlineEdit();
          // Auto-open document files in the editor (markdown, office,
          // images, PDFs). Other plain-text files (code / config) fall
          // through to the editor by virtue of `Editor`'s default mode
          // and do not need a special-case auto-open here.
          const openableKinds = new Set([
            'markdown', 'word', 'excel', 'image', 'pdf',
          ]);
          const createdKind = detectFileKind(result.path);
          if (openableKinds.has(createdKind)) {
            openWorkspaceFile(result.path, {
              name: result.path.split(/[\\/]/).pop() ?? trimmed,
            });
          } else {
            pushNotification({ kind: 'success', title: '已创建', message: trimmed });
          }
        } catch (err) {
          const message = reportError('inline-create', err);
          if (message.toLowerCase().includes('exist')) {
            setError('同名文件已存在');
            return;
          }
          throw err;
        }
      }
    } catch (err) {
      pushNotification({
        kind: 'error',
        title: state.mode === 'rename' ? '重命名失败' : '创建失败',
        message: reportError('inline-submit', err),
      });
      cancelInlineEdit();
    } finally {
      setSubmitting(false);
    }
  }, [
    value,
    state,
    submitting,
    cancelInlineEdit,
    evictCachedChildren,
    setCachedChildren,
    openWorkspaceFile,
    pushNotification,
  ]);

  const handleKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      void submit();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancelInlineEdit();
    }
  };

  const handleChange = (e: ChangeEvent<HTMLInputElement>) => {
    setValue(e.target.value);
    if (error) setError(null);
  };

  const handleBlur = () => {
    if (submitting) return;
    void submit();
  };

  return (
    <div className={styles.inlineRow} data-depth={Math.min(depth, 4)}>
      <span className={styles.iconPlaceholder} />
      <input
        ref={inputRef}
        className={`${styles.input} ${error ? styles.invalid : ''}`}
        value={value}
        onChange={handleChange}
        onKeyDown={handleKey}
        onBlur={handleBlur}
        disabled={submitting}
        spellCheck={false}
        autoComplete="off"
        aria-invalid={error ? 'true' : 'false'}
      />
      {error ? (
        <span className={styles.errorHint}>{error}</span>
      ) : (
        <span className={styles.hint}>
          {state.mode === 'rename' ? 'Enter 提交' : 'Enter 创建'}
        </span>
      )}
    </div>
  );
};
