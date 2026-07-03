/**
 * fortuneSheetToSheetJS.ts — Convert FortuneSheet data → SheetJS (xlsx) workbook
 *
 * This is the primary save path. Instead of going through Rust's structured xlsx
 * writer (which has a fragile Rust→FortuneSheet→Rust conversion layer), we
 * convert directly in the browser using SheetJS. SheetJS produces standards-compliant
 * .xlsx files that Excel/WPS/LibreOffice open without issues.
 *
 * SheetJS cell object reference:
 *   { v: value,     // computed result
 *     f: formula,   // formula text (without leading '='), set via sheet_set_array_formula
 *     t: type,      // 's'=string, 'n'=number, 'b'=boolean, 'e'=error, 'd'=date
 *     w: formatted,  // display string
 *     z: number_fmt, // number format code or format string
 *     s: style_idx,  // Xf index (for fonts, fills, alignment etc.)
 *   }
 *
 * Merged cells: set the top-left cell in the range; all covered cells stay empty.
 */

import type { Sheet as FortuneSheetCoreSheet, Cell as FortuneCell, CellWithRowAndCol } from '@fortune-sheet/core';

// ─── Rust xlsx types (mirrored from backend) ─────────────────────────────────

export interface RustCellValue {
  type: 'empty' | 'int' | 'float' | 'bool' | 'string' | 'error' | 'datetime';
  // All typed variants use the unified "value" field (serde content = "value")
  value?: number | string;
}

export interface RustCellStyle {
  number_format?: string;
  fill_fg_color?: string;
  fill_bg_color?: string;
  font_bold?: boolean;
  font_italic?: boolean;
  font_color?: string;
  font_size?: number;
  font_name?: string;
  alignment_h?: string;
  alignment_v?: string;
}

export interface RustCell {
  row: number;
  col: number;
  value: RustCellValue;
  formula?: string;
  style?: RustCellStyle;
}

export interface RustMergedRange {
  start_row: number;
  start_col: number;
  end_row: number;
  end_col: number;
}

export interface RustXlsxSheet {
  name: string;
  state: string;
  cells: RustCell[];
  merged_cells: RustMergedRange[];
  max_row: number;
  max_col: number;
  /** Row heights: map of row index (0-based) to height in points. */
  row_heights?: Record<string, number>;
  /** Column widths: map of column index (0-based) to width in Excel character units. */
  col_widths?: Record<string, number>;
}

export interface RustXlsxWorkbook {
  sheets: RustXlsxSheet[];
  shared_strings: string[];
}

// ─── Style helpers ────────────────────────────────────────────────────────────

/** Normalise a colour string from Rust/OOXML to a CSS hex colour.
 *  OOXML can produce:
 *    - #RRGGBB   (6-char with hash — from Rust after our fix)
 *    - #AARRGGBB (8-char with hash — rare, from OOXML direct)
 *    - RRGGBB    (6-char without hash — old Rust or direct OOXML)
 *    - AARRGGBB  (8-char without hash — direct OOXML)
 *  FortuneSheet expects #RRGGBB (6-char with hash).
 */
function normaliseColor(color: string | undefined): string | undefined {
  if (!color) return undefined;
  // Already well-formed with hash
  if (/^#[0-9a-fA-F]{6}$/.test(color)) return color.toLowerCase();
  // 8-char ARGB with hash → strip alpha
  if (/^#[0-9a-fA-F]{8}$/.test(color)) return ('#' + color.slice(3)).toLowerCase();
  // 6-char without hash
  if (/^[0-9a-fA-F]{6}$/.test(color)) return '#' + color.toLowerCase();
  // 8-char ARGB without hash → strip alpha
  if (/^[0-9a-fA-F]{8}$/.test(color)) return '#' + color.slice(2).toLowerCase();
  // Named colours, rgb(), etc. — return as-is
  return color;
}

/** Convert Rust horizontal alignment string to FortuneSheet ht value.
 *  FortuneSheet: 0 = center, 1 = left, 2 = right */
function alignH(h: string | undefined): number | undefined {
  if (!h) return undefined;
  switch (h) {
    case 'center': case 'centre': return 0;
    case 'left':   return 1;
    case 'right':  return 2;
    default:       return undefined;
  }
}

/** Convert Rust vertical alignment string to FortuneSheet vt value.
 *  FortuneSheet: 0 = middle, 1 = top, 2 = bottom */
function alignV(v: string | undefined): number | undefined {
  if (!v) return undefined;
  switch (v) {
    case 'center':  return 0;
    case 'top':     return 1;
    case 'bottom':  return 2;
    default:        return undefined;
  }
}

// ─── Core conversion: Rust Cell -> FortuneSheet Cell ────────────────────────────

function rustCellToFortune(cell: RustCell): FortuneCell {
  const fortune: FortuneCell = {};

  switch (cell.value.type) {
    case 'empty':
      break;
    case 'int':
      fortune.v = cell.value.value ?? 0;
      fortune.ct = { fa: '0', t: 'n' };
      fortune.m = String(cell.value.value ?? 0);
      break;
    case 'float':
      fortune.v = cell.value.value ?? 0;
      fortune.ct = { fa: 'General', t: 'n' };
      fortune.m = String(cell.value.value ?? 0);
      break;
    case 'datetime': {
      const serial = cell.value.value ?? 0;
      fortune.v = serial;
      const fmt = cell.style?.number_format;
      fortune.ct = {
        fa: fmt && fmt !== 'General' ? fmt : 'yyyy-mm-dd',
        t: 'n',
      };
      fortune.m = String(serial);
      break;
    }
    case 'bool':
      fortune.v = cell.value.value !== 0;
      fortune.m = fortune.v ? 'TRUE' : 'FALSE';
      fortune.ct = { fa: 'General', t: 'g' };
      break;
    case 'string':
      fortune.v = cell.value.value ?? '';
      fortune.m = cell.value.value ?? '';
      fortune.ct = { fa: '@', t: 's' };
      break;
    case 'error':
      fortune.v = `#ERR:${cell.value.value ?? ''}`;
      fortune.m = `#ERR:${cell.value.value ?? ''}`;
      fortune.ct = { fa: 'General', t: 'g' };
      break;
  }

  if (cell.formula) {
    // OOXML <f> has no leading "=", HyperFormula requires it.
    fortune.f = cell.formula.startsWith('=') ? cell.formula : `=${cell.formula}`;
  }

  const s = cell.style;
  if (s) {
    if (s.font_bold)    fortune.bl = 1;
    if (s.font_italic) fortune.it = 1;
    if (s.font_size)    fortune.fs = s.font_size;
    if (s.font_name)    fortune.ff = s.font_name;
    const fc = normaliseColor(s.font_color);
    if (fc) fortune.fc = fc;
    const bg = normaliseColor(s.fill_fg_color);
    if (bg) fortune.bg = bg;
    const h = alignH(s.alignment_h);
    if (h !== undefined) fortune.ht = h;
    const v = alignV(s.alignment_v);
    if (v !== undefined) fortune.vt = v;
    if (s.number_format && s.number_format !== 'General') {
      if (!fortune.ct) fortune.ct = { fa: s.number_format, t: 'g' };
    }
  }

  return fortune;
}

// ─── Core conversion: Rust XlsxSheet -> FortuneSheet ──────────────────────────

/**
 * Convert a Rust XlsxSheet (sparse cells, typed values, styles, merged ranges)
 * to a FortuneSheet Sheet. Uses the `celldata` (sparse) format.
 */
export function rustSheetToFortuneSheet(sheet: RustXlsxSheet): FortuneSheetCoreSheet {
  // Build merged-cell config map: key = "anchorRow_anchorCol"

  // Build merged-cell config map: key = "anchorRow_anchorCol"
  const mergeConfig: Record<string, { r: number; c: number; rs: number; cs: number }> = {};
  for (const mr of sheet.merged_cells) {
    const rs = mr.end_row - mr.start_row + 1;
    const cs = mr.end_col - mr.start_col + 1;
    const key = `${mr.start_row}_${mr.start_col}`;
    mergeConfig[key] = { r: mr.start_row, c: mr.start_col, rs, cs };
  }

  // Build row heights map: key = row index string, value = height in pixels
  // Rust sends height in points; FortuneSheet uses pixels (approx 1.333px per point)
  const rowlen: Record<string, number> = {};
  if (sheet.row_heights) {
    for (const [key, value] of Object.entries(sheet.row_heights)) {
      rowlen[key] = Math.round(value * 1.333);
    }
  }
  // Build column widths map: key = column index string, value = width in pixels
  // Excel stores width in character units; pixel display = Truncate(charWidth * MDW) + 5px padding
  // (MDW = Maximum Digit Width = 7 for Calibri 11pt default font).
  const columnlen: Record<string, number> = {};
  if (sheet.col_widths) {
    for (const [key, value] of Object.entries(sheet.col_widths)) {
      columnlen[key] = Math.round(value * 7) + 5;
    }
  }
  // Build the config object
  const config: Record<string, unknown> = {};
  if (Object.keys(mergeConfig).length > 0) config.merge = mergeConfig;
  if (Object.keys(rowlen).length > 0) config.rowlen = rowlen;
  if (Object.keys(columnlen).length > 0) config.columnlen = columnlen;

  const celldata: CellWithRowAndCol[] = [];

  for (const cell of sheet.cells) {
    const mergeKey = `${cell.row}_${cell.col}`;
    const mergeDef = mergeConfig[mergeKey];
    const fortune = rustCellToFortune(cell);

    if (mergeDef) {
      // Top-left anchor of a merged region
      fortune.mc = { r: mergeDef.r, c: mergeDef.c, rs: mergeDef.rs, cs: mergeDef.cs };
    } else {
      // Check if this cell falls inside any merged range (but is not an anchor)
      const inside = sheet.merged_cells.find(
        (mr) =>
          cell.row >= mr.start_row &&
          cell.row <= mr.end_row &&
          cell.col >= mr.start_col &&
          cell.col <= mr.end_col &&
          !(cell.row === mr.start_row && cell.col === mr.start_col),
      );
      if (inside) {
        // Point back to the anchor cell
        fortune.mc = { r: inside.start_row, c: inside.start_col };
      }
    }

    celldata.push({ r: cell.row, c: cell.col, v: fortune });
  }

  return {
    name: sheet.name,
    column: Math.max(sheet.max_col || 26, 26),
    row:    Math.max(sheet.max_row || 100, 100),
    celldata,
    config: Object.keys(config).length > 0 ? config : undefined,
    // Omit calcChain so FortuneSheet rebuilds it from celldata.
    // calcChain becomes stale after loading external data and causes cross-sheet
    // formula references to stop working. calculateFormula() will regenerate it.
  };
}

/**
 * Convert a Rust XlsxWorkbook to FortuneSheet data (Sheet[]).
 */
export function rustWorkbookToFortuneSheets(workbook: RustXlsxWorkbook): FortuneSheetCoreSheet[] {
  return workbook.sheets.map(rustSheetToFortuneSheet);
}

// ─── Reverse conversion: FortuneSheet Sheet -> Rust XlsxSheet ─────────────────

/** Infer Rust CellValue from a FortuneSheet cell value.
 * FortuneSheet formula cells:
 *   - f: formula text (e.g. "=SUM(A1:A10)")
 *   - v: computed value (set by HyperFormula after calculateFormula)
 *   - m: formatted string of the computed value
 *
 * When a cell has a formula but HyperFormula hasn't computed yet, v may be
 * the formula string itself or the previous cached value. We only treat v
 * as a result if it doesn't start with '=' or if there's an m field.
 */
function fortuneValueToRust(v: FortuneCell): RustCellValue {
  const hasFormula = typeof v.f === 'string' && v.f.length > 0;
  const raw = v.v;

  // Empty / no value — even for formula cells (formula may produce empty)
  if (raw === undefined || raw === null || raw === '') {
    return { type: 'empty' };
  }

  // Formula that produced a zero/numeric result
  if (hasFormula && typeof raw === 'number' && !isNaN(raw)) {
    if (Number.isInteger(raw)) return { type: 'int', value: raw };
    return { type: 'float', value: raw };
  }

  // Formula that produced a boolean
  if (hasFormula && typeof raw === 'boolean') {
    return { type: 'bool', value: raw ? 1 : 0 };
  }

  // Formula that produced an error string
  if (hasFormula && typeof raw === 'string' && raw.startsWith('#')) {
    return { type: 'error', value: raw };
  }

  // Formula that produced a string result
  if (hasFormula && typeof raw === 'string') {
    if (raw.startsWith('=')) {
      // HyperFormula hasn't run yet — treat as empty; formula text is stored
      // separately in the Rust formula field (handled by the caller).
      return { type: 'empty' };
    }
    return { type: 'string', value: raw };
  }

  // Non-formula cells
  const ct = v.ct;
  if (ct?.t === 's') {
    return { type: 'string', value: String(raw) };
  }
  if (ct?.t === 'n') {
    const num = typeof raw === 'number' ? raw : Number(raw);
    if (isNaN(num)) return { type: 'string', value: String(raw) };
    if (Number.isInteger(num)) return { type: 'int', value: num };
    const fa = ct.fa ?? '';
    if (fa.includes('yy') || fa.includes('mm') || fa.includes('dd') || fa.includes('hh')) {
      return { type: 'datetime', value: num };
    }
    return { type: 'float', value: num };
  }
  if (ct?.t === 'b') {
    return { type: 'bool', value: raw ? 1 : 0 };
  }
  if (typeof raw === 'string' && raw.startsWith('=')) {
    return { type: 'string', value: raw };
  }
  return { type: 'string', value: String(raw) };
}

/** Infer Rust CellStyle from FortuneSheet cell styles. */
function fortuneStyleToRust(v: FortuneCell): RustCellStyle {
  const style: RustCellStyle = {};
  if (v.bl === 1)       style.font_bold = true;
  if (v.it === 1)       style.font_italic = true;
  if (v.fs != null)      style.font_size = v.fs;
  if (v.ff != null && typeof v.ff === 'string') style.font_name = v.ff;
  if (v.fc != null)      style.font_color = v.fc;
  if (v.bg != null)      style.fill_fg_color = v.bg;
  if (v.ht !== undefined) {
    switch (v.ht) {
      case 0: style.alignment_h = 'center'; break;
      case 1: style.alignment_h = 'left';   break;
      case 2: style.alignment_h = 'right';  break;
    }
  }
  if (v.vt !== undefined) {
    switch (v.vt) {
      case 0: style.alignment_v = 'center'; break;
      case 1: style.alignment_v = 'top';    break;
      case 2: style.alignment_v = 'bottom'; break;
    }
  }
  if (v.ct?.fa) style.number_format = v.ct.fa;
  return style;
}

/**
 * Convert a FortuneSheet Sheet back to a Rust XlsxSheet.
 * Used when saving edits back to an xlsx file.
 */
/**
 * Convert a FortuneSheet Sheet back to a Rust XlsxSheet.
 * Used when saving edits back to an xlsx file.
 *
 * The `celldata` parameter should be the result of calling
 * `workbookRef.current.dataToCelldata(sheet.data)`. This converts the dense
 * data matrix back to the sparse celldata format — the standard/authoritative
 * format for storing and loading spreadsheet data.
 */
export function fortuneSheetToRustSheet(sheet: FortuneSheetCoreSheet, celldata: import('@fortune-sheet/core').CellWithRowAndCol[]): RustXlsxSheet {
  const cells: RustCell[] = [];
  const mergedRanges: RustMergedRange[] = [];

  // Collect merged anchors
  const mergeConfig = sheet.config?.merge ?? {};

  for (const def of Object.values(mergeConfig)) {
    mergedRanges.push({
      start_row: def.r,
      start_col: def.c,
      end_row:   def.r + (def.rs ?? 1) - 1,
      end_col:   def.c + (def.cs ?? 1) - 1,
    });
  }

  for (const item of celldata) {
    const v = item.v;
    if (!v || typeof v !== 'object') continue;

    const cellValue = fortuneValueToRust(v);
    const cellStyle = fortuneStyleToRust(v);
    const hasStyle = Object.keys(cellStyle).length > 0;

    cells.push({
      row:    item.r,
      col:    item.c,
      value:  cellValue,
      formula: v.f?.startsWith('=') ? v.f.slice(1) : v.f,
      style:  hasStyle ? cellStyle : undefined,
    });
  }

  const maxRow = cells.reduce((m, cell) => Math.max(m, cell.row + 1), 0);
  const maxCol = cells.reduce((m, cell) => Math.max(m, cell.col + 1), 0);

  return {
    name: sheet.name,
    state: sheet.hide === 1 ? 'hidden' : 'visible',
    cells,
    merged_cells: mergedRanges,
    max_row: maxRow,
    max_col: maxCol,
  };
}

/**
 * Convert a FortuneSheet Workbook (Sheet[]) back to a Rust XlsxWorkbook.
 * Calls `workbookRef.current.dataToCelldata` for each sheet's data matrix to get
 * the authoritative celldata, then converts to Rust format for xlsx writing.
 */
export function fortuneSheetsToRustWorkbook(
  sheets: FortuneSheetCoreSheet[],
  dataToCelldata: (data: import('@fortune-sheet/core').CellMatrix) => import('@fortune-sheet/core').CellWithRowAndCol[],
): RustXlsxWorkbook {
  return {
    sheets: sheets.map((sheet) => {
      const data = sheet.data ?? [];
      const celldata = dataToCelldata(data);
      return fortuneSheetToRustSheet(sheet, celldata);
    }),
    shared_strings: [],
  };
}

// ─── SheetJS (xlsx) export ────────────────────────────────────────────────────

// Lazily imported to avoid loading SheetJS until the first save.
// The `xlsx` package (SheetJS 0.18.x) is a peer dep already present in the project.
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
 * Build a stylesheet XML string from a list of unique style keys.
 * This replaces xl/styles.xml in the generated xlsx zip.
 * Style key format: number_format|fill_fg|fill_bg|bold|italic|font_color|font_size|font_name|h_align|v_align
 */
function buildStylesXml(uniqueStyleKeys: string[]): string {
  function parseKey(key: string) {
    const parts = key.split('|');
    return {
      numberFormat: parts[0] || '',
      fillFg: parts[1] || '',
      bold: parts[3] === '1',
      italic: parts[4] === '1',
      fontColor: parts[5] || '',
      fontSize: parts[6] ? parseInt(parts[6], 10) : null,
      fontName: parts[7] || '',
    };
  }

  // Build unique fonts list (index 0 = default Calibri 11pt black).
  const fontSet = new Map<string, number>();
  fontSet.set('default', 0);
  const fontList: { name: string; sz: number; color: string; bold: boolean; italic: boolean }[] = [
    { name: 'Calibri', sz: 11, color: '000000', bold: false, italic: false },
  ];
  for (const key of uniqueStyleKeys) {
    const s = parseKey(key);
    // FIX: Include bold/italic in fontKey to properly deduplicate fonts with different styles
    const fontKey = `${s.fontName || 'Calibri'}|${s.fontSize ?? 11}|${s.fontColor || '000000'}|${s.bold ? '1' : '0'}|${s.italic ? '1' : '0'}`;
    if (!fontSet.has(fontKey)) {
      fontSet.set(fontKey, fontList.length);
      fontList.push({
        name: s.fontName || 'Calibri',
        sz: s.fontSize ?? 11,
        color: s.fontColor || '000000',
        bold: s.bold,
        italic: s.italic,
      });
    }
  }

  // Build unique fills list (index 0 = none, index 1 = gray125).
  const fillSet = new Map<string, number>();
  fillSet.set('none', 0);
  fillSet.set('gray125', 1);
  const fillList: { patternType: string; fgColor: string }[] = [
    { patternType: '', fgColor: '' },
    { patternType: 'gray125', fgColor: '' },
  ];
  for (const key of uniqueStyleKeys) {
    const s = parseKey(key);
    if (s.fillFg) {
      const fillKey = s.fillFg;
      if (!fillSet.has(fillKey)) {
        fillSet.set(fillKey, fillList.length);
        fillList.push({ patternType: 'solid', fgColor: s.fillFg });
      }
    }
  }

  // Build numFmts.
  const numFmtMap = new Map<string, number>();
  let nextNumFmtId = 164;
  for (const key of uniqueStyleKeys) {
    const s = parseKey(key);
    if (s.numberFormat && !numFmtMap.has(s.numberFormat)) {
      numFmtMap.set(s.numberFormat, nextNumFmtId++);
    }
  }

  // Build cellXfs (index 0 = default, then one per style).
  const cellXfs: { fontId: number; fillId: number; numFmtId: number; bold: boolean; italic: boolean }[] = [
    { fontId: 0, fillId: 0, numFmtId: 0, bold: false, italic: false },
  ];
  for (const key of uniqueStyleKeys) {
    const s = parseKey(key);
    // FIX: Include bold/italic in fontKey to properly deduplicate fonts with different styles
    const fontKey = `${s.fontName || 'Calibri'}|${s.fontSize ?? 11}|${s.fontColor || '000000'}|${s.bold ? '1' : '0'}|${s.italic ? '1' : '0'}`;
    const fillKey = s.fillFg || 'none';
    const numFmtId = s.numberFormat ? (numFmtMap.get(s.numberFormat) ?? 0) : 0;
    cellXfs.push({
      fontId: fontSet.get(fontKey) ?? 0,
      fillId: fillSet.get(fillKey) ?? 0,
      numFmtId,
      bold: s.bold,
      italic: s.italic,
    });
  }

  const esc = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

  // Build <fonts>.
  let fontsXml = `<fonts count="${fontList.length}">`;
  fontsXml += '<font><name val="Calibri"/><family val="2"/><color theme="1"/><sz val="11"/><scheme val="minor"/></font>';
  for (const f of fontList.slice(1)) {
    fontsXml += '<font>';
    fontsXml += `<name val="${esc(f.name)}"/><family val="2"/>`;
    fontsXml += f.color ? `<color rgb="${esc(f.color)}"/>` : '<color theme="1"/>';
    fontsXml += `<sz val="${f.sz}"/>`;
    if (f.bold) fontsXml += '<b/>';
    if (f.italic) fontsXml += '<i/>';
    fontsXml += '<scheme val="minor"/></font>';
  }
  fontsXml += '</fonts>';

  // Build <fills>.
  let fillsXml = `<fills count="${fillList.length}">`;
  for (const fl of fillList) {
    if (!fl.patternType) {
      fillsXml += '<fill><patternFill/></fill>';
    } else {
      fillsXml += `<fill><patternFill patternType="${fl.patternType}">`;
      if (fl.fgColor) fillsXml += `<fgColor rgb="${esc(fl.fgColor)}"/>`;
      fillsXml += '</patternFill></fill>';
    }
  }
  fillsXml += '</fills>';

  // Build <numFmts>.
  let numFmtsXml = `<numFmts count="${numFmtMap.size}">`;
  for (const [fmt, id] of numFmtMap) {
    numFmtsXml += `<numFmt numFmtId="${id}" formatCode="${esc(fmt)}"/>`;
  }
  numFmtsXml += '</numFmts>';

  // Build <cellXfs>.
  let cellXfsXml = `<cellXfs count="${cellXfs.length}">`;
  for (const xf of cellXfs) {
    let attrs = `numFmtId="${xf.numFmtId}" fontId="${xf.fontId}" fillId="${xf.fillId}" borderId="0" xfId="0"`;
    if (xf.bold || xf.italic) attrs += ' applyFont="1"';
    if (xf.fillId > 1) attrs += ' applyFill="1"';
    if (xf.numFmtId > 0) attrs += ' applyNumberFormat="1"';
    cellXfsXml += `<xf ${attrs}/>`;
  }
  cellXfsXml += '</cellXfs>';

  return [
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>',
    `<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">`,
    numFmtsXml,
    fontsXml,
    fillsXml,
    '<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>',
    '<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>',
    cellXfsXml,
    '<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0" hidden="0"/></cellStyles>',
    '<dxfs count="0"/>',
    '<tableStyles count="0" defaultTableStyle="TableStyleMedium9" defaultPivotStyle="PivotStyleLight16"/>',
    '</styleSheet>',
  ].join('');
}

/** Convert a FortuneSheet cell value to a plain serialisable value + type tag.
 *  SheetJS uses:
 *    t = 'n'  → number
 *    t = 's'  → string (stored in sharedStrings)
 *    t = 'b'  → boolean
 *    t = 'e'  → error string
 *    t = 'd'  → Date
 *    t = 'str' → inline string (formula result or inline string)
 *
 *  For string values, we use 'str' type to avoid needing a sharedStrings array.
 */
function fortuneCellToSheetJS(v: FortuneCell): object {
  const result: Record<string, unknown> = {};

  // ── Value (computed result) ────────────────────────────────────────────────
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

  // ── Formula ────────────────────────────────────────────────────────────────
  // SheetJS stores formulas WITHOUT leading '='.
  // We set f=formula text; if there's also a computed value, both coexist.
  if (v.f && typeof v.f === 'string') {
    result.f = v.f.startsWith('=') ? v.f.slice(1) : v.f;
  }

  // ── Number format ─────────────────────────────────────────────────────────
  if (v.ct?.fa && v.ct.fa !== 'General') {
    result.z = v.ct.fa;
  }

  // ── Style ────────────────────────────────────────────────────────────────
  // Style key for deduplication. Match the fields used by Rust's SheetStyleKey.
  const styleKey = [
    v.ct?.fa ?? '',
    v.bg ?? '',
    '',  // fill_bg_color not in FortuneCell — placeholder
    v.bl === 1 ? '1' : '0',
    v.it === 1 ? '1' : '0',
    v.fc ?? '',
    v.fs != null ? String(v.fs) : '',
    v.ff ?? '',
    v.ht !== undefined ? String(v.ht) : '',
    v.vt !== undefined ? String(v.vt) : '',
  ].join('|');

  if (styleKey !== '||||||||') {
    result._styleKey = styleKey;
  }

  return result;
}

/**
 * Convert a FortuneSheet Sheet to a SheetJS worksheet object.
 * Merged regions, row heights, and column widths are stored in SheetJS's worksheet properties.
 */
async function fortuneSheetToSheetJSWorksheet(
  sheet: FortuneSheetCoreSheet,
  XLSX: SheetJSLazy,
  dataToCelldata: (data: import('@fortune-sheet/core').CellMatrix) => import('@fortune-sheet/core').CellWithRowAndCol[],
): Promise<{ worksheet: Record<string, unknown>; styleKeys: string[] }> {
  const cells: Record<string, object> = {};
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
    const jsCell = fortuneCellToSheetJS(v) as Record<string, unknown>;
    if (Object.keys(jsCell).length > 0) {
      cells[addr] = jsCell;
      if (typeof jsCell._styleKey === 'string' && jsCell._styleKey !== '||||||||') {
        styleKeys.push(jsCell._styleKey);
      }
    }
  }

  const merges: object[] = [];
  const mergeConfig = sheet.config?.merge ?? {};
  for (const def of Object.values(mergeConfig)) {
    if (!def) continue;
    merges.push({
      s: { r: def.r, c: def.c },
      e: { r: def.r + (def.rs ?? 1) - 1, c: def.c + (def.cs ?? 1) - 1 },
    });
  }

  // Build row heights (!rows) - FortuneSheet uses pixels, SheetJS uses hpx
  const rows: { hpx?: number }[] = [];
  const rowlen = sheet.config?.rowlen ?? {};
  for (const [key, heightPx] of Object.entries(rowlen)) {
    const rowIdx = parseInt(key, 10);
    while (rows.length <= rowIdx) rows.push({});
    rows[rowIdx] = { hpx: Math.round(heightPx) };
  }
  // Build column widths (!cols) - FortuneSheet uses pixels, SheetJS uses wpx
  const cols: { wpx?: number }[] = [];
  const columnlen = sheet.config?.columnlen ?? {};
  for (const [key, widthPx] of Object.entries(columnlen)) {
    const colIdx = parseInt(key, 10);
    while (cols.length <= colIdx) cols.push({});
    cols[colIdx] = { wpx: Math.round(widthPx) };
  }

  let minRow = Infinity, maxRow = 0, minCol = Infinity, maxCol = 0;
  for (const key of sparseMap.keys()) {
    const [rStr, cStr] = key.split(',');
    const r = parseInt(rStr, 10);
    const c = parseInt(cStr, 10);
    minRow = Math.min(minRow, r);
    maxRow = Math.max(maxRow, r);
    minCol = Math.min(minCol, c);
    maxCol = Math.max(maxCol, c);
  }

  const rangeRef = minRow <= maxRow
    ? `${XLSX.utils.encode_cell({ r: minRow, c: minCol })}:${XLSX.utils.encode_cell({ r: maxRow, c: maxCol })}`
    : 'A1';

  return {
    worksheet: {
      ...cells,
      '!ref': rangeRef,
      ...(merges.length > 0 ? { '!merges': merges } : {}),
      ...(rows.length > 0 ? { '!rows': rows } : {}),
      ...(cols.length > 0 ? { '!cols': cols } : {}),
    },
    styleKeys,
  };
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
  dataToCelldata: (data: import('@fortune-sheet/core').CellMatrix) => import('@fortune-sheet/core').CellWithRowAndCol[],
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

// --- Helper XML builders ---

function buildContentTypesXml(): string {
  return `<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>`;
}

function buildRelsXml(): string {
  return `<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>`;
}

function buildWorkbookRelsXml(sheetCount: number): string {
  let sheets = '';
  for (let i = 1; i <= sheetCount; i++) {
    sheets += `  <Relationship Id="rId${i + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet${i}.xml"/>\n`;
  }
  return `<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
${sheets}</Relationships>`;
}

function buildWorkbookXml(sheetNames: string[]): string {
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

function buildWorksheetXml(
  worksheet: Record<string, unknown>,
  styleIndexMap: Map<string, number>,
): string {
  const merges = worksheet['!merges'] as Array<{ r: number; c: number; rs: number; cs: number }> || [];
  const rows = worksheet['!rows'] as Array<{ hpx?: number }> || [];
  const cols = worksheet['!cols'] as Array<{ wpx?: number }> || [];

  let xml = `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
  xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheetViews><sheetView workbookViewId="0"/></sheetViews>`;

  // Column widths
  if (cols.length > 0) {
    xml += '<cols>';
    cols.forEach((col, i) => {
      if (col.wpx) {
        // Excel stores width in character units (1/256 precision); pixel
        // display = round(charWidth * MDW) + 5px padding. Reverse:
        // charWidth = (pixel - 5) / MDW, with MDW = 7 for Calibri 11pt.
        const w = Math.round(((col.wpx - 5) / 7) * 256) / 256;
        xml += `<col min="${i + 1}" max="${i + 1}" width="${w}" customWidth="1"/>`;
      }
    });
    xml += '</cols>';
  }

  xml += `<sheetData>`;

  // Group cells by row
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

  for (const rowNum of sortedRows) {
    const rowCells = rowGroups[rowNum];
    xml += `<row r="${rowNum}"`;
    if (rows[rowNum - 1]?.hpx) {
      xml += ` ht="${rows[rowNum - 1].hpx}" customHeight="1"`;
    }
    xml += '>';

    // Sort cells by column
    const sortedAddrs = Object.keys(rowCells).sort((a, b) => {
      const colA = a.match(/^([A-Z]+)/)?.[1] || '';
      const colB = b.match(/^([A-Z]+)/)?.[1] || '';
      return colA.localeCompare(colB);
    });

    for (const addr of sortedAddrs) {
      const cell = rowCells[addr] as Record<string, unknown>;
      xml += cellToXml(addr, cell, styleIndexMap);
    }

    xml += '</row>';
  }

  xml += '</sheetData>';

  // Merges
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
  if (t === 's') xml += ` t="s"`;
  else if (t === 'str') xml += ` t="inlineStr"`;
  else if (t === 'b') xml += ` t="b"`;
  else if (t === 'e') xml += ` t="e"`;

  xml += '>';

  if (cell.f) {
    // Strip leading = from formula
    const f = escapeXml(String(cell.f).replace(/^=/, ''));
    xml += `<f>${f}</f>`;
    if (cell.v !== undefined) {
      xml += `<v>${escapeXml(String(cell.v))}</v>`;
    }
  } else if (cell.v !== undefined) {
    if (t === 's') {
      xml += `<v>${cell.v}</v>`;
    } else if (t === 'str' || t === 'e') {
      xml += `<is><t>${escapeXml(String(cell.v))}</t></is>`;
    } else if (t === 'b') {
      xml += `<v>${cell.v ? 1 : 0}</v>`;
    } else {
      xml += `<v>${cell.v}</v>`;
    }
  }

  xml += '</c>';
  return xml;
}

function colLetter(col: number): string {
  let result = '';
  col++;
  while (col > 0) {
    col--;
    result = String.fromCharCode(65 + (col % 26)) + result;
    col = Math.floor(col / 26);
  }
  return result;
}

function escapeXml(str: string): string {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&apos;');
}
