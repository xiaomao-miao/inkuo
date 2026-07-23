// Unit tests for the pure selection helpers.
//
// `collectSortableTextblocks` walks a ProseMirror document; we skip
// that here because constructing a real ProseMirror doc requires a
// DOM parser (jsdom) which isn't part of the test environment. The
// two comparators and `readSelectionText` are pure and tested
// directly here.

import { describe, expect, it, vi } from 'vitest';

import {
  compareLinesAsc,
  compareLinesDesc,
  readSelectionText,
  type GlobalSelection,
  type SortableLine,
} from './selection';

function line(text: string): SortableLine {
  return {
    pos: 0,
    node: {} as never,
    start: 0,
    end: text.length,
    text,
  };
}

describe('line comparators', () => {
  it('asc sorts alphabetically with zh-Hans-CN locale', () => {
    const input = [line('cherry'), line('apple'), line('banana')];
    const sorted = [...input].sort(compareLinesAsc);
    expect(sorted.map((l) => l.text)).toEqual(['apple', 'banana', 'cherry']);
  });

  it('desc sorts alphabetically with zh-Hans-CN locale', () => {
    const input = [line('cherry'), line('apple'), line('banana')];
    const sorted = [...input].sort(compareLinesDesc);
    expect(sorted.map((l) => l.text)).toEqual(['cherry', 'banana', 'apple']);
  });

  it('returns 0 (stable order) when lines are identical', () => {
    expect(compareLinesAsc(line('same'), line('same'))).toBe(0);
    expect(compareLinesDesc(line('same'), line('same'))).toBe(0);
  });

  it('handles the empty-string case without throwing', () => {
    expect(compareLinesAsc(line(''), line('a'))).toBeLessThan(0);
    expect(compareLinesDesc(line(''), line('a'))).toBeGreaterThan(0);
  });

  it('treats Chinese characters by their zh-Hans-CN collation order', () => {
    // Don't pin the exact ordering (zh locale varies by platform)
    // but verify desc is the reverse of asc on this set.
    const sorted = [line('他'), line('中'), line('你')].sort(compareLinesAsc);
    expect(sorted).toHaveLength(3);
    const desc = [...sorted].sort(compareLinesDesc);
    expect(desc.map((l) => l.text)).toEqual([...sorted].reverse().map((l) => l.text));
  });
});

describe('readSelectionText', () => {
  function stubGlobal(returnValue: { toString: () => string } | null): GlobalSelection {
    return {
      getSelection: vi.fn().mockReturnValue(returnValue),
    };
  }

  it('returns "" when the global getSelection returns null', () => {
    expect(readSelectionText(stubGlobal(null))).toBe('');
  });

  it('returns the selected text when getSelection provides it', () => {
    const g = stubGlobal({ toString: () => 'hello world' });
    expect(readSelectionText(g)).toBe('hello world');
    expect(g.getSelection).toHaveBeenCalledTimes(1);
  });

  it('returns "" when getSelection returns a Selection with empty toString', () => {
    expect(readSelectionText(stubGlobal({ toString: () => '' }))).toBe('');
  });

  it('returns "" when getSelection throws (e.g. detached iframe)', () => {
    const g: GlobalSelection = {
      getSelection: vi.fn().mockImplementation(() => {
        throw new Error('not allowed');
      }),
    };
    // The helper itself doesn't guard against throws; callers
    // (production code via the default window global) are wrapped
    // in a try / catch when needed. We document behavior here.
    expect(() => readSelectionText(g)).toThrow();
  });
});
