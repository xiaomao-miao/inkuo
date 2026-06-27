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
  renamePath,
} from '../../services/workspace';
import { reportError } from '../../utils/errors';
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
  const invalidateCache = useSidebarStore((s) => s.invalidateCache);
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
      const originalName = state.originalPath.split('/').pop() ?? '';
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
    try {
      if (state.mode === 'rename' && state.originalPath) {
        const parent = state.parentPath;
        const target = `${parent}/${trimmed}`;
        // Backend returns TargetExists if collision. Surface as inline error.
        try {
          await renamePath(state.originalPath, target);
        } catch (err) {
          const message = reportError('inline-rename', err);
          if (message.includes('Target') || message.toLowerCase().includes('exist')) {
            setError('同名文件已存在');
            return;
          }
          throw err;
        }
        invalidateCache(parent);
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
          invalidateCache(state.parentPath);
          cancelInlineEdit();
          // Auto-open markdown/office files in the editor.
          const isMarkdown =
            state.createPayload.kind === 'file' && state.createPayload.extension === 'md';
          const isOffice =
            state.createPayload.kind === 'file' &&
            ['docx', 'xlsx'].includes(state.createPayload.extension);
          if (isMarkdown || isOffice) {
            openWorkspaceFile(result.path, {
              name: result.path.split('/').pop() ?? trimmed,
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
    invalidateCache,
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
