import { describe, expect, it } from 'vitest';
import { shouldApplyDiskDocument } from './documentLoadPolicy';

describe('shouldApplyDiskDocument', () => {
  it('loads the initial disk document when no local buffer exists', () => {
    expect(shouldApplyDiskDocument(false, true, false)).toBe(true);
  });

  it('allows automatic refresh for a clean local buffer', () => {
    expect(shouldApplyDiskDocument(true, false, false)).toBe(true);
  });

  it('protects a dirty local buffer from an automatic refresh', () => {
    expect(shouldApplyDiskDocument(true, true, false)).toBe(false);
  });

  it('reloads only after the user explicitly approves discarding changes', () => {
    expect(shouldApplyDiskDocument(true, true, true)).toBe(true);
  });
});
