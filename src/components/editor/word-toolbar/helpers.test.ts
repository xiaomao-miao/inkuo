import { describe, expect, it } from 'vitest';
import { hpToPt, rgbToHex } from './helpers';

describe('word-toolbar/helpers', () => {
  describe('hpToPt', () => {
    it('converts half-points to points (round-down)', () => {
      expect(hpToPt(24)).toBe(12); // 24 half-points = 12pt
      expect(hpToPt(20)).toBe(10);
    });

    it('rounds when the half-point value is odd', () => {
      // 13 hp → 6.5pt → rounds to 7
      expect(hpToPt(13)).toBe(7);
    });

    it('returns null for null / undefined input', () => {
      expect(hpToPt(null)).toBeNull();
      expect(hpToPt(undefined)).toBeNull();
    });

    it('returns null for non-finite input', () => {
      expect(hpToPt(NaN)).toBeNull();
      expect(hpToPt('not a number' as unknown as number)).toBeNull();
    });

    it('accepts numeric strings', () => {
      expect(hpToPt('24')).toBe(12);
    });
  });

  describe('rgbToHex', () => {
    it('prefixes with # when missing', () => {
      expect(rgbToHex('ff0000')).toBe('#ff0000');
      expect(rgbToHex('000000')).toBe('#000000');
    });

    it('leaves existing # prefix intact', () => {
      expect(rgbToHex('#ff0000')).toBe('#ff0000');
    });

    it('returns null for falsy input', () => {
      expect(rgbToHex('')).toBeNull();
      expect(rgbToHex(undefined)).toBeNull();
      expect(rgbToHex(null)).toBeNull();
    });

    it('treats numbers as strings', () => {
      // Defensive: if someone accidentally passes a number, string-coerce.
      expect(rgbToHex(123 as unknown as string)).toBe('#123');
    });
  });
});
