/**
 * SnapshotRestoreDialog — confirmation dialog that lists the files that
 * will change when restoring a snapshot, then performs the restore on
 * confirm.
 *
 * For text files that will be "modified" we fetch the current contents
 * from disk and the snapshot's stored copy, then ask the Rust `compute_diff`
 * command to render hunks so the user can see exactly what's changing.
 */

import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ChevronDown, ChevronRight, FileText, File as FileIcon, RotateCcw, X } from 'lucide-react';
import { useNotificationStore } from '../../store/notificationStore';
import { useSidebarStore } from '../../store/sidebarStore';
import {
  previewRestore,
  restoreSnapshot,
  type FileDiffPreview,
  type SnapshotIndexEntry,
} from '../../services/snapshots';
import { reportError } from '../../utils/errors';
import styles from './Snapshots.module.css';

interface SnapshotRestoreDialogProps {
  snapshot: SnapshotIndexEntry;
  onClose: () => void;
  onRestored: () => void;
}

interface DiffChange {
  tag: 'Equal' | 'Insert' | 'Delete' | 'Replace';
  old_line: number | null;
  new_line: number | null;
  content: string;
}

interface DiffHunk {
  id: string;
  old_range: { start_line: number; end_line: number };
  new_range: { start_line: number; end_line: number };
  changes: DiffChange[];
  summary: string;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function changeLabel(kind: FileDiffPreview['changeKind']): string {
  switch (kind) {
    case 'added': return '新增';
    case 'modified': return '修改';
    case 'deleted': return '删除';
    case 'unchanged': return '不变';
  }
}

export const SnapshotRestoreDialog = ({
  snapshot,
  onClose,
  onRestored,
}: SnapshotRestoreDialogProps) => {
  const workspacePath = useSidebarStore((s) => s.workspacePath);
  const pushNotification = useNotificationStore((s) => s.pushNotification);
  const [previews, setPreviews] = useState<FileDiffPreview[] | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [hunksByFile, setHunksByFile] = useState<Record<string, DiffHunk[]>>({});
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [isRestoring, setIsRestoring] = useState(false);

  useEffect(() => {
    if (!workspacePath) return;
    let cancelled = false;
    (async () => {
      try {
        const result = await previewRestore(workspacePath, snapshot.id);
        if (!cancelled) {
          setPreviews(result);
          // Auto-expand only the first modified text file
          const first = result.find(
            (p) => p.changeKind === 'modified' && !p.isBinary
          );
          if (first) setExpanded(new Set([first.absPath]));
        }
      } catch (err) {
        if (!cancelled) {
          setPreviewError(err instanceof Error ? err.message : String(err));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [workspacePath, snapshot.id]);

  const counts = useMemo(() => {
    const out = { added: 0, modified: 0, deleted: 0, unchanged: 0 };
    for (const p of previews ?? []) {
      out[p.changeKind]++;
    }
    return out;
  }, [previews]);

  const toggleExpand = useCallback(async (file: FileDiffPreview) => {
    if (file.isBinary || file.changeKind !== 'modified') {
      setExpanded((prev) => {
        const next = new Set(prev);
        if (next.has(file.absPath)) next.delete(file.absPath);
        else next.add(file.absPath);
        return next;
      });
      return;
    }
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(file.absPath)) {
        next.delete(file.absPath);
      } else {
        next.add(file.absPath);
        // Lazily compute diff
        if (!hunksByFile[file.absPath] && workspacePath) {
          void (async () => {
            try {
              const snapshotContent = await readSnapshotFileBytes(
                workspacePath,
                snapshot.id,
                file.relPath
              );
              const currentContent = await readCurrentFileText(file.absPath);
              if (snapshotContent == null || currentContent == null) return;
              const result = await invoke<{ hunks?: DiffHunk[] }>('compute_diff', {
                oldText: currentContent,
                newText: snapshotContent,
              });
              setHunksByFile((prevHunks) => ({
                ...prevHunks,
                [file.absPath]: result.hunks ?? [],
              }));
            } catch (err) {
              reportError(`compute_diff for ${file.relPath}`, err);
            }
          })();
        }
      }
      return next;
    });
  }, [hunksByFile, snapshot.id, workspacePath]);

  const handleRestore = useCallback(async () => {
    if (!workspacePath) return;
    setIsRestoring(true);
    try {
      await restoreSnapshot(workspacePath, snapshot.id);
      onRestored();
    } catch (err) {
      pushNotification({
        kind: 'error',
        title: '回滚失败',
        message: err instanceof Error ? err.message : String(err),
      });
    } finally {
      setIsRestoring(false);
    }
  }, [workspacePath, snapshot.id, onRestored, pushNotification]);

  const totalChanges = counts.added + counts.modified + counts.deleted;

  return (
    <div className={styles.dialogBackdrop} onClick={onClose}>
      <div
        className={styles.dialog}
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
      >
        <div className={styles.dialogHeader}>
          <h3 className={styles.dialogTitle}>
            <RotateCcw size={16} />
            回滚到 {new Date(snapshot.createdAt).toLocaleString('zh-CN')}
          </h3>
          <button className={styles.iconButton} onClick={onClose} title="关闭">
            <X size={14} />
          </button>
        </div>

        <div className={styles.dialogBody}>
          {previewError ? (
            <div className={styles.error}>{previewError}</div>
          ) : previews == null ? (
            <div className={styles.loading}>加载预览…</div>
          ) : previews.length === 0 ? (
            <div className={styles.empty}>该快照不包含任何文件。</div>
          ) : (
            <>
              <div className={styles.dialogSummary}>
                将还原 <strong>{previews.length}</strong> 个文件。
                {counts.added > 0 && <> <strong>{counts.added}</strong> 个新增。</>}
                {counts.modified > 0 && <> <strong>{counts.modified}</strong> 个修改。</>}
                {counts.deleted > 0 && <> <strong>{counts.deleted}</strong> 个删除。</>}
                {counts.unchanged > 0 && <> <strong>{counts.unchanged}</strong> 个不变。</>}
              </div>

              <div className={styles.dialogFileList}>
                {previews.map((file) => {
                  const isOpen = expanded.has(file.absPath);
                  return (
                    <div key={file.absPath} className={styles.dialogFile}>
                      <button
                        className={styles.dialogFileHeader}
                        onClick={() => void toggleExpand(file)}
                      >
                        {isOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                        {file.isBinary ? <FileIcon size={12} /> : <FileText size={12} />}
                        <span className={styles.dialogFilePath}>{file.relPath}</span>
                        <span
                          className={styles.changeBadge}
                          data-kind={file.changeKind}
                        >
                          {changeLabel(file.changeKind)}
                        </span>
                        <span className={styles.dialogFileBytes}>
                          {formatBytes(file.snapshotBytes)} → {formatBytes(file.diskBytesNow)}
                        </span>
                      </button>
                      {isOpen && !file.isBinary && file.changeKind === 'modified' && (
                        <div className={styles.diffContainer}>
                          {hunksByFile[file.absPath] ? (
                            <DiffView hunks={hunksByFile[file.absPath]} />
                          ) : (
                            <div className={styles.loadingInline}>计算 diff…</div>
                          )}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </>
          )}
        </div>

        <div className={styles.dialogFooter}>
          <button
            className={styles.secondaryBtn}
            onClick={onClose}
            disabled={isRestoring}
          >
            取消
          </button>
          <button
            className={styles.primaryBtn}
            onClick={handleRestore}
            disabled={isRestoring || previews == null}
            title={
              previews == null
                ? '正在加载预览…'
                : totalChanges === 0
                  ? '当前工作区与该快照完全一致（点击仍会执行）'
                  : ''
            }
          >
            {isRestoring ? <span className={styles.spinner} /> : <RotateCcw size={12} />}
            {isRestoring
              ? '回滚中…'
              : totalChanges === 0
                ? '无变更，仍要回滚'
                : '确认回滚'}
          </button>
        </div>
      </div>
    </div>
  );
};

async function readCurrentFileText(path: string): Promise<string | null> {
  try {
    const res = await invoke<{ content: string }>('read_document', { path });
    return res.content;
  } catch {
    return null;
  }
}

async function readSnapshotFileBytes(
  workspacePath: string,
  snapshotId: string,
  relPath: string
): Promise<string | null> {
  // The snapshot stores raw bytes under
  // ~/.inkuo/snapshots/{wsHash}/{snapshotId}/files/{relPath}.  We can't
  // reach the same path from the frontend without a Tauri command, so
  // expose one in commands.rs: `read_snapshot_file_cmd`.
  try {
    return await invoke<string>('read_snapshot_file_cmd', {
      workspacePath,
      snapshotId,
      relPath,
    });
  } catch {
    return null;
  }
}

interface DiffViewProps {
  hunks: DiffHunk[];
}

const DiffView = ({ hunks }: DiffViewProps) => {
  if (hunks.length === 0) {
    return <div className={styles.diffEmpty}>无差异</div>;
  }
  return (
    <div className={styles.diffBlock}>
      {hunks.flatMap((hunk) => {
        const before = hunk.old_range.start_line;
        const after = hunk.new_range.start_line;
        return hunk.changes.map((change, idx) => {
          const kind =
            change.tag === 'Insert'
              ? 'insert'
              : change.tag === 'Delete'
                ? 'delete'
                : change.tag === 'Replace'
                  ? 'change'
                  : 'context';
          return (
            <div
              key={`${hunk.id}-${idx}`}
              className={styles.diffLine}
              data-kind={kind}
            >
              <span className={styles.diffLineNumber}>
                {change.old_line ?? before}
              </span>
              <span className={styles.diffLineNumber}>
                {change.new_line ?? after}
              </span>
              <span className={styles.diffMarker}>
                {kind === 'insert' ? '+' : kind === 'delete' ? '-' : ' '}
              </span>
              <span className={styles.diffText}>{change.content || ' '}</span>
            </div>
          );
        });
      })}
    </div>
  );
};

export default SnapshotRestoreDialog;
