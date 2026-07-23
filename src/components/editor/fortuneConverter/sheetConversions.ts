// Workbook / sheet-level conversion between Rust and FortuneSheet.
//
// The forward path (`rustSheetToFortuneSheet`) ingests a Rust xlsx sheet
// — sparse cells with merged ranges, row heights in points, column
// widths in Excel character units — and produces the FortuneSheet
// sheet layout that the editor renders. The reverse path
// (`fortuneSheetToRustSheet`) takes the `celldata` sparse form
// (already converted from the editor's dense `data` matrix via
// `dataToCelldata`) and produces a Rust sheet ready for the backend.
//
// The Point-to-Pixel and charWidth-to-Pixel conversions on row/col
// dimensions live here because they only matter when translating the
// whole sheet, not individual cells.

import type {
  CellWithRowAndCol,
  Sheet as FortuneSheetCoreSheet,
} from '@fortune-sheet/core';

import {
  fortuneStyleToRust,
  fortuneValueToRust,
  rustCellToFortune,
} from './cellConversions';
import type {
  RustCell,
  RustMergedRange,
  RustXlsxSheet,
  RustXlsxWorkbook,
} from './types';

/** Points → pixels used for row heights. (1pt ≈ 1.333px at 96 DPI.) */
const POINTS_TO_PIXELS = 1.333;

/**
 * Excel char-units → pixels used for column widths.
 *
 * Standard formula: pixels = truncate(charWidth × MDW) + 5px padding.
 * MDW = 7 (Max Digit Width for Calibri 11pt — Excel's default).
 */
const EXCEL_MDW = 7;
const COLUMN_PADDING_PX = 5;

/** Minimum plausible values used when the Rust sheet reports 0 cells. */
const MIN_SHEET_ROWS = 100;
const MIN_SHEET_COLS = 26;

/**
 * Convert a Rust XlsxSheet (sparse cells, typed values, styles,
 * merged ranges) to a FortuneSheet Sheet. Uses the `celldata` (sparse)
 * format that FortuneSheet loads directly.
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

  // Build row heights map: key = row index string, value = height in pixels.
  // Rust sends height in points; FortuneSheet uses pixels.
  const rowlen: Record<string, number> = {};
  if (sheet.row_heights) {
    for (const [key, value] of Object.entries(sheet.row_heights)) {
      rowlen[key] = Math.round(value * POINTS_TO_PIXELS);
    }
  }

  // Build column widths map: key = column index string, value = width in pixels.
  // Excel stores width in character units; convert via MDW + padding.
  const columnlen: Record<string, number> = {};
  if (sheet.col_widths) {
    for (const [key, value] of Object.entries(sheet.col_widths)) {
      columnlen[key] = Math.round(value * EXCEL_MDW) + COLUMN_PADDING_PX;
    }
  }

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
    column: Math.max(sheet.max_col || MIN_SHEET_COLS, MIN_SHEET_COLS),
    row: Math.max(sheet.max_row || MIN_SHEET_ROWS, MIN_SHEET_ROWS),
    celldata,
    config: Object.keys(config).length > 0 ? config : undefined,
    // Omit calcChain so FortuneSheet rebuilds it from celldata.
    // calcChain becomes stale after loading external data and causes cross-sheet
    // formula references to stop working. calculateFormula() will regenerate it.
  };
}

/** Convert a Rust XlsxWorkbook to FortuneSheet data (`Sheet[]`). */
export function rustWorkbookToFortuneSheets(
  workbook: RustXlsxWorkbook,
): FortuneSheetCoreSheet[] {
  return workbook.sheets.map(rustSheetToFortuneSheet);
}

/**
 * Convert a FortuneSheet Sheet back to a Rust XlsxSheet. Used when
 * saving edits back to an xlsx file.
 *
 * The `celldata` parameter should be the result of calling
 * `workbookRef.current.dataToCelldata(sheet.data)`. This converts the
 * dense data matrix back to the sparse celldata format — the
 * standard/authoritative format for storing and loading spreadsheet
 * data.
 */
export function fortuneSheetToRustSheet(
  sheet: FortuneSheetCoreSheet,
  celldata: CellWithRowAndCol[],
): RustXlsxSheet {
  const cells: RustCell[] = [];
  const mergedRanges: RustMergedRange[] = [];

  const mergeConfig = sheet.config?.merge ?? {};

  for (const def of Object.values(mergeConfig)) {
    mergedRanges.push({
      start_row: def.r,
      start_col: def.c,
      end_row: def.r + (def.rs ?? 1) - 1,
      end_col: def.c + (def.cs ?? 1) - 1,
    });
  }

  for (const item of celldata) {
    const v = item.v;
    if (!v || typeof v !== 'object') continue;

    const cellValue = fortuneValueToRust(v);
    const cellStyle = fortuneStyleToRust(v);
    const hasStyle = Object.keys(cellStyle).length > 0;

    cells.push({
      row: item.r,
      col: item.c,
      value: cellValue,
      formula: v.f?.startsWith('=') ? v.f.slice(1) : v.f,
      style: hasStyle ? cellStyle : undefined,
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
 * Convert a FortuneSheet Workbook (`Sheet[]`) back to a Rust XlsxWorkbook.
 * Calls `dataToCelldata` for each sheet's data matrix to get the
 * authoritative celldata, then converts to Rust format for xlsx writing.
 */
export function fortuneSheetsToRustWorkbook(
  sheets: FortuneSheetCoreSheet[],
  dataToCelldata: (data: FortuneSheetCoreSheet['data']) => CellWithRowAndCol[],
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
