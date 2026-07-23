// Unit tests for the small predicates in `types.ts`.

import { describe, expect, it } from 'vitest';

import {
  DIVIDER_ID,
  WORKSPACE_TARGET_KIND,
  isDivider,
} from './types';
import type { MenuItem } from './types';

describe('isDivider', () => {
  it('returns true when id is the divider sentinel', () => {
    expect(isDivider({ id: DIVIDER_ID, label: '' })).toBe(true);
  });

  it('returns false for any other id', () => {
    const item: MenuItem = { id: 'cut', label: '剪切', icon: null };
    expect(isDivider(item)).toBe(false);
  });

  it('returns false when an item has no id', () => {
    // Items always have ids in practice, but the helper should not crash.
    expect(isDivider({ id: '' as string, label: '' })).toBe(false);
  });
});

describe('constants', () => {
  it('DIVIDER_ID is a stable string', () => {
    expect(DIVIDER_ID).toBe('divider');
  });

  it('WORKSPACE_TARGET_KIND is a stable string', () => {
    expect(WORKSPACE_TARGET_KIND).toBe('workspace');
  });
});
