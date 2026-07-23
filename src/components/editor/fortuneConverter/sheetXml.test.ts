// Unit tests for `sheetXml.ts` — the workbook + worksheet XML builders.
//
// These focus on structural correctness (root element, namespace
// declarations, presence of well-known sub-elements) rather than
// every possible cell type, which is exercised via the styles + cell
// tests.

import { describe, expect, it } from 'vitest';

import {
  buildContentTypesXml,
  buildRelsXml,
  buildWorkbookRelsXml,
  buildWorkbookXml,
  buildWorksheetXml,
} from './sheetXml';

describe('buildContentTypesXml', () => {
  it('declares rels + xml defaults and the workbook/styles/part overrides', () => {
    const xml = buildContentTypesXml();
    expect(xml).toContain('<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">');
    expect(xml).toContain('Extension="rels"');
    expect(xml).toContain('Extension="xml"');
    expect(xml).toContain('PartName="/xl/workbook.xml"');
    expect(xml).toContain('PartName="/xl/styles.xml"');
  });
});

describe('buildRelsXml', () => {
  it('points rId1 at xl/workbook.xml', () => {
    const xml = buildRelsXml();
    expect(xml).toContain('<Relationship Id="rId1"');
    expect(xml).toContain('Target="xl/workbook.xml"');
  });
});

describe('buildWorkbookRelsXml', () => {
  it('emits exactly one styles relationship + one per sheet', () => {
    const xml = buildWorkbookRelsXml(3);
    expect((xml.match(/Type="[^"]*\/styles"/g) ?? []).length).toBe(1);
    expect((xml.match(/Type="[^"]*\/worksheet"/g) ?? []).length).toBe(3);
  });

  it('uses rIds rId2..rId(N+1) for the worksheets (rId1 = styles)', () => {
    const xml = buildWorkbookRelsXml(2);
    expect(xml).toContain('Id="rId2"');
    expect(xml).toContain('Id="rId3"');
    expect(xml).not.toContain('Id="rId4"');
  });

  it('handles zero sheets (just the styles relationship)', () => {
    const xml = buildWorkbookRelsXml(0);
    expect((xml.match(/worksheet/g) ?? []).length).toBe(0);
    expect(xml).toContain('Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles"');
  });
});

describe('buildWorkbookXml', () => {
  it('lists each sheet with name, sheetId, and r:id', () => {
    const xml = buildWorkbookXml(['销售', '库存']);
    expect(xml).toContain('<sheet name="销售" sheetId="1" r:id="rId2"');
    expect(xml).toContain('<sheet name="库存" sheetId="2" r:id="rId3"');
  });

  it('escapes sheet names containing special characters', () => {
    const xml = buildWorkbookXml(['Q1 & Q2']);
    expect(xml).toContain('name="Q1 &amp; Q2"');
    expect(xml).not.toContain('name="Q1 & Q2"');
  });

  it('handles an empty sheet list', () => {
    const xml = buildWorkbookXml([]);
    expect(xml).toContain('<sheets>');
    expect(xml).toContain('</sheets>');
    expect(xml).not.toContain('<sheet ');
  });
});

describe('buildWorksheetXml', () => {
  function makeStyleIndexMap(keys: string[]): Map<string, number> {
    const map = new Map<string, number>();
    keys.forEach((k, i) => map.set(k, i + 1));
    return map;
  }

  it('emits the worksheet root + sheetData root for an empty sheet', () => {
    const xml = buildWorksheetXml({}, new Map());
    expect(xml).toContain('<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"');
    expect(xml).toContain('<sheetData>');
    expect(xml).toContain('</sheetData>');
    expect(xml).toContain('</worksheet>');
  });

  it('emits one <row> per row, sorted by row number', () => {
    const xml = buildWorksheetXml(
      {
        'A1': { v: 1 },
        'C3': { v: 'three' },
        'B2': { v: 2 },
      },
      makeStyleIndexMap([]),
    );
    const rowOrder = [...xml.matchAll(/<row r="(\d+)"/g)].map((m) => m[1]);
    expect(rowOrder).toEqual(['1', '2', '3']);
  });

  it('attaches a style index from the styleIndexMap', () => {
    const xml = buildWorksheetXml(
      { 'A1': { v: 1, _styleKey: 'k1' } },
      new Map([['k1', 4]]),
    );
    expect(xml).toContain('<c r="A1" s="4"');
  });

  it('renders inline strings for type "str"', () => {
    const xml = buildWorksheetXml(
      { 'A1': { v: 'hello', t: 'str' } },
      new Map(),
    );
    expect(xml).toContain('t="inlineStr"');
    expect(xml).toContain('<is><t>hello</t></is>');
  });

  it('renders boolean cells as 0/1 with t="b"', () => {
    const xml = buildWorksheetXml(
      {
        'A1': { v: 1, t: 'b' },
        'A2': { v: 0, t: 'b' },
      },
      new Map(),
    );
    expect(xml).toMatch(/<c r="A1"[^>]*t="b"[^>]*>.*<v>1<\/v>/s);
    expect(xml).toMatch(/<c r="A2"[^>]*t="b"[^>]*>.*<v>0<\/v>/s);
  });

  it('renders formula cells with <f> and a computed <v>', () => {
    const xml = buildWorksheetXml(
      { 'A1': { v: 7, f: '=SUM(B1:B5)' } },
      new Map(),
    );
    expect(xml).toContain('<f>SUM(B1:B5)</f>');
    expect(xml).toContain('<v>7</v>');
  });

  it('strips leading = from formulas', () => {
    const xml = buildWorksheetXml(
      { 'A1': { v: 42, f: '=2*21' } },
      new Map(),
    );
    expect(xml).not.toContain('<f>=2*21</f>');
    expect(xml).toContain('<f>2*21</f>');
  });

  it('emits mergedCells with the right address syntax', () => {
    const xml = buildWorksheetXml(
      { A1: { v: 'merged' } },
      new Map(),
      // We pass merges via worksheet
    ) as unknown as string;
    // Re-issue with merges:
    const xml2 = buildWorksheetXml(
      {
        A1: { v: 'merged' },
        '!merges': [{ r: 0, c: 0, rs: 2, cs: 3 }],
      },
      new Map(),
    );
    expect(xml2).toContain('<mergeCells count="1">');
    expect(xml2).toContain('<mergeCell ref="A1:C2"/>');
    // Sanity: the first call (without merges prop) shouldn't crash either
    expect(typeof xml).toBe('string');
  });

  it('writes column widths from !cols', () => {
    const xml = buildWorksheetXml(
      {
        A1: { v: 1 },
        '!cols': [{ wpx: 80 }, { wpx: 12 }],
      },
      new Map(),
    );
    expect(xml).toContain('<col min="1" max="1"');
    expect(xml).toContain('<col min="2" max="2"');
    // Skip undefined col entries
    expect(xml).not.toContain('<col min="3"');
  });

  it('writes row heights from !rows', () => {
    const xml = buildWorksheetXml(
      {
        A1: { v: 1 },
        A2: { v: 2 },
        '!rows': [{}, { hpx: 36 }],
      },
      new Map(),
    );
    expect(xml).toMatch(/<row r="1"/);
    expect(xml).toMatch(/<row r="2" ht="36" customHeight="1"/);
  });

  it('skips reserved keys like "!ref"', () => {
    const xml = buildWorksheetXml(
      {
        '!ref': 'A1:B2',
        A1: { v: 1 },
      },
      new Map(),
    );
    expect(xml).not.toContain('!ref');
    expect(xml).toContain('<c r="A1"');
  });

  it('escapes XML-significant characters in cell values', () => {
    const xml = buildWorksheetXml(
      { A1: { v: 'a & b < c', t: 'str' } },
      new Map(),
    );
    expect(xml).toContain('a &amp; b &lt; c');
  });
});
