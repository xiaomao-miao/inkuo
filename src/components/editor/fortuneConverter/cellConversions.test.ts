// Unit tests for the cell-level conversions between Rust and FortuneSheet.
//
// Each function takes a single cell and is responsible for *one* of the
// three conversion aspects: value, style, or the combined Rust→Fortune
// flow. The tests below exercise each branch of the per-type dispatch.

import type { Cell as FortuneCell } from '@fortune-sheet/core';
import { describe, expect, it } from 'vitest';

import {
  fortuneStyleToRust,
  fortuneValueToRust,
  rustCellToFortune,
} from './cellConversions';
import type { RustCell } from './types';

function makeRustCell(over: Partial<RustCell> = {}): RustCell {
  return {
    row: 0,
    col: 0,
    value: { type: 'empty' },
    ...over,
  };
}

function makeFortuneCell(over: Partial<FortuneCell> = {}): FortuneCell {
  return { ...over };
}

describe('rustCellToFortune', () => {
  describe('value dispatch', () => {
    it('maps "empty" to a Fortune cell with no v/m/ct', () => {
      const out = rustCellToFortune(makeRustCell({ value: { type: 'empty' } }));
      expect(out.v).toBeUndefined();
      expect(out.m).toBeUndefined();
      expect(out.ct).toBeUndefined();
    });

    it('maps "int" to a numeric Fortune cell with "0" format', () => {
      const out = rustCellToFortune(
        makeRustCell({ value: { type: 'int', value: 42 } }),
      );
      expect(out.v).toBe(42);
      expect(out.m).toBe('42');
      expect(out.ct).toEqual({ fa: '0', t: 'n' });
    });

    it('maps "float" to a numeric Fortune cell with General format', () => {
      const out = rustCellToFortune(
        makeRustCell({ value: { type: 'float', value: 1.5 } }),
      );
      expect(out.v).toBe(1.5);
      expect(out.ct).toEqual({ fa: 'General', t: 'n' });
    });

    it('maps "bool" to TRUE/FALSE', () => {
      const on = rustCellToFortune(
        makeRustCell({ value: { type: 'bool', value: 1 } }),
      );
      expect(on.v).toBe(true);
      expect(on.m).toBe('TRUE');

      const off = rustCellToFortune(
        makeRustCell({ value: { type: 'bool', value: 0 } }),
      );
      expect(off.v).toBe(false);
      expect(off.m).toBe('FALSE');
    });

    it('maps "string" to a "@" string cell', () => {
      const out = rustCellToFortune(
        makeRustCell({ value: { type: 'string', value: 'hi' } }),
      );
      expect(out.v).toBe('hi');
      expect(out.m).toBe('hi');
      expect(out.ct).toEqual({ fa: '@', t: 's' });
    });

    it('maps "datetime" honoring a custom number format', () => {
      const out = rustCellToFortune(
        makeRustCell({
          value: { type: 'datetime', value: 45000 },
          style: { number_format: 'yyyy-mm-dd' },
        }),
      );
      expect(out.v).toBe(45000);
      expect(out.ct?.t).toBe('n');
      expect(out.ct?.fa).toBe('yyyy-mm-dd');
    });

    it('falls back to yyyy-mm-dd for datetime with no format', () => {
      const out = rustCellToFortune(
        makeRustCell({ value: { type: 'datetime', value: 45000 } }),
      );
      expect(out.ct?.fa).toBe('yyyy-mm-dd');
    });

    it('maps "error" to "#ERR:..." with t="g"', () => {
      const out = rustCellToFortune(
        makeRustCell({ value: { type: 'error', value: 'DIV0' } }),
      );
      expect(out.v).toBe('#ERR:DIV0');
      expect(out.m).toBe('#ERR:DIV0');
      expect(out.ct).toEqual({ fa: 'General', t: 'g' });
    });

    it('handles missing optional value fields by treating them as 0/""', () => {
      const out = rustCellToFortune(
        makeRustCell({ value: { type: 'int' } }),
      );
      expect(out.v).toBe(0);
    });
  });

  describe('formula', () => {
    it('prepends "=" when missing', () => {
      const out = rustCellToFortune(makeRustCell({ formula: 'SUM(A1:A5)' }));
      expect(out.f).toBe('=SUM(A1:A5)');
    });

    it('does not double-up when already prefixed', () => {
      const out = rustCellToFortune(makeRustCell({ formula: '=SUM(A1:A5)' }));
      expect(out.f).toBe('=SUM(A1:A5)');
    });
  });

  describe('style', () => {
    it('maps font_bold/italic/size/name to bl/it/fs/ff', () => {
      const out = rustCellToFortune(
        makeRustCell({
          value: { type: 'string', value: 'x' },
          style: { font_bold: true, font_italic: true, font_size: 14, font_name: 'Arial' },
        }),
      );
      expect(out.bl).toBe(1);
      expect(out.it).toBe(1);
      expect(out.fs).toBe(14);
      expect(out.ff).toBe('Arial');
    });

    it('normalises font_color into a #RRGGBB string', () => {
      const out = rustCellToFortune(
        makeRustCell({
          value: { type: 'string', value: 'x' },
          style: { font_color: '#ff8800' },
        }),
      );
      expect(out.fc).toBe('#ff8800');
    });

    it('normalises fill_fg_color and stores it as bg', () => {
      const out = rustCellToFortune(
        makeRustCell({
          value: { type: 'string', value: 'x' },
          style: { fill_fg_color: 'ffeedd' },
        }),
      );
      expect(out.bg).toBe('#ffeedd');
    });

    it('maps Rust alignment strings to numeric ht/vt', () => {
      const out = rustCellToFortune(
        makeRustCell({
          value: { type: 'string', value: 'x' },
          style: { alignment_h: 'right', alignment_v: 'top' },
        }),
      );
      expect(out.ht).toBe(2);
      expect(out.vt).toBe(1);
    });

    it('only sets number format when it differs from General', () => {
      // string cell already has ct={fa:"@",t:"s"}; adding number_format=General
      // should NOT clobber it.
      const out = rustCellToFortune(
        makeRustCell({
          value: { type: 'string', value: 'x' },
          style: { number_format: 'General' },
        }),
      );
      expect(out.ct).toEqual({ fa: '@', t: 's' });
    });

    it('sets number format when a non-General string cell exists', () => {
      const out = rustCellToFortune(
        makeRustCell({
          value: { type: 'string', value: 'x' },
          style: { number_format: '0.00' },
        }),
      );
      expect(out.ct).toEqual({ fa: '@', t: 's' });
    });
  });
});

describe('fortuneValueToRust', () => {
  it('returns empty for undefined/null/empty raw values', () => {
    expect(fortuneValueToRust(makeFortuneCell())).toEqual({ type: 'empty' });
    expect(fortuneValueToRust(makeFortuneCell({ v: null as unknown as string }))).toEqual({ type: 'empty' });
    expect(fortuneValueToRust(makeFortuneCell({ v: '' }))).toEqual({ type: 'empty' });
  });

  it('returns int/float for formula cells whose v is numeric', () => {
    const out = fortuneValueToRust(makeFortuneCell({ f: '=SUM(A1:A5)', v: 7 }));
    expect(out.type).toBe('int');
    expect(out.value).toBe(7);

    const out2 = fortuneValueToRust(makeFortuneCell({ f: '=A1/2', v: 3.5 }));
    expect(out2.type).toBe('float');
    expect(out2.value).toBe(3.5);
  });

  it('returns bool for formula cells whose v is boolean', () => {
    const out = fortuneValueToRust(makeFortuneCell({ f: '=A1>0', v: true }));
    expect(out.type).toBe('bool');
    expect(out.value).toBe(1);

    const out2 = fortuneValueToRust(makeFortuneCell({ f: '=A1>0', v: false }));
    expect(out2.type).toBe('bool');
    expect(out2.value).toBe(0);
  });

  it('treats formula + v starting with = as empty (HF has not run)', () => {
    const out = fortuneValueToRust(makeFortuneCell({ f: '=A1', v: '=A1' }));
    expect(out.type).toBe('empty');
  });

  it('returns string for formula cells whose v is a non-= string', () => {
    const out = fortuneValueToRust(makeFortuneCell({ f: '=A1', v: 'hello' }));
    expect(out).toEqual({ type: 'string', value: 'hello' });
  });

  it('returns error for formula cells whose v looks like an error (#)', () => {
    const out = fortuneValueToRust(makeFortuneCell({ f: '=A1', v: '#REF!' }));
    expect(out).toEqual({ type: 'error', value: '#REF!' });
  });

  it('maps non-formula numeric v via ct.t === "n"', () => {
    const out = fortuneValueToRust(
      makeFortuneCell({ v: 1.5, ct: { fa: '0.00', t: 'n' } }),
    );
    expect(out.type).toBe('float');
    expect(out.value).toBe(1.5);
  });

  it('detects datetime from a date-style number format', () => {
    const out = fortuneValueToRust(
      makeFortuneCell({ v: 45000, ct: { fa: 'yyyy-mm-dd', t: 'n' } }),
    );
    expect(out.type).toBe('datetime');
  });

  it('returns int when value is integer-shaped even without formula', () => {
    const out = fortuneValueToRust(
      makeFortuneCell({ v: 5, ct: { fa: 'General', t: 'n' } }),
    );
    expect(out).toEqual({ type: 'int', value: 5 });
  });

  it('returns string when numeric v cannot be parsed', () => {
    const out = fortuneValueToRust(
      makeFortuneCell({ v: 'NaN-stuff', ct: { fa: '', t: 'n' } }),
    );
    expect(out.type).toBe('string');
    expect(out.value).toBe('NaN-stuff');
  });

  it('maps non-formula boolean v via ct.t === "b"', () => {
    const out = fortuneValueToRust(makeFortuneCell({ v: true, ct: { fa: 'General', t: 'b' } }));
    expect(out).toEqual({ type: 'bool', value: 1 });
  });

  it('falls through to string for unknown cell-type combinations', () => {
    const out = fortuneValueToRust(makeFortuneCell({ v: 'plain' }));
    expect(out).toEqual({ type: 'string', value: 'plain' });
  });
});

describe('fortuneStyleToRust', () => {
  it('maps numeric bl/it to font_bold/font_italic', () => {
    const out = fortuneStyleToRust(
      makeFortuneCell({ bl: 1, it: 1, fs: 16, ff: 'Arial', fc: '#ff8800', bg: '#ffeedd' }),
    );
    expect(out).toMatchObject({
      font_bold: true,
      font_italic: true,
      font_size: 16,
      font_name: 'Arial',
      font_color: '#ff8800',
      fill_fg_color: '#ffeedd',
    });
  });

  it('maps ht values to alignment_h strings', () => {
    expect(fortuneStyleToRust(makeFortuneCell({ ht: 0 })).alignment_h).toBe('center');
    expect(fortuneStyleToRust(makeFortuneCell({ ht: 1 })).alignment_h).toBe('left');
    expect(fortuneStyleToRust(makeFortuneCell({ ht: 2 })).alignment_h).toBe('right');
  });

  it('maps vt values to alignment_v strings', () => {
    expect(fortuneStyleToRust(makeFortuneCell({ vt: 0 })).alignment_v).toBe('center');
    expect(fortuneStyleToRust(makeFortuneCell({ vt: 1 })).alignment_v).toBe('top');
    expect(fortuneStyleToRust(makeFortuneCell({ vt: 2 })).alignment_v).toBe('bottom');
  });

  it('passes through the cell-type number format', () => {
    expect(
      fortuneStyleToRust(makeFortuneCell({ ct: { fa: '0.00', t: 'g' } })),
    ).toMatchObject({ number_format: '0.00' });
  });

  it('returns an empty object for a completely-default Fortune cell', () => {
    expect(fortuneStyleToRust(makeFortuneCell())).toEqual({});
  });
});
