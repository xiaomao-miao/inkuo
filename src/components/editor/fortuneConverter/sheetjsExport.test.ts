import { describe, expect, it } from 'vitest';

import { encodeCellAddress, fortuneSheetsToXlsxBuffer, normalizeSheetNames } from './sheetjsExport';

describe('encodeCellAddress', () => {
  it.each([
    [0, 0, 'A1'],
    [0, 25, 'Z1'],
    [0, 26, 'AA1'],
    [8, 51, 'AZ9'],
    [99, 701, 'ZZ100'],
    [1_048_575, 16_383, 'XFD1048576'],
  ])('encodes row %i column %i as %s', (row, column, expected) => {
    expect(encodeCellAddress(row, column)).toBe(expected);
  });

  it('rejects invalid coordinates', () => {
    expect(() => encodeCellAddress(-1, 0)).toThrow(RangeError);
    expect(() => encodeCellAddress(0, 1.5)).toThrow(RangeError);
  });
});

describe('normalizeSheetNames', () => {
  it('removes invalid characters, caps length, and de-duplicates names', () => {
    const names = normalizeSheetNames([
      '预算/2026',
      '预算:2026',
      'A very long worksheet name that exceeds thirty one characters',
      '',
    ]);
    expect(names[0]).toBe('预算_2026');
    expect(names[1]).toBe('预算_2026 (2)');
    expect(Array.from(names[2]).length).toBeLessThanOrEqual(31);
    expect(names[3]).toBe('Sheet4');
  });
});

describe('fortuneSheetsToXlsxBuffer', () => {
  it('writes a readable OOXML workbook without SheetJS', async () => {
    const cell = { v: '你好', m: '你好' };
    const sheets = ([
      {
        name: '报告',
        data: [[cell]],
        config: { rowlen: { 5: 42 }, columnlen: { 3: 100 } },
      },
      { name: '附录', data: [], config: {} },
    ] as unknown) as Parameters<typeof fortuneSheetsToXlsxBuffer>[0];
    const buffer = await fortuneSheetsToXlsxBuffer(
      sheets,
      () => [{ r: 0, c: 0, v: cell }],
    );

    const { default: JSZip } = await import('jszip');
    const zip = await JSZip.loadAsync(buffer);
    const workbookXml = await zip.file('xl/workbook.xml')?.async('string');
    const worksheetXml = await zip.file('xl/worksheets/sheet1.xml')?.async('string');
    const contentTypesXml = await zip.file('[Content_Types].xml')?.async('string');

    expect(workbookXml).toContain('name="报告"');
    expect(workbookXml).toContain('name="附录"');
    expect(contentTypesXml).toContain('/xl/worksheets/sheet2.xml');
    expect(zip.file('xl/styles.xml')).not.toBeNull();
    expect(worksheetXml).toContain('r="A1"');
    expect(worksheetXml).toContain('你好');
    expect(worksheetXml).toContain('<row r="6" ht="42" customHeight="1">');
    expect(worksheetXml).toContain('<col min="4" max="4"');
  });
});
