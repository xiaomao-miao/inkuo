import { detectFileKind } from '../types';
import { useConfirmDialogStore } from '../store/confirmDialogStore';
import { useEditorStore } from '../store/editorStore';
import {
  getDocumentSaveHandler,
  useEditorHandleStore,
} from '../store/editorHandleStore';
import { useNotificationStore } from '../store/notificationStore';
import { useSidebarStore, type OpenTab } from '../store/sidebarStore';
import { areFilePathsEqual, isPathInside } from '../utils/path';
import { persistDocument } from './documentSave';

function currentTab(tab: OpenTab): OpenTab | null {
  const tabs = useSidebarStore.getState().openTabs;
  return tabs.find((candidate) => candidate.id === tab.id)
    ?? tabs.find((candidate) => areFilePathsEqual(candidate.path, tab.path))
    ?? null;
}

function hasUnsavedChanges(tab: OpenTab): boolean {
  if (tab.isSettings || tab.isCloud) return false;
  // Text edits reach editor metadata synchronously and the sidebar dirty flag
  // in a React effect. Reading both closes the tiny "type, then immediately
  // close" race; Office publishes directly to the tab flag.
  return tab.isDirty
    || useEditorStore.getState().documentContents[tab.path]?.metadata.isDirty === true;
}

function notifySaveUnavailable(tab: OpenTab, message: string): void {
  useNotificationStore.getState().pushNotification({
    kind: 'error',
    title: `无法保存 ${tab.name}`,
    message,
  });
}

/**
 * Save one tab through the editor that owns its authoritative state.
 *
 * Text documents use the shared editor store. Word and Excel register live
 * handlers because their unsaved state lives inside Bapbong/FortuneSheet and
 * cannot safely be serialized by a toolbar or close-dialog component.
 */
export async function saveOpenTab(tab: OpenTab): Promise<boolean> {
  const liveTab = currentTab(tab);
  if (!liveTab || !hasUnsavedChanges(liveTab)) {
    return true;
  }

  const editorOwnedSave = getDocumentSaveHandler(liveTab.path);
  if (editorOwnedSave) {
    try {
      const saved = await editorOwnedSave();
      if (!saved) return false;
      // Saving is only considered complete when the editor has published its
      // clean state. This catches a broken handler before a caller closes it.
      const savedTab = currentTab(liveTab);
      if (!savedTab || !hasUnsavedChanges(savedTab)) return true;
      notifySaveUnavailable(liveTab, '编辑器未确认保存完成，文件仍保持打开。');
      return false;
    } catch (error) {
      notifySaveUnavailable(
        liveTab,
        error instanceof Error ? error.message : String(error),
      );
      return false;
    }
  }

  const kind = detectFileKind(liveTab.path);
  if (kind === 'word' || kind === 'excel') {
    notifySaveUnavailable(liveTab, '文档编辑器尚未就绪，请稍后重试。');
    return false;
  }

  const documentState = useEditorStore.getState().documentContents[liveTab.path];
  if (!documentState) {
    notifySaveUnavailable(liveTab, '没有找到可写入的编辑内容，文件仍保持打开。');
    return false;
  }

  const result = await persistDocument({
    path: liveTab.path,
    content: documentState.metadata.content,
    isDirty: true,
  });
  return result.ok;
}

function uniqueCurrentTabs(tabs: readonly OpenTab[]): OpenTab[] {
  const seen = new Set<string>();
  const result: OpenTab[] = [];
  for (const tab of tabs) {
    const liveTab = currentTab(tab);
    if (!liveTab || seen.has(liveTab.id)) continue;
    seen.add(liveTab.id);
    result.push(liveTab);
  }
  return result;
}

async function askHowToClose(dirtyTabs: readonly OpenTab[]): Promise<'save' | 'discard' | 'cancel'> {
  const single = dirtyTabs.length === 1;
  const names = dirtyTabs.slice(0, 8).map((tab) => `• ${tab.name}`).join('\n');
  const remainder = dirtyTabs.length > 8 ? `\n• 以及另外 ${dirtyTabs.length - 8} 个文件` : '';
  const result = await useConfirmDialogStore.getState().askChoice({
    title: single ? '保存对文件的更改？' : `保存 ${dirtyTabs.length} 个文件的更改？`,
    message: single
      ? `“${dirtyTabs[0].name}”有尚未保存的更改。`
      : `以下文件有尚未保存的更改：\n\n${names}${remainder}`,
    confirmLabel: single ? '保存并关闭' : '全部保存并关闭',
    secondaryLabel: single ? '不保存' : '全部不保存',
    cancelLabel: '取消',
  });
  if (result === 'confirm') return 'save';
  if (result === 'secondary') return 'discard';
  return 'cancel';
}

async function saveDirtyTabs(tabs: readonly OpenTab[]): Promise<boolean> {
  for (const tab of tabs) {
    if (!(await saveOpenTab(tab))) return false;
  }
  // A user can continue typing in an earlier document while a later Office
  // document is still saving. Re-scan the whole group before allowing a close
  // or workspace switch; otherwise that new edit would be silently discarded.
  const changedAgain = tabs.some((tab) => {
    const liveTab = currentTab(tab);
    return liveTab ? hasUnsavedChanges(liveTab) : false;
  });
  if (changedAgain) {
    useNotificationStore.getState().pushNotification({
      kind: 'warning',
      title: '保存期间又有新更改',
      message: '文件仍保持打开，请再次保存后再关闭。',
    });
    return false;
  }
  return true;
}

function closeTabs(tabs: readonly OpenTab[], discardDirty: boolean): void {
  const sidebar = useSidebarStore.getState();
  const editor = useEditorStore.getState();
  for (const tab of tabs) {
    const liveTab = currentTab(tab);
    if (!liveTab) continue;
    sidebar.closeTab(liveTab.id);
    const samePathStillOpen = useSidebarStore.getState().openTabs.some(
      (candidate) => areFilePathsEqual(candidate.path, liveTab.path),
    );
    if (discardDirty && hasUnsavedChanges(liveTab) && !samePathStillOpen) {
      // Dropping the cache is essential for text files: otherwise reopening a
      // discarded tab can resurrect its old in-memory content when mtime is
      // unchanged. A duplicate tab shares this cache, so retain it until the
      // final view of that path closes.
      editor.removeDocumentContent(liveTab.path);
    }
  }
}

/** Close one tab using the same Save / Don't Save / Cancel contract as exit. */
export async function requestCloseOpenTab(tab: OpenTab): Promise<boolean> {
  return requestCloseOpenTabs([tab]);
}

/**
 * Close a group atomically from the user's point of view. Dirty files share
 * one three-way prompt; failed saves keep every requested tab open.
 */
export async function requestCloseOpenTabs(tabs: readonly OpenTab[]): Promise<boolean> {
  const liveTabs = uniqueCurrentTabs(tabs);
  if (liveTabs.length === 0) return true;

  const dirtyTabs = liveTabs.filter(hasUnsavedChanges);
  if (dirtyTabs.length === 0) {
    closeTabs(liveTabs, false);
    return true;
  }

  const choice = await askHowToClose(dirtyTabs);
  if (choice === 'cancel') return false;
  if (choice === 'save' && !(await saveDirtyTabs(dirtyTabs))) return false;

  closeTabs(liveTabs, choice === 'discard');
  return true;
}

function pathBelongsToMutation(
  rootPath: string,
  candidatePath: string,
  includeDescendants: boolean,
): boolean {
  return includeDescendants
    ? isPathInside(rootPath, candidatePath)
    : areFilePathsEqual(rootPath, candidatePath);
}

/**
 * Run a delete / rename only after every editor affected by the old path has
 * completed the normal Save / Don't Save / Cancel lifecycle.
 *
 * Directory mutations include all descendant tabs. Closing before invoking
 * the disk mutation prevents a late editor save from recreating the old path.
 * Once the mutation succeeds, every cache entry owned by the old path is
 * removed so reopening the destination can only load the renamed on-disk
 * document, never an old in-memory snapshot.
 *
 * `false` means the user cancelled or a save failed, and `mutate` was never
 * called. Errors thrown by `mutate` are intentionally propagated to the UI so
 * it can report the filesystem failure.
 */
export async function runPathMutationWithOpenTabLifecycle(options: {
  path: string;
  includeDescendants: boolean;
  mutate: () => Promise<unknown>;
}): Promise<boolean> {
  const { path, includeDescendants, mutate } = options;
  const sidebarBeforeMutation = useSidebarStore.getState();
  const originalActiveTabId = sidebarBeforeMutation.activeTabId;
  const affectedTabs = sidebarBeforeMutation.openTabs.filter((tab) => (
    !tab.isSettings
    && !tab.isCloud
    && pathBelongsToMutation(path, tab.path, includeDescendants)
  ));

  if (!(await requestCloseOpenTabs(affectedTabs))) return false;

  // A React effect normally unregisters an Office save handler as its tab
  // unmounts. Do it synchronously as well: the filesystem mutation runs before
  // React is guaranteed to have flushed that cleanup.
  const removeAffectedSaveHandlers = () => {
    const handleStore = useEditorHandleStore.getState();
    for (const [handlerPath, handler] of handleStore.documentSaveHandlers) {
      if (pathBelongsToMutation(path, handlerPath, includeDescendants)) {
        handleStore.unregisterDocumentSaveHandler(handlerPath, handler);
      }
    }
  };
  removeAffectedSaveHandlers();

  try {
    await mutate();
  } catch (error) {
    // The disk still has the old path. Restore the affected views so a failed
    // rename/delete does not also feel like an unrelated tab-close. Discarded
    // text buffers were intentionally evicted and will reload from disk;
    // saved/clean caches can be reused. Office editors remount fresh handlers.
    const sidebar = useSidebarStore.getState();
    const restoredTabs = [...sidebar.openTabs];
    for (const tab of affectedTabs) {
      if (!restoredTabs.some((candidate) => candidate.id === tab.id)) {
        // Reaching `mutate` means every dirty tab was either saved or
        // explicitly discarded. The captured tab predates that resolution;
        // restoring its old `isDirty: true` flag would manufacture a conflict
        // against the just-saved disk version after a failed rename/delete.
        restoredTabs.push({ ...tab, isDirty: false });
      }
    }
    const restoredActiveId = originalActiveTabId
      && restoredTabs.some((tab) => tab.id === originalActiveTabId)
      ? originalActiveTabId
      : sidebar.activeTabId;
    sidebar.replaceTabs(restoredTabs, restoredActiveId);
    throw error;
  }

  // Include closed-but-cached documents as well as the tabs captured above.
  // They are safe to drop only after the mutation has actually succeeded.
  const editor = useEditorStore.getState();
  for (const cachedPath of Object.keys(editor.documentContents)) {
    if (pathBelongsToMutation(path, cachedPath, includeDescendants)) {
      editor.removeDocumentContent(cachedPath);
    }
  }
  removeAffectedSaveHandlers();
  return true;
}

/**
 * Ask whether the native window may close. The caller must synchronously call
 * `event.preventDefault()` first, await this function, then issue one guarded
 * second `window.close()` when it returns true.
 */
export async function confirmWindowClose(): Promise<boolean> {
  const tabs = uniqueCurrentTabs(useSidebarStore.getState().openTabs);
  const dirtyTabs = tabs.filter(hasUnsavedChanges);
  if (dirtyTabs.length === 0) return true;

  const choice = await askHowToClose(dirtyTabs);
  if (choice === 'cancel') return false;
  // Do not clear in-memory buffers for a window-level discard. Native close
  // can still fail; mutating the stores before it succeeds would erase the
  // only recoverable copy while leaving the window open. A restored snapshot
  // may briefly retain a stale dirty marker, but the disk load safely resets
  // that marker and never loses user content in the current process.
  if (choice === 'discard') return true;
  return saveDirtyTabs(dirtyTabs);
}

/**
 * Prepare the current editors for a same-window workspace switch without
 * closing their tabs before the outgoing workspace snapshot is written.
 * Unlike native-window discard, a successful switch immediately unmounts the
 * editors, so it is safe and necessary to evict discarded text buffers and
 * clear stale dirty flags before persisting the tab-only snapshot.
 */
export async function prepareForWorkspaceSwitch(): Promise<boolean> {
  const tabs = uniqueCurrentTabs(useSidebarStore.getState().openTabs);
  const dirtyTabs = tabs.filter(hasUnsavedChanges);
  if (dirtyTabs.length === 0) return true;

  const choice = await askHowToClose(dirtyTabs);
  if (choice === 'cancel') return false;
  if (choice === 'save') return saveDirtyTabs(dirtyTabs);

  const sidebar = useSidebarStore.getState();
  const editor = useEditorStore.getState();
  for (const tab of dirtyTabs) {
    const liveTab = currentTab(tab);
    if (!liveTab) continue;
    const kind = detectFileKind(liveTab.path);
    if (kind !== 'word' && kind !== 'excel') {
      editor.removeDocumentContent(liveTab.path);
    }
    sidebar.setOpenTabDirty(liveTab.path, false);
  }
  return true;
}
