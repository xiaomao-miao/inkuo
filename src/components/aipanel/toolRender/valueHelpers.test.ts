// Unit tests for the small value-shaping helpers used by the renderers.

import { describe, expect, it } from 'vitest';

import {
  cellText,
  previewValue,
  textFromRuns,
  unwrapCellValue,
} from './valueHelpers';

describe('valueHelpers', () => {
  describe('previewValue', () => {
    it('returns empty for null / undefined', () => {
      expect(previewValue(null)).toBe('');
      expect(previewValue(undefined)).toBe('');
    });

    it('returns the string itself when short enough', () => {
      expect(previewValue('hello')).toBe('hello');
    });

    it('truncates strings longer than maxLen with an ellipsis', () => {
      expect(previewValue('a'.repeat(100), 10)).toBe('aaaaaaaaaa…');
    });

    it('respects a custom maxLen', () => {
      expect(previewValue('abcdef', 3)).toBe('abc…');
    });

    it('returns the stringification for numbers and booleans', () => {
      expect(previewValue(42)).toBe('42');
      expect(previewValue(-1.5)).toBe('-1.5');
      expect(previewValue(true)).toBe('true');
      expect(previewValue(false)).toBe('false');
    });

    it('summarizes empty vs non-empty arrays', () => {
      expect(previewValue([])).toBe('[]');
      expect(previewValue([1, 2, 3])).toBe('[3 项]');
    });

    it('summarizes objects by key count', () => {
      expect(previewValue({})).toBe('{}');
      expect(previewValue({ a: 1, b: 2, c: 3 })).toBe('{3 字段}');
    });

    it('falls back to String() for anything else', () => {
      // A Symbol satisfies the typeof chain check above (none of
      // number/boolean/object) and lands in the final String() branch.
      // Symbols stringify to "Symbol(description)" so we assert that
      // rather than a literal.
      const sym = Symbol('foo');
      expect(previewValue(sym)).toBe(String(sym));
    });
  });

  describe('textFromRuns', () => {
    it('concatenates each run object text', () => {
      expect(
        textFromRuns([{ text: 'hello ' }, { text: 'world' }]),
      ).toBe('hello world');
    });

    it('skips non-string text fields', () => {
      expect(
        textFromRuns([{ text: 'a' }, { text: 1 as unknown as string }, { text: 'b' }]),
      ).toBe('ab');
    });

    it('skips non-objects in the run list', () => {
      expect(textFromRuns(['x', null, 'y', { text: 'z' } as unknown])).toBe('z');
      // Note: the leading 'x' / 'y' strings get filtered because
      // `r && typeof r === 'object'` rejects them.
    });

    it('returns empty for non-array input', () => {
      expect(textFromRuns(null)).toBe('');
      expect(textFromRuns(undefined)).toBe('');
      expect(textFromRuns('not-an-array')).toBe('');
    });
  });

  describe('cellText', () => {
    it('returns plain strings as-is', () => {
      expect(cellText('hello')).toBe('hello');
    });

    it('unwraps {text} objects', () => {
      expect(cellText({ text: 'world' })).toBe('world');
    });

    it('returns "" for objects without a string text field', () => {
      expect(cellText({ value: 42 })).toBe('');
    });

    it('returns "" for primitives other than string', () => {
      expect(cellText(42)).toBe('');
      expect(cellText(null)).toBe('');
      expect(cellText(undefined)).toBe('');
    });
  });

  describe('unwrapCellValue', () => {
    it('unwraps typed {value} objects', () => {
      expect(unwrapCellValue({ value: 42 })).toBe('42');
      expect(unwrapCellValue({ value: 'foo' })).toBe('foo');
    });

    it('returns "" for null/undefined', () => {
      expect(unwrapCellValue(null)).toBe('');
      expect(unwrapCellValue(undefined)).toBe('');
    });

    it('returns "" for wrapped {value: null}', () => {
      expect(unwrapCellValue({ value: null })).toBe('');
    });

    it('returns String() for plain scalars', () => {
      expect(unwrapCellValue(123)).toBe('123');
      expect(unwrapCellValue(true)).toBe('true');
    });
  });
});
