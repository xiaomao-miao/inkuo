import { useEffect, useRef, useState } from 'react';
import {
  Minus,
  Square,
  X,
  Settings,
  User,
  UserCheck,
  Maximize2,
  Minimize2,
} from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { useSidebarStore, useEditorStore, useNotificationStore, useSettingsStore } from '../../store';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { applyWorkspaceDirectoryLoad, openWorkspaceDirectory, switchWorkspace } from '../../services/workspace';
import { persistDocument } from '../../services/documentSave';
import { reportError } from '../../utils/errors';
import { openSettingsTab } from '../../utils/openSettingsTab';
import { openCloudTab } from '../../utils/openCloudTab';

import { isTauriRuntime } from '../../utils/tauri';
import styles from './TitleBar.module.css';

interface MenuItem {
  label: string;
  shortcut?: string;
  action?: () => void;
  divider?: boolean;
  disabled?: boolean;
}

interface Menu {
  label: string;
  items: MenuItem[];
}

export const TitleBar: React.FC = () => {
  const [activeMenu, setActiveMenu] = useState<string | null>(null);
  const [windowState, setWindowState] = useState<'normal' | 'maximized' | 'fullscreen'>('normal');
  const menuRef = useRef<HTMLDivElement>(null);
  const isTauri = isTauriRuntime();

  const selectedFile = useSidebarStore((state) => state.selectedFile);
  const workspacePath = useSidebarStore((state) => state.workspacePath);
  const pushNotification = useNotificationStore((state) => state.pushNotification);
  const cloudAccount = useSettingsStore((s) => s.settings.cloud.account);
  const currentMetadata = useEditorStore((state) => (
    selectedFile ? state.documentContents[selectedFile]?.metadata : null
  ));
  const isDirty = currentMetadata?.isDirty ?? false;

  const handleOpenSettings = () => {
    openSettingsTab();
  };

  const handleOpenCloud = () => {
    openCloudTab();
  };

  // Check initial maximized/fullscreen state
  useEffect(() => {
    if (!isTauri) {
      return;
    }

    const win = getCurrentWindow();
    let disposed = false;
    let unlistenResize: (() => void) | null = null;

    const syncWindowState = async () => {
      try {
        const [fullscreen, maximized] = await Promise.all([
          win.isFullscreen(),
          win.isMaximized(),
        ]);
        if (!disposed) {
          // Priority: fullscreen > maximized > normal — fullscreen wins when both
          // are set so the UI matches what the user actually sees on screen.
          if (fullscreen) {
            setWindowState('fullscreen');
          } else if (maximized) {
            setWindowState('maximized');
          } else {
            setWindowState('normal');
          }
        }
      } catch (err) {
        reportError('titlebar-sync-window-state', err);
      }
    };

    const setupListeners = async () => {
      await syncWindowState();

      try {
        const resizeUnlisten = await win.onResized(() => {
          void syncWindowState();
        });

        unlistenResize = resizeUnlisten;
      } catch (err) {
        reportError('titlebar-window-listeners', err);
      }
    };

    void setupListeners();

    return () => {
      disposed = true;
      unlistenResize?.();
    };
  }, [isTauri]);

  // Close menu when clicking outside
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setActiveMenu(null);
      }
    };
    
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const handleSave = async () => {
    if (!isTauri) {
      pushNotification({
        kind: 'info',
        title: '浏览器模式不支持保存',
        message: '当前页面未运行在 Tauri 桌面环境中，无法写入本地文件。',
      });
      setActiveMenu(null);
      return;
    }

    const result = await persistDocument({
      path: selectedFile,
      content: currentMetadata?.content || '',
      isDirty,
    });

    if (!result.ok) {
      // reserved for future user-facing notification surface
    }

    setActiveMenu(null);
  };

  const handleOpenFolder = async () => {
    if (!isTauri) {
      pushNotification({
        kind: 'info',
        title: '浏览器模式不支持打开文件夹',
        message: '文件夹选择依赖 Tauri 对话框与本地后端能力。',
      });
      setActiveMenu(null);
      return;
    }

    try {
      const selected = await openWorkspaceDirectory();
      if (selected) {
        switchWorkspace(selected);
        await applyWorkspaceDirectoryLoad(selected, { mergeWithExisting: false });
      }
    } catch (err) {
      reportError('titlebar-open-folder', err);
      pushNotification({
        kind: 'error',
        title: '打开文件夹失败',
        message: String((err as Error)?.message ?? err),
      });
    }
    setActiveMenu(null);
  };

  const handleNewWindow = async () => {
    try {
      await invoke('create_new_window');
      pushNotification({
        kind: 'info',
        title: '新窗口已创建',
        message: '正在打开新窗口...',
      });
    } catch (err) {
      reportError('titlebar-new-window', err);
      pushNotification({
        kind: 'error',
        title: '创建新窗口失败',
        message: String(err),
      });
    }
    setActiveMenu(null);
  };

  const handleMinimize = async () => {
    if (!isTauri) {
      return;
    }

    const win = getCurrentWindow();
    await win.minimize();
  };

  const handleMaximize = async () => {
    if (!isTauri) {
      return;
    }

    const win = getCurrentWindow();
    try {
      // Triple state cycle: normal -> maximized -> fullscreen -> normal
      if (windowState === 'normal') {
        await win.maximize();
        setWindowState('maximized');
      } else if (windowState === 'maximized') {
        await win.setFullscreen(true);
        setWindowState('fullscreen');
      } else {
        // fullscreen: exit fullscreen first, then unmaximize so we land in 'normal'
        await win.setFullscreen(false);
        await win.unmaximize();
        setWindowState('normal');
      }
    } catch (err) {
      reportError('titlebar-toggle-maximize', err);
    }
  };

  const handleClose = async () => {
    if (!isTauri) {
      return;
    }

    const win = getCurrentWindow();
    await win.close();
  };

  const menus: Menu[] = [
    {
      label: '文件',
      items: [
        { label: '新建窗口', shortcut: 'Ctrl+Shift+N', action: handleNewWindow },
        { label: '新建文件', shortcut: 'Ctrl+N', action: () => setActiveMenu(null), disabled: true },
        // Same-window "Open folder" is disabled once a workspace is loaded:
// a window is tied to a single workspace for its lifetime. To switch
// workspaces the user opens a new window from the welcome page.
        {
          label: '打开文件夹...',
          shortcut: 'Ctrl+O',
          action: handleOpenFolder,
          disabled: Boolean(workspacePath),
        },
        { divider: true, label: '' },
        { label: '保存', shortcut: 'Ctrl+S', action: handleSave, disabled: !isDirty },
        { label: '另存为...', shortcut: 'Ctrl+Shift+S', disabled: true },
        { divider: true, label: '' },
        { label: '关闭编辑器', shortcut: 'Ctrl+W', disabled: !selectedFile },
        { divider: true, label: '' },
        { label: '退出', shortcut: 'Alt+F4', action: handleClose },
      ],
    },
    {
      label: '编辑',
      items: [
        { label: '撤销', shortcut: 'Ctrl+Z', disabled: true },
        { label: '重做', shortcut: 'Ctrl+Y', disabled: true },
        { divider: true, label: '' },
        { label: '剪切', shortcut: 'Ctrl+X', disabled: true },
        { label: '复制', shortcut: 'Ctrl+C', disabled: true },
        { label: '粘贴', shortcut: 'Ctrl+V', disabled: true },
        { divider: true, label: '' },
        { label: '全选', shortcut: 'Ctrl+A', disabled: true },
        { label: '查找', shortcut: 'Ctrl+F', disabled: true },
        { label: '替换', shortcut: 'Ctrl+H', disabled: true },
      ],
    },
    {
      label: '选择',
      items: [
        { label: '全选', shortcut: 'Ctrl+A', disabled: true },
        { label: '展开', shortcut: 'Ctrl+=', disabled: true },
        { label: '收起', shortcut: 'Ctrl+-', disabled: true },
      ],
    },
    {
      label: '视图',
      items: [
        { label: '侧边栏', shortcut: 'Ctrl+B', disabled: true },
        { label: 'AI 面板', shortcut: 'Ctrl+Shift+L', disabled: true },
        { divider: true, label: '' },
        { label: '放大字体', shortcut: 'Ctrl++', disabled: true },
        { label: '缩小字体', shortcut: 'Ctrl+-', disabled: true },
        { label: '重置字体大小', shortcut: 'Ctrl+0', disabled: true },
        { divider: true, label: '' },
        { label: '全屏', shortcut: 'F11', disabled: true },
      ],
    },
    {
      label: '帮助',
      items: [
        { label: '关于 inkuo', disabled: true },
        { label: '快捷键参考', shortcut: 'Ctrl+K Ctrl+R', disabled: true },
      ],
    },
  ];

  return (
    <div className={styles.titleBar} ref={menuRef} data-tauri-drag-region>
      <div className={styles.menuArea}>
        {menus.map(menu => (
          <div key={menu.label} className={styles.menuContainer}>
            <button
              className={`${styles.menuButton} ${activeMenu === menu.label ? styles.active : ''}`}
              onClick={() => setActiveMenu(activeMenu === menu.label ? null : menu.label)}
              onMouseEnter={() => activeMenu && setActiveMenu(menu.label)}
            >
              {menu.label}
            </button>
            {activeMenu === menu.label && (
              <div className={styles.dropdown}>
                {menu.items.map((item, index) => (
                  item.divider ? (
                    <div key={index} className={styles.divider} />
                  ) : (
                    <button
                      key={index}
                      className={`${styles.menuItem} ${item.disabled ? styles.disabled : ''}`}
                      onClick={() => !item.disabled && item.action?.()}
                      disabled={item.disabled}
                    >
                      <span className={styles.menuItemLabel}>{item.label}</span>
                      {item.shortcut && (
                        <span className={styles.menuItemShortcut}>{item.shortcut}</span>
                      )}
                    </button>
                  )
                ))}
              </div>
            )}
          </div>
        ))}
      </div>
      
      <div className={styles.title} data-tauri-drag-region>
        <span className={styles.appName}>inkuo</span>
        {selectedFile && (
          <>
            <span className={styles.separator}>—</span>
            <span className={styles.fileName}>
              {currentMetadata?.document?.title || '未命名'}
              {isDirty && <span className={styles.dirty}>●</span>}
            </span>
          </>
        )}
      </div>
      
      <div className={styles.actions}>
        <button
          className={styles.actionButton}
          onClick={handleOpenCloud}
          title={cloudAccount ? `${cloudAccount.email} · 账号设置` : '登录 inkuo Cloud'}
          data-signed-in={cloudAccount ? 'true' : undefined}
        >
          {cloudAccount ? <UserCheck size={14} /> : <User size={14} />}
        </button>
        <button
          className={styles.actionButton}
          onClick={handleOpenSettings}
          title="设置"
        >
          <Settings size={14} />
        </button>
        {isTauri && (
          <>
            <button
              className={styles.actionButton}
              onClick={handleMinimize}
              title="最小化"
            >
              <Minus size={14} />
            </button>
            <button
              className={styles.actionButton}
              onClick={handleMaximize}
              title={
                windowState === 'fullscreen'
                  ? '退出全屏'
                  : windowState === 'maximized'
                    ? '进入全屏'
                    : '最大化'
              }
            >
              {windowState === 'fullscreen' ? (
                <Minimize2 size={12} />
              ) : windowState === 'maximized' ? (
                <Maximize2 size={12} />
              ) : (
                <Square size={12} />
              )}
            </button>
            <button
              className={`${styles.actionButton} ${styles.close}`}
              onClick={handleClose}
              title="关闭"
            >
              <X size={14} />
            </button>
          </>
        )}
      </div>
    </div>
  );
};
