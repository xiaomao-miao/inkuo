// SheetJS (xlsx) export path: FortuneSheet → .xlsx buffer.
//
// This is the primary save path. Instead of going through Rust's
// structured xlsx writer (which has a fragile Rust→FortuneSheet→Rust
// conversion layer), we convert directly in the browser using
// SheetJS, then post-process the resulting .xlsx zip to swap in a
// custom `xl/styles.xml` so we keep cell-level formatting intact.
//
// Pieces of this pipeline:
//   1. `fortuneCellToSheetJS`           — single-cell conversion
//   2. `fortuneSheetToSheetJSWorksheet` — sheet-level conversion
//   3. `fortuneSheetsToSheetJSBuffer`   — top-level entry point
//
// SheetJS cell object reference:
//   { v: value,     // computed result
//     f: formula,   // formula text (without leading '=')
//     t: type,      // 's'=string, 'n'=number, 'b'=boolean, 'e'=error
//     w: formatted, // display string
//     z: number_fmt // number format code
//   }
//
// Merged cells: set the top-left cell in the range; all covered cells
// stay empty.

import type {
  Cell as FortuneCell,
  CellMatrix,
  CellWithRowAndCol,
  Sheet as FortuneSheetCoreSheet,
} from '@fortune-sheet/core';

import {
  buildContentTypesXml,
  buildRelsXml,
  buildWorkbookRelsXml,
  buildWorkbookXml,
  buildWorksheetXml,
} from './sheetXml';
import { composeStyleKey, escapeXml, parseStyleKey } from './stylesheetXml';

// Lazily imported to avoid loading SheetJS until the first save.
// The `xlsx` package (SheetJS 0.18.x) is a peer dep already present
// in the project.
type SheetJSLazy = typeof import('xlsx');

let _XLSX: SheetJSLazy | null = null;
async function getXLSX(): Promise<SheetJSLazy> {
  if (!_XLSX) {
    const m = await import('xlsx');
    // Handle both CJS `module.exports` and ESM `export default` patterns.
    _XLSX = (m.default ?? m) as SheetJSLazy;
  }
  return _XLSX;
}

/**
 * Style key shape used by SheetJS-side dedup. Matches the field
 * shape used by Rust's SheetStyleKey so the XML index is portable.
 */
const STYLE_KEY_PARTS = 10;
const STYLE_KEY_ALL_EMPTY = Array(STYLE_KEY_PARTS).fill('').join('|');

/** Build the dedup key for a FortuneSheet cell's style hints. */
function buildStyleKey(v: FortuneCell): string {
  const key = [
    v.ct?.fa ?? '',
    v.bg ?? '',
    '', // fill_bg_color not in FortuneCell — placeholder
    v.bl === 1 ? '1' : '0',
    v.it === 1 ? '1' : '0',
    v.fc ?? '',
    v.fs != null ? String(v.fs) : '',
    v.ff ?? '',
    v.ht !== undefined ? String(v.ht) : '',
    v.vt !== undefined ? String(v.vt) : '',
  ].join('|');
  return key;
}

/**
 * Convert a FortuneSheet cell value to a plain serialisable value +
 * type tag. SheetJS uses:
 *   t = 'n'   → number
 *   t = 's'   → string (stored in sharedStrings)
 *   t = 'b'   → boolean
 *   t = 'e'   → error string
 *   t = 'd'   → Date
 *   t = 'str' → inline string (formula result or inline string)
 *
 * We use `'str'` for strings instead of `'s'` so we don't have to
 * allocate a sharedStrings array for simple edits.
 */
function fortuneCellToSheetJS(v: FortuneCell): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  const raw = v.v;

  if (raw === undefined || raw === null) {
    // No value at all — leave result empty
  } else if (typeof raw === 'number' && !isNaN(raw)) {
    result.v = raw;
    result.t = 'n';
    if (v.m !== undefined) result.w = v.m;
  } else if (typeof raw === 'boolean') {
    result.v = raw ? 1 : 0;
    result.t = 'b';
    result.w = raw ? 'TRUE' : 'FALSE';
  } else if (typeof raw === 'string') {
    if (raw.startsWith('#')) {
      // Error value
      result.v = raw;
      result.t = 'e';
      result.w = raw;
    } else {
      // Use 'str' type for inline strings to avoid needing a sharedStrings array
      result.v = raw;
      result.t = 'str';
      if (v.m !== undefined) result.w = v.m;
    }
  }

  // ── Formula ──────────────────────────────────────────────────
  // SheetJS stores formulas WITHOUT leading '='. We set f=formula
  // text; if there's also a computed value, both coexist.
  if (v.f && typeof v.f === 'string') {
    result.f = v.f.startsWith('=') ? v.f.slice(1) : v.f;
  }

  // ── Number format ────────────────────────────────────────────
  if (v.ct?.fa && v.ct.fa !== 'General') {
    result.z = v.ct.fa;
  }

  // ── Style ────────────────────────────────────────────────────
  const styleKey = buildStyleKey(v);
  if (styleKey !== STYLE_KEY_ALL_EMPTY) {
    result._styleKey = styleKey;
  }

  return result;
}

interface SheetJSWorksheet {
  /** Sparse map of cell address ("A1") → cell. */
  [addr: string]: unknown;
  /** Cell range like `"A1:C3"`. */
  '!ref'?: string;
  /** Merged ranges. */
  '!merges'?: Array<{ r: number; c: number; rs: number; cs: number }>;
  /** Per-row heights (index 0 = row 1). */
  '!rows'?: Array<{ hpx?: number }>;
  /** Per-column widths (index 0 = column A). */
  '!cols'?: Array<{ wpx?: number }>;
}

/** Result of converting one FortuneSheet sheet. */
interface BuiltWorksheet {
  worksheet: SheetJSWorksheet;
  styleKeys: string[];
}

/**
 * Convert a FortuneSheet Sheet to a SheetJS worksheet object.
 * Merged regions, row heights, and column widths are stored in
 * SheetJS's worksheet metadata properties.
 */
async function fortuneSheetToSheetJSWorksheet(
  sheet: FortuneSheetCoreSheet,
  XLSX: SheetJSLazy,
  dataToCelldata: (data: CellMatrix) => CellWithRowAndCol[],
): Promise<BuiltWorksheet> {
  const cells: Record<string, Record<string, unknown>> = {};
  const styleKeys: string[] = [];
  const data = sheet.data ?? [];

  let celldata = sheet.celldata ?? [];
  try {
    if (data.length > 0) {
      const convertedCelldata = dataToCelldata(data);
      if (convertedCelldata && convertedCelldata.length > 0) {
        celldata = convertedCelldata;
      }
    }
  } catch {
    // Fall back to existing celldata
  }

  const sparseMap = new Map<string, FortuneCell>();

  for (const item of celldata) {
    if (item.v) sparseMap.set(`${item.r},${item.c}`, item.v);
  }

  for (const [key, v] of sparseMap) {
    const [rStr, cStr] = key.split(',');
    const r = parseInt(rStr, 10);
    const c = parseInt(cStr, 10);
    const addr = XLSX.utils.encode_cell({ r, c });
    const jsCell = fortuneCellToSheetJS(v);
    if (Object.keys(jsCell).length > 0) {
      cells[addr] = jsCell;
      const sk = jsCell._styleKey;
      if (typeof sk === 'string' && sk !== STYLE_KEY_ALL_EMPTY) {
        styleKeys.push(sk);
      }
    }
  }

  // Build the worksheet struct including its `!ref`, `!merges`, `!rows`, `!cols`
  // metadata. Row/col heights + widths + merges come from FortuneSheet's config.
  const mergeConfig = (sheet.config?.merge ?? {}) as Record<
    string,
    { r: number; c: number; rs: number; cs: number }
  >;
  const rowlenConfig = (sheet.config?.rowlen ?? {}) as Record<string, number>;
  const columnlenConfig = (sheet.config?.columnlen ?? {}) as Record<string, number>;

  const merges = Object.values(mergeConfig);
  const rows: Array<{ hpx?: number }> = Object.entries(rowlenConfig).map(([_, hpx]) => ({ hpx }));
  const cols: Array<{ wpx?: number }> = Object.entries(columnlenConfig).map(([_, wpx]) => ({ wpx }));

  // Determine overall ref range.
  let minRow = Infinity, maxRow = 0, minCol = Infinity, maxCol = 0;
  for (const key of Object.keys(cells)) {
    const [rStr, cStr] = key.split(',');
    // Cells are keyed by SheetJS address ("A1"), not "r,c" — convert back.
    const m = key.match(/^([A-Z]+)(\d+)$/);
    if (!m) continue;
    const r = parseInt(m[2], 10) - 1;
    const colLetters = m[1];
    let c = 0;
    for (let i = 0; i < colLetters.length; i++) {
      c = c * 26 + (colLetters.charCodeAt(i) - 64);
    }
    c -= 1;
    minRow = Math.min(minRow, r);
    maxRow = Math.max(maxRow, r);
    minCol = Math.min(minCol, c);
    maxCol = Math.max(maxCol, c);
    // also consume rStr/cStr to satisfy unused-locals
    void rStr; void cStr;
  }

  const rangeRef = minRow <= maxRow
    ? `${XLSX.utils.encode_cell({ r: minRow, c: minCol })}:${XLSX.utils.encode_cell({ r: maxRow, c: maxCol })}`
    : 'A1';

  const worksheet: SheetJSWorksheet = {
    ...cells,
    '!ref': rangeRef,
    ...(merges.length > 0 ? { '!merges': merges } : {}),
    ...(rows.length > 0 ? { '!rows': rows } : {}),
    ...(cols.length > 0 ? { '!cols': cols } : {}),
  };

  return { worksheet, styleKeys };
}

/**
 * Convert FortuneSheet sheets to a SheetJS workbook AND return the binary buffer.
 * Styles are preserved by injecting a custom stylesheet via JSZip post-processing.
 *
 * Usage:
 *   const buffer = await fortuneSheetsToSheetJSBuffer(sheets, wb.dataToCelldata.bind(wb));
 *   // Write buffer to disk via Tauri invoke('write_office_file', ...)
 */
export async function fortuneSheetsToSheetJSBuffer(
  allSheets: FortuneSheetCoreSheet[],
  dataToCelldata: (data: CellMatrix) => CellWithRowAndCol[],
): Promise<Uint8Array> {
  const XLSX = await getXLSX();

  // First pass: collect all style keys across all sheets.
  const allStyleKeys: string[] = [];
  // `worksheet` is the parsed SheetJS Worksheet object (a `Record<string, unknown>`
  // keyed by cell address like `"A1"` plus reserved keys prefixed with `!`).
  // Typing it as `Record<string, unknown>` up front lets `buildWorksheetXml`
  // accept it directly without a separate narrowing step.
  const sheetRawData: { name: string; worksheet: Record<string, unknown> }[] = [];

  for (const sheet of allSheets) {
    const { worksheet, styleKeys } = await fortuneSheetToSheetJSWorksheet(sheet, XLSX, dataToCelldata);
    allStyleKeys.push(...styleKeys);
    sheetRawData.push({ name: sheet.name ?? 'Sheet1', worksheet });
  }

  // Deduplicate style keys.
  const uniqueStyleKeys = [...new Set(allStyleKeys)];
  const styleIndexMap: Map<string, number> = new Map();
  uniqueStyleKeys.forEach((key, idx) => styleIndexMap.set(key, idx + 1)); // 0 = default

  // Generate xlsx with custom worksheet XML (not SheetJS cell writing).
  // SheetJS re-processes cells and ignores our s values, so we build
  // the worksheet XML directly to ensure correct style references.
  const { buildStylesXml } = await import('./stylesheetXml');
  const stylesXml = uniqueStyleKeys.length > 0 ? buildStylesXml(uniqueStyleKeys) : null;
  const jszipMod = await import('jszip');
  const JSZip = jszipMod.default;

  const zip = new JSZip();
  zip.file('[Content_Types].xml', buildContentTypesXml());
  zip.file('_rels/.rels', buildRelsXml());
  zip.file('xl/_rels/workbook.xml.rels', buildWorkbookRelsXml(sheetRawData.length));
  zip.file('xl/workbook.xml', buildWorkbookXml(sheetRawData.map(s => s.name)));
  if (stylesXml) {
    zip.file('xl/styles.xml', stylesXml);
  }

  for (let i = 0; i < sheetRawData.length; i++) {
    const { worksheet } = sheetRawData[i];
    const worksheetXml = buildWorksheetXml(worksheet, styleIndexMap);
    zip.file(`xl/worksheets/sheet${i + 1}.xml`, worksheetXml);
  }

  const modified = await zip.generateAsync({ type: 'uint8array', compression: 'DEFLATE' });
  return modified;
}

// Re-export XML helpers so any caller that wants to compose its own
// pieces (e.g. for tests) can grab them from the same module surface.
export {
  composeStyleKey,
  escapeXml,
  parseStyleKey,
};
