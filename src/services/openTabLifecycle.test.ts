import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useConfirmDialogStore } from '../store/confirmDialogStore';
import { useEditorHandleStore } from '../store/editorHandleStore';
import { useEditorStore } from '../store/editorStore';
import { useSidebarStore, type OpenTab } from '../store/sidebarStore';
import {
  confirmWindowClose,
  requestCloseOpenTab,
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
});
