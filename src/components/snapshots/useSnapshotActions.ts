/**
 * `useSnapshotActions` — encapsulates the create / delete / restore flows for
 * workspace file-content snapshots, including the side-effects (notifications,
 * dialog management) and reading the live workspace file contents via Tauri.
 *
 * Used by both the SnapshotPanel (manual operations) and the AI panel's
 * baseline-on-resend flow.
 */

import { useCallback } from 'react';
import { useNotificationStore } from '../../store/notificationStore';
import { useConfirmDialogStore } from '../../store/confirmDialogStore';
import { useSidebarStore } from '../../store/sidebarStore';
import {
  collectWorkspaceFiles,
  collectWorkspaceEmptyDirs,
  createSnapshot,
  deleteSnapshot,
  listSnapshots,
  previewRestore,
  restoreSnapshot,
  type RestoreSnapshotResult,
  type SnapshotIndexEntry,
  type SnapshotManifest,
} from '../../services/snapshots';
import { reportError } from '../../utils/errors';

interface CreateOptions {
  label?: string | null;
  trigger?: 'manual' | 'ai_baseline';
}

export function useSnapshotActions() {
  const pushNotification = useNotificationStore((s) => s.pushNotification);
  const askConfirm = useConfirmDialogStore((s) => s.ask);
  const workspacePath = useSidebarStore((s) => s.workspacePath);

  const create = useCallback(
    async (opts: CreateOptions = {}): Promise<SnapshotManifest | null> => {
      if (!workspacePath) {
        pushNotification({
          kind: 'error',
          title: '无法创建快照',
          message: '请先打开一个工作区',
        });
        return null;
      }
      try {
        const [files, directories] = await Promise.all([
          collectWorkspaceFiles(workspacePath),
          collectWorkspaceEmptyDirs(workspacePath),
        ]);
        if (files.length === 0 && directories.length === 0) {
          pushNotification({
            kind: 'info',
            title: '工作区为空',
            message: '没有文件可快照',
          });
          return null;
        }
        const manifest = await createSnapshot(
          workspacePath,
          opts.label ?? null,
          opts.trigger ?? 'manual',
          files,
          directories
        );
        const dirPart =
          manifest.directories.length > 0
            ? `、${manifest.directories.length} 个空目录`
            : '';
        pushNotification({
          kind: 'success',
          title: '快照已创建',
          message: `共 ${manifest.files.length} 个文件${dirPart}`,
        });
        return manifest;
      } catch (err) {
        reportError('createSnapshot', err);
        pushNotification({
          kind: 'error',
          title: '创建快照失败',
          message: err instanceof Error ? err.message : String(err),
        });
        return null;
      }
    },
    [pushNotification, workspacePath]
  );

  const remove = useCallback(
    async (id: string): Promise<boolean> => {
      if (!workspacePath) return false;
      const confirmed = await askConfirm({
        title: '删除快照？',
        message: '该快照将被永久删除，无法恢复。',
        confirmLabel: '删除',
        danger: true,
      });
      if (!confirmed) return false;
      try {
        await deleteSnapshot(workspacePath, id);
        pushNotification({
          kind: 'success',
          title: '快照已删除',
        });
        return true;
      } catch (err) {
        reportError('deleteSnapshot', err);
        pushNotification({
          kind: 'error',
          title: '删除失败',
          message: err instanceof Error ? err.message : String(err),
        });
        return false;
      }
    },
    [askConfirm, pushNotification, workspacePath]
  );

  const restore = useCallback(
    async (id: string): Promise<RestoreSnapshotResult | null> => {
      if (!workspacePath) return null;
      try {
        const result = await restoreSnapshot(workspacePath, id);
        const parts: string[] = [];
        if (result.restored.length > 0)
          parts.push(`还原 ${result.restored.length} 个文件`);
        if (result.deleted.length > 0)
          parts.push(`删除 ${result.deleted.length} 个新增文件`);
        if (result.deletedDirs.length > 0)
          parts.push(`删除 ${result.deletedDirs.length} 个目录`);
        if (result.createdDirs.length > 0)
          parts.push(`重建 ${result.createdDirs.length} 个目录`);
        pushNotification({
          kind: 'success',
          title: '已回滚到快照',
          message:
            parts.length > 0
              ? `${parts.join('，')}（备份到 ${result.backupPath}）`
              : `无变更（备份到 ${result.backupPath}）`,
        });
        return result;
      } catch (err) {
        reportError('restoreSnapshot', err);
        pushNotification({
          kind: 'error',
          title: '回滚失败',
          message: err instanceof Error ? err.message : String(err),
        });
        return null;
      }
    },
    [pushNotification, workspacePath]
  );

  return {
    create,
    remove,
    restore,
    collectWorkspaceFiles,
    workspacePath,
  };
}

export type { SnapshotIndexEntry };
export { listSnapshots, previewRestore };
