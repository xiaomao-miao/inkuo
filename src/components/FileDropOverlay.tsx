import { useEffect, useRef, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import { FileInput } from 'lucide-react';
import { useNotificationStore, useSidebarStore } from '../store';
import { applyWorkspaceDirectoryLoad, switchWorkspace } from '../services/workspace';
import { getBaseName, normalizeDirPath } from '../utils/path';
import { reportError } from '../utils/errors';
import { planFileDrop, type DroppedPathInfo } from './fileDropPlan';
import styles from './FileDropOverlay.module.css';

interface DroppedPathInspection {
  path: string;
  is_directory: boolean | null;
  error?: string | null;
}

/**
 * Native window drop target. A dropped directory becomes the workspace; files
 * become tabs. On the welcome screen, the first file's parent is opened as the
 * workspace so the tab participates in snapshots and filesystem watching.
 */
export function FileDropOverlay() {
  const [isDragging, setIsDragging] = useState(false);
  const processingRef = useRef(false);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    const processDrop = async (paths: string[]) => {
      if (processingRef.current || paths.length === 0) return;
      processingRef.current = true;
      try {
        const inspected = await invoke<DroppedPathInspection[]>('inspect_dropped_paths', { paths });
        const validEntries = inspected
          .filter((entry) => entry.is_directory !== null && !entry.error)
          .map((entry): DroppedPathInfo => ({
            path: normalizeDirPath(entry.path),
            isDirectory: entry.is_directory === true,
          }));
        if (validEntries.length === 0) {
          throw new Error('无法读取拖入的文件或文件夹');
        }

        const sidebar = useSidebarStore.getState();
        const plan = planFileDrop(validEntries, sidebar.workspacePath);
        if (plan.workspaceToOpen) {
          await switchWorkspace(plan.workspaceToOpen);
          await applyWorkspaceDirectoryLoad(plan.workspaceToOpen, {
            mergeWithExisting: false,
            showSkeleton: true,
          });
        }

        const liveSidebar = useSidebarStore.getState();
        for (const path of plan.filesToOpen) {
          liveSidebar.openWorkspaceFile(path, { name: getBaseName(path) });
        }

        const failedCount = inspected.length - validEntries.length;
        const skippedCount = plan.skippedPaths.length + failedCount;
        useNotificationStore.getState().pushNotification({
          kind: 'success',
          title: plan.workspaceToOpen ? '已打开拖入的工作区' : '已打开拖入的文件',
          message: skippedCount > 0
            ? `已打开 ${plan.filesToOpen.length} 个文件，另有 ${skippedCount} 项不在目标工作区或无法读取，未打开`
            : plan.filesToOpen.length > 0
              ? `已打开 ${plan.filesToOpen.length} 个文件`
              : plan.workspaceToOpen ?? undefined,
        });
      } catch (err) {
        const message = reportError('window-file-drop', err);
        useNotificationStore.getState().pushNotification({
          kind: 'error',
          title: '无法打开拖入内容',
          message,
        });
      } finally {
        processingRef.current = false;
      }
    };

    void getCurrentWindow().onDragDropEvent((event) => {
      if (disposed) return;
      if (event.payload.type === 'enter' || event.payload.type === 'over') {
        setIsDragging(true);
      } else if (event.payload.type === 'leave') {
        setIsDragging(false);
      } else {
        setIsDragging(false);
        void processDrop(event.payload.paths);
      }
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }).catch(() => {
      // Browser-only development and unit tests do not provide a Tauri window.
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  if (!isDragging) return null;
  return (
    <div className={styles.overlay} role="status" aria-live="polite">
      <div className={styles.card}>
        <FileInput size={28} aria-hidden />
        <strong>松开以打开</strong>
        <span>文件将作为标签页打开，文件夹将作为工作区打开</span>
      </div>
    </div>
  );
}
