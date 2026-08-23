export interface BapbongLoadCursor<TEditor, TBuffer> {
  editor: TEditor | null;
  buffer: TBuffer | null;
}

/**
 * Atomically claim an editor+buffer pair for loading. This makes both arrival
 * orders equivalent (buffer before editor, or editor before buffer) and guards
 * against an incidental effect rerun parsing the same DOCX twice.
 */
export function claimBapbongLoad<TEditor extends object, TBuffer extends object>(
  cursor: BapbongLoadCursor<TEditor, TBuffer>,
  editor: TEditor | null,
  buffer: TBuffer | null,
): boolean {
  if (!editor || !buffer) return false;
  if (cursor.editor === editor && cursor.buffer === buffer) return false;
  cursor.editor = editor;
  cursor.buffer = buffer;
  return true;
}
