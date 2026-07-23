// Pure-menu builders for the workspace context menu.
//
// `buildWorkspaceMenu` describes the menu that shows when the user
// right-clicks an empty area inside the file tree. `buildEntryMenu`
// describes the per-entry menu (right-click on a file or folder) and
// is the bulk of the file — it composes dozens of actions that read
// from the sidebar / clipboard / settings stores, mutate backend
// state via Tauri, and refresh the tree.
//
// Split out of the original `ContextMenu.tsx` so the menu shape and
// its side-effect closures are easy to scan / edit / mock. The
// orchestrating React component is now a thin wrapper around these.

import { invoke } from '@tauri-apps/api/core';
import {
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

import {
  useClipboardStore,
  useConfirmDialogStore,
  useSidebarStore,
} from '../../../store';
import type { FileEntry, NewFileTemplate } from '../../../types';
import { NEW_FILE_TEMPLATES } from '../../../types';
import {
  copyPath,
  deletePath,
  movePath,
  openWithDefaultApp,
  pathExists,
  reloadCurrentWorkspace,
  revealInFileManager,
} from '../../../services/workspace';
import { reportError } from '../../../utils/errors';
import { getRelativePath } from '../../../utils/path';

import { basename, joinPath, parentPath, uniqueSiblingName } from './pathHelpers';
import {
  DIVIDER_ID,
  isDivider,
  type MenuBuilderContext,
  type MenuItem,
} from './types';

/**
 * Compose an entry-submenu populated from `NEW_FILE_TEMPLATES`. When
 * `parentDir` is provided, the inline-editor is anchored inside that
 * directory (auto-expanding it if it's a collapsed folder).
 *
 * Split out because it's used by both the empty-area builder and the
 * per-entry builder, with subtly different parent-dir logic.
 */
function buildNewFileSubmenu(
  parentDir: string,
  options?: { autoExpand?: boolean },
): MenuItem[] {
  return NEW_FILE_TEMPLATES.map((tpl) =>
    buildNewFileTemplateItem(tpl, parentDir, options),
  );
}

function buildNewFileTemplateItem(
  tpl: NewFileTemplate,
  parentDir: string,
  options?: { autoExpand?: boolean },
): MenuItem {
  return {
    id: `new-file-${tpl.id}`,
    label: tpl.label,
    icon: <FileType2 size={14} />,
    action: () => {
      if (options?.autoExpand) {
        if (!useSidebarStore.getState().isDirExpanded(parentDir)) {
          useSidebarStore.getState().toggleDir(parentDir);
        }
      }
      useSidebarStore.getState().startInlineEdit({
        parentPath: parentDir,
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
  };
}

/**
 * Build a destination path for `parent / name` that doesn't collide
 * with anything already on disk. Strategy:
 *
 *   1. try `parent / name`
 *   2. if it exists, increment a counter (`- 2`, ` 3`, ...) until a
 *      slot is free (up to ~1000 tries)
 *   3. fall back to `parent / name-<timestamp>` (matches the legacy
 *      Sidebar inline-create behavior so users see consistent names
 *      whether they typed or pasted)
 *
 * `isDir` controls the suffix shape — folders use `"<name> <n>"`,
 * files preserve the extension by appending the counter before it.
 */
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
  return uniqueSiblingName(parent, name);
}

/**
 * Right-click on empty area in the file tree: new file / new folder,
 * refresh, reveal-in-file-manager. Self-contained — no per-entry
 * state to inject.
 */
export function buildWorkspaceMenu(ctx: MenuBuilderContext): MenuItem[] {
  const { workspacePath, notify } = ctx;

  const newFileSubmenu = workspacePath
    ? buildNewFileSubmenu(workspacePath)
    : [];

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
    { id: DIVIDER_ID, label: '' },
    {
      id: 'refresh',
      label: '重新加载工作区',
      icon: <RefreshCw size={14} />,
      disabled: !workspacePath,
      action: async () => {
        if (!workspacePath) return;
        // `reloadCurrentWorkspace` walks `clearCache` → root read → each
        // expanded-dir read, wrapped in an `isLoading` skeleton. Earlier
        // this menu only called `invalidateCache(workspacePath)`, which
        // blanks the visible tree without refetching and left the user
        // staring at an empty side-panel until they expanded something.
        await reloadCurrentWorkspace();
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

// ── Per-entry menu: split into focused builders ────────────────────────────────
//
// `buildEntryMenu` is broken into append-helpers that each push their
// section onto a shared `items` array. This makes the menu shape far
// easier to follow than one giant function, and keeps the helper names
// self-documenting in error messages / future test descriptions.

/** Open-the-file section: open, open-new-tab, open-with-OS. */
function appendOpenItems(
  items: MenuItem[],
  entry: FileEntry,
  itemName: string,
  notify: MenuBuilderContext['notify'],
): void {
  items.push({
    id: 'open',
    label: '打开',
    icon: <Eye size={14} />,
    action: () =>
      useSidebarStore.getState().openWorkspaceFile(entry.path, { name: itemName }),
  });
  items.push({
    id: 'open-new-tab',
    label: '在新标签页中打开',
    icon: <FileText size={14} />,
    action: () =>
      useSidebarStore.getState().openWorkspaceFile(entry.path, {
        name: itemName,
        forceNew: true,
      }),
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

/** Reveal, then optionally "find in folder" (placeholder for the search hook). */
function appendExploreItems(
  items: MenuItem[],
  entry: FileEntry,
  isDir: boolean,
  notify: MenuBuilderContext['notify'],
): void {
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
}

/** New file / new folder inside an entry — directory targets only. */
function appendNewItems(
  items: MenuItem[],
  entry: FileEntry,
  isDir: boolean,
): void {
  if (!isDir) return;
  items.push({
    id: 'new-file',
    label: '新建文件',
    icon: <FilePlus size={14} />,
    submenu: buildNewFileSubmenu(entry.path, { autoExpand: true }),
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
  items.push({ id: 'new-divider', label: '' });
}

/** Cut / copy / (paste + duplicate). Paste is folder-only; duplicate is file-only. */
function appendClipboardItems(
  items: MenuItem[],
  entry: FileEntry,
  itemName: string,
  isDir: boolean,
  ctx: MenuBuilderContext,
): void {
  const { refresh, notify } = ctx;
  const clipboard = useClipboardStore.getState();
  const canPaste = clipboard.mode !== null && clipboard.paths.length > 0;

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
}

/** Rename (always) and delete (with confirmation). */
function appendMutateItems(
  items: MenuItem[],
  entry: FileEntry,
  itemName: string,
  isDir: boolean,
  ctx: MenuBuilderContext,
): void {
  const { refresh, notify } = ctx;

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
}

/** Copy absolute path / copy relative path (when in a workspace). */
function appendPathCopyItems(
  items: MenuItem[],
  entry: FileEntry,
  workspacePath: string | null,
  notify: MenuBuilderContext['notify'],
): void {
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
}

/**
 * Files only: add / remove the entry from the workspace knowledge
 * base. After the backend `knowledge_add_members` /
 * `knowledge_remove_members` calls, we re-read the workspace's status
 * to keep the sidebar's knowledgeBase store in sync — patching the
 * store directly would race with backend events.
 */
function appendKnowledgeItems(
  items: MenuItem[],
  entry: FileEntry,
  itemName: string,
  isDir: boolean,
  workspacePath: string | null,
  relativePath: string,
  isKnowledgeMember: boolean,
  ctx: MenuBuilderContext,
): void {
  if (isDir || !workspacePath) return;
  const { knowledgeMembers, notify } = ctx;
  void entry;

  items.push({ id: 'kb-divider', label: '' });
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

/** Files only: close-this / close-others / close-right / close-all (when tabs are open). */
function appendTabItems(
  items: MenuItem[],
  entry: FileEntry,
  isDir: boolean,
  ctx: MenuBuilderContext,
): void {
  if (isDir) return;
  const { openTabs } = ctx;
  const tabForPath = openTabs.find((t) => t.path === entry.path && !t.isSettings);
  if (tabForPath || openTabs.length > 1) {
    items.push({ id: 'tabs-divider', label: '' });
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
            message: `"${basename(entry.path)}" 有未保存的更改，关闭将丢弃。`,
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
    const tabIndex = tabForPath
      ? openTabs.findIndex((t) => t.id === tabForPath.id)
      : -1;
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

/**
 * Right-click on a file tree entry. Composes the per-entry menu by
 * calling the section builders above. Section dividers between
 * groups keep the visual rhythm consistent — each section can also
 * drop its own divider where it makes sense (e.g. before "kb").
 */
export function buildEntryMenu(
  entry: FileEntry,
  ctx: MenuBuilderContext,
): MenuItem[] {
  const { workspacePath, openTabs, knowledgeMembers, notify } = ctx;
  // `selectedFile` is part of the context surface so other consumers
  // can build context menus that depend on it. The entry menu
  // currently doesn't, so silence the unused-var warning.
  void ctx.selectedFile;
  void openTabs;

  const isDir = entry.is_dir;
  const itemName = basename(entry.path);

  // Knowledge-base membership is matched on workspace-relative paths,
  // see Sidebar.tsx / KnowledgeView.tsx for the same logic. Both sides
  // need to be normalized so the comparison is correct on Windows,
  // where `entry.path` is `E:\文档\file.md` and a stored member also
  // has `\`.
  const relativePath = workspacePath
    ? getRelativePath(workspacePath, entry.path)
    : entry.path;
  const isKnowledgeMember = knowledgeMembers.includes(relativePath);

  const items: MenuItem[] = [];

  if (!isDir) appendOpenItems(items, entry, itemName, notify);
  appendExploreItems(items, entry, isDir, notify);
  items.push({ id: 'divider-explore', label: '' });

  if (isDir) {
    appendNewItems(items, entry, isDir);
  }
  appendClipboardItems(items, entry, itemName, isDir, ctx);
  items.push({ id: 'divider-clip', label: '' });

  appendMutateItems(items, entry, itemName, isDir, ctx);
  items.push({ id: 'divider-mut', label: '' });

  appendPathCopyItems(items, entry, workspacePath, notify);

  appendKnowledgeItems(
    items,
    entry,
    itemName,
    isDir,
    workspacePath,
    relativePath,
    isKnowledgeMember,
    ctx,
  );

  appendTabItems(items, entry, isDir, ctx);

  // Strip the trailing divider that some sections leave behind. The
  // menu should end on an action, not a hairline.
  while (items.length > 0 && isDivider(items[items.length - 1])) {
    items.pop();
  }

  return items;
}
