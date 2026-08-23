import { describe, expect, it } from 'vitest';
import { shouldMountOfficeTab } from './officeTabRetention';

describe('shouldMountOfficeTab', () => {
  it('mounts the active Office tab', () => {
    expect(shouldMountOfficeTab(true, false)).toBe(true);
  });

  it('retains an inactive dirty Office tab so tab switches cannot drop edits', () => {
    expect(shouldMountOfficeTab(false, true)).toBe(true);
  });

  it('allows an inactive clean Office tab to unmount', () => {
    expect(shouldMountOfficeTab(false, false)).toBe(false);
  });
});
