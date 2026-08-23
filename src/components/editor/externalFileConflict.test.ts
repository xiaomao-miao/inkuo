import { describe, expect, it } from 'vitest';
import { decideExternalRefresh } from './externalFileConflict';

describe('decideExternalRefresh', () => {
  it('automatically reloads a clean editor', () => {
    expect(decideExternalRefresh(false, false)).toBe('reload');
  });

  it('never automatically reloads a dirty editor', () => {
    expect(decideExternalRefresh(true, false)).toBe('show-conflict');
  });

  it('coalesces duplicate events during a user-approved reload', () => {
    expect(decideExternalRefresh(true, true)).toBe('ignore');
    expect(decideExternalRefresh(false, true)).toBe('ignore');
  });
});
