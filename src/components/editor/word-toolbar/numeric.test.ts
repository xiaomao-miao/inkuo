import { describe, expect, it } from 'vitest';
import {
  clampFontSizePt,
  parseFontSizePt,
  parseZoomFactorFromPct,
  ptToHalfPoints,
  stepFontSizePt,
  FONT_SIZE_MAX_PT,
  FONT_SIZE_MIN_PT,
  ZOOM_MAX_PCT,
  ZOOM_MIN_PCT,
} from './numeric';

describe('word-toolbar/numeric', () => {
  describe('clampFontSizePt', () => {
    it('passes through in-range values', () => {
      expect(clampFontSizePt(12, 12)).toBe(12);
      expect(clampFontSizePt(7.5, 12)).toBe(8); // rounds to nearest int
    });

    it('clamps below the minimum', () => {
      expect(clampFontSizePt(0, 12)).toBe(FONT_SIZE_MIN_PT);
      expect(clampFontSizePt(-50, 12)).toBe(FONT_SIZE_MIN_PT);
    });

    it('clamps above the maximum', () => {
      expect(clampFontSizePt(FONT_SIZE_MAX_PT + 10, 12)).toBe(FONT_SIZE_MAX_PT);
      expect(clampFontSizePt(100000, 12)).toBe(FONT_SIZE_MAX_PT);
    });

    it('falls back when input is non-finite', () => {
      expect(clampFontSizePt(NaN, 12)).toBe(12);
      expect(clampFontSizePt(Infinity, 16)).toBe(16);
    });
  });

  describe('parseFontSizePt', () => {
    it('parses valid sizes from strings', () => {
      expect(parseFontSizePt('12')).toBe(12);
      expect(parseFontSizePt('96')).toBe(96);
    });

    it('returns null for non-numeric input', () => {
      expect(parseFontSizePt('abc')).toBeNull();
      expect(parseFontSizePt('')).toBeNull();
    });

    it('returns null for out-of-range values', () => {
      expect(parseFontSizePt('0')).toBeNull();
      expect(parseFontSizePt('-5')).toBeNull();
      expect(parseFontSizePt(String(FONT_SIZE_MAX_PT + 1))).toBeNull();
    });

    it('accepts the inclusive endpoints', () => {
      expect(parseFontSizePt(String(FONT_SIZE_MIN_PT))).toBe(FONT_SIZE_MIN_PT);
      expect(parseFontSizePt(String(FONT_SIZE_MAX_PT))).toBe(FONT_SIZE_MAX_PT);
    });
  });

  describe('stepFontSizePt', () => {
    it('adds the delta when in range', () => {
      expect(stepFontSizePt(12, 1, 12)).toBe(13);
      expect(stepFontSizePt(20, -1, 20)).toBe(19);
    });

    it('clamps at the upper bound', () => {
      expect(stepFontSizePt(FONT_SIZE_MAX_PT, 1, FONT_SIZE_MAX_PT)).toBe(FONT_SIZE_MAX_PT);
    });

    it('clamps at the lower bound', () => {
      expect(stepFontSizePt(FONT_SIZE_MIN_PT, -1, FONT_SIZE_MIN_PT)).toBe(FONT_SIZE_MIN_PT);
    });

    it('falls back when current size is non-finite', () => {
      // NaN + delta is still NaN, so we should see the fallback.
      expect(stepFontSizePt(NaN, 1, 12)).toBe(12);
    });
  });

  describe('ptToHalfPoints', () => {
    it('doubles and rounds', () => {
      expect(ptToHalfPoints(12)).toBe(24);
      expect(ptToHalfPoints(10.5)).toBe(21);
      expect(ptToHalfPoints(0.4)).toBe(1);
    });
  });

  describe('parseZoomFactorFromPct', () => {
    it('parses valid percentages into factors', () => {
      expect(parseZoomFactorFromPct('100')).toBe(1);
      expect(parseZoomFactorFromPct('75')).toBe(0.75);
      expect(parseZoomFactorFromPct('250')).toBe(2.5);
    });

    it('returns null for non-numeric input', () => {
      expect(parseZoomFactorFromPct('foo')).toBeNull();
      expect(parseZoomFactorFromPct('')).toBeNull();
    });

    it('returns null for out-of-range percentages', () => {
      expect(parseZoomFactorFromPct('0')).toBeNull();
      expect(parseZoomFactorFromPct(String(ZOOM_MAX_PCT + 1))).toBeNull();
      expect(parseZoomFactorFromPct('-1')).toBeNull();
    });

    it('accepts the inclusive endpoints', () => {
      expect(parseZoomFactorFromPct(String(ZOOM_MIN_PCT))).toBe(ZOOM_MIN_PCT / 100);
      expect(parseZoomFactorFromPct(String(ZOOM_MAX_PCT))).toBe(ZOOM_MAX_PCT / 100);
    });
  });
});
