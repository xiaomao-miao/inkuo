import { describe, expect, it, vi } from 'vitest';

import { resolveCloudBaseUrl } from './cloudBaseUrl';

describe('resolveCloudBaseUrl', () => {
  it('uses localhost only in development', () => {
    expect(resolveCloudBaseUrl(undefined, true)).toBe('http://localhost:8080');
    expect(resolveCloudBaseUrl(undefined, false)).toBe('https://cloud.inkuo.com');
  });

  it('normalizes a configured endpoint', () => {
    expect(resolveCloudBaseUrl(' https://staging.inkuo.com/api/ ', false))
      .toBe('https://staging.inkuo.com/api');
  });

  it('rejects non-http URLs and embedded credentials', () => {
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    expect(resolveCloudBaseUrl('file:///tmp/cloud', false)).toBe('https://cloud.inkuo.com');
    expect(resolveCloudBaseUrl('https://user:secret@example.com', true)).toBe('http://localhost:8080');
    consoleSpy.mockRestore();
  });
});
