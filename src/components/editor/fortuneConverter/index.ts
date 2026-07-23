// `fortuneConverter/` — split out of the original monolithic
// `fortuneSheetConverter.ts`.
//
// Modules:
//   - `types.ts`             — Rust xlsx mirror types
//   - `colorAlignment.ts`    — pure helpers (colour / alignment)
//   - `cellConversions.ts`   — single-cell Rust⇄FortuneSheet
//   - `sheetConversions.ts`  — workbook / sheet-level Rust⇄FortuneSheet
//   - `stylesheetXml.ts`     — `xl/styles.xml` builder + small XML helpers
//   - `sheetXml.ts`          — worksheet + content-types + rels XML builders
//   - `sheetjsExport.ts`     — top-level SheetJS export orchestrator

export type {
  RustCell,
  RustCellStyle,
  RustCellValue,
  RustMergedRange,
  RustXlsxSheet,
  RustXlsxWorkbook,
} from './types';

export {
  alignH,
  alignV,
  normaliseColor,
} from './colorAlignment';

export {
  fortuneStyleToRust,
  fortuneValueToRust,
  rustCellToFortune,
} from './cellConversions';

export {
  fortuneSheetToRustSheet,
  fortuneSheetsToRustWorkbook,
  rustSheetToFortuneSheet,
  rustWorkbookToFortuneSheets,
} from './sheetConversions';

export {
  buildStylesXml,
  colLetter,
  composeStyleKey,
  escapeXml,
  parseStyleKey,
} from './stylesheetXml';
export type {
  CellXfSpec,
  FillSpec,
  FontSpec,
  ParsedStyleKey,
} from './stylesheetXml';

export {
  buildContentTypesXml,
  buildRelsXml,
  buildWorkbookRelsXml,
  buildWorkbookXml,
  buildWorksheetXml,
} from './sheetXml';
export type {
  ColWidthSpec,
  MergeSpec,
  RowHeightSpec,
} from './sheetXml';

export {
  composeStyleKey as composeSheetJSStyleKey,
  escapeXml as escapeSheetJSXml,
  fortuneSheetsToSheetJSBuffer,
  parseStyleKey as parseSheetJSStyleKey,
} from './sheetjsExport';