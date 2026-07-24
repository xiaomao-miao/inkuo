import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { useBaselineStore } from './baselineStore';

const STORAGE_KEY = 'inkuo-baselines';

const resetStore = () => {
  useBaselineStore.getState().reset();
  if (typeof localStorage !== 'undefined') {
    localStorage.removeItem(STORAGE_KEY);
  }
};

describe('useBaselineStore', () => {
  beforeEach(() => {
    resetStore();
  });

  afterEach(() => {
    resetStore();
  });

  it('records and peeks without consuming the baseline', () => {
    useBaselineStore.getState().recordBaseline('m1', 'snap-1');
    expect(useBaselineStore.getState().peekBaseline('m1')).toBe('snap-1');
    expect(useBaselineStore.getState().peekBaseline('m1')).toBe('snap-1');
  });

  it('keeps the baseline after a successful run (no auto-consume)', () => {
    useBaselineStore.getState().recordBaseline('m1', 'snap-1');
    // simulate the success path: no consumeBaseline call is issued
    expect(useBaselineStore.getState().peekBaseline('m1')).toBe('snap-1');
  });

  it('clearBaselineForSession drops every listed id and nothing else', () => {
    useBaselineStore.getState().recordBaseline('kept-1', 'snap-kept-1');
    useBaselineStore.getState().recordBaseline('drop-1', 'snap-drop-1');
    useBaselineStore.getState().recordBaseline('drop-2', 'snap-drop-2');

    useBaselineStore.getState().clearBaselinesForSession(['drop-1', 'drop-2']);

    expect(useBaselineStore.getState().baselines).toEqual({
      'kept-1': 'snap-kept-1',
    });
  });

  it('clearBaselineForSession is a no-op when given an empty list', () => {
    useBaselineStore.getState().recordBaseline('m1', 'snap-1');
    useBaselineStore.getState().clearBaselinesForSession([]);
    expect(useBaselineStore.getState().baselines).toEqual({ 'm1': 'snap-1' });
  });

  it('consumeBaseline still removes the entry for explicit invalidations', () => {
    useBaselineStore.getState().recordBaseline('m1', 'snap-1');
    expect(useBaselineStore.getState().consumeBaseline('m1')).toBe('snap-1');
    expect(useBaselineStore.getState().peekBaseline('m1')).toBeUndefined();
  });

  it('clearBaseline drops one entry without returning the snapshot id', () => {
    useBaselineStore.getState().recordBaseline('m1', 'snap-1');
    useBaselineStore.getState().recordBaseline('m2', 'snap-2');
    useBaselineStore.getState().clearBaseline('m1');
    expect(useBaselineStore.getState().baselines).toEqual({ 'm2': 'snap-2' });
  });
});
