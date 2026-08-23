import { describe, expect, it } from 'vitest';
import { claimBapbongLoad, type BapbongLoadCursor } from './bapbongLoadState';

function cursor(): BapbongLoadCursor<object, Uint8Array> {
  return { editor: null, buffer: null };
}

describe('claimBapbongLoad', () => {
  it('loads exactly once when the buffer arrives before the editor', () => {
    const state = cursor();
    const editor = {};
    const buffer = new Uint8Array([1]);
    expect(claimBapbongLoad(state, null, buffer)).toBe(false);
    expect(claimBapbongLoad(state, editor, buffer)).toBe(true);
    expect(claimBapbongLoad(state, editor, buffer)).toBe(false);
  });

  it('loads exactly once when the editor arrives before the buffer', () => {
    const state = cursor();
    const editor = {};
    const buffer = new Uint8Array([1]);
    expect(claimBapbongLoad(state, editor, null)).toBe(false);
    expect(claimBapbongLoad(state, editor, buffer)).toBe(true);
    expect(claimBapbongLoad(state, editor, buffer)).toBe(false);
  });

  it('claims a new buffer on the same editor for an external refresh', () => {
    const state = cursor();
    const editor = {};
    expect(claimBapbongLoad(state, editor, new Uint8Array([1]))).toBe(true);
    expect(claimBapbongLoad(state, editor, new Uint8Array([2]))).toBe(true);
  });
});
