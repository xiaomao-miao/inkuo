import { describe, expect, it } from 'vitest';
import { externalFileEventPath } from './useExternalFileSync';

describe('externalFileEventPath', () => {
  it('reads the direct file-written payload', () => {
    expect(externalFileEventPath({ path: 'reports/paper.docx' })).toBe('reports/paper.docx');
  });

  it('reads the tagged semantic file-change payload', () => {
    expect(externalFileEventPath({
      type: 'Modified',
      data: { path: 'C:\\Work\\paper.docx' },
    })).toBe('C:\\Work\\paper.docx');
  });

  it('is defensive around malformed events', () => {
    expect(externalFileEventPath(undefined)).toBe('');
    expect(externalFileEventPath({ type: 'Modified', data: {} })).toBe('');
  });
});
