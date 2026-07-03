import { useCallback, useState } from 'react';
import { FolderOpen, Plus, ChevronRight } from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { useNotificationStore } from '../../store';
import {
  applyWorkspaceDirectoryLoad,
  switchWorkspace,
} from '../../services/workspace';
import { reportError } from '../../utils/errors';
import styles from './WelcomePage.module.css';

interface WelcomePageProps {
  onWorkspaceSelected?: () => void;
}

export const WelcomePage: React.FC<WelcomePageProps> = ({ onWorkspaceSelected }) => {
  const [isLoading, setIsLoading] = useState(false);
  const pushNotification = useNotificationStore((state) => state.pushNotification);

  const handleSelectWorkspace = useCallback(async () => {
    setIsLoading(true);
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: '选择工作区文件夹',
      });

      if (selected) {
        switchWorkspace(selected);
        await applyWorkspaceDirectoryLoad(selected, { mergeWithExisting: false });
        pushNotification({
          kind: 'info',
          title: '工作区已打开',
          message: `已加载: ${selected}`,
        });
        onWorkspaceSelected?.();
      }
    } catch (err) {
      reportError('welcome-select-workspace', err);
      pushNotification({
        kind: 'error',
        title: '打开工作区失败',
        message: String(err),
      });
    } finally {
      setIsLoading(false);
    }
  }, [pushNotification, onWorkspaceSelected]);

  const handleNewWindow = useCallback(async () => {
    try {
      await invoke('create_new_window');
      pushNotification({
        kind: 'info',
        title: '新窗口已创建',
        message: '正在打开新窗口...',
      });
    } catch (err) {
      reportError('welcome-new-window', err);
      pushNotification({
        kind: 'error',
        title: '创建新窗口失败',
        message: String(err),
      });
    }
  }, [pushNotification]);

  return (
    <div className={styles.welcomePage}>
      <div className={styles.centerContent}>
        <div className={styles.logoSection}>
          <div className={styles.logo}>
            <span className={styles.logoIcon}>I</span>
          </div>
          <h1 className={styles.title}>inkuo</h1>
          <p className={styles.subtitle}>AI 文档编辑器</p>
        </div>

        <div className={styles.actions}>
          <button
            className={styles.primaryButton}
            onClick={handleSelectWorkspace}
            disabled={isLoading}
          >
            <FolderOpen size={20} />
            <span>打开工作区</span>
            <ChevronRight size={16} className={styles.arrowIcon} />
          </button>

          <button
            className={styles.secondaryButton}
            onClick={handleNewWindow}
            disabled={isLoading}
          >
            <Plus size={20} />
            <span>新窗口</span>
          </button>
        </div>

        <div className={styles.hint}>
          <span>选择工作区文件夹以开始编辑文档</span>
        </div>
      </div>

      <div className={styles.footer}>
        <p className={styles.footerText}>
          快捷键 <kbd>Ctrl</kbd>+<kbd>O</kbd> 快速打开工作区
        </p>
      </div>
    </div>
  );
};
