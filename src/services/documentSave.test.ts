import { beforeEach, describe, expect, it, vi } from 'vitest';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));

import { useEditorStore } from '../store/editorStore';
import { useSidebarStore } from '../store/sidebarStore';
import { persistDocument } from './documentSave';

const path = '/workspace/draft.md';

function installDocument(content: string): void {
  useEditorStore.getState().setDocumentContent(path, {
    id: 'draft',
    path,
    doc_type: 'Markdown',
    title: 'draft',
    blocks: [],
    updated_at: '',
    hash: '',
  }, content, 1);
  useEditorStore.getState().setContent(path, content);
  useSidebarStore.setState({
    openTabs: [{ id: path, path, name: 'draft.md', isDirty: true }],
    activeTabId: path,
    selectedFile: path,
  });
}

describe('document save generations', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    useEditorStore.setState({ documentContents: {} });
    useSidebarStore.setState({ openTabs: [], activeTabId: null, selectedFile: null });
  });

  it('keeps a newer edit dirty when it arrives during the disk write', async () => {
    installDocument('first draft');
    let finishWrite: (() => void) | undefined;
    invokeMock.mockReturnValue(new Promise<void>((resolve) => {
      finishWrite = resolve;
    }));

    const saving = persistDocument({ path, content: 'first draft', isDirty: true });
    useEditorStore.getState().setContent(path, 'newer draft');
    finishWrite?.();

    await expect(saving).resolves.toEqual({ ok: true });
    const live = useEditorStore.getState().documentContents[path];
    expect(live.metadata.content).toBe('newer draft');
    expect(live.metadata.isDirty).toBe(true);
    expect(useSidebarStore.getState().openTabs[0].isDirty).toBe(true);
  });

  it('marks the matching generation clean after a successful write', async () => {
    installDocument('stable draft');
    invokeMock.mockResolvedValue(undefined);

    await expect(persistDocument({
      path,
      content: 'stable draft',
      isDirty: true,
    })).resolves.toEqual({ ok: true });

    expect(useEditorStore.getState().documentContents[path].metadata.isDirty).toBe(false);
    expect(useSidebarStore.getState().openTabs[0].isDirty).toBe(false);
  });
});
