/**
 * xlsx (Rust structured) <-> FortuneSheet converter
 *
 * Rust types (from src-tauri/src/office/xlsx.rs):
 *   CellValue: { type: "empty" | "int" | "float" | "bool" | "string" | "error" | "datetime", ... }
 *   Cell:      { row, col, value, formula?, style? }
 *   CellStyle: { number_format, fill_fg_color?, fill_bg_color?, font_bold, font_italic,
 *                font_color?, font_size?, font_name?, alignment_h?, alignment_v? }
 *   MergedRange: { start_row, start_col, end_row, end_col }
 *   XlsxSheet:  { name, state, cells, merged_cells, max_row, max_col }
 *   XlsxWorkbook: { sheets, shared_strings }
 *
 * FortuneSheet types (from @fortune-sheet/core):
 *   Cell:      { v?, m?, mc?, f?, ct?, bg?, bl?, it?, ff?, fs?, fc?, ht?, vt?, tb?, ... }
 *   Sheet:     { name, data?, celldata?, config?: { merge?, ... }, column?, row?, ... }
 *   Workbook:   Sheet[]
 */

import type {
  Cell as FortuneCell,
  Sheet as FortuneSheetCoreSheet,
  CellWithRowAndCol,
} from '@fortune-sheet/core';

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
}

export interface RustXlsxWorkbook {
  sheets: RustXlsxSheet[];
  shared_strings: string[];
}

// ─── Style helpers ────────────────────────────────────────────────────────────

/** Normalise a colour string from Rust to a CSS hex colour. */
function normaliseColor(color: string | undefined): string | undefined {
  if (!color) return undefined;
  if (/^#[0-9a-fA-F]{6}$/.test(color)) return color.toLowerCase();
  if (/^[0-9a-fA-F]{8}$/.test(color)) return '#' + color.slice(2).toLowerCase();
  if (/^[0-9a-fA-F]{6}$/.test(color)) return '#' + color.toLowerCase();
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
  const mergeConfig: Record<string, { r: number; c: number; rs: number; cs: number }> = {};
  for (const mr of sheet.merged_cells) {
    const rs = mr.end_row - mr.start_row + 1;
    const cs = mr.end_col - mr.start_col + 1;
    const key = `${mr.start_row}_${mr.start_col}`;
    mergeConfig[key] = { r: mr.start_row, c: mr.start_col, rs, cs };
  }

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
    config: Object.keys(mergeConfig).length > 0 ? { merge: mergeConfig } : undefined,
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
export function fortuneSheetToRustSheet(sheet: FortuneSheetCoreSheet): RustXlsxSheet {
  const cells: RustCell[] = [];
  const mergedRanges: RustMergedRange[] = [];

  const celldata = sheet.celldata ?? [];
  // Collect merged anchors
  const mergeConfig = sheet.config?.merge ?? {};
  const anchorKeys = new Set(Object.keys(mergeConfig));

  // Build a sparse set of all cells that exist in celldata (authoritative source
  // of what the user explicitly created). We only add cells to this set — we do
  // NOT iterate over sheet.data and add cells that are not in celldata, because:
  //   1. sheet.data is a dense array of size (row × column) — by default
  //      100 × 26 = 2600 cells, most of which are empty {}
  //   2. Converting those empty {} to Rust Cells and writing them to the xlsx
  //      pollutes the file with phantom cells that overwrite real data on reload
  //   3. sheet.data is updated by HyperFormula with computed formula results;
  //      we only use it to enrich cells already in celldata, not to discover new ones
  const celldataKeys = new Set<string>();
  const celldataMap = new Map<string, FortuneCell>();
  for (const item of celldata) {
    if (item.v) {
      const key = `${item.r}_${item.c}`;
      celldataKeys.add(key);
      celldataMap.set(key, item.v);
    }
  }

  // For cells in celldata: prefer sheet.data's computed value (from HyperFormula)
  // if the cell is non-null there. This ensures formula results (v) are captured.
  const dataMatrix = sheet.data ?? [];
  for (let r = 0; r < dataMatrix.length; r++) {
    const row = dataMatrix[r];
    if (!row) continue;
    for (let c = 0; c < row.length; c++) {
      const cell = row[c];
      if (!cell) continue;
      const key = `${r}_${c}`;
      // Only use sheet.data for cells that are already in celldata.
      // This prevents phantom empty cells from polluting the save.
      if (celldataKeys.has(key)) {
        celldataMap.set(key, cell);
      }
    }
  }

  for (const key of celldataKeys) {
    const [r, c] = key.split('_').map(Number);
    const v = celldataMap.get(key)!;

    const mergeKey = `${r}_${c}`;
    const isAnchor = anchorKeys.has(mergeKey);

    const cellValue = fortuneValueToRust(v);
    const cellStyle = fortuneStyleToRust(v);
    const hasStyle = Object.keys(cellStyle).length > 0;

    cells.push({
      row: r,
      col: c,
      value: cellValue,
      formula: v.f?.startsWith('=') ? v.f.slice(1) : v.f,
      style: hasStyle ? cellStyle : undefined,
    });

    if (isAnchor) {
      const def = mergeConfig[mergeKey];
      mergedRanges.push({
        start_row: def.r,
        start_col: def.c,
        end_row:  def.r + (def.rs ?? 1) - 1,
        end_col:  def.c + (def.cs ?? 1) - 1,
      });
    }
  }

  const maxRow = cells.reduce((m, c) => Math.max(m, c.row + 1), 0);
  const maxCol = cells.reduce((m, c) => Math.max(m, c.col + 1), 0);

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
 */
export function fortuneSheetsToRustWorkbook(sheets: FortuneSheetCoreSheet[]): RustXlsxWorkbook {
  return {
    sheets: sheets.map(fortuneSheetToRustSheet),
    shared_strings: [],
  };
}
