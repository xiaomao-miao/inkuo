// Mirror types for the Rust xlsx writer's serialization format.
//
// These types are kept here (separate from the conversion logic) so the
// importer, exporter, and tests can reference them without pulling in
// the full SheetJS / Rust backend dependencies. They are intentionally
// minimal — only the fields the JS layer actually reads or writes —
// because anything added here creates a binding the backend must honor.

/**
 * Discriminated value type used by the Rust xlsx writer. The
 * `type` field drives the corresponding `<c t="…"/>` attribute on the
 * XML cell. All typed variants share the same `value` field (the
 * backend serializes via `serde(content = "value")`).
 */
export interface RustCellValue {
  type: 'empty' | 'int' | 'float' | 'bool' | 'string' | 'error' | 'datetime';
  value?: number | string;
}

/** Inline cell style. Each Rust field maps 1:1 to an OOXML xf sub-element. */
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

/** Single cell in a Rust-serialized sheet (sparse: omitted if empty). */
export interface RustCell {
  row: number;
  col: number;
  value: RustCellValue;
  formula?: string;
  style?: RustCellStyle;
}

/** A merged region. Endpoints are inclusive on both axes. */
export interface RustMergedRange {
  start_row: number;
  start_col: number;
  end_row: number;
  end_col: number;
}

/** One sheet in the Rust xlsx format. */
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

/** Full workbook. `shared_strings` is kept here so callers can preserve it on round-trip. */
export interface RustXlsxWorkbook {
  sheets: RustXlsxSheet[];
  shared_strings: string[];
}
