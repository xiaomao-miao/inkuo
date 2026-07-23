// Unit tests for `colorAlignment.ts` — pure helpers used during the
// Rust⇄FortuneSheet style conversion. These were extracted from the
// original monolithic `fortuneSheetConverter.ts` so their behavior
// (especially the obscure OOXML hex variants) is lockable.

import { describe, expect, it } from 'vitest';

import { alignH, alignV, normaliseColor } from './colorAlignment';

describe('normaliseColor', () => {
  it('returns undefined for falsy input', () => {
    expect(normaliseColor(undefined)).toBeUndefined();
    expect(normaliseColor('')).toBeUndefined();
  });

  it('returns lowercase 6-char hex with hash unchanged', () => {
    expect(normaliseColor('#FF8800')).toBe('#ff8800');
    expect(normaliseColor('#abc123')).toBe('#abc123');
  });

  it('strips the alpha channel from #AARRGGBB', () => {
    expect(normaliseColor('#80FF8800')).toBe('#ff8800');
    expect(normaliseColor('#00ABCDEF')).toBe('#abcdef');
  });

  it('prepends # to bare 6-char hex', () => {
    expect(normaliseColor('FF8800')).toBe('#ff8800');
    expect(normaliseColor('abc123')).toBe('#abc123');
  });

  it('strips alpha and prepends # to bare 8-char ARGB', () => {
    expect(normaliseColor('80FF8800')).toBe('#ff8800');
    expect(normaliseColor('00abcdef')).toBe('#abcdef');
  });

  it('returns named colours / rgb() etc. unchanged', () => {
    expect(normaliseColor('red')).toBe('red');
    expect(normaliseColor('rgb(255, 136, 0)')).toBe('rgb(255, 136, 0)');
  });
});

describe('alignH', () => {
  it('returns undefined for falsy input', () => {
    expect(alignH(undefined)).toBeUndefined();
    expect(alignH('')).toBeUndefined();
  });

  it('maps the recognized Rust alignment strings to FortuneSheet values', () => {
    expect(alignH('center')).toBe(0);
    expect(alignH('left')).toBe(1);
    expect(alignH('right')).toBe(2);
  });

  it('accepts the UK spelling of centre', () => {
    expect(alignH('centre')).toBe(0);
  });

  it('returns undefined for unknown alignment strings', () => {
    expect(alignH('justify')).toBeUndefined();
    expect(alignH('CENTER')).toBeUndefined(); // case-sensitive
  });
});

describe('alignV', () => {
  it('returns undefined for falsy input', () => {
    expect(alignV(undefined)).toBeUndefined();
    expect(alignV('')).toBeUndefined();
  });

  it('maps the recognized Rust alignment strings to FortuneSheet values', () => {
    expect(alignV('center')).toBe(0);
    expect(alignV('top')).toBe(1);
    expect(alignV('bottom')).toBe(2);
  });

  it('returns undefined for unknown alignment strings', () => {
    expect(alignV('middle')).toBeUndefined();
    expect(alignV('TOP')).toBeUndefined();
  });
});
