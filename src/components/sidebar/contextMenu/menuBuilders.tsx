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
  ClipboardPaste,
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
  Languages,
  ListChecks,
  Redo2,
  RefreshCw,
  Replace,
  Scissors,
  Search,
  Search as SearchIcon,
  Sparkles,
  Trash2,
  Type,
  Undo2,
  X,
} from 'lucide-react';

import {
  useClipboardStore,
  useConfirmDialogStore,
  useContextMenuStore,
  useFloatingAiStore,
  useSidebarStore,
} from '../../../store';
import type { DocxCommands, EditorCommands, OpenTab } from '../../../store';
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
import {
  requestCloseOpenTab,
  requestCloseOpenTabs,
  runPathMutationWithOpenTabLifecycle,
} from '../../../services/openTabLifecycle';
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
        isDirectory: isDir,
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
        const deleted = await runPathMutationWithOpenTabLifecycle({
          path: entry.path,
          includeDescendants: isDir,
          mutate: () => deletePath(entry.path, isDir),
        });
        if (!deleted) return;
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
        const defaultMembers = useSidebarStore.getState().knowledgeBase?.collections.default
          ?? knowledgeMembers;
        const members = isKnowledgeMember
          ? defaultMembers.filter((p) => p !== relativePath)
          : [...defaultMembers, relativePath];
        if (workspacePath) {
          if (isKnowledgeMember) {
            await invoke('knowledge_remove_members', {
              workspacePath,
              memberPaths: [relativePath],
              collection: 'default',
            });
          } else {
            await invoke('knowledge_add_members', {
              workspacePath,
              memberPaths: [relativePath],
              sessionId: `kb-context-${Date.now()}`,
              collection: 'default',
            });
          }
        }
        // Update sidebar knowledgeBase.members via store mutation; we
        // re-read from the backend rather than patch the store directly to
        // stay consistent with knowledgeStatus events.
        try {
          const status = await invoke<{
            workspace_id: string;
            document_count: number;
            chunk_count: number;
            last_updated: string;
            members: string[];
            collections?: Record<string, string[]>;
            supported_extensions?: string[];
            documents?: Array<{
              path: string;
              collection: string;
              status: 'indexed' | 'pending' | 'error';
              chunk_count: number;
              source_type: string;
              size_bytes: number;
              indexed_at?: string | null;
              error?: string | null;
            }>;
          }>('knowledge_status', { workspacePath });
          useSidebarStore.getState().setKnowledgeBase({
            workspaceId: status.workspace_id,
            documentCount: status.document_count,
            chunkCount: status.chunk_count,
            lastUpdated: new Date(status.last_updated).getTime() || Date.now(),
            members: status.members ?? members,
            collections: status.collections ?? { default: status.members ?? members },
            supportedExtensions: status.supported_extensions ?? [],
            documents: (status.documents ?? []).map((document) => ({
              path: document.path,
              collection: document.collection,
              status: document.status,
              chunkCount: document.chunk_count,
              sourceType: document.source_type,
              sizeBytes: document.size_bytes,
              indexedAt: document.indexed_at
                ? new Date(document.indexed_at).getTime()
                : undefined,
              error: document.error ?? undefined,
            })),
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

// Tab-closing actions used to live here as a per-entry section appended
// to the file-tree right-click menu, but they fit much better on the
// tab-bar's own context menu (the user can actually see which tab they're
// acting on). The tab menu now owns these — see `buildTabMenu` below.

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
  // The file-tree shortcut explicitly manages the backward-compatible
  // default collection. `knowledgeBase.members` is a union of every named
  // collection, so using that union here would show "remove" for a file that
  // exists only in (for example) `research` and then remove nothing.
  const defaultKnowledgeMembers = useSidebarStore.getState().knowledgeBase?.collections.default
    ?? knowledgeMembers;
  const isKnowledgeMember = defaultKnowledgeMembers.includes(relativePath);

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

  // "用 AI 处理此文件" — only available for text-readable file kinds
  // (markdown, text, code, config, data). The submenu reads the file
  // lazily on action, so right-clicking a 1 MB markdown and
  // dismissing the menu doesn't pay any I/O cost.
  const aiSubmenu = buildEntryAiSubmenu(entry, ctx);
  if (aiSubmenu.length > 0) {
    items.push({ id: 'divider-ai', label: '' });
    items.push({
      id: 'entry-ai',
      label: '用 AI 处理此文件',
      icon: <Sparkles size={14} />,
      submenu: aiSubmenu,
    });
  }

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

  // Strip the trailing divider that some sections leave behind. The
  // menu should end on an action, not a hairline.
  while (items.length > 0 && isDivider(items[items.length - 1])) {
    items.pop();
  }

  return items;
}

// ── Tab menu: right-click on an editor tab ────────────────────────────────────
//
// Sections (in order, separated by dividers):
//   1. Close actions on this tab + neighbours
//   2. Bulk close (saved-only / all)
//   3. Per-tab file actions (refresh, copy path, reveal) — file tabs only
//
// Settings and Cloud tabs get a pared-down menu (no file actions), but
// they still participate in "close others" / "close all" since the user
// might want to dismiss them en masse.

/** Close this single tab. Dirty files trigger a confirm dialog. */
function appendCloseThisTab(
  items: MenuItem[],
  tab: OpenTab,
): void {
  items.push({
    id: 'tab-close',
    label: '关闭',
    icon: <X size={14} />,
    action: async () => {
      await requestCloseOpenTab(tab);
    },
  });
}

/**
 * Close every tab except the right-clicked one. Dirty files share the same
 * Save / Don't Save / Cancel prompt used everywhere else.
 */
function appendCloseOtherTabs(
  items: MenuItem[],
  tab: OpenTab,
  ctx: MenuBuilderContext,
): void {
  const otherTabs = ctx.openTabs.filter((t) => t.id !== tab.id);
  if (otherTabs.length === 0) return;

  items.push({
    id: 'tab-close-others',
    label: '关闭其他标签页',
    icon: <X size={14} />,
    action: async () => {
      await requestCloseOpenTabs(otherTabs);
    },
  });
}

/** Close every tab to the right of the right-clicked one (same lifecycle). */
function appendCloseRightTabs(
  items: MenuItem[],
  tab: OpenTab,
  ctx: MenuBuilderContext,
): void {
  const { openTabs } = ctx;
  const tabIndex = openTabs.findIndex((t) => t.id === tab.id);
  if (tabIndex < 0 || tabIndex >= openTabs.length - 1) return;

  items.push({
    id: 'tab-close-right',
    label: '关闭右侧标签页',
    icon: <X size={14} />,
    action: async () => {
      await requestCloseOpenTabs(openTabs.slice(tabIndex + 1));
    },
  });
}

/**
 * "关闭已保存的文件" — silently close every file tab that has no
 * unsaved changes. Settings / Cloud tabs are left alone (they're not
 * "files" and the user can still see them in the tab bar as anchors).
 * If nothing matches, the row is disabled so the menu doesn't lie.
 */
function appendCloseSavedTabs(
  items: MenuItem[],
  tab: OpenTab,
  ctx: MenuBuilderContext,
): void {
  const savedFileTabs = ctx.openTabs.filter(
    (t) => t.id !== tab.id && !t.isSettings && !t.isCloud && !t.isDirty,
  );

  items.push({
    id: 'tab-close-saved',
    label: '关闭已保存的文件',
    icon: <X size={14} />,
    disabled: savedFileTabs.length === 0,
    action: async () => {
      await requestCloseOpenTabs(savedFileTabs);
    },
  });
}

/**
 * "全部关闭" — close every tab. Dirty files get one three-way dialog so the
 * user can save or discard them as a group instead of answering N prompts.
 */
function appendCloseAllTabs(
  items: MenuItem[],
  _tab: OpenTab,
  ctx: MenuBuilderContext,
): void {
  const { openTabs } = ctx;
  items.push({
    id: 'tab-close-all',
    label: '全部关闭',
    icon: <X size={14} />,
    disabled: openTabs.length === 0,
    action: async () => {
      await requestCloseOpenTabs(openTabs);
    },
  });
}

/**
 * File-only actions: refresh the workspace, copy the absolute /
 * relative path to the clipboard, reveal in the OS file manager. The
 * Editor already handles "reload the current file from disk" on save,
 * so a top-level "refresh file" is intentionally not exposed here.
 */
function appendTabFileActions(
  items: MenuItem[],
  tab: OpenTab,
  ctx: MenuBuilderContext,
): void {
  if (tab.isSettings || tab.isCloud) return;
  const { workspacePath, notify } = ctx;

  items.push({ id: 'tab-file-divider', label: '' });

  items.push({
    id: 'tab-refresh',
    label: '刷新工作区',
    icon: <RefreshCw size={14} />,
    disabled: !workspacePath,
    action: async () => {
      if (!workspacePath) return;
      await reloadCurrentWorkspace();
    },
  });

  items.push({
    id: 'tab-copy-path',
    label: '复制绝对路径',
    icon: <CopyIcon size={14} />,
    action: async () => {
      try {
        await navigator.clipboard.writeText(tab.path);
        notify('success', '已复制路径', tab.path);
      } catch (err) {
        notify('error', '复制路径失败', reportError('contextmenu-tab-copy-path', err));
      }
    },
  });

  if (workspacePath) {
    items.push({
      id: 'tab-copy-rel-path',
      label: '复制相对路径',
      icon: <CopyIcon size={14} />,
      action: async () => {
        const rel = tab.path.startsWith(workspacePath + '/')
          ? tab.path.slice(workspacePath.length + 1)
          : tab.path;
        try {
          await navigator.clipboard.writeText(rel);
          notify('success', '已复制相对路径', rel);
        } catch (err) {
          notify('error', '复制相对路径失败', reportError('contextmenu-tab-copy-rel', err));
        }
      },
    });
  }

  items.push({
    id: 'tab-reveal',
    label: '在文件管理器中显示',
    icon: <FolderOpen size={14} />,
    action: async () => {
      try {
        await revealInFileManager(tab.path);
      } catch (err) {
        notify('error', '无法打开文件管理器', reportError('contextmenu-tab-reveal', err));
      }
    },
  });
}

/**
 * Right-click on an editor tab. Sections in order:
 *   1. Close this / close others / close right
 *   2. Close saved files / close all
 *   3. Refresh workspace / copy path / reveal (file tabs only)
 */
export function buildTabMenu(tab: OpenTab, ctx: MenuBuilderContext): MenuItem[] {
  const items: MenuItem[] = [];

  // ── Section 1: single-tab + neighbour closes ────────────────────────────
  appendCloseThisTab(items, tab);
  appendCloseOtherTabs(items, tab, ctx);
  appendCloseRightTabs(items, tab, ctx);

  // ── Section 2: bulk close ───────────────────────────────────────────────
  const bulkStart = items.length;
  appendCloseSavedTabs(items, tab, ctx);
  appendCloseAllTabs(items, tab, ctx);
  // If neither bulk row was actually usable, drop the section (e.g. only
  // one tab is open and the saved-files row was disabled). Keeps the menu
  // honest rather than showing a hairline divider.
  const bulkUsable = items
    .slice(bulkStart)
    .some((i) => !isDivider(i) && !i.disabled);
  if (!bulkUsable) {
    items.length = bulkStart;
  } else {
    items.splice(bulkStart, 0, { id: 'tab-divider-bulk', label: '' });
  }

  // ── Section 3: file actions (file tabs only) ────────────────────────────
  appendTabFileActions(items, tab, ctx);

  while (items.length > 0 && isDivider(items[items.length - 1])) {
    items.pop();
  }

  return items;
}

// ── Selection menu: right-click on a text selection inside the editor ──────────
//
// Fired by `OfficeViewer` when the user right-clicks on a non-collapsed
// selection in the docx editor. Every AI action here spawns a
// standalone floating popover (see `useFloatingAiStore` +
// `FloatingAiWindow`), so the action closes the menu and lets the
// user continue interacting with the document while the AI streams
// its answer in the corner. Non-AI actions stay local to the
// document (copy / search).
//
// Spawn a floating AI popover near the right-click position.
//
// Both `buildSelectionMenu` and `buildEntryMenu` call into this so the
// popover spawning logic stays in one place — including the viewport
// clamping that prevents the popover from being born off-screen when
// the user right-clicks near the bottom edge of the workspace.
//
// `quote` is the text shown in the popover header (the user's original
// passage or the file's first lines); `instruction` is what actually
// goes to the model. They can be identical, but for the entry-level
// menu the instruction wraps the file contents with a template like
// "请帮我解释以下文件的内容：\n\n\"\"\"\n<file>\n\"\"\"".
function openAiPopoverFor(input: {
  title: string;
  subtitle: string;
  quote: string;
  instruction: string;
}): void {
  const target = useContextMenuStore.getState().target;
  const baseX = target?.x ?? 40;
  const baseY = target?.y ?? 40;
  const vw = globalThis.window?.innerWidth ?? 1280;
  const vh = globalThis.window?.innerHeight ?? 800;
  // Default popover footprint. We use the *initial* size of the
  // popover here (matching what `open` will receive), so the clamp
  // produces a sensible result for the spawn position. The popover
  // can grow / shrink afterwards via the resize handle, so we don't
  // need to re-clamp on drag.
  const w = 480;
  const h = 440;
  const position = clampPopoverSpawnPosition(baseX, baseY, w, h, vw, vh);
  useContextMenuStore.getState().close();
  useFloatingAiStore.getState().open({
    ...input,
    position,
    width: w,
    height: h,
  });
}

/**
 * Clamp a popover's spawn position so the entire window stays inside
 * the viewport with a small margin. Without this, right-clicking near
 * the bottom edge spawns a popover that gets cut off (only the top is
 * visible) — the previous code used `target.y + 12` verbatim which is
 * fine for top-right clicks but unusable at the bottom.
 *
 * Anchor choice: prefer to drop the popover just below the cursor,
 * but if there's not enough room below, flip above. Likewise for the
 * right edge. This mirrors how most native context-menus auto-flip.
 */
function clampPopoverSpawnPosition(
  baseX: number,
  baseY: number,
  width: number,
  height: number,
  viewportW: number,
  viewportH: number,
): { x: number; y: number } {
  const margin = 8;
  // Anchor just below-right of the cursor, mirroring native menus.
  let x = baseX + 12;
  let y = baseY + 12;
  // Flip horizontally if we don't fit on the right.
  if (x + width + margin > viewportW) {
    x = baseX - width - 12;
  }
  // Flip vertically if we don't fit below.
  if (y + height + margin > viewportH) {
    y = baseY - height - 12;
  }
  // Final hard clamp — guarantees visibility even if both directions
  // overflow (very small viewports, very large popovers, etc.).
  const maxX = Math.max(margin, viewportW - width - margin);
  const maxY = Math.max(margin, viewportH - height - margin);
  return {
    x: Math.max(margin, Math.min(maxX, x)),
    y: Math.max(margin, Math.min(maxY, y)),
  };
}

/** Common AI-processing submenu used by both selection- and entry-level menus.
 *
 *  The selection-level caller passes its own trimmed selection as
 *  `quote`; the entry-level caller already wraps the file's contents
 *  in its own prompt template (see `buildEntryAiSubmenu`) and passes
 *  the wrapped instruction as `quote` so the header preview matches
 *  what the model actually sees. The four inner items use the same
 *  hardcoded templates so the prompts stay consistent across the two
 *  entry points. */
function buildAiProcessSubmenu(
  options: { idPrefix: string; quote: string; subtitle: string },
): MenuItem[] {
  const { idPrefix, quote, subtitle } = options;
  const trigger = (label: string, template: (q: string) => string) => () => {
    openAiPopoverFor({
      title: label,
      subtitle,
      quote,
      instruction: template(quote),
    });
  };
  return [
    {
      id: `${idPrefix}-explain`,
      label: '解释',
      icon: <Sparkles size={14} />,
      action: trigger('AI 解释', (q) => `请帮我解释以下内容：\n\n"""\n${q}\n"""`),
    },
    {
      id: `${idPrefix}-translate`,
      label: '翻译成英文',
      icon: <Languages size={14} />,
      action: trigger(
        'AI 翻译',
        (q) => `请把以下内容翻译成英文（保留原文格式与代码块）：\n\n"""\n${q}\n"""`,
      ),
    },
    {
      id: `${idPrefix}-summarize`,
      label: '总结',
      icon: <ListChecks size={14} />,
      action: trigger(
        'AI 总结',
        (q) => `请简要总结以下内容的要点：\n\n"""\n${q}\n"""`,
      ),
    },
    {
      id: `${idPrefix}-rewrite`,
      label: '改写',
      icon: <FileText size={14} />,
      action: trigger(
        'AI 改写',
        (q) => `请把以下内容改写得更清晰流畅，保留原意：\n\n"""\n${q}\n"""`,
      ),
    },
  ];
}

/**
 * File kinds that can be safely read as plain text and fed to the
 * floating AI popover. Docx / xlsx / pdf / images are out — they need
 * a richer extractor path (already covered elsewhere) and a raw read
 * would produce garbage. Code / config / data are all UTF-8-friendly
 * in practice so we include them too.
 */
const AI_TEXT_FILE_KINDS = new Set([
  'markdown',
  'text',
  'code',
  'config',
  'data',
]);

/** Cap for file content sent into the AI prompt. Large files would
 *  blow past the model's context window. 24 KB ≈ 6k CJK chars, which
 *  fits comfortably in any reasonable ask-mode model. The cap is
 *  applied per file, per popover; a separate popover can be opened
 *  for additional content if needed. */
const AI_FILE_PROMPT_BYTE_CAP = 24 * 1024;

/** Build an "AI 处理" submenu for a file-tree entry. Returns an empty
 *  array when the entry is a directory or a binary kind — callers
 *  can spread the result unconditionally. The menu reads the file
 *  lazily on action so we don't pay the I/O cost when the user just
 *  browses the right-click options. */
function buildEntryAiSubmenu(
  entry: FileEntry,
  ctx: MenuBuilderContext,
): MenuItem[] {
  const { notify } = ctx;
  if (entry.is_dir) return [];
  if (!AI_TEXT_FILE_KINDS.has(entry.file_kind)) return [];

  const itemName = basename(entry.path);
  const subtitle = `${itemName} · ${entry.file_kind}`;

  // The handler is shared across all four submenu items — they only
  // differ in their prompt template, so we read the file once and
  // synthesize four actions off the same in-memory content.
  const trigger = (
    label: string,
    buildInstruction: (q: string) => string,
  ) =>
    async () => {
      try {
        const result = await invoke<{ content: string }>('read_document', {
          path: entry.path,
        });
        const full = result?.content ?? '';
        // Truncate at a character boundary to avoid splitting a
        // multi-byte UTF-8 sequence when the cap lands mid-character.
        let content = full;
        if (content.length > AI_FILE_PROMPT_BYTE_CAP) {
          content = `${content.slice(0, AI_FILE_PROMPT_BYTE_CAP)}\n\n[…内容已截断…]`;
        }
        const trimmed = content.trim();
        if (!trimmed) {
          notify('error', '文件为空', itemName);
          return;
        }
        // Show the *truncated* content in the popover header quote
        // too — keeps the visible excerpt aligned with what the
        // model actually sees.
        openAiPopoverFor({
          title: label,
          subtitle,
          quote: trimmed.length > 800 ? `${trimmed.slice(0, 800)}…` : trimmed,
          instruction: buildInstruction(trimmed),
        });
      } catch (err) {
        notify(
          'error',
          '读取文件失败',
          reportError('contextmenu-ai-file-read', err),
        );
      }
    };

  return [
    {
      id: 'entry-ai-explain',
      label: '解释',
      icon: <Sparkles size={14} />,
      action: trigger(
        'AI 解释',
        (q) => `请帮我解释以下文件的内容：\n\n"""\n${q}\n"""`,
      ),
    },
    {
      id: 'entry-ai-translate',
      label: '翻译成英文',
      icon: <Languages size={14} />,
      action: trigger(
        'AI 翻译',
        (q) => `请把以下文件的内容翻译成英文（保留原文格式与代码块）：\n\n"""\n${q}\n"""`,
      ),
    },
    {
      id: 'entry-ai-summarize',
      label: '总结',
      icon: <ListChecks size={14} />,
      action: trigger(
        'AI 总结',
        (q) => `请简要总结以下文件的要点：\n\n"""\n${q}\n"""`,
      ),
    },
    {
      id: 'entry-ai-rewrite',
      label: '改写',
      icon: <FileText size={14} />,
      action: trigger(
        'AI 改写',
        (q) => `请把以下文件的内容改写得更清晰流畅，保留原意：\n\n"""\n${q}\n"""`,
      ),
    },
  ];
}

export function buildSelectionMenu(
  selectionText: string,
  ctx: MenuBuilderContext,
): MenuItem[] {
  const { closeMenu, notify } = ctx;
  const text = selectionText;
  const trimmed = text.trim();
  // Empty / whitespace-only selections should never reach the builder,
  // but guard anyway so a misuse can't render a useless menu.
  if (trimmed.length === 0) return [];

  const copyAndClose = () => {
    closeMenu();
  };

  const copyToClipboard = async () => {
    try {
      await navigator.clipboard.writeText(text);
      notify('success', '已复制', `${text.length} 字`);
    } catch (err) {
      notify('error', '复制失败', reportError('contextmenu-selection-copy', err));
    }
    copyAndClose();
  };

  const searchSelection = () => {
    // The CmdK palette already supports `? <query>` to push a search
    // intent. Dispatching through it keeps the workspace search and
    // the context-menu search on the same code path.
    window.dispatchEvent(
      new CustomEvent('inkuo:workspace-search', { detail: { query: trimmed } }),
    );
    copyAndClose();
  };

  // The selection-level AI submenu shares its core with the entry-level
  // AI submenu (both feed the same floating AI popover). The shared
  // helper also clamps the spawn position so right-clicking near the
  // bottom-right corner doesn't push the popover off-screen.
  const aiSubmenu = buildAiProcessSubmenu({
    idPrefix: 'selection-ai',
    quote: trimmed,
    subtitle: `选区 · ${trimmed.length} 字`,
  });

  return [
    {
      id: 'selection-ai',
      label: '用 AI 处理选中文本',
      icon: <Sparkles size={14} />,
      submenu: aiSubmenu,
    },
    {
      id: 'selection-search',
      label: '在工作区中搜索',
      icon: <Search size={14} />,
      action: searchSelection,
    },
    { id: DIVIDER_ID, label: '' },
    {
      id: 'selection-copy',
      label: '复制',
      icon: <ClipboardCopy size={14} />,
      action: copyToClipboard,
    },
  ];
}

// ── DOCX editor menu: right-click on the docx editor (empty or collapsed caret) ──
//
// Fired by `OfficeViewer` for any right-click inside the docx editor
// container — regardless of whether the user has a non-empty selection.
// The previous behaviour fell through to the webview's native context
// menu on empty selections, which Chromium renders snapped to whatever
// happens to be on screen (often the bottom-right of the viewport for
// spell-check suggestions). Routing everything through our app menu
// keeps the experience consistent across selections and gives the user
// real, keyboard-accessible editing actions instead of the OS default.
//
// We deliberately keep the menu small (Undo / Redo / Cut / Copy /
// Paste / Find / Replace / Select All) — these are the actions every
// user expects on a right-click in any text area. The docx editor's
// own AI / rewrite toolbar is a different surface (the "组件自带的"
// right-click menu the user reported in the bug report) and stays
// under the AI flow rather than the right-click menu.
//
// The PM commands are captured at click time via `commands` so the
// action closures don't need to re-resolve the editor view later.
export function buildDocxMenu(
  commands: DocxCommands,
  ctx: MenuBuilderContext,
): MenuItem[] {
  const { closeMenu } = ctx;
  const items: MenuItem[] = [];

  const wrap = (action: () => void): (() => void) => {
    const wrapped = () => {
      try {
        action();
      } finally {
        closeMenu();
      }
    };
    return wrapped;
  };

  items.push({
    id: 'docx-undo',
    label: '撤销',
    icon: <Undo2 size={14} />,
    shortcut: '⌘Z',
    disabled: !commands.canUndo,
    action: wrap(commands.undo),
  });
  items.push({
    id: 'docx-redo',
    label: '重做',
    icon: <Redo2 size={14} />,
    shortcut: '⌘⇧Z',
    disabled: !commands.canRedo,
    action: wrap(commands.redo),
  });
  items.push({ id: DIVIDER_ID, label: '' });
  items.push({
    id: 'docx-cut',
    label: '剪切',
    icon: <Scissors size={14} />,
    shortcut: '⌘X',
    disabled: !commands.hasSelection,
    action: wrap(commands.cut),
  });
  items.push({
    id: 'docx-copy',
    label: '复制',
    icon: <ClipboardCopy size={14} />,
    shortcut: '⌘C',
    disabled: !commands.hasSelection,
    action: wrap(commands.copy),
  });
  items.push({
    id: 'docx-paste',
    label: '粘贴',
    icon: <ClipboardPaste size={14} />,
    shortcut: '⌘V',
    disabled: !commands.hasClipboard,
    action: wrap(commands.paste),
  });
  items.push({ id: DIVIDER_ID, label: '' });
  items.push({
    id: 'docx-find',
    label: '查找',
    icon: <SearchIcon size={14} />,
    shortcut: '⌘F',
    action: wrap(commands.find),
  });
  items.push({
    id: 'docx-replace',
    label: '替换',
    icon: <Replace size={14} />,
    shortcut: '⌘H',
    action: wrap(commands.replace),
  });
  items.push({
    id: 'docx-select-all',
    label: '全选',
    icon: <Type size={14} />,
    shortcut: '⌘A',
    action: wrap(commands.selectAll),
  });

  return items;
}

/**
 * Empty-selection right-click menu for the markdown / code / text
 * editor (CodeMirror). Mirrors `buildDocxMenu` for the docx editor
 * but is scoped down: no Undo/Redo (CodeMirror's history keymap is
 * mounted separately and the user can already hit ⌘Z directly), and
 * adds an "AI 处理当前文件" submenu that reads the live document
 * content via `commands.readContent()`.
 *
 * When the user has a non-empty selection we route them to
 * `buildSelectionMenu` instead (kind: 'selection'), which already
 * includes the AI submenu. The two paths overlap a little at the
 * "用 AI 处理" boundary but the selection-menu version acts on the
 * passage the user highlighted, while this one acts on the whole
 * document — they target different intents.
 */
export function buildEditorMenu(
  commands: EditorCommands,
  ctx: MenuBuilderContext,
): MenuItem[] {
  const { closeMenu, selectedFile, notify } = ctx;
  const items: MenuItem[] = [];

  const wrap = (action: () => void): (() => void) => {
    const wrapped = () => {
      try {
        action();
      } finally {
        closeMenu();
      }
    };
    return wrapped;
  };

  items.push({
    id: 'editor-cut',
    label: '剪切',
    icon: <Scissors size={14} />,
    shortcut: '⌘X',
    disabled: !commands.readContent || commands.readContent().length === 0,
    action: wrap(commands.cut),
  });
  items.push({
    id: 'editor-copy',
    label: '复制',
    icon: <ClipboardCopy size={14} />,
    shortcut: '⌘C',
    action: wrap(commands.copy),
  });
  items.push({
    id: 'editor-paste',
    label: '粘贴',
    icon: <ClipboardPaste size={14} />,
    shortcut: '⌘V',
    action: wrap(commands.paste),
  });
  items.push({ id: DIVIDER_ID, label: '' });

  items.push({
    id: 'editor-find',
    label: '查找',
    icon: <SearchIcon size={14} />,
    shortcut: '⌘F',
    action: wrap(commands.find),
  });
  items.push({
    id: 'editor-replace',
    label: '替换',
    icon: <Replace size={14} />,
    shortcut: '⌘H',
    action: wrap(commands.replace),
  });
  items.push({
    id: 'editor-select-all',
    label: '全选',
    icon: <Type size={14} />,
    shortcut: '⌘A',
    action: wrap(commands.selectAll),
  });

  // "AI 处理当前文件" — same idea as the file-tree entry AI submenu,
  // but uses the *live* editor buffer instead of re-reading the file
  // from disk. This is the right behavior for users who have unsaved
  // edits in the editor: the popover should reason about what's
  // actually on screen, not the last saved snapshot.
  if (selectedFile) {
    const aiSubmenu = buildEditorFileAiSubmenu(
      commands,
      selectedFile,
      notify,
      closeMenu,
    );
    if (aiSubmenu.length > 0) {
      items.push({ id: DIVIDER_ID, label: '' });
      items.push({
        id: 'editor-ai',
        label: '用 AI 处理当前文件',
        icon: <Sparkles size={14} />,
        submenu: aiSubmenu,
      });
    }
  }

  return items;
}

/**
 * Build the four-item AI submenu for the editor empty-selection
 * context. Reads the live document content via `commands.readContent`
 * so the user can pop the AI window on unsaved edits.
 *
 * Returns an empty array when the buffer is empty so the caller can
 * skip the section cleanly.
 */
function buildEditorFileAiSubmenu(
  commands: EditorCommands,
  filePath: string,
  notify: MenuBuilderContext['notify'],
  closeMenu: () => void,
): MenuItem[] {
  // Read the buffer synchronously here — `commands.readContent` is a
  // cheap accessor over the live CM state. Doing it inside the
  // `trigger` closures would also work but means the user could
  // dismiss the menu, edit the file, and have the AI run on stale
  // content. Snapshot at menu-build time instead, which matches the
  // snapshot-at-right-click rationale used elsewhere in this module.
  const full = (commands.readContent() ?? '').trim();
  if (!full) return [];

  const itemName = basename(filePath);
  let content = full;
  if (content.length > AI_FILE_PROMPT_BYTE_CAP) {
    content = `${content.slice(0, AI_FILE_PROMPT_BYTE_CAP)}\n\n[…内容已截断…]`;
  }
  const quote = content.length > 800 ? `${content.slice(0, 800)}…` : content;
  const subtitle = `${itemName} · 编辑器中`;

  const trigger = (label: string, template: (q: string) => string) => () => {
    openAiPopoverFor({
      title: label,
      subtitle,
      quote,
      instruction: template(content),
    });
    closeMenu();
  };

  // Surface a notification if the buffer is too small to be useful.
  if (content.length < 4) {
    return [];
  }
  void notify;

  return [
    {
      id: 'editor-ai-explain',
      label: '解释',
      icon: <Sparkles size={14} />,
      action: trigger('AI 解释', (q) => `请帮我解释以下文件的内容：\n\n"""\n${q}\n"""`),
    },
    {
      id: 'editor-ai-translate',
      label: '翻译成英文',
      icon: <Languages size={14} />,
      action: trigger(
        'AI 翻译',
        (q) => `请把以下文件的内容翻译成英文（保留原文格式与代码块）：\n\n"""\n${q}\n"""`,
      ),
    },
    {
      id: 'editor-ai-summarize',
      label: '总结',
      icon: <ListChecks size={14} />,
      action: trigger('AI 总结', (q) => `请简要总结以下文件的要点：\n\n"""\n${q}\n"""`),
    },
    {
      id: 'editor-ai-rewrite',
      label: '改写',
      icon: <FileText size={14} />,
      action: trigger(
        'AI 改写',
        (q) => `请把以下文件的内容改写得更清晰流畅，保留原意：\n\n"""\n${q}\n"""`,
      ),
    },
  ];
}
