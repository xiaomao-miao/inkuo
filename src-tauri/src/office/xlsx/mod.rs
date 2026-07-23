//! Excel (.xlsx) workbook reading, structured parsing, and conservative writing.
//!
//! Two layers of API:
//! - The legacy [`ExcelWorkbook`] / [`ExcelSheet`] types (flat 2D string grid)
//!   are kept for backward compatibility with existing callers.
//! - The structured [`XlsxWorkbook`] / [`XlsxSheet`] / [`Cell`] / [`CellStyle`]
//!   types provide cell-level fidelity (formulas, merged ranges, styles)
//!   suitable for AI editing and conservative round-tripping.
//!
//! ## Module layout
//!
//! | File | Responsibility |
//! |------|----------------|
//! | `mod.rs` (~1 300 lines) | Public types, `read_excel_workbook`, `write_excel_workbook`, `read_xlsx_structured`, plus the streaming XML readers and `XlsxWorkbook::apply_operations`. |
//! | `legacy_text.rs` | `cell_to_string` + `excel_workbook_to_text` (legacy flat API rendering). |
//! | `structured_text.rs` | `xlsx_workbook_to_text` (structured API rendering). |
//! | `styles_parser.rs` | `CellXf` / `AlignmentXf` / `FontXf` / `FillXf` / `StylesInfo` + the `parse_styles` state machine. |
//! | `ooxml_boilerplate.rs` | `MINIMAL_STYLES_XML` / `MINIMAL_THEME_XML` verbatim XML constants. |
//! | `writer.rs` (~860 lines) | `create_xlsx_workbook` + `write_excel_document` + `build_workbook_styles` + `build_sheet_xml` + `build_cell_xml` + `parse_sheet_name_to_path_map` + `escape_xml_attr` — everything needed to turn an [`XlsxWorkbook`] into an OOXML zip package. |
//! | `incremental_writer.rs` (~735 lines) | `CellModification` / `ExcelOperation` / `incremental_write_xlsx` + the byte-level XML splicing helpers (`find_c_element_end`, `find_matching_close_c`, `build_replacement_cell_xml`, `value_to_xml_body`, …) used only by the conservative cell-by-cell writer. |

pub mod types;
pub(crate) mod legacy_text;
pub(crate) mod structured_text;
pub(crate) mod styles_parser;
pub(crate) mod ooxml_boilerplate;
pub(crate) mod writer;
pub(crate) mod incremental_writer;

// Re-export so callers can keep using `crate::office::xlsx::cell_to_string`,
// `crate::office::xlsx::excel_workbook_to_text`, and
// `crate::office::xlsx::xlsx_workbook_to_text` unchanged.
pub use legacy_text::{cell_to_string, excel_workbook_to_text};
pub use structured_text::xlsx_workbook_to_text;
pub(crate) use styles_parser::{
    attr_value, parse_styles, resolve_number_format, strip_xml_ns, StylesInfo,
};
pub(crate) use ooxml_boilerplate::{MINIMAL_STYLES_XML, MINIMAL_THEME_XML};
pub use writer::{create_xlsx_workbook, write_excel_document};
pub use incremental_writer::{incremental_write_xlsx, CellModification, ExcelOperation};

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use calamine::{Data, Reader as CalamineReader, Xlsx};
use quick_xml::events::Event;
use quick_xml::reader::Reader as XmlReader;

use super::shared::OfficeError;

// DEBUG: Enable verbose logging
const DEBUG_XLSX: bool = true;

// ─── Legacy flat API (kept for backward compatibility) ────────────────────────

/// Deprecated: use StructuredExcelWorkbook instead.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExcelSheet {
    pub name: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Deprecated: use StructuredExcelWorkbook instead.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExcelWorkbook {
    pub sheets: Vec<ExcelSheet>,
}

// ─── Structured cell-level API ────────────────────────────────────────────────

/// Alias for the structured cell type — use this instead of the flat string grid.
pub type StructuredCell = Cell;

/// Alias for the structured style type.
pub type StructuredCellStyle = CellStyle;

/// Alias for the structured value type.
pub type StructuredCellValue = CellValue;

/// Alias for a merged cell range.
pub type StructuredMergedRange = MergedRange;

/// Alias for a structured sheet.
pub type StructuredXlsxSheet = XlsxSheet;

/// Alias for a structured workbook — the recommended type for Excel operations.
pub type StructuredExcelWorkbook = XlsxWorkbook;

pub fn read_excel_workbook(bytes: &[u8]) -> Result<ExcelWorkbook, OfficeError> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut workbook: Xlsx<Cursor<Vec<u8>>> = calamine::open_workbook_from_rs(cursor)
        .map_err(|e: calamine::XlsxError| OfficeError::Excel(e.to_string()))?;

    let sheet_names = workbook.sheet_names();
    let mut sheets = Vec::new();

    for name in sheet_names {
        if let Ok(range) = workbook.worksheet_range(name.as_str()) {
            let mut headers = Vec::new();
            let mut rows: Vec<Vec<String>> = Vec::new();

            for (row_idx, row) in range.rows().enumerate() {
                let row_data: Vec<String> = row.iter().map(|c| cell_to_string(c)).collect();

                if row_idx == 0 && !row_data.is_empty() && !row_data.iter().all(|s| s.is_empty()) {
                    headers = row_data.clone();
                    rows.push(row_data);
                } else {
                    rows.push(row_data);
                }
            }

            sheets.push(ExcelSheet {
                name: name.to_string(),
                headers,
                rows,
            });
        }
    }

    Ok(ExcelWorkbook { sheets })
}


pub fn write_excel_workbook(workbook: &ExcelWorkbook, output_path: &std::path::Path) -> Result<(), OfficeError> {
    use rust_xlsxwriter::*;

    let mut xl_workbook = Workbook::new();

    for sheet in &workbook.sheets {
        let worksheet = xl_workbook.add_worksheet();
        worksheet.set_name(&sheet.name).map_err(|e| OfficeError::Excel(e.to_string()))?;

        for (row_idx, row) in sheet.rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                let trimmed = cell.trim();
                if trimmed.eq_ignore_ascii_case("true") || trimmed.eq_ignore_ascii_case("false") {
                    let val = trimmed.eq_ignore_ascii_case("true");
                    worksheet.write(row_idx as u32, col_idx as u16, val)
                        .map_err(|e| OfficeError::Excel(e.to_string()))?;
                } else if let Ok(num) = trimmed.parse::<f64>() {
                    worksheet.write(row_idx as u32, col_idx as u16, num)
                        .map_err(|e| OfficeError::Excel(e.to_string()))?;
                } else if let Ok(num) = trimmed.parse::<i64>() {
                    worksheet.write(row_idx as u32, col_idx as u16, num)
                        .map_err(|e| OfficeError::Excel(e.to_string()))?;
                } else {
                    worksheet.write(row_idx as u32, col_idx as u16, cell.as_str())
                        .map_err(|e| OfficeError::Excel(e.to_string()))?;
                }
            }
        }
    }

    xl_workbook.save(output_path)
        .map_err(|e| OfficeError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

    Ok(())
}

// ─── Structured cell-level API ────────────────────────────────────────────────

/// Strongly-typed cell value. Excel cells are typed; treating them all as
/// strings discards information the AI needs (e.g. a date stored as a serial
/// number with `number_format="yyyy-mm-dd"` is not a string).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", content = "value")]
pub enum CellValue {
    Empty,
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Error(String),
    DateTime(f64),
}

impl CellValue {
    pub fn as_string_for_display(&self) -> String {
        match self {
            CellValue::Empty => String::new(),
            CellValue::Int(n) => n.to_string(),
            CellValue::Float(f) => {
                if f.fract() == 0.0 {
                    format!("{:.0}", f)
                } else {
                    format!("{}", f)
                }
            }
            CellValue::Bool(b) => b.to_string(),
            CellValue::String(s) => s.clone(),
            CellValue::Error(e) => format!("#ERR:{}", e),
            CellValue::DateTime(dt) => format!("{:.0}", dt),
        }
    }
}

/// Subset of an xlsx cell style we expose to AI. The original xlsx can carry
/// hundreds of style attributes; we only keep the ones that meaningfully
/// affect what the AI sees or modifies.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct CellStyle {
    #[serde(default)]
    pub number_format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_fg_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_bg_color: Option<String>,
    #[serde(default)]
    pub font_bold: bool,
    #[serde(default)]
    pub font_italic: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment_h: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment_v: Option<String>,
}

/// A single cell in a sheet.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Cell {
    pub row: usize,
    pub col: usize,
    pub value: CellValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<CellStyle>,
}

impl Cell {
    pub fn address(&self) -> String {
        cell_address(self.row, self.col)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq, Eq, Hash)]
pub struct SheetStyleKey {
    pub number_format: String,
    pub fill_fg_color: Option<String>,
    pub fill_bg_color: Option<String>,
    pub font_bold: bool,
    pub font_italic: bool,
    pub font_color: Option<String>,
    pub font_size: Option<u32>,
    pub font_name: Option<String>,
    pub alignment_h: Option<String>,
    pub alignment_v: Option<String>,
}

impl From<&CellStyle> for SheetStyleKey {
    fn from(value: &CellStyle) -> Self {
        Self {
            number_format: value.number_format.clone(),
            fill_fg_color: value.fill_fg_color.clone(),
            fill_bg_color: value.fill_bg_color.clone(),
            font_bold: value.font_bold,
            font_italic: value.font_italic,
            font_color: value.font_color.clone(),
            font_size: value.font_size,
            font_name: value.font_name.clone(),
            alignment_h: value.alignment_h.clone(),
            alignment_v: value.alignment_v.clone(),
        }
    }
}


/// Merged cell range in 0-indexed (row, col) coordinates, inclusive on both ends.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MergedRange {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
}

impl MergedRange {
    pub fn address(&self) -> String {
        format!(
            "{}:{}",
            cell_address(self.start_row, self.start_col),
            cell_address(self.end_row, self.end_col)
        )
    }
}

/// Structured worksheet data.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct XlsxSheet {
    pub name: String,
    #[serde(default = "default_visible")]
    pub state: String,
    pub cells: Vec<Cell>,
    #[serde(default)]
    pub merged_cells: Vec<MergedRange>,
    #[serde(default)]
    pub max_row: usize,
    #[serde(default)]
    pub max_col: usize,
    /// Row heights: map of row index (0-based) to height in points.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub row_heights: std::collections::HashMap<usize, f64>,
    /// Column widths: map of column index (0-based) to width in Excel character units.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub col_widths: std::collections::HashMap<usize, f64>,
}

fn default_visible() -> String {
    "visible".to_string()
}

impl XlsxSheet {
    /// Create a new blank sheet with the given name.
    pub fn new(name: String) -> Self {
        Self {
            name,
            state: "visible".to_string(),
            cells: Vec::new(),
            merged_cells: Vec::new(),
            max_row: 0,
            max_col: 0,
            row_heights: std::collections::HashMap::new(),
            col_widths: std::collections::HashMap::new(),
        }
    }

    /// Get a mutable reference to a cell, creating it if it doesn't exist.
    /// Returns a mutable reference to the (possibly newly created) cell.
    pub fn cell_mut(&mut self, row: usize, col: usize) -> &mut Cell {
        // Find existing cell
        if let Some(idx) = self.cells.iter().position(|c| c.row == row && c.col == col) {
            return &mut self.cells[idx];
        }
        // Create new cell
        let cell = Cell {
            row,
            col,
            value: CellValue::Empty,
            formula: None,
            style: None,
        };
        self.cells.push(cell);
        // Update bounds
        if row + 1 > self.max_row {
            self.max_row = row + 1;
        }
        if col + 1 > self.max_col {
            self.max_col = col + 1;
        }
        // Return mutable reference to the last element
        self.cells.last_mut().unwrap()
    }
}

/// Structured workbook parsed from xlsx. Each sheet carries full cell-level
/// fidelity (formulas, styles, merged ranges).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct XlsxWorkbook {
    pub sheets: Vec<XlsxSheet>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared_strings: Vec<String>,
}

impl XlsxWorkbook {
    pub fn sheet(&self, name: &str) -> Option<&XlsxSheet> {
        self.sheets.iter().find(|s| s.name == name)
    }
    pub fn sheet_mut(&mut self, name: &str) -> Option<&mut XlsxSheet> {
        self.sheets.iter_mut().find(|s| s.name == name)
    }
    pub fn sheet_index(&self, name: &str) -> Option<usize> {
        self.sheets.iter().position(|s| s.name == name)
    }

    /// Apply a sequence of [`ExcelOperation`]s to the workbook in-place.
    ///
    /// Operations are applied sequentially. Later operations can affect the results
    /// of earlier ones (e.g. writing a range then modifying a cell within it).
    pub fn apply_operations(&mut self, ops: Vec<ExcelOperation>) -> Result<(), OfficeError> {
        for op in ops {
            match op {
                ExcelOperation::ModifyCell {
                    sheet,
                    address,
                    value,
                    formula,
                    number_format,
                    bg_color,
                    font_bold,
                    font_italic,
                    font_color,
                    font_size,
                    font_name,
                    alignment_h,
                    alignment_v,
                } => {
                    let sheet = self.sheet_mut(&sheet)
                        .ok_or_else(|| OfficeError::Excel(format!("Sheet not found: {}", sheet)))?;
                    let (row, col) = parse_cell_address(&address)
                        .ok_or_else(|| OfficeError::Excel(format!("Invalid address: {}", address)))?;

                    let cell = sheet.cell_mut(row, col);
                    if let Some(v) = value {
                        cell.value = v;
                    }
                    if let Some(f) = formula {
                        cell.formula = Some(f);
                    }
                    // Style is stored as style_index in XML, but CellStyle is a structured
                    // representation we expose to the AI. For now, we handle number_format
                    // and other style fields by rebuilding a new CellStyle.
                    if number_format.is_some()
                        || bg_color.is_some()
                        || font_bold.is_some()
                        || font_italic.is_some()
                        || font_color.is_some()
                        || font_size.is_some()
                        || font_name.is_some()
                        || alignment_h.is_some()
                        || alignment_v.is_some()
                    {
                        /// Normalise a hex RGB colour string so that the round-trip through
                        /// `modify_excel` → file → `read_excel` is stable regardless of which
                        /// casing/prefix the caller used. The internal canonical form is the
                        /// 6-digit uppercase hex with no `#`, matching the convention the read
                        /// path emits (after it strips its own `#` prefix).
                        fn normalise_hex_color(s: &str) -> String {
                            let hex = s.trim().trim_start_matches('#');
                            hex.to_ascii_uppercase()
                        }

                        // Take ownership of the existing style so we can decide
                        // field-by-field whether to keep, replace, or clear it.
                        // Empty string from the caller means "clear", None means
                        // "leave alone".
                        let mut style = cell.style.take();
                        if let Some(nf) = number_format {
                            style.get_or_insert_with(CellStyle::default).number_format = nf;
                        }
                        if let Some(bc) = bg_color {
                            let s = style.get_or_insert_with(CellStyle::default);
                            if bc.is_empty() {
                                s.fill_fg_color = None;
                            } else {
                                s.fill_fg_color = Some(normalise_hex_color(&bc));
                            }
                        }
                        if let Some(b) = font_bold {
                            style.get_or_insert_with(CellStyle::default).font_bold = b;
                        }
                        if let Some(i) = font_italic {
                            style.get_or_insert_with(CellStyle::default).font_italic = i;
                        }
                        if let Some(fc) = font_color {
                            let s = style.get_or_insert_with(CellStyle::default);
                            if fc.is_empty() {
                                s.font_color = None;
                            } else {
                                s.font_color = Some(normalise_hex_color(&fc));
                            }
                        }
                        if let Some(sz) = font_size {
                            style.get_or_insert_with(CellStyle::default).font_size = Some(sz);
                        }
                        if let Some(fn_) = font_name {
                            style.get_or_insert_with(CellStyle::default).font_name = Some(fn_);
                        }
                        if let Some(ah) = alignment_h {
                            style.get_or_insert_with(CellStyle::default).alignment_h = Some(ah);
                        }
                        if let Some(av) = alignment_v {
                            style.get_or_insert_with(CellStyle::default).alignment_v = Some(av);
                        }
                        cell.style = style;
                    }
                }
                ExcelOperation::WriteRange {
                    sheet,
                    start_cell,
                    values,
                    number_format,
                } => {
                    let sheet = self.sheet_mut(&sheet)
                        .ok_or_else(|| OfficeError::Excel(format!("Sheet not found: {}", sheet)))?;
                    let (start_row, start_col) = parse_cell_address(&start_cell)
                        .ok_or_else(|| OfficeError::Excel(format!("Invalid start_cell: {}", start_cell)))?;

                    for (r_off, row_vals) in values.iter().enumerate() {
                        for (c_off, val) in row_vals.iter().enumerate() {
                            let row = start_row + r_off;
                            let col = start_col + c_off;
                            let cell = sheet.cell_mut(row, col);
                            cell.value = json_value_to_cell_value(val);
                            if let Some(ref fmt) = number_format {
                                let style = cell.style.get_or_insert_with(CellStyle::default);
                                style.number_format = fmt.clone();
                            }
                        }
                    }
                }
                ExcelOperation::MergeCells {
                    sheet,
                    op,
                    start_cell,
                    end_cell,
                } => {
                    let sheet = self.sheet_mut(&sheet)
                        .ok_or_else(|| OfficeError::Excel(format!("Sheet not found: {}", sheet)))?;
                    let (sr, sc) = parse_cell_address(&start_cell)
                        .ok_or_else(|| OfficeError::Excel(format!("Invalid start_cell: {}", start_cell)))?;
                    let (er, ec) = parse_cell_address(&end_cell)
                        .ok_or_else(|| OfficeError::Excel(format!("Invalid end_cell: {}", end_cell)))?;
                    let range = MergedRange {
                        start_row: sr,
                        start_col: sc,
                        end_row: er,
                        end_col: ec,
                    };
                    match op.as_str() {
                        "unmerge" => {
                            sheet.merged_cells.retain(|m| m.address() != range.address());
                        }
                        _ => {
                            // "merge" or default: add if not already present
                            if !sheet.merged_cells.iter().any(|m| m.address() == range.address()) {
                                sheet.merged_cells.push(range);
                            }
                        }
                    }
                }
                ExcelOperation::ResizeDimension {
                    sheet,
                    dimension,
                    index,
                    size,
                    hidden,
                } => {
                    let sheet = self.sheet_mut(&sheet)
                        .ok_or_else(|| OfficeError::Excel(format!("Sheet not found: {}", sheet)))?;
                    if dimension == "row" {
                        if hidden {
                            // Store hidden rows by negative height marker
                            sheet.row_heights.insert(index, -1.0);
                        } else if size > 0.0 {
                            sheet.row_heights.insert(index, size);
                        } else {
                            sheet.row_heights.remove(&index);
                        }
                    } else {
                        // "col"
                        if hidden {
                            sheet.col_widths.insert(index, -1.0);
                        } else if size > 0.0 {
                            sheet.col_widths.insert(index, size);
                        } else {
                            sheet.col_widths.remove(&index);
                        }
                    }
                }
                ExcelOperation::SheetOp {
                    op,
                    sheet: target,
                    new_name,
                    insert_index,
                } => {
                    match op.as_str() {
                        "create" => {
                            let name = new_name.unwrap_or_else(|| "Sheet".to_string());
                            let idx = insert_index.unwrap_or(self.sheets.len());
                            let new_sheet = XlsxSheet::new(name);
                            if idx >= self.sheets.len() {
                                self.sheets.push(new_sheet);
                            } else {
                                self.sheets.insert(idx, new_sheet);
                            }
                        }
                        "rename" => {
                            let new_name = new_name
                                .ok_or_else(|| OfficeError::Excel("rename requires new_name".into()))?;
                            if let Some(s) = self.sheet_mut(&target) {
                                s.name = new_name;
                            }
                        }
                        "delete" => {
                            if self.sheets.len() <= 1 {
                                return Err(OfficeError::Excel(
                                    "Cannot delete the last sheet".into(),
                                ));
                            }
                            let idx = self.sheet_index(&target)
                                .ok_or_else(|| OfficeError::Excel(format!("Sheet not found: {}", target)))?;
                            self.sheets.remove(idx);
                        }
                        "hide" => {
                            if let Some(s) = self.sheet_mut(&target) {
                                s.state = "hidden".to_string();
                            }
                        }
                        "unhide" => {
                            if let Some(s) = self.sheet_mut(&target) {
                                s.state = "visible".to_string();
                            }
                        }
                        _ => {
                            return Err(OfficeError::Excel(format!("Unknown sheet op: {}", op)));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn json_value_to_cell_value(v: &serde_json::Value) -> CellValue {
    match v {
        serde_json::Value::Null => CellValue::Empty,
        serde_json::Value::Object(obj)
            if obj.is_empty() || obj.get("type").and_then(|t| t.as_str()) == Some("empty") =>
        {
            CellValue::Empty
        }
        serde_json::Value::Object(obj) => {
            let t = obj.get("type").and_then(|t| t.as_str()).unwrap_or("string");
            let val = obj.get("value");
            match t {
                "int" => CellValue::Int(val.and_then(|v| v.as_i64()).unwrap_or(0)),
                "float" => CellValue::Float(val.and_then(|v| v.as_f64()).unwrap_or(0.0)),
                "bool" => CellValue::Bool(val.and_then(|v| v.as_bool()).unwrap_or(false)),
                "string" => {
                    CellValue::String(val.and_then(|v| v.as_str()).unwrap_or("").to_string())
                }
                "datetime" => CellValue::DateTime(val.and_then(|v| v.as_f64()).unwrap_or(0.0)),
                "error" => {
                    CellValue::Error(val.and_then(|v| v.as_str()).unwrap_or("").to_string())
                }
                _ => CellValue::Empty,
            }
        }
        serde_json::Value::String(s) => CellValue::String(s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                CellValue::Int(i)
            } else {
                CellValue::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::Bool(b) => CellValue::Bool(*b),
        _ => CellValue::Empty,
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Convert a (row, col) zero-based pair to an A1-style address string.
pub fn cell_address(row: usize, col: usize) -> String {
    let mut s = String::new();
    let mut c = col;
    loop {
        s.insert(0, (b'A' + (c % 26) as u8) as char);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s.push_str(&(row + 1).to_string());
    s
}

/// Parse an A1-style address (e.g. "B12") into zero-based (row, col).
pub fn parse_cell_address(addr: &str) -> Option<(usize, usize)> {
    let bytes = addr.as_bytes();
    let mut col: usize = 0;
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        col = col * 26 + (bytes[i].to_ascii_uppercase() - b'A' + 1) as usize;
        i += 1;
    }
    if i == 0 || i == bytes.len() {
        return None;
    }
    let row_part = std::str::from_utf8(&bytes[i..]).ok()?;
    let row: usize = row_part.parse().ok()?;
    if row == 0 {
        return None;
    }
    Some((row - 1, col - 1))
}

/// Parse an A1:B3 style range into a MergedRange.
fn parse_range(s: &str) -> Option<MergedRange> {
    let (start, end) = s.split_once(':')?;
    let (sr, sc) = parse_cell_address(start)?;
    let (er, ec) = parse_cell_address(end)?;
    Some(MergedRange {
        start_row: sr,
        start_col: sc,
        end_row: er,
        end_col: ec,
    })
}

// ─── Structured parse ────────────────────────────────────────────────────────

/// Internal record while walking a sheet XML.
#[derive(Default, Debug)]
struct ParsedCell {
    ref_addr: String,
    cell_type: Option<String>,
    style_index: Option<u32>,
    formula: Option<String>,
    inline_string: Option<String>,
    value_text: String,
}

#[derive(Default, Debug)]
struct SheetParseResult {
    cells: Vec<Cell>,
    merged: Vec<MergedRange>,
    state: String,
    max_row: usize,
    max_col: usize,
    row_heights: std::collections::HashMap<usize, f64>,
    col_widths: std::collections::HashMap<usize, f64>,
}

/// Read the shared string pool from `xl/sharedStrings.xml`.
fn parse_shared_strings(xml: &str) -> Vec<String> {
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut strings = Vec::new();
    let mut current = String::new();
    let mut in_si = false;
    let mut in_t = false;
    let mut is_present = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"si" {
                    in_si = true;
                    current.clear();
                    is_present = false;
                } else if in_si && name.as_ref() == b"t" {
                    in_t = true;
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"si" {
                    if is_present {
                        strings.push(current.clone());
                    }
                    in_si = false;
                } else if in_si && name.as_ref() == b"t" {
                    in_t = false;
                }
            }
            Ok(Event::Text(ref t)) => {
                if in_t {
                    if let Ok(s) = t.unescape() {
                        current.push_str(&s);
                        is_present = true;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    strings
}

fn parse_sheet_xml(xml: &str, shared_strings: &[String], styles_info: Option<&StylesInfo>) -> SheetParseResult {
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut cells: Vec<Cell> = Vec::new();
    let mut merged: Vec<MergedRange> = Vec::new();
    let mut max_row: usize = 0;
    let mut max_col: usize = 0;
    let mut row_heights: std::collections::HashMap<usize, f64> = std::collections::HashMap::new();
    let mut col_widths: std::collections::HashMap<usize, f64> = std::collections::HashMap::new();

    if DEBUG_XLSX {
        eprintln!("[xlsx] parse_sheet_xml: starting, xml len = {}", xml.len());
    }

    let mut in_sheet_data = false;
    let mut in_cols = false;
    let mut current_cell: Option<ParsedCell> = None;
    let mut in_formula = false;
    let mut in_value = false;
    let mut in_inline_string = false;

    // Parse the attributes of a `<c ...>` tag into a fresh `ParsedCell`. Shared by
    // both `Event::Start` (paired with `</c>`) and `Event::Empty` (self-closing
    // `<c .../>`, which is what `build_cell_xml` emits for empty cells).
    let parse_c_attrs = |e: &quick_xml::events::BytesStart| -> ParsedCell {
        let mut c = ParsedCell::default();
        for attr in e.attributes().with_checks(false).flatten() {
            let v = attr.value.as_ref();
            if let Ok(s) = std::str::from_utf8(v) {
                let key = attr.key.as_ref();
                let local = strip_xml_ns(key);
                match local {
                    b"r" => c.ref_addr = s.to_string(),
                    b"t" => c.cell_type = Some(s.to_string()),
                    b"s" => c.style_index = s.parse().ok(),
                    _ => {}
                }
            }
        }
        c
    };

    // Push a parsed cell into the result. Empty cells without style are dropped
    // to match Excel's "missing cell" semantics; cells with style only are kept
    // so that style-only edits survive a round-trip.
    let mut push_cell = |c: ParsedCell| {
        if let Some((row, col)) = parse_cell_address(&c.ref_addr) {
            if row + 1 > max_row {
                max_row = row + 1;
            }
            if col + 1 > max_col {
                max_col = col + 1;
            }
            let value = resolve_cell_value(&c, shared_strings);
            let style = c.style_index.and_then(|idx| {
                styles_info.and_then(|si| si.resolve_style(idx as usize))
            });
            if !matches!(value, CellValue::Empty)
                || style.is_some()
                || c.formula.is_some()
            {
                cells.push(Cell {
                    row,
                    col,
                    value,
                    formula: c.formula,
                    style,
                });
            }
        }
    };

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"sheetData" => in_sheet_data = true,
                    b"cols" => in_cols = true,
                    b"row" if in_sheet_data => {
                        // Parse row attributes for height
                        let mut row_index: Option<usize> = None;
                        let mut ht: Option<f64> = None;

                        for attr in e.attributes().with_checks(false).flatten() {
                            let v = attr.value.as_ref();
                            if let Ok(s) = std::str::from_utf8(v) {
                                let key = attr.key.as_ref();
                                let local = strip_xml_ns(key);
                                match local {
                                    b"r" => row_index = s.parse().ok(),
                                    b"ht" => ht = s.parse().ok(),
                                    _ => {}
                                }
                            }
                        }

                        if let Some(idx) = row_index {
                            if let Some(h) = ht {
                                if DEBUG_XLSX {
                                    eprintln!("[xlsx] row {}: ht={}", idx, h);
                                }
                                row_heights.insert(idx - 1, h); // Convert to 0-based
                            }
                        }
                    }
                    b"c" if in_sheet_data => {
                        current_cell = Some(parse_c_attrs(e));
                        in_value = false;
                        in_inline_string = false;
                    }
                    b"f" if current_cell.is_some() => in_formula = true,
                    b"v" if current_cell.is_some() => in_value = true,
                    b"is" if current_cell.is_some() => in_inline_string = true,
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = e.local_name();
                if name.as_ref() == b"mergeCell" {
                    if let Some(v) = attr_value(e, b"ref") {
                        if let Ok(s) = std::str::from_utf8(&v) {
                            if let Some(r) = parse_range(s) {
                                merged.push(r);
                            }
                        }
                    }
                } else if name.as_ref() == b"c" && in_sheet_data {
                    // Self-closing cell — must be parsed in the same way as the
                    // <c ...>...</c> branch so style-only cells (no value, no
                    // formula) survive a write→read round-trip.
                    push_cell(parse_c_attrs(e));
                } else if name.as_ref() == b"col" && in_cols {
                    // Parse column attributes for width
                    let mut col_min: Option<usize> = None;
                    let mut col_max: Option<usize> = None;
                    let mut width: Option<f64> = None;

                    for attr in e.attributes().with_checks(false).flatten() {
                        let v = attr.value.as_ref();
                        if let Ok(s) = std::str::from_utf8(v) {
                            let key = attr.key.as_ref();
                            let local = strip_xml_ns(key);
                            match local {
                                b"min" => col_min = s.parse().ok(),
                                b"max" => col_max = s.parse().ok(),
                                b"width" => width = s.parse().ok(),
                                _ => {}
                            }
                        }
                    }

                    if DEBUG_XLSX {
                        eprintln!("[xlsx] col tag: min={:?}, max={:?}, width={:?}, in_cols={}", col_min, col_max, width, in_cols);
                    }

                    // Apply width to all columns in the range
                    if let Some(min_idx) = col_min {
                        let max_idx = col_max.unwrap_or(min_idx);
                        for i in min_idx..=max_idx {
                            if let Some(w) = width {
                                if DEBUG_XLSX {
                                    eprintln!("[xlsx]   -> col_widths[{}] = {}", i - 1, w);
                                }
                                col_widths.insert(i - 1, w); // Convert to 0-based
                            }
                        }
                    }
                }
            }
            Ok(Event::Text(ref t)) => {
                if in_value {
                    if let Some(c) = current_cell.as_mut() {
                        if let Ok(s) = t.unescape() {
                            c.value_text.push_str(&s);
                        }
                    }
                } else if in_formula {
                    if let Some(c) = current_cell.as_mut() {
                        if let Ok(s) = t.unescape() {
                            c.formula = Some(s.to_string());
                        }
                    }
                } else if in_inline_string {
                    if let Some(c) = current_cell.as_mut() {
                        if let Ok(s) = t.unescape() {
                            let cur = c.inline_string.get_or_insert_with(String::new);
                            cur.push_str(&s);
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"sheetData" => in_sheet_data = false,
                    b"cols" => in_cols = false,
                    b"v" => in_value = false,
                    b"f" => in_formula = false,
                    b"is" => in_inline_string = false,
                    b"c" => {
                        if let Some(c) = current_cell.take() {
                            push_cell(c);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    if DEBUG_XLSX {
        eprintln!("[xlsx] parse_sheet_xml: done - cells={}, merged={}, max_row={}, max_col={}", cells.len(), merged.len(), max_row, max_col);
        eprintln!("[xlsx] parse_sheet_xml: row_heights={:?}", row_heights);
        eprintln!("[xlsx] parse_sheet_xml: col_widths={:?}", col_widths);
    }

    SheetParseResult {
        cells,
        merged,
        state: "visible".to_string(),
        max_row,
        max_col,
        row_heights,
        col_widths,
    }
}

fn resolve_cell_value(c: &ParsedCell, shared_strings: &[String]) -> CellValue {
    match c.cell_type.as_deref() {
        Some("inlineStr") | Some("str") => {
            if let Some(s) = &c.inline_string {
                if s.is_empty() {
                    CellValue::String(c.value_text.clone())
                } else {
                    CellValue::String(s.clone())
                }
            } else {
                CellValue::String(c.value_text.clone())
            }
        }
        Some("s") => {
            if let Ok(idx) = c.value_text.parse::<usize>() {
                if let Some(s) = shared_strings.get(idx) {
                    return CellValue::String(s.clone());
                }
            }
            CellValue::String(c.value_text.clone())
        }
        Some("b") => CellValue::Bool(c.value_text == "1" || c.value_text.eq_ignore_ascii_case("true")),
        Some("e") => CellValue::Error(c.value_text.clone()),
        Some("n") | None => {
            if c.value_text.is_empty() {
                CellValue::Empty
            } else if let Ok(i) = c.value_text.parse::<i64>() {
                CellValue::Int(i)
            } else if let Ok(f) = c.value_text.parse::<f64>() {
                CellValue::Float(f)
            } else {
                CellValue::String(c.value_text.clone())
            }
        }
        Some(_) => CellValue::String(c.value_text.clone()),
    }
}

pub(crate) fn read_entry<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<String, OfficeError> {
    let mut file = archive.by_name(name)?;
    let mut s = String::new();
    file.read_to_string(&mut s)?;
    Ok(s)
}

/// Structured entry-point: parse a complete xlsx into the structured
/// [`XlsxWorkbook`]. Falls back to an empty workbook if any of the auxiliary
/// parts (styles, sharedStrings) are missing.
pub fn read_xlsx_structured(bytes: &[u8]) -> Result<XlsxWorkbook, OfficeError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec()))?;

    let shared_strings = match read_entry(&mut archive, "xl/sharedStrings.xml") {
        Ok(xml) => parse_shared_strings(&xml),
        Err(_) => Vec::new(),
    };

    let styles_info = match read_entry(&mut archive, "xl/styles.xml") {
        Ok(xml) => Some(parse_styles(&xml)),
        Err(_) => None,
    };

    let workbook_xml = read_entry(&mut archive, "xl/workbook.xml")?;
    let rels_xml = read_entry(&mut archive, "xl/_rels/workbook.xml.rels")
        .unwrap_or_default();

    let sheet_entries = parse_sheet_entries(&workbook_xml, &rels_xml)?;

    let mut sheets = Vec::new();
    for entry in &sheet_entries {
        let xml = match read_entry(&mut archive, &entry.path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let parsed = parse_sheet_xml(&xml, &shared_strings, styles_info.as_ref());
        sheets.push(XlsxSheet {
            name: entry.name.clone(),
            state: entry.state.clone(),
            cells: parsed.cells,
            merged_cells: parsed.merged,
            max_row: parsed.max_row,
            max_col: parsed.max_col,
            row_heights: parsed.row_heights,
            col_widths: parsed.col_widths,
        });
    }

    Ok(XlsxWorkbook {
        sheets,
        shared_strings,
    })
}

/// Convert a structured workbook to a readable text representation. Includes
/// formulas and number formats so the model knows what the data represents.

pub(crate) fn escape_xml_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

// ─── Sheet discovery helpers ─────────────────────────────────────────────────

/// Information about a sheet parsed from workbook.xml.
#[derive(Debug)]
struct SheetEntry {
    name: String,
    path: String,
    state: String,
}

/// Parse `xl/workbook.xml` + `xl/_rels/workbook.xml.rels` to map sheet name
/// to the path of its `xl/worksheets/sheetN.xml` file. Also reads the `state`
/// attribute from each `<sheet>` element so hidden sheets are tracked.
fn parse_sheet_entries(
    workbook_xml: &str,
    rels_xml: &str,
) -> Result<Vec<SheetEntry>, OfficeError> {
    let mut reader = XmlReader::from_str(workbook_xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut raw_sheets: Vec<(String, String, String)> = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                if e.local_name().as_ref() == b"sheet" {
                    let mut name = String::new();
                    let mut rid = String::new();
                    let mut state = "visible".to_string();
                    for attr in e.attributes().with_checks(false).flatten() {
                        let v = match std::str::from_utf8(&attr.value) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                        let local = strip_xml_ns(attr.key.as_ref());
                        match local {
                            b"name" => name = v.to_string(),
                            b"id" => rid = v.to_string(),
                            b"state" => state = v.to_string(),
                            _ => {}
                        }
                    }
                    if !name.is_empty() && !rid.is_empty() {
                        raw_sheets.push((name, rid, state));
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    let mut rel_reader = XmlReader::from_str(rels_xml);
    rel_reader.config_mut().trim_text(false);
    let mut rel_buf = Vec::new();
    let mut rid_to_target: HashMap<String, String> = HashMap::new();
    loop {
        match rel_reader.read_event_into(&mut rel_buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                if e.local_name().as_ref() == b"Relationship" {
                    let mut id = String::new();
                    let mut target = String::new();
                    for attr in e.attributes().with_checks(false).flatten() {
                        let v = match std::str::from_utf8(&attr.value) {
                            Ok(s) => s,
                            Err(_) => continue,
                        };
                        let local = strip_xml_ns(attr.key.as_ref());
                        match local {
                            b"Id" => id = v.to_string(),
                            b"Target" => target = v.to_string(),
                            _ => {}
                        }
                    }
                    if !id.is_empty() && !target.is_empty() {
                        rid_to_target.insert(id, target);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        rel_buf.clear();
    }

    let mut out = Vec::new();
    for (name, rid, state) in raw_sheets {
        if let Some(target) = rid_to_target.get(&rid) {
            let path = if target.starts_with('/') {
                target.trim_start_matches('/').to_string()
            } else {
                format!("xl/{}", target)
            };
            out.push(SheetEntry { name, path, state });
        }
    }
    Ok(out)
}

/// Backward-compatible wrapper: returns (name, path) pairs only.
/// All callers that don't need `state` use this to avoid massive churn.
pub(crate) fn parse_sheet_name_to_path(
    workbook_xml: &str,
    rels_xml: &str,
) -> Result<Vec<(String, String)>, OfficeError> {
    parse_sheet_entries(workbook_xml, rels_xml)
        .map(|entries| entries.into_iter().map(|e| (e.name, e.path)).collect())
}

// ─── Styles document (lightweight; conservative) ─────────────────────────────

/// Minimal `xl/styles.xml` tracking. Only used to emit `xl/styles.xml` when
/// a number format was added; for read-side fidelity we use `StylesInfo`.
#[derive(Debug, Clone, Default)]
struct StylesDocument {
    raw: String,
    custom_num_fmts: Vec<(u32, String)>,
    modified: bool,
}

impl StylesDocument {
    fn parse(xml: &str) -> Self {
        Self {
            raw: xml.to_string(),
            custom_num_fmts: Vec::new(),
            modified: false,
        }
    }

    fn is_modified(&self) -> bool {
        self.modified
    }

    fn add_num_fmt(&mut self, id: u32, code: String) {
        self.custom_num_fmts.push((id, code));
        self.modified = true;
    }

    fn serialize(&self) -> String {
        if self.raw.is_empty() {
            return build_minimal_styles_xml(&self.custom_num_fmts);
        }
        let mut xml = self.raw.clone();
        if !self.custom_num_fmts.is_empty() {
            let block: String = self
                .custom_num_fmts
                .iter()
                .map(|(id, code)| {
                    format!(
                        "<numFmt numFmtId=\"{}\" formatCode=\"{}\"/>",
                        id,
                        escape_xml_text(code)
                    )
                })
                .collect();
            if xml.contains("<numFmts") {
                xml = xml.replacen("</numFmts>", &format!("{}{}", block, "</numFmts>"), 1);
            } else if let Some(pos) = xml.find("</styleSheet>") {
                xml.insert_str(
                    pos,
                    &format!("<numFmts count=\"{}\">{}</numFmts>", self.custom_num_fmts.len(), block),
                );
            }
        }
        xml
    }
}

fn build_minimal_styles_xml(num_fmts: &[(u32, String)]) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>
<fills count="1"><fill><patternFill patternType="none"/></fill></fills>
<borders count="1"><border/></borders>
<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
"#,
    );
    if !num_fmts.is_empty() {
        xml.push_str(&format!("<numFmts count=\"{}\">", num_fmts.len()));
        for (id, code) in num_fmts {
            xml.push_str(&format!(
                "<numFmt numFmtId=\"{}\" formatCode=\"{}\"/>",
                id,
                escape_xml_text(code)
            ));
        }
        xml.push_str("</numFmts>\n");
    }
    xml.push_str("<cellXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/></cellXfs>\n");
    xml.push_str("</styleSheet>");
    xml
}


