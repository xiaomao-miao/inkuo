import { describe, expect, it, vi } from 'vitest';
import type { CurrentDiff } from '../../types';
import { syncPendingDiffToEditor } from './InlineDiffPreview';

describe('syncPendingDiffToEditor', () => {
  it('stages hunks without applying document content', () => {
    const sync = vi.fn();
    const diff = {
      filePath: '/workspace/report.md',
      originalText: 'before',
      newText: 'after',
      hunks: [],
    } as CurrentDiff;

    expect(syncPendingDiffToEditor(diff, sync)).toBe(true);
    expect(sync).toHaveBeenCalledWith(
      '/workspace/report.md', [], 'before', 0,
    );
  });

  it('does nothing when the diff is not tied to a file', () => {
    const sync = vi.fn();
    const diff = {
      filePath: '',
      originalText: 'before',
      newText: 'after',
      hunks: [],
    } as CurrentDiff;

    expect(syncPendingDiffToEditor(diff, sync)).toBe(false);
    expect(sync).not.toHaveBeenCalled();
  });
});
