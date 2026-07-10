import { useCallback, useState } from 'react';
import { FolderOpen, Plus, ArrowRight } from 'lucide-react';
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
        <Wordmark />

        <p className={styles.subtitle}>
          一款为长文而生的编辑器 · 内置本地 Agent 与知识库
        </p>

        <div className={styles.actions}>
          <button
            className={styles.primaryButton}
            onClick={handleSelectWorkspace}
            disabled={isLoading}
          >
            <FolderOpen size={18} />
            <span>打开工作区</span>
            <ArrowRight size={16} className={styles.arrowIcon} />
          </button>

          <button
            className={styles.secondaryButton}
            onClick={handleNewWindow}
            disabled={isLoading}
          >
            <Plus size={18} />
            <span>新窗口</span>
          </button>
        </div>
      </div>

      <div className={styles.footer}>
        <span>快捷键</span>
        <kbd>⌘</kbd>
        <kbd>O</kbd>
        <span className={styles.footerHint}>快速打开工作区</span>
      </div>
    </div>
  );
};

/**
 * 品牌字标 —— 不用圆形 emoji 字母,改用一个简单的几何符号 + 文字。
 * 笔画"i"被画成一滴墨水的形状,呼应 "inkuo / 墨" 这个品牌隐喻;
 * 符号大小写排版遵循 Lineto / Vercel 一类的克制风格。
 */
const Wordmark: React.FC = () => (
  <div className={styles.wordmark}>
    <svg
      className={styles.symbol}
      width="36"
      height="36"
      viewBox="0 0 36 36"
      fill="none"
      aria-hidden
    >
      {/* 上半:简洁的方形外框 */}
      <rect
        x="3"
        y="3"
        width="30"
        height="30"
        rx="8"
        stroke="currentColor"
        strokeWidth="1.5"
      />
      {/* 下半:墨滴 */}
      <path
        d="M18 11c-2.8 3.5-4.6 6-4.6 8.4a4.6 4.6 0 0 0 9.2 0c0-2.4-1.8-4.9-4.6-8.4z"
        fill="currentColor"
        opacity="0.92"
      />
    </svg>
    <span className={styles.wordmarkText}>inkuo</span>
  </div>
);