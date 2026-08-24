import { invoke } from '@tauri-apps/api/core';
import { useEditorStore, useSidebarStore, useNotificationStore } from '../store';
import { reportError } from '../utils/errors';

export async function saveDocument(path: string, content: string): Promise<void> {
  await invoke('write_document', {
    path,
    content,
  });
}

export async function persistDocument(options: {
  path: string | null;
  content: string;
  isDirty: boolean;
}): Promise<{ ok: true } | { ok: false; message: string }> {
  const { path, content, isDirty } = options;

  if (!path || !isDirty) {
    return { ok: true };
  }

  try {
    await saveDocument(path, content);

    const editorStore = useEditorStore.getState();
    const sidebarStore = useSidebarStore.getState();
    const liveDocument = editorStore.documentContents[path];
    // The write is asynchronous. If the user typed again while it was in
    // flight, disk contains `content` but the editor contains a newer value;
    // never mark that newer buffer clean. A future save will persist it.
    if (liveDocument && liveDocument.metadata.content === content) {
      editorStore.markSaved(path);
      editorStore.updateTabDirty(path, false);
      sidebarStore.setOpenTabDirty(path, false);
    } else if (liveDocument) {
      editorStore.updateTabDirty(path, true);
      sidebarStore.setOpenTabDirty(path, true);
    }

    return { ok: true };
  } catch (error) {
    const message = reportError('document-save', error);
    useNotificationStore.getState().pushNotification({
      kind: 'error',
      title: '保存失败',
      message,
    });
    return {
      ok: false,
      message,
    };
  }
}
