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
import {
  useSidebarStore,
  useEditorStore,
  useNotificationStore,
  useSettingsStore,
  useLayoutStore,
  useAIPanelStore,
  useEditorHandleStore,
  type EditorCommands,
} from '../../store';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { applyWorkspaceDirectoryLoad, openWorkspaceDirectory, switchWorkspace } from '../../services/workspace';
import { requestCloseOpenTab, saveOpenTab } from '../../services/openTabLifecycle';
import { reportError } from '../../utils/errors';
import { openSettingsTab } from '../../utils/openSettingsTab';
import { openCloudTab } from '../../utils/openCloudTab';

import { isTauriRuntime } from '../../utils/tauri';
import { AppIcon } from '../brand/AppIcon';
import styles from './TitleBar.module.css';

const FONT_SIZE_MIN = 8;
const FONT_SIZE_MAX = 32;
const FONT_SIZE_DEFAULT = 14;
const FONT_SIZE_STEP = 1;

const clampFontSize = (value: number) =>
  Math.min(FONT_SIZE_MAX, Math.max(FONT_SIZE_MIN, Math.round(value)));

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
  const activeTab = useSidebarStore((state) => (
    state.openTabs.find((tab) => tab.id === state.activeTabId) ?? null
  ));
  const pushNotification = useNotificationStore((state) => state.pushNotification);
  const cloudAccount = useSettingsStore((s) => s.settings.cloud.account);
  const currentMetadata = useEditorStore((state) => (
    selectedFile ? state.documentContents[selectedFile]?.metadata : null
  ));
  // Office editors keep their authoritative dirty flag on the tab, while
  // text editors publish it in metadata first. Observe both so the title-bar
  // Save action never goes missing for one editor family.
  const isDirty = Boolean(activeTab?.isDirty || currentMetadata?.isDirty);

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

    if (activeTab) await saveOpenTab(activeTab);

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
        const switched = await switchWorkspace(selected);
        if (switched) {
          await applyWorkspaceDirectoryLoad(selected, { mergeWithExisting: false });
        }
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

  // ---------------------------------------------------------------------------
  // File menu — 新建文件 / 关闭编辑器
  // ---------------------------------------------------------------------------

  // `New file` from the top bar: spawn the inline-rename input inside the
  // workspace root. The existing InlineRenameInput polls the sidebar's
  // `inlineEdit` state and runs the create flow (`createFileEntry` under
  // `/services/workspace`), so we just need to flip the state. We default
  // to a Markdown file because that's the editor's bread and butter.
  const handleNewFile = () => {
    if (!isTauri) {
      pushNotification({
        kind: 'info',
        title: '浏览器模式不支持新建文件',
        message: '新建文件依赖 Tauri 提供的本地后端能力。',
      });
      setActiveMenu(null);
      return;
    }
    if (!workspacePath) {
      pushNotification({
        kind: 'info',
        title: '请先打开工作区',
        message: '新建文件需要先选择一个工作区文件夹。',
      });
      setActiveMenu(null);
      return;
    }
    // Make sure the workspace root is expanded so the inline row is
    // visible immediately — `openWorkspaceFile` does the same dance for
    // newly opened files.
    if (!useSidebarStore.getState().isDirExpanded(workspacePath)) {
      useSidebarStore.getState().toggleDir(workspacePath);
    }
    useSidebarStore.getState().startInlineEdit({
      parentPath: workspacePath,
      originalPath: null,
      initialValue: 'untitled.md',
      extension: 'md',
      createPayload: {
        kind: 'file',
        extension: 'md',
        template: '# 无标题\n\n开始书写…\n',
      },
      mode: 'create',
    });
    setActiveMenu(null);
  };

  // `Close editor` from the top bar follows the same three-way lifecycle as
  // tab buttons, tab context menus, and native-window close.
  const handleCloseEditor = async () => {
    if (activeTab) await requestCloseOpenTab(activeTab);
    setActiveMenu(null);
  };

  useEffect(() => {
    if (!isTauri || !activeTab) return;
    const handleCloseShortcut = (event: KeyboardEvent) => {
      if (
        !(event.ctrlKey || event.metaKey)
        || event.shiftKey
        || event.altKey
        || event.key.toLowerCase() !== 'w'
      ) return;
      event.preventDefault();
      void requestCloseOpenTab(activeTab);
      setActiveMenu(null);
    };
    window.addEventListener('keydown', handleCloseShortcut);
    return () => window.removeEventListener('keydown', handleCloseShortcut);
  }, [activeTab, isTauri]);

  // ---------------------------------------------------------------------------
  // Edit menu — undo/redo/cut/copy/paste/selectAll/find/replace
  // ---------------------------------------------------------------------------

  // All editing commands dispatch through the editor handle store. The
  // Editor publishes a stable `EditorCommands` snapshot whenever the
  // markdown/code/text editor is mounted; we wrap each call so the menu
  // can `setActiveMenu(null)` between the click and the dispatch.
  const handleEditCommand = (kind: keyof EditorCommands) => {
    const commands = useEditorHandleStore.getState().commands;
    if (!commands) return;
    commands[kind]();
    setActiveMenu(null);
  };

  // ---------------------------------------------------------------------------
  // View menu — sidebar / AI panel / font size / fullscreen
  // ---------------------------------------------------------------------------

  const toggleSidebar = useLayoutStore((state) => state.toggleSidebar);
  const toggleAiPanel = useAIPanelStore((state) => state.togglePanel);
  const updateSetting = useSettingsStore((state) => state.updateSetting);
  const editorFontSize = useSettingsStore((state) => state.settings.editor_font_size);

  const handleToggleSidebar = () => {
    toggleSidebar();
    setActiveMenu(null);
  };

  const handleToggleAiPanel = () => {
    toggleAiPanel();
    setActiveMenu(null);
  };

  const changeFontSize = (next: number) => {
    const clamped = clampFontSize(next);
    if (clamped === editorFontSize) return;
    void updateSetting('editor_font_size', clamped);
    setActiveMenu(null);
  };

  const handleIncreaseFontSize = () => changeFontSize(editorFontSize + FONT_SIZE_STEP);
  const handleDecreaseFontSize = () => changeFontSize(editorFontSize - FONT_SIZE_STEP);
  const handleResetFontSize = () => changeFontSize(FONT_SIZE_DEFAULT);

  // `Fullscreen` from the top bar: toggle the Tauri window's fullscreen
  // state. In the browser (non-Tauri) we fall back to the web's
  // `requestFullscreen` on the document body so the shortcut still
  // does something visible during local dev.
  const handleToggleFullscreen = async () => {
    if (isTauri) {
      const win = getCurrentWindow();
      try {
        if (windowState === 'fullscreen') {
          await win.setFullscreen(false);
          setWindowState('normal');
        } else {
          await win.setFullscreen(true);
          setWindowState('fullscreen');
        }
      } catch (err) {
        reportError('titlebar-toggle-fullscreen', err);
      }
    } else if (typeof document !== 'undefined') {
      if (document.fullscreenElement) {
        void document.exitFullscreen();
      } else {
        void document.documentElement.requestFullscreen?.();
      }
    }
    setActiveMenu(null);
  };

  // ---------------------------------------------------------------------------
  // Editor capability flags (used to disable Edit-menu items when the
  // current editor doesn't have anything to undo/redo/etc.).
  // ---------------------------------------------------------------------------
  const editorCaps = useEditorHandleStore((state) => state.capabilities);
  const hasEditor = useEditorHandleStore((state) => state.commands !== null);
  const canUndo = hasEditor && editorCaps.canUndo;
  const canRedo = hasEditor && editorCaps.canRedo;
  const hasSelection = hasEditor && editorCaps.hasSelection;

  const menus: Menu[] = [
    {
      label: '文件',
      items: [
        { label: '新建窗口', shortcut: 'Ctrl+Shift+N', action: handleNewWindow },
        { label: '新建文件', shortcut: 'Ctrl+N', action: handleNewFile, disabled: !workspacePath },
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
        { divider: true, label: '' },
        { label: '关闭编辑器', shortcut: 'Ctrl+W', action: handleCloseEditor, disabled: !activeTab },
        { divider: true, label: '' },
        { label: '退出', shortcut: 'Alt+F4', action: handleClose },
      ],
    },
    {
      label: '编辑',
      items: [
        { label: '撤销', shortcut: 'Ctrl+Z', action: () => handleEditCommand('undo'), disabled: !canUndo },
        { label: '重做', shortcut: 'Ctrl+Y', action: () => handleEditCommand('redo'), disabled: !canRedo },
        { divider: true, label: '' },
        { label: '剪切', shortcut: 'Ctrl+X', action: () => handleEditCommand('cut'), disabled: !hasSelection },
        { label: '复制', shortcut: 'Ctrl+C', action: () => handleEditCommand('copy'), disabled: !hasSelection },
        { label: '粘贴', shortcut: 'Ctrl+V', action: () => handleEditCommand('paste'), disabled: !hasEditor },
        { divider: true, label: '' },
        { label: '全选', shortcut: 'Ctrl+A', action: () => handleEditCommand('selectAll'), disabled: !hasEditor },
        { label: '查找', shortcut: 'Ctrl+F', action: () => handleEditCommand('find'), disabled: !hasEditor },
        { label: '替换', shortcut: 'Ctrl+H', action: () => handleEditCommand('replace'), disabled: !hasEditor },
      ],
    },
    {
      label: '视图',
      items: [
        { label: '侧边栏', shortcut: 'Ctrl+B', action: handleToggleSidebar },
        { label: 'AI 面板', shortcut: 'Ctrl+Shift+L', action: handleToggleAiPanel },
        { divider: true, label: '' },
        {
          label: '放大字体',
          shortcut: 'Ctrl++',
          action: handleIncreaseFontSize,
          disabled: editorFontSize >= FONT_SIZE_MAX,
        },
        {
          label: '缩小字体',
          shortcut: 'Ctrl+-',
          action: handleDecreaseFontSize,
          disabled: editorFontSize <= FONT_SIZE_MIN,
        },
        {
          label: '重置字体大小',
          shortcut: 'Ctrl+0',
          action: handleResetFontSize,
          disabled: editorFontSize === FONT_SIZE_DEFAULT,
        },
        { divider: true, label: '' },
        {
          label: windowState === 'fullscreen' ? '退出全屏' : '全屏',
          shortcut: 'F11',
          action: handleToggleFullscreen,
        },
      ],
    },
    {
      label: '帮助',
      items: [
        { label: '快捷键参考', shortcut: 'Ctrl+K Ctrl+R', disabled: true },
      ],
    },
  ];

  return (
    <div className={styles.titleBar} ref={menuRef} data-tauri-drag-region>
      <div className={styles.brand}>
        <AppIcon size={16} className={styles.brandIcon} />
      </div>
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
