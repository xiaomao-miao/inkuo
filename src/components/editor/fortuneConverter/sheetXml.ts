// Build the worksheet + workbook-level XML files that make up an
// .xlsx package. Cell ranges, merged regions, column widths, and
// row heights all live in the worksheet XML; styles live in
// `stylesheetXml.ts` (kept separate because their construction is
// much heavier).
//
// One small ColumnSpec/RowsSpec/MergeSpec helper type is used here;
// see `sheetjsExport.ts` for the upstream source of these arrays.

import { colLetter, escapeXml } from './stylesheetXml';

export interface MergeSpec {
  r: number;
  c: number;
  rs: number;
  cs: number;
}

export interface ColWidthSpec {
  wpx?: number;
}

export interface RowHeightSpec {
  hpx?: number;
}

/**
 * Char-units-per-pixel conversion used for column widths. Excel
 * stores column widths as character units; we round to 1/256
 * precision. Constants match the inverse of `excelToPixel`.
 */
const EXCEL_MDW = 7;
const COLUMN_PADDING_PX = 5;

/** `[Content_Types].xml` — declares MIME content types for every part. */
export function buildContentTypesXml(sheetCount = 1): string {
  const worksheetOverrides = Array.from(
    { length: Math.max(0, sheetCount) },
    (_, index) => `  <Override PartName="/xl/worksheets/sheet${index + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>`,
  ).join('\n');
  return `<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>
${worksheetOverrides}
</Types>`;
}

/** `_rels/.rels` — top-level relationships for the package. */
export function buildRelsXml(): string {
  return `<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>`;
}

/** `xl/_rels/workbook.xml.rels` — workbook's references to sheets + styles. */
export function buildWorkbookRelsXml(sheetCount: number): string {
  let sheets = '';
  for (let i = 1; i <= sheetCount; i++) {
    sheets += `  <Relationship Id="rId${i + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet${i}.xml"/>\n`;
  }
  return `<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
${sheets}</Relationships>`;
}

/** `xl/workbook.xml` — lists every sheet's name + rId. */
export function buildWorkbookXml(sheetNames: string[]): string {
  let sheets = '';
  sheetNames.forEach((name, i) => {
    const sheetName = escapeXml(name);
    sheets += `    <sheet name="${sheetName}" sheetId="${i + 1}" r:id="rId${i + 2}"/>\n`;
  });
  return `<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
  xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
${sheets}  </sheets>
</workbook>`;
}

/**
 * Build one `<c r="…"/>` element. Dispatches on the SheetJS cell type
 * tag (`t`) to pick the right XML element (`<v>` vs `<is><t>`).
 *
 *   - `t = 's'`    — shared-string reference, raw `<v>`
 *   - `t = 'str'`  — inline string, `<is><t>`
 *   - `t = 'b'`    — boolean as 0/1
 *   - `t = 'e'`    — error, inline string
 *   - else         — raw `<v>` (number / inline numeric)
 */
function cellToXml(
  addr: string,
  cell: Record<string, unknown>,
  styleIndexMap: Map<string, number>,
): string {
  const styleKey = cell._styleKey as string | undefined;
  const s = styleKey ? styleIndexMap.get(styleKey) : undefined;

  let xml = `<c r="${addr}"`;
  if (s !== undefined) xml += ` s="${s}"`;

  const t = cell.t as string | undefined;
  const hasFormula = cell.f !== undefined && cell.f !== null && cell.f !== '';
  if (t === 's') xml += ` t="s"`;
  else if (t === 'str') xml += hasFormula ? ` t="str"` : ` t="inlineStr"`;
  else if (t === 'b') xml += ` t="b"`;
  else if (t === 'e') xml += ` t="e"`;

  xml += '>';

  if (hasFormula) {
    // Strip leading = from formula
    const f = escapeXml(String(cell.f).replace(/^=/, ''));
    xml += `<f>${f}</f>`;
    if (cell.v !== undefined) {
      xml += `<v>${escapeXml(String(cell.v))}</v>`;
    }
  } else if (cell.v !== undefined) {
    if (t === 's') {
      xml += `<v>${cell.v}</v>`;
    } else if (t === 'str') {
      xml += `<is><t>${escapeXml(String(cell.v))}</t></is>`;
    } else if (t === 'e') {
      xml += `<v>${escapeXml(String(cell.v))}</v>`;
    } else if (t === 'b') {
      xml += `<v>${cell.v ? 1 : 0}</v>`;
    } else {
      xml += `<v>${cell.v}</v>`;
    }
  }

  xml += '</c>';
  return xml;
}

/**
 * Group cell addresses by row number, then sort rows + addresses.
 */
function groupCellsByRow(
  worksheet: Record<string, unknown>,
): { rowGroups: Record<number, Record<string, unknown>>; sortedRows: number[] } {
  const rowGroups: Record<number, Record<string, unknown>> = {};
  for (const [addr, cell] of Object.entries(worksheet)) {
    if (addr.startsWith('!')) continue;
    const match = addr.match(/^([A-Z]+)(\d+)$/);
    if (!match) continue;
    const row = parseInt(match[2], 10);
    if (!rowGroups[row]) rowGroups[row] = {};
    rowGroups[row][addr] = cell;
  }
  const sortedRows = Object.keys(rowGroups).map(Number).sort((a, b) => a - b);
  return { rowGroups, sortedRows };
}

/**
 * Convert a pixel column width back into Excel char-unit width.
 * Inverse of the forward conversion in `sheetConversions.ts`.
 */
function pixelWidthToExcelUnits(wpx: number): number {
  return Math.round(((wpx - COLUMN_PADDING_PX) / EXCEL_MDW) * 256) / 256;
}

/** Convert cell column letters ("A", "AB") into a one-based column index. */
function cellColumnIndex(addr: string): number {
  const letters = addr.match(/^([A-Z]+)/)?.[1] ?? '';
  let index = 0;
  for (const letter of letters) index = index * 26 + letter.charCodeAt(0) - 64;
  return index;
}

/**
 * Build one `<worksheet>` document from a SheetJS worksheet object
 * keyed by cell address (`"A1"`) with reserved keys prefixed `!`
 * (`!merges`, `!rows`, `!cols`).
 */
export function buildWorksheetXml(
  worksheet: Record<string, unknown>,
  styleIndexMap: Map<string, number>,
): string {
  const merges = (worksheet['!merges'] as MergeSpec[] | undefined) ?? [];
  const rows = (worksheet['!rows'] as RowHeightSpec[] | undefined) ?? [];
  const cols = (worksheet['!cols'] as ColWidthSpec[] | undefined) ?? [];

  let xml = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
  xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheetViews><sheetView workbookViewId="0"/></sheetViews>`;

  // Column widths
  if (cols.length > 0) {
    xml += '<cols>';
    cols.forEach((col, i) => {
      if (col.wpx) {
        const w = pixelWidthToExcelUnits(col.wpx);
        xml += `<col min="${i + 1}" max="${i + 1}" width="${w}" customWidth="1"/>`;
      }
    });
    xml += '</cols>';
  }

  xml += `<sheetData>`;

  const { rowGroups, sortedRows } = groupCellsByRow(worksheet);
  // A custom height on an otherwise empty row is still meaningful. Include
  // those sparse row indexes instead of emitting rows only when they contain
  // a cell.
  const rowsToWrite = [...new Set([
    ...sortedRows,
    ...rows.flatMap((row, index) => row?.hpx ? [index + 1] : []),
  ])].sort((a, b) => a - b);

  for (const rowNum of rowsToWrite) {
    const rowCells = rowGroups[rowNum] ?? {};
    xml += `<row r="${rowNum}"`;
    if (rows[rowNum - 1]?.hpx) {
      xml += ` ht="${rows[rowNum - 1].hpx}" customHeight="1"`;
    }
    xml += '>';

    const sortedAddrs = Object.keys(rowCells).sort((a, b) => {
      return cellColumnIndex(a) - cellColumnIndex(b);
    });

    for (const addr of sortedAddrs) {
      const cell = rowCells[addr] as Record<string, unknown>;
      xml += cellToXml(addr, cell, styleIndexMap);
    }

    xml += '</row>';
  }

  xml += '</sheetData>';

  if (merges.length > 0) {
    xml += '<mergeCells count="' + merges.length + '">';
    for (const mc of merges) {
      const ref = `${colLetter(mc.c)}${mc.r + 1}:${colLetter(mc.c + mc.cs - 1)}${mc.r + mc.rs}`;
      xml += `<mergeCell ref="${ref}"/>`;
    }
    xml += '</mergeCells>';
  }

  xml += '</worksheet>';
  return xml;
}
