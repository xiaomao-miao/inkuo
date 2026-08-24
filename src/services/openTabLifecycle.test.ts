import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useConfirmDialogStore } from '../store/confirmDialogStore';
import { useEditorHandleStore } from '../store/editorHandleStore';
import { useEditorStore } from '../store/editorStore';
import { useSidebarStore, type OpenTab } from '../store/sidebarStore';
import {
  confirmWindowClose,
  prepareForWorkspaceSwitch,
  requestCloseOpenTab,
  runPathMutationWithOpenTabLifecycle,
} from './openTabLifecycle';

const fileTab = (overrides: Partial<OpenTab> = {}): OpenTab => ({
  id: 'tab-a',
  path: '/workspace/a.docx',
  name: 'a.docx',
  isDirty: true,
  ...overrides,
});

function installTabs(tabs: OpenTab[]): void {
  useSidebarStore.setState({
    openTabs: tabs,
    activeTabId: tabs[0]?.id ?? null,
    selectedFile: tabs[0]?.path ?? null,
  });
}

describe('open tab lifecycle', () => {
  beforeEach(() => {
    installTabs([]);
    useEditorStore.setState({ documentContents: {} });
    useEditorHandleStore.setState({ documentSaveHandlers: new Map() });
    useConfirmDialogStore.setState({ request: null });
  });

  it('keeps a dirty tab open when the user cancels', async () => {
    const tab = fileTab();
    installTabs([tab]);

    const closing = requestCloseOpenTab(tab);
    expect(useConfirmDialogStore.getState().request?.secondaryLabel).toBe('不保存');
    useConfirmDialogStore.getState().close('cancel');

    await expect(closing).resolves.toBe(false);
    expect(useSidebarStore.getState().openTabs).toHaveLength(1);
  });

  it('saves through the editor-owned handler before closing', async () => {
    const tab = fileTab();
    installTabs([tab]);
    const save = vi.fn(async () => {
      useSidebarStore.getState().setOpenTabDirty(tab.path, false);
      return true;
    });
    useEditorHandleStore.getState().registerDocumentSaveHandler(tab.path, save);

    const closing = requestCloseOpenTab(tab);
    useConfirmDialogStore.getState().close('confirm');

    await expect(closing).resolves.toBe(true);
    expect(save).toHaveBeenCalledOnce();
    expect(useSidebarStore.getState().openTabs).toHaveLength(0);
  });

  it('never closes when saving fails', async () => {
    const tab = fileTab();
    installTabs([tab]);
    useEditorHandleStore.getState().registerDocumentSaveHandler(
      tab.path,
      vi.fn(async () => false),
    );

    const closing = requestCloseOpenTab(tab);
    useConfirmDialogStore.getState().close('confirm');

    await expect(closing).resolves.toBe(false);
    expect(useSidebarStore.getState().openTabs).toHaveLength(1);
  });

  it('detects a text edit before the sidebar dirty effect and evicts it on discard', async () => {
    const tab = fileTab({
      path: '/workspace/a.md',
      name: 'a.md',
      isDirty: false,
    });
    installTabs([tab]);
    useEditorStore.getState().setDocumentContent(tab.path, {
      id: 'a',
      path: tab.path,
      doc_type: 'Markdown',
      title: 'a',
      blocks: [],
      updated_at: '',
      hash: '',
    }, 'saved', 1);
    useEditorStore.getState().setContent(tab.path, 'draft');

    const closing = requestCloseOpenTab(tab);
    expect(useConfirmDialogStore.getState().request).not.toBeNull();
    useConfirmDialogStore.getState().close('secondary');

    await expect(closing).resolves.toBe(true);
    expect(useSidebarStore.getState().openTabs).toHaveLength(0);
    expect(useEditorStore.getState().documentContents[tab.path]).toBeUndefined();
  });

  it('saves on window close without prematurely removing tabs', async () => {
    const tab = fileTab();
    installTabs([tab]);
    const save = vi.fn(async () => {
      useSidebarStore.getState().setOpenTabDirty(tab.path, false);
      return true;
    });
    useEditorHandleStore.getState().registerDocumentSaveHandler(tab.path, save);

    const closing = confirmWindowClose();
    useConfirmDialogStore.getState().close('confirm');

    await expect(closing).resolves.toBe(true);
    expect(save).toHaveBeenCalledOnce();
    expect(useSidebarStore.getState().openTabs).toHaveLength(1);
  });

  it('keeps recoverable buffers intact until native window close succeeds', async () => {
    const tab = fileTab();
    installTabs([tab]);

    const closing = confirmWindowClose();
    useConfirmDialogStore.getState().close('secondary');

    await expect(closing).resolves.toBe(true);
    // If native close subsequently fails, the app can still prompt/save this
    // tab instead of having destroyed its only in-memory state.
    expect(useSidebarStore.getState().openTabs[0].isDirty).toBe(true);
  });

  it('does not let an old cleanup remove a replacement save handler', () => {
    const path = '/workspace/a.docx';
    const oldHandler = vi.fn(async () => true);
    const newHandler = vi.fn(async () => true);
    const store = useEditorHandleStore.getState();
    store.registerDocumentSaveHandler(path, oldHandler);
    store.registerDocumentSaveHandler(path, newHandler);
    store.unregisterDocumentSaveHandler(path, oldHandler);

    expect(useEditorHandleStore.getState().documentSaveHandlers.get(path)).toBe(newHandler);
  });

  it('focuses an existing Office document instead of creating a competing editor', () => {
    const tab = fileTab();
    installTabs([tab]);

    useSidebarStore.getState().openWorkspaceFile(tab.path, { forceNew: true });

    expect(useSidebarStore.getState().openTabs).toEqual([tab]);
    expect(useSidebarStore.getState().activeTabId).toBe(tab.id);
  });

  it('deduplicates Windows Office path aliases case-insensitively', () => {
    const tab = fileTab({
      id: 'C:\\Work\\Paper.DOCX',
      path: 'C:\\Work\\Paper.DOCX',
      name: 'Paper.DOCX',
    });
    installTabs([tab]);

    useSidebarStore.getState().openWorkspaceFile('c:/work/paper.docx', { forceNew: true });

    expect(useSidebarStore.getState().openTabs).toEqual([tab]);
    expect(useSidebarStore.getState().activeTabId).toBe(tab.id);
  });

  it('cancels a workspace switch while a dirty editor is open', async () => {
    const tab = fileTab();
    installTabs([tab]);

    const switching = prepareForWorkspaceSwitch();
    useConfirmDialogStore.getState().close('cancel');

    await expect(switching).resolves.toBe(false);
    expect(useSidebarStore.getState().openTabs[0].isDirty).toBe(true);
  });

  it('evicts a discarded text buffer before switching workspaces', async () => {
    const tab = fileTab({
      path: '/workspace/a.md',
      name: 'a.md',
      isDirty: false,
    });
    installTabs([tab]);
    useEditorStore.getState().setDocumentContent(tab.path, {
      id: 'a',
      path: tab.path,
      doc_type: 'Markdown',
      title: 'a',
      blocks: [],
      updated_at: '',
      hash: '',
    }, 'saved', 1);
    useEditorStore.getState().setContent(tab.path, 'unsaved draft');

    const switching = prepareForWorkspaceSwitch();
    useConfirmDialogStore.getState().close('secondary');

    await expect(switching).resolves.toBe(true);
    expect(useEditorStore.getState().documentContents[tab.path]).toBeUndefined();
    expect(useSidebarStore.getState().openTabs[0].isDirty).toBe(false);
  });

  it('cancels a path mutation before touching the filesystem', async () => {
    const tab = fileTab({
      path: 'C:\\Work\\Drafts\\paper.docx',
      name: 'paper.docx',
    });
    installTabs([tab]);
    const save = vi.fn(async () => true);
    useEditorHandleStore.getState().registerDocumentSaveHandler(tab.path, save);
    const mutate = vi.fn(async () => undefined);

    const mutation = runPathMutationWithOpenTabLifecycle({
      path: 'c:/work/drafts',
      includeDescendants: true,
      mutate,
    });
    useConfirmDialogStore.getState().close('cancel');

    await expect(mutation).resolves.toBe(false);
    expect(mutate).not.toHaveBeenCalled();
    expect(save).not.toHaveBeenCalled();
    expect(useSidebarStore.getState().openTabs).toEqual([tab]);
    expect(useEditorHandleStore.getState().documentSaveHandlers.get(tab.path)).toBe(save);
  });

  it('saves and closes every folder descendant before mutation, then purges old-path state', async () => {
    const dirtyChild = fileTab({
      id: 'dirty-child',
      path: 'C:\\Work\\Drafts\\paper.docx',
      name: 'paper.docx',
    });
    const cleanChild = fileTab({
      id: 'clean-child',
      path: 'c:/work/drafts/notes.md',
      name: 'notes.md',
      isDirty: false,
    });
    const sibling = fileTab({
      id: 'sibling',
      path: 'C:/Work/Drafts-old/keep.md',
      name: 'keep.md',
      isDirty: false,
    });
    installTabs([dirtyChild, cleanChild, sibling]);

    for (const path of [cleanChild.path, sibling.path]) {
      useEditorStore.getState().setDocumentContent(path, {
        id: path,
        path,
        doc_type: 'Markdown',
        title: path,
        blocks: [],
        updated_at: '',
        hash: '',
      }, 'saved', 1);
    }
    const save = vi.fn(async () => {
      useSidebarStore.getState().setOpenTabDirty(dirtyChild.path, false);
      return true;
    });
    useEditorHandleStore.getState().registerDocumentSaveHandler(dirtyChild.path, save);
    const mutate = vi.fn(async () => {
      expect(useSidebarStore.getState().openTabs).toEqual([sibling]);
      expect(
        useEditorHandleStore.getState().documentSaveHandlers.has(dirtyChild.path),
      ).toBe(false);
    });

    const mutation = runPathMutationWithOpenTabLifecycle({
      path: 'c:/work/drafts',
      includeDescendants: true,
      mutate,
    });
    useConfirmDialogStore.getState().close('confirm');

    await expect(mutation).resolves.toBe(true);
    expect(save).toHaveBeenCalledOnce();
    expect(mutate).toHaveBeenCalledOnce();
    expect(useEditorStore.getState().documentContents[cleanChild.path]).toBeUndefined();
    expect(useEditorStore.getState().documentContents[sibling.path]).toBeDefined();
  });

  it('never mutates a file when its save handler fails', async () => {
    const tab = fileTab();
    installTabs([tab]);
    useEditorHandleStore.getState().registerDocumentSaveHandler(
      tab.path,
      vi.fn(async () => false),
    );
    const mutate = vi.fn(async () => undefined);

    const mutation = runPathMutationWithOpenTabLifecycle({
      path: tab.path,
      includeDescendants: false,
      mutate,
    });
    useConfirmDialogStore.getState().close('confirm');

    await expect(mutation).resolves.toBe(false);
    expect(mutate).not.toHaveBeenCalled();
    expect(useSidebarStore.getState().openTabs).toEqual([tab]);
  });

  it('restores a closed tab when the filesystem mutation fails', async () => {
    const tab = fileTab({ isDirty: false });
    installTabs([tab]);
    const error = new Error('disk busy');

    await expect(runPathMutationWithOpenTabLifecycle({
      path: tab.path,
      includeDescendants: false,
      mutate: async () => { throw error; },
    })).rejects.toBe(error);

    expect(useSidebarStore.getState().openTabs).toEqual([tab]);
    expect(useSidebarStore.getState().activeTabId).toBe(tab.id);
  });

  it('restores a resolved dirty tab as clean when the filesystem mutation fails', async () => {
    const tab = fileTab();
    installTabs([tab]);
    useEditorHandleStore.getState().registerDocumentSaveHandler(tab.path, async () => {
      useSidebarStore.getState().setOpenTabDirty(tab.path, false);
      return true;
    });

    const mutation = runPathMutationWithOpenTabLifecycle({
      path: tab.path,
      includeDescendants: false,
      mutate: async () => { throw new Error('disk busy'); },
    });
    useConfirmDialogStore.getState().close('confirm');
    await expect(mutation).rejects.toThrow('disk busy');

    expect(useSidebarStore.getState().openTabs).toEqual([{ ...tab, isDirty: false }]);
  });

  it('clears stale file selection when replacing tabs with an empty snapshot', () => {
    const tab = fileTab({ isDirty: false });
    installTabs([tab]);

    useSidebarStore.getState().replaceTabs([], null);

    expect(useSidebarStore.getState().selectedFile).toBeNull();
  });
});
