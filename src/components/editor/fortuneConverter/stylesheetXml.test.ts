// Unit tests for the pure helpers exported from `stylesheetXml.ts`.
//
//   - `parseStyleKey` / `composeStyleKey` — round-trip the 10-part pipe-joined key
//   - `escapeXml` — XML safety
//   - `colLetter` — column-number → A-Z column letters
//   - `buildStylesXml` — full styles document construction

import { describe, expect, it } from 'vitest';

import {
  buildStylesXml,
  colLetter,
  composeStyleKey,
  escapeXml,
  parseStyleKey,
} from './stylesheetXml';

describe('parseStyleKey', () => {
  it('parses all 10 parts of a full key', () => {
    expect(parseStyleKey('0|#ff0000||1|0|#ffffff|12|Calibri|1|2')).toEqual({
      numberFormat: '0',
      fillFg: '#ff0000',
      fillBg: '',
      bold: true,
      italic: false,
      fontColor: '#ffffff',
      fontSize: 12,
      fontName: 'Calibri',
      horizontalAlign: '1',
      verticalAlign: '2',
    });
  });

  it('parses an all-empty key as defaults', () => {
    expect(parseStyleKey('||||||||||')).toEqual({
      numberFormat: '',
      fillFg: '',
      fillBg: '',
      bold: false,
      italic: false,
      fontColor: '',
      fontSize: null,
      fontName: '',
      horizontalAlign: '',
      verticalAlign: '',
    });
  });

  it('treats fields beyond the 10-part shape as empty strings', () => {
    // Defensive: if upstream ever adds fields without expanding the
    // helper, parseStyleKey should still return a sensible object.
    expect(parseStyleKey('a|b')).toEqual({
      numberFormat: 'a',
      fillFg: 'b',
      fillBg: '',
      bold: false,
      italic: false,
      fontColor: '',
      fontSize: null,
      fontName: '',
      horizontalAlign: '',
      verticalAlign: '',
    });
  });
});

describe('composeStyleKey', () => {
  it('round-trips through parseStyleKey', () => {
    const key = '#ff0000';
    const composed = composeStyleKey({
      numberFormat: '0.00',
      fillFg: key,
      fillBg: '',
      bold: true,
      italic: false,
      fontColor: '#ffffff',
      fontSize: 12,
      fontName: 'Calibri',
      horizontalAlign: '1',
      verticalAlign: '2',
    });
    expect(composed).toBe('0.00|#ff0000||1|0|#ffffff|12|Calibri|1|2');
    expect(parseStyleKey(composed).fillFg).toBe(key);
  });

  it('represents null fontSize as empty string in the key', () => {
    const out = composeStyleKey({
      numberFormat: '',
      fillFg: '',
      fillBg: '',
      bold: false,
      italic: false,
      fontColor: '',
      fontSize: null,
      fontName: '',
      horizontalAlign: '',
      verticalAlign: '',
    });
    // Slot 3+4 are bold/italic — false renders as '0', not ''.
    expect(out).toBe('|||0|0|||||');
    expect(parseStyleKey(out)).toEqual({
      numberFormat: '',
      fillFg: '',
      fillBg: '',
      bold: false,
      italic: false,
      fontColor: '',
      fontSize: null,
      fontName: '',
      horizontalAlign: '',
      verticalAlign: '',
    });
  });
});

describe('escapeXml', () => {
  it('escapes the five XML-significant characters', () => {
    expect(escapeXml('a & b')).toBe('a &amp; b');
    expect(escapeXml('<tag>')).toBe('&lt;tag&gt;');
    expect(escapeXml('"hi"')).toBe('&quot;hi&quot;');
    expect(escapeXml("it's")).toBe('it&apos;s');
  });

  it('passes plain text through unchanged', () => {
    expect(escapeXml('Hello, World!')).toBe('Hello, World!');
    expect(escapeXml('文件.xlsx')).toBe('文件.xlsx');
  });
});

describe('colLetter', () => {
  it('maps 1-based indices to A-Z for the first 26', () => {
    expect(colLetter(0)).toBe('A');
    expect(colLetter(1)).toBe('B');
    expect(colLetter(25)).toBe('Z');
  });

  it('handles the AA, AB wrap-around correctly', () => {
    expect(colLetter(26)).toBe('AA');
    expect(colLetter(27)).toBe('AB');
    expect(colLetter(51)).toBe('AZ');
    expect(colLetter(52)).toBe('BA');
  });

  it('handles triple-letter columns', () => {
    // 0-based: ZZ = index 701 (per Excel numbering where ZZ=702, AAA=703)
    expect(colLetter(701)).toBe('ZZ');
    expect(colLetter(702)).toBe('AAA');
    expect(colLetter(703)).toBe('AAB');
  });
});

describe('buildStylesXml', () => {
  it('returns an empty string when there are no styles', () => {
    expect(buildStylesXml([])).toBe('');
  });

  it('produces a valid stylesheet root with only the default font + cellXfs', () => {
    // All-empty key resolves to the default Calibri 11pt black, so the
    // output should contain exactly one font and one cellXf (the default).
    const xml = buildStylesXml(['||||||||||']);
    expect(xml).toContain('<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">');
    expect(xml).toContain('<fonts count="1">');
    expect(xml).toContain('<fills count="2">');
    expect(xml).toContain('<numFmts count="0">');
    expect(xml).toContain('<cellXfs count="2">');
  });

  it('counts fonts/fills/cellXfs correctly for distinct styles', () => {
    // 3 keys → 3 distinct font specs (Calibri 11pt default, Calibri 12pt bold,
    // Arial 14pt). The default 0-indexed font + 1 explicit cellXf + 2 keys.
    const keys = [
      '||||||||||', // default
      '0|#ff0000||1|0|#ffffff|12|Calibri|0|0', // red bg, 12pt, bold
      '0|||#abcdef|0|#000000|14|Arial|0|0', // blue text, 14pt
    ];
    const xml = buildStylesXml(keys);
    expect(xml).toContain('<fonts count="3">');
    // 0=none, 1=gray125, 2=solid #ff0000
    expect(xml).toContain('<fills count="3">');
    // 1 default cellXf + 3 keys
    expect(xml).toContain('<cellXfs count="4">');
  });

  it('escapes user-provided fill colors', () => {
    // Key shape (10 fields, all-empty except fill_fg in slot 1):
    //   number_format | fill_fg | fill_bg | bold | italic | color | size | name | h | v
    // The `<bad>` payload must round-trip through `escapeXml` so the
    // resulting `<fgColor rgb="…"/>` attribute is safe XML.
    const xml = buildStylesXml(['x|#<bad>||0|0||||x|x']);
    expect(xml).toContain('&lt;bad');
    expect(xml).not.toContain('#<bad');
  });

  it('skips emitting numFmts when no key declares a number format', () => {
    const xml = buildStylesXml(['||||||||||']);
    expect(xml).toContain('<numFmts count="0">');
    expect(xml).not.toContain('<numFmt ');
  });

  it('emits a custom numFmt when at least one key declares a number format', () => {
    const xml = buildStylesXml(['#,##0.00|||||||||||']);
    expect(xml).toContain('<numFmts count="1">');
    expect(xml).toContain('formatCode="#,##0.00"');
  });
});
