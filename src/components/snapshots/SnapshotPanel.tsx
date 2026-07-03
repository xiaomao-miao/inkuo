/**
 * SnapshotPanel — left-sidebar view that lists workspace file-content
 * snapshots and lets the user create / preview / restore / delete them.
 *
 * State is fetched on mount and refreshed after every action.  The
 * `useSnapshotActions` hook centralises side-effects (notifications,
 * confirmation dialogs).
 */

import { useCallback, useEffect, useState } from 'react';
import { History, Plus, RotateCcw, Trash2, X, Search, Bot } from 'lucide-react';
import { listSnapshots, type SnapshotIndexEntry } from '../../services/snapshots';
import { useSidebarStore } from '../../store/sidebarStore';
import { useSnapshotActions } from './useSnapshotActions';
import { SnapshotRestoreDialog } from './SnapshotRestoreDialog';
import styles from './Snapshots.module.css';

function formatRelative(timestampMs: number): string {
  const diff = Date.now() - timestampMs;
  if (diff < 0) return '刚刚';
  const sec = Math.floor(diff / 1000);
  if (sec < 60) return `${sec} 秒前`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min} 分钟前`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr} 小时前`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${day} 天前`;
  return new Date(timestampMs).toLocaleString('zh-CN');
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export const SnapshotPanel = () => {
  const workspacePath = useSidebarStore((s) => s.workspacePath);
  const { create, remove } = useSnapshotActions();
  const [snapshots, setSnapshots] = useState<SnapshotIndexEntry[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [isCreating, setIsCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [restoreTarget, setRestoreTarget] = useState<SnapshotIndexEntry | null>(null);
  const [searchQuery, setSearchQuery] = useState('');

  const refresh = useCallback(async () => {
    if (!workspacePath) {
      setSnapshots([]);
      return;
    }
    setIsLoading(true);
    setError(null);
    try {
      const items = await listSnapshots(workspacePath);
      setSnapshots(items);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsLoading(false);
    }
  }, [workspacePath]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleCreate = useCallback(async () => {
    setIsCreating(true);
    try {
      const manifest = await create({ trigger: 'manual' });
      if (manifest) {
        await refresh();
      }
    } finally {
      setIsCreating(false);
    }
  }, [create, refresh]);

  const handleDelete = useCallback(
    async (id: string) => {
      const ok = await remove(id);
      if (ok) await refresh();
    },
    [remove, refresh]
  );

  const filtered = snapshots.filter((snap) => {
    if (!searchQuery.trim()) return true;
    const q = searchQuery.toLowerCase();
    return (
      snap.id.toLowerCase().includes(q) ||
      (snap.label ?? '').toLowerCase().includes(q)
    );
  });

  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        <span className={styles.title}>
          <History size={14} /> 快照
        </span>
        <div className={styles.headerActions}>
          <button
            className={styles.iconButton}
            onClick={handleCreate}
            disabled={!workspacePath || isCreating}
            title="创建快照"
          >
            {isCreating ? <span className={styles.spinner} /> : <Plus size={14} />}
          </button>
        </div>
      </div>

      <div className={styles.toolbar}>
        <div className={styles.searchBox}>
          <Search size={12} className={styles.searchIcon} />
          <input
            type="text"
            placeholder="搜索快照…"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className={styles.searchInput}
          />
          {searchQuery && (
            <button
              className={styles.searchClear}
              onClick={() => setSearchQuery('')}
              title="清除"
            >
              <X size={12} />
            </button>
          )}
        </div>
      </div>

      {!workspacePath && (
        <div className={styles.empty}>请先打开一个工作区</div>
      )}

      {workspacePath && error && (
        <div className={styles.error}>{error}</div>
      )}

      {workspacePath && isLoading && (
        <div className={styles.loading}>加载中…</div>
      )}

      {workspacePath && !isLoading && filtered.length === 0 && (
        <div className={styles.empty}>
          {snapshots.length === 0
            ? '尚无快照，点击右上角 + 创建第一份。'
            : '没有匹配的快照'}
        </div>
      )}

      <div className={styles.list}>
        {filtered.map((snap) => (
          <SnapshotRow
            key={snap.id}
            snap={snap}
            onRestore={() => setRestoreTarget(snap)}
            onDelete={() => void handleDelete(snap.id)}
          />
        ))}
      </div>

      {restoreTarget && (
        <SnapshotRestoreDialog
          snapshot={restoreTarget}
          onClose={() => setRestoreTarget(null)}
          onRestored={() => {
            setRestoreTarget(null);
            void refresh();
          }}
        />
      )}
    </div>
  );
};

interface SnapshotRowProps {
  snap: SnapshotIndexEntry;
  onRestore: () => void;
  onDelete: () => void;
}

const SnapshotRow = ({ snap, onRestore, onDelete }: SnapshotRowProps) => {
  const isBaseline = snap.trigger === 'ai_baseline';
  return (
    <div className={styles.row}>
      <div className={styles.rowHeader}>
        {isBaseline ? (
          <span className={styles.baselineBadge} title="AI 基线">
            <Bot size={10} /> AI
          </span>
        ) : (
          <span className={styles.manualBadge} title="手动">
            <History size={10} />
          </span>
        )}
        <span className={styles.rowTime} title={new Date(snap.createdAt).toLocaleString('zh-CN')}>
          {formatRelative(snap.createdAt)}
        </span>
      </div>
      <div className={styles.rowLabel}>
        {snap.label || `快照 ${snap.id.slice(-6)}`}
      </div>
      <div className={styles.rowMeta}>
        {snap.fileCount} 个文件 · {formatBytes(snap.totalBytes)}
      </div>
      <div className={styles.rowActions}>
        <button
          className={styles.rowActionBtn}
          onClick={onRestore}
          title="回滚到该快照"
        >
          <RotateCcw size={12} /> 回滚
        </button>
        <button
          className={`${styles.rowActionBtn} ${styles.rowActionDanger}`}
          onClick={onDelete}
          title="删除"
        >
          <Trash2 size={12} />
        </button>
      </div>
    </div>
  );
};

export default SnapshotPanel;
