import { describe, expect, it } from 'vitest';
import { getDirectoryContentState } from './fileTreeState';

describe('getDirectoryContentState', () => {
  it('uses loading only before the first directory result exists', () => {
    expect(getDirectoryContentState(false, 0, true, false)).toBe('loading');
  });

  it('shows an explicit error after an uncached load fails', () => {
    expect(getDirectoryContentState(false, 0, false, true)).toBe('error');
  });

  it('keeps a cached empty directory stable during background refreshes', () => {
    // Refreshing is deliberately not an input: cached data remains visible.
    expect(getDirectoryContentState(true, 0)).toBe('empty');
    expect(getDirectoryContentState(true, 0)).toBe('empty');
  });

  it('keeps stale cached children visible after a refresh error', () => {
    expect(getDirectoryContentState(true, 2, false, true)).toBe('populated');
    expect(getDirectoryContentState(true, 0, false, true)).toBe('empty');
  });

  it('renders cached children', () => {
    expect(getDirectoryContentState(true, 2)).toBe('populated');
  });
});
