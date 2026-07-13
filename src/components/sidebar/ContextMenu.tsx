import {
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { createPortal } from 'react-dom';
import {
  ChevronRight,
  Clipboard,
  ClipboardCopy,
  Copy as CopyIcon,
  Edit3,
  ExternalLink,
  Eye,
  File,
  FilePlus,
  FileText,
  FileType2,
  FolderOpen,
  FolderPlus,
  ListChecks,
  RefreshCw,
  Scissors,
  Search,
  Trash2,
  X,
} from 'lucide-react';
import { useSidebarStore, useNotificationStore } from '../../store';
import {
  useClipboardStore,
  useConfirmDialogStore,
  useContextMenuStore,
} from '../../store';
import type { OpenTab } from '../../store';
import type { FileEntry } from '../../types';
import { NEW_FILE_TEMPLATES } from '../../types';
import {
  copyPath,
  deletePath,
  movePath,
  openWithDefaultApp,
  pathExists,
  revealInFileManager,
} from '../../services/workspace';
import { reportError } from '../../utils/errors';
import { invoke } from '@tauri-apps/api/core';
import {
  getBaseName,
  getDirName,
  getRelativePath,
  joinPath as joinDirPath,
} from '../../utils/path';
import styles from './ContextMenu.module.css';

// ----------------------------------------------------------------------------
// Menu item model
// ----------------------------------------------------------------------------

interface MenuItem {
  id: string;
  label: string;
  icon?: ReactNode;
  shortcut?: string;
  disabled?: boolean;
  danger?: boolean;
  checked?: boolean;
  submenu?: MenuItem[];
  action?: () => void | Promise<void>;
}

// ----------------------------------------------------------------------------
// Position clamping
// ----------------------------------------------------------------------------

interface Position {
  left: number;
  top: number;
}

function clampToViewport(x: number, y: number, menu: HTMLElement | null): Position {
  if (!menu) return { left: x, top: y };
  const rect = menu.getBoundingClientRect();
  const margin = 4;
  const viewportW = window.innerWidth;
  const viewportH = window.innerHeight;
  const left = Math.min(Math.max(margin, x), Math.max(margin, viewportW - rect.width - margin));
  const top = Math.min(Math.max(margin, y), Math.max(margin, viewportH - rect.height - margin));
  return { left, top };
}

// ----------------------------------------------------------------------------
// Menu item row
// ----------------------------------------------------------------------------

interface MenuRowProps {
  item: MenuItem;
  depth?: number;
}

const MenuRow = ({ item, depth = 0 }: MenuRowProps) => {
  const isSubmenu = !!item.submenu && item.submenu.length > 0;
  const className = [
    styles.item,
    item.disabled ? styles.disabled : '',
    item.danger ? styles.danger : '',
    item.checked ? styles.checked : '',
  ]
    .filter(Boolean)
    .join(' ');

  const handleClick = (e: ReactMouseEvent) => {
    e.stopPropagation();
    if (item.disabled) return;
    if (isSubmenu) return; // hover opens submenu
    item.action?.();
    useContextMenuStore.getState().close();
  };

  if (item.id === 'divider') {
    return <div role="separator" className={styles.divider} />;
  }

  const content = (
    <>
      {item.icon && <span className={styles.itemIcon}>{item.icon}</span>}
      <span className={styles.itemLabel}>{item.label}</span>
      {item.shortcut && <span className={styles.itemShortcut}>{item.shortcut}</span>}
      {isSubmenu && (
        <span className={styles.itemChevron}>
          <ChevronRight size={12} />
        </span>
      )}
    </>
  );

  if (isSubmenu) {
    return (
      <div className={styles.submenuHost}>
        <button type="button" className={className} tabIndex={depth === 0 ? 0 : -1}>
          {content}
        </button>
        <div className={`${styles.contextMenu} ${styles.submenu}`} role="menu">
          {item.submenu!.map((sub) => (
            <MenuRow key={sub.id} item={sub} depth={depth + 1} />
          ))}
        </div>
      </div>
    );
  }

  return (
    <button
      type="button"
      className={className}
      onClick={handleClick}
      onMouseDown={(e) => e.stopPropagation()}
      disabled={item.disabled}
      tabIndex={depth === 0 ? 0 : -1}
    >
      {content}
    </button>
  );
};

// ----------------------------------------------------------------------------
// Menu builder
// ----------------------------------------------------------------------------

interface MenuBuilderContext {
  workspacePath: string | null;
  openTabs: OpenTab[];
  selectedFile: string | null;
  /** Relative paths already part of the knowledge base. */
  knowledgeMembers: string[];
  /** Path to invalidate / refresh after a mutation. */
  refresh: (parentPath: string) => Promise<void> | void;
  closeMenu: () => void;
  notify: (kind: 'success' | 'error' | 'info', title: string, message?: string) => void;
}

function basename(path: string): string {
  return getBaseName(path);
}

function parentPath(path: string): string {
  return getDirName(path);
}

function joinPath(parent: string, name: string): string {
  return joinDirPath(parent, name);
}

async function uniqueDestination(parent: string, name: string, isDir: boolean): Promise<string> {
  const exists = async (p: string) => {
    try {
      return await invoke<boolean>('path_exists', { path: p });
    } catch {
      return false;
    }
  };
  const dotIdx = name.lastIndexOf('.');
  const stem = dotIdx > 0 ? name.slice(0, dotIdx) : name;
  const ext = dotIdx > 0 ? name.slice(dotIdx) : '';
  let candidate = joinPath(parent, name);
  if (!(await exists(candidate))) return candidate;
  let counter = 2;
  while (counter < 1000) {
    const tryName = isDir ? `${name} ${counter}` : `${stem} ${counter}${ext}`;
    candidate = joinPath(parent, tryName);
    if (!(await exists(candidate))) return candidate;
    counter += 1;
  }
  return joinPath(parent, `${stem}-${Date.now()}${ext}`);
}

function buildWorkspaceMenu(ctx: MenuBuilderContext): MenuItem[] {
  const { workspacePath, refresh, notify } = ctx;

  const newFileSubmenu: MenuItem[] = NEW_FILE_TEMPLATES.map((tpl) => ({
    id: `new-file-${tpl.id}`,
    label: tpl.label,
    icon: <FileType2 size={14} />,
    action: () => {
      if (!workspacePath) return;
      useSidebarStore.getState().startInlineEdit({
        parentPath: workspacePath,
        originalPath: null,
        initialValue: tpl.id === 'md' ? 'untitled.md' : `untitled.${tpl.extension}`,
        extension: tpl.extension,
        createPayload: {
          kind: 'file',
          extension: tpl.extension,
          template: tpl.template,
        },
        mode: 'create',
      });
    },
  }));

  return [
    {
      id: 'new-file',
      label: '新建文件',
      icon: <FilePlus size={14} />,
      submenu: newFileSubmenu,
      disabled: !workspacePath,
    },
    {
      id: 'new-folder',
      label: '新建文件夹',
      icon: <FolderPlus size={14} />,
      disabled: !workspacePath,
      action: () => {
        if (!workspacePath) return;
        useSidebarStore.getState().startInlineEdit({
          parentPath: workspacePath,
          originalPath: null,
          initialValue: 'untitled',
          createPayload: { kind: 'directory' },
          mode: 'create',
        });
      },
    },
    { id: 'divider', label: '' },
    {
      id: 'refresh',
      label: '重新加载工作区',
      icon: <RefreshCw size={14} />,
      disabled: !workspacePath,
      action: async () => {
        if (!workspacePath) return;
        await refresh(workspacePath);
        notify('success', '已刷新工作区');
      },
    },
    {
      id: 'reveal-workspace',
      label: '在文件管理器中显示',
      icon: <FolderOpen size={14} />,
      disabled: !workspacePath,
      action: async () => {
        if (!workspacePath) return;
        try {
          await revealInFileManager(workspacePath);
        } catch (err) {
          notify('error', '无法打开文件管理器', reportError('contextmenu-reveal', err));
        }
      },
    },
  ];
}

function buildEntryMenu(
  entry: FileEntry,
  ctx: MenuBuilderContext,
): MenuItem[] {
  const { workspacePath, openTabs, selectedFile, knowledgeMembers, refresh, notify } = ctx;
  const isDir = entry.is_dir;
  const clipboard = useClipboardStore.getState();
  const canPaste = clipboard.mode !== null && clipboard.paths.length > 0;
  const itemName = basename(entry.path);

  // Knowledge-base membership is matched on workspace-relative paths, see
  // Sidebar.tsx / KnowledgeView.tsx for the same logic.
  // Both sides need to be normalized so the comparison is correct on
  // Windows, where `entry.path` is `E:\文档\file.md` and a stored member
  // also has `\`.
  const relativePath = workspacePath
    ? getRelativePath(workspacePath, entry.path)
    : entry.path;
  const isKnowledgeMember = knowledgeMembers.includes(relativePath);

  const newFileSubmenu: MenuItem[] = isDir
    ? NEW_FILE_TEMPLATES.map((tpl) => ({
        id: `new-file-${tpl.id}`,
        label: tpl.label,
        icon: <FileType2 size={14} />,
        action: () => {
          // Auto-expand the directory so the inline input is visible.
          if (!useSidebarStore.getState().isDirExpanded(entry.path)) {
            useSidebarStore.getState().toggleDir(entry.path);
          }
          useSidebarStore.getState().startInlineEdit({
            parentPath: entry.path,
            originalPath: null,
            initialValue: tpl.id === 'md' ? 'untitled.md' : `untitled.${tpl.extension}`,
            extension: tpl.extension,
            createPayload: {
              kind: 'file',
              extension: tpl.extension,
              template: tpl.template,
            },
            mode: 'create',
          });
        },
      }))
    : [];

  const items: MenuItem[] = [];

  if (!isDir) {
    items.push({
      id: 'open',
      label: '打开',
      icon: <Eye size={14} />,
      action: () => useSidebarStore.getState().openWorkspaceFile(entry.path, { name: itemName }),
    });
    items.push({
      id: 'open-new-tab',
      label: '在新标签页中打开',
      icon: <FileText size={14} />,
      action: () =>
        useSidebarStore.getState().openWorkspaceFile(entry.path, { name: itemName, forceNew: true }),
    });
    items.push({
      id: 'open-with-os',
      label: '用系统应用打开',
      icon: <ExternalLink size={14} />,
      action: async () => {
        try {
          await openWithDefaultApp(entry.path);
        } catch (err) {
          notify('error', '无法打开', reportError('contextmenu-open-with-os', err));
        }
      },
    });
  }

  items.push({
    id: 'reveal',
    label: '在文件管理器中显示',
    icon: isDir ? <FolderOpen size={14} /> : <File size={14} />,
    action: async () => {
      try {
        await revealInFileManager(entry.path);
      } catch (err) {
        notify('error', '无法打开文件管理器', reportError('contextmenu-reveal', err));
      }
    },
  });

  if (isDir) {
    items.push({
      id: 'find-in-folder',
      label: '在此文件夹中查找',
      icon: <Search size={14} />,
      disabled: true, // Wired up in a follow-up; surfaces the intent.
      action: () => {
        // Hook into existing search input via a custom event so the Sidebar
        // can pick it up. Implementation deferred to wiring step.
        window.dispatchEvent(
          new CustomEvent('inkuo:sidebar-search', { detail: { path: entry.path } }),
        );
      },
    });
  }

  items.push({ id: 'divider-1', label: '' });

  if (isDir) {
    items.push({
      id: 'new-file',
      label: '新建文件',
      icon: <FilePlus size={14} />,
      submenu: newFileSubmenu,
    });
    items.push({
      id: 'new-folder',
      label: '新建文件夹',
      icon: <FolderPlus size={14} />,
      action: () => {
        if (!useSidebarStore.getState().isDirExpanded(entry.path)) {
          useSidebarStore.getState().toggleDir(entry.path);
        }
        useSidebarStore.getState().startInlineEdit({
          parentPath: entry.path,
          originalPath: null,
          initialValue: 'untitled',
          createPayload: { kind: 'directory' },
          mode: 'create',
        });
      },
    });
    items.push({ id: 'divider-2', label: '' });
  }

  items.push({
    id: 'cut',
    label: '剪切',
    icon: <Scissors size={14} />,
    action: () => {
      useClipboardStore.getState().setClipboard('cut', [entry.path]);
      notify('info', `已剪切：${itemName}`, '切换到目标文件夹后粘贴');
    },
  });
  items.push({
    id: 'copy',
    label: '复制',
    icon: <ClipboardCopy size={14} />,
    action: () => {
      useClipboardStore.getState().setClipboard('copy', [entry.path]);
      notify('info', `已复制：${itemName}`);
    },
  });
  if (isDir) {
    items.push({
      id: 'paste',
      label: '粘贴',
      icon: <Clipboard size={14} />,
      disabled: !canPaste,
      action: async () => {
        const state = useClipboardStore.getState();
        if (!state.mode || state.paths.length === 0) return;
        const mode = state.mode;
        const sources = state.paths;
        const wasCut = mode === 'cut';
        let movedCount = 0;
        try {
          for (const src of sources) {
            if (!src) continue;
            if (src === entry.path) continue;
            if (src.startsWith(entry.path + '/')) {
              notify('error', '无法粘贴', '目标文件夹是源文件夹的子目录');
              continue;
            }
            const exists = await pathExists(src);
            if (!exists) continue;
            const target = await uniqueDestination(entry.path, basename(src), !src.includes('.'));
            if (wasCut) {
              await movePath(src, target);
            } else {
              await copyPath(src, target);
            }
            movedCount += 1;
          }
          if (wasCut) state.clear();
          if (movedCount > 0) {
            await refresh(parentPath(entry.path));
            notify('success', wasCut ? '已移动' : '已粘贴', `${movedCount} 项`);
          }
        } catch (err) {
          notify('error', wasCut ? '移动失败' : '粘贴失败', reportError('contextmenu-paste', err));
        }
      },
    });
  }

  if (!isDir) {
    items.push({
      id: 'duplicate',
      label: '复制为',
      icon: <CopyIcon size={14} />,
      action: async () => {
        const parent = parentPath(entry.path);
        const dest = await uniqueDestination(parent, itemName, false);
        try {
          await copyPath(entry.path, dest);
          await refresh(parent);
          notify('success', '已创建副本', basename(dest));
        } catch (err) {
          notify('error', '复制失败', reportError('contextmenu-duplicate', err));
        }
      },
    });
  }

  items.push({ id: 'divider-3', label: '' });

  items.push({
    id: 'rename',
    label: '重命名',
    icon: <Edit3 size={14} />,
    action: () => {
      useSidebarStore.getState().startInlineEdit({
        parentPath: parentPath(entry.path),
        originalPath: entry.path,
        initialValue: itemName,
        mode: 'rename',
      });
    },
  });

  items.push({
    id: 'delete',
    label: '删除',
    icon: <Trash2 size={14} />,
    danger: true,
    action: async () => {
      const confirm = useConfirmDialogStore.getState().ask;
      const ok = await confirm({
        title: '确认删除',
        message: isDir
          ? `确定要删除文件夹 "${itemName}" 及其全部内容吗？此操作无法撤销。`
          : `确定要删除 "${itemName}" 吗？此操作无法撤销。`,
        confirmLabel: '删除',
        danger: true,
      });
      if (!ok) return;
      try {
        await deletePath(entry.path, isDir);
        // Close any open tab pointing at the deleted entry.
        const state = useSidebarStore.getState();
        const openTab = state.openTabs.find((t) => t.path === entry.path);
        if (openTab) state.closeTab(openTab.id);
        await refresh(parentPath(entry.path));
        notify('success', '已删除', itemName);
      } catch (err) {
        notify('error', '删除失败', reportError('contextmenu-delete', err));
      }
    },
  });

  items.push({ id: 'divider-4', label: '' });

  items.push({
    id: 'copy-path',
    label: '复制绝对路径',
    icon: <CopyIcon size={14} />,
    action: async () => {
      try {
        await navigator.clipboard.writeText(entry.path);
        notify('success', '已复制路径', entry.path);
      } catch (err) {
        notify('error', '复制路径失败', reportError('contextmenu-copy-path', err));
      }
    },
  });
  if (workspacePath) {
    items.push({
      id: 'copy-rel-path',
      label: '复制相对路径',
      icon: <CopyIcon size={14} />,
      action: async () => {
        const rel = workspacePath && entry.path.startsWith(workspacePath + '/')
          ? entry.path.slice(workspacePath.length + 1)
          : entry.path;
        try {
          await navigator.clipboard.writeText(rel);
          notify('success', '已复制相对路径', rel);
        } catch (err) {
          notify('error', '复制相对路径失败', reportError('contextmenu-copy-rel', err));
        }
      },
    });
  }

  // Knowledge-base integration (files only — folders are not indexable).
  if (!isDir) {
    items.push({ id: 'divider-5', label: '' });
    items.push({
      id: 'kb-toggle',
      label: isKnowledgeMember ? '从知识库移除' : '添加到知识库',
      icon: <ListChecks size={14} />,
      checked: isKnowledgeMember,
      action: async () => {
        try {
          const members = isKnowledgeMember
            ? knowledgeMembers.filter((p) => p !== relativePath)
            : [...knowledgeMembers, relativePath];
          if (workspacePath) {
            await invoke('knowledge_add_members', {
              workspacePath,
              members: isKnowledgeMember ? [] : [relativePath],
            });
            if (isKnowledgeMember) {
              await invoke('knowledge_remove_members', {
                workspacePath,
                members: [relativePath],
              });
            }
          }
          // Update sidebar knowledgeBase.members via store mutation; we
          // re-read from the backend rather than patch the store directly to
          // stay consistent with knowledgeStatus events.
          try {
            const status = await invoke<{
              workspace_id: string;
              total_documents: number;
              total_chunks: number;
              members: string[];
            }>('knowledge_status', { workspacePath });
            useSidebarStore.getState().setKnowledgeBase({
              workspaceId: status.workspace_id,
              documentCount: status.total_documents,
              chunkCount: status.total_chunks,
              lastUpdated: Date.now(),
              members: status.members ?? members,
            });
          } catch {
            // Status may not be available; keep local optimistic update.
            useSidebarStore.getState().setKnowledgeBase(
              useSidebarStore.getState().knowledgeBase
                ? {
                    ...useSidebarStore.getState().knowledgeBase!,
                    members,
                  }
                : undefined,
            );
          }
          notify(
            'success',
            isKnowledgeMember ? '已从知识库移除' : '已添加到知识库',
            itemName,
          );
        } catch (err) {
          notify('error', '操作知识库失败', reportError('contextmenu-kb', err));
        }
      },
    });
  }

  // Tab integration for files: close-this / close-others / close-right / close-all.
  if (!isDir) {
    const tabForPath = openTabs.find((t) => t.path === entry.path && !t.isSettings);
    if (tabForPath || openTabs.length > 1) {
      items.push({ id: 'divider-6', label: '' });
    }
    if (tabForPath) {
      items.push({
        id: 'close-tab',
        label: '关闭此标签页',
        icon: <X size={14} />,
        action: async () => {
          const ok = useSidebarStore.getState().requestCloseTab(entry.path);
          if (!ok) {
            const confirm = useConfirmDialogStore.getState().ask;
            const discard = await confirm({
              title: '未保存的更改',
              message: `"${itemName}" 有未保存的更改，关闭将丢弃。`,
              confirmLabel: '丢弃并关闭',
              danger: true,
            });
            if (discard) {
              useSidebarStore.getState().setOpenTabDirty(entry.path, false);
              useSidebarStore.getState().requestCloseTab(entry.path);
            }
          }
        },
      });
    }
    if (openTabs.length > 1) {
      const tabIndex = tabForPath ? openTabs.findIndex((t) => t.id === tabForPath.id) : -1;
      items.push({
        id: 'close-others',
        label: '关闭其他标签页',
        icon: <X size={14} />,
        action: () => {
          const state = useSidebarStore.getState();
          openTabs
            .filter((t) => t.id !== tabForPath?.id)
            .forEach((t) => {
              if (!t.isDirty && !t.isSettings) state.closeTab(t.id);
            });
          // We deliberately leave dirty / settings tabs alone to avoid silent
          // data loss. Caller can confirm via context-menu again.
        },
      });
      if (tabIndex >= 0 && tabIndex < openTabs.length - 1) {
        items.push({
          id: 'close-right',
          label: '关闭右侧标签页',
          icon: <X size={14} />,
          action: () => {
            const state = useSidebarStore.getState();
            openTabs.slice(tabIndex + 1).forEach((t) => {
              if (!t.isDirty && !t.isSettings) state.closeTab(t.id);
            });
          },
        });
      }
      items.push({
        id: 'close-all',
        label: '关闭全部标签页',
        icon: <X size={14} />,
        action: () => {
          const state = useSidebarStore.getState();
          openTabs
            .filter((t) => !t.isDirty && !t.isSettings)
            .forEach((t) => state.closeTab(t.id));
        },
      });
    }
  }

  // Active-file highlight: surface the fact in UI for clarity.
  void selectedFile;

  return items;
}

// ----------------------------------------------------------------------------
// Component
// ----------------------------------------------------------------------------

export const ContextMenu = () => {
  const target = useContextMenuStore((s) => s.target);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const [pos, setPos] = useState<Position>({ left: 0, top: 0 });

  const close = useContextMenuStore((s) => s.close);
  const workspacePath = useSidebarStore((s) => s.workspacePath);
  const openTabs = useSidebarStore((s) => s.openTabs);
  const selectedFile = useSidebarStore((s) => s.selectedFile);
  const knowledgeBase = useSidebarStore((s) => s.knowledgeBase);
  const refreshWorkspace = useSidebarStore((s) => s.invalidateCache);
  const pushNotification = useNotificationStore((s) => s.pushNotification);

  // Position the menu at the click coordinates, then clamp to viewport.
  useLayoutEffect(() => {
    if (!target) return;
    setPos({ left: target.x, top: target.y });
  }, [target]);

  useEffect(() => {
    if (!target) return;
    // After first paint, clamp to viewport.
    requestAnimationFrame(() => {
      if (menuRef.current) {
        setPos(clampToViewport(target.x, target.y, menuRef.current));
      }
    });
  }, [target]);

  // Close on outside click, Escape, scroll, resize.
  useEffect(() => {
    if (!target) return;
    const onMouseDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        close();
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        close();
      }
    };
    const onScroll = () => close();
    const onResize = () => close();
    window.addEventListener('mousedown', onMouseDown, true);
    window.addEventListener('keydown', onKey);
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('resize', onResize);
    return () => {
      window.removeEventListener('mousedown', onMouseDown, true);
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('scroll', onScroll, true);
      window.removeEventListener('resize', onResize);
    };
  }, [target, close]);

  const refresh = useCallback(
    async (parentPath: string) => {
      refreshWorkspace(parentPath);
    },
    [refreshWorkspace],
  );

  const notify = useCallback(
    (kind: 'success' | 'error' | 'info', title: string, message?: string) => {
      pushNotification({
        kind,
        title,
        ...(message ? { message } : {}),
      });
    },
    [pushNotification],
  );

  const items = useMemo<MenuItem[]>(() => {
    if (!target) return [];
    const ctx: MenuBuilderContext = {
      workspacePath,
      openTabs,
      selectedFile,
      knowledgeMembers: knowledgeBase?.members ?? [],
      refresh,
      closeMenu: close,
      notify,
    };
    if (target.kind === 'workspace') {
      return buildWorkspaceMenu(ctx);
    }
    if (target.entry) {
      return buildEntryMenu(target.entry, ctx);
    }
    return [];
  }, [target, workspacePath, openTabs, selectedFile, knowledgeBase, refresh, close, notify]);

  if (!target || typeof document === 'undefined') return null;

  return createPortal(
    <div
      ref={menuRef}
      className={styles.contextMenu}
      role="menu"
      style={{ left: pos.left, top: pos.top }}
      onMouseDown={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((item) => (
        <MenuRow key={item.id} item={item} />
      ))}
    </div>,
    document.body,
  );
};
