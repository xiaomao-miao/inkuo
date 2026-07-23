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
//! | `mod.rs` (~3 300 lines) | Public types, `read_excel_workbook`, `write_excel_workbook`, `read_xlsx_structured`, `incremental_write_xlsx`, plus the streaming XML readers / writers. |
//! | `legacy_text.rs` | `cell_to_string` + `excel_workbook_to_text` (legacy flat API rendering). |
//! | `structured_text.rs` | `xlsx_workbook_to_text` (structured API rendering). |
//!
//! Future splits to consider:
//! - `styles_parser.rs` (~360 lines) — `CellXf` / `AlignmentXf` / `FontXf` /
//!   `FillXf` / `StylesInfo` + the `parse_styles` state machine.
//! - `sheet_parser.rs` (~230 lines) — `parse_sheet_xml` / `parse_shared_strings`
//!   / `ParsedCell` / `SheetParseResult`.
//! - `incremental_writer.rs` (~600 lines) — `CellModification` /
//!   `ExcelOperation` / `find_c_element_end` / `build_replacement_cell_xml` /
//!   related string-assemble helpers used only by `incremental_write_xlsx`.

pub mod types;
pub(crate) mod legacy_text;
pub(crate) mod structured_text;
pub(crate) mod styles_parser;
pub(crate) mod ooxml_boilerplate;

// Re-export so callers can keep using `crate::office::xlsx::cell_to_string`,
// `crate::office::xlsx::excel_workbook_to_text`, and
// `crate::office::xlsx::xlsx_workbook_to_text` unchanged.
pub use legacy_text::{cell_to_string, excel_workbook_to_text};
pub use structured_text::xlsx_workbook_to_text;
pub(crate) use styles_parser::{
    attr_value, parse_styles, resolve_number_format, strip_xml_ns, StylesInfo,
};
pub(crate) use ooxml_boilerplate::{MINIMAL_STYLES_XML, MINIMAL_THEME_XML};

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

fn build_styles_xml(used_styles: &[(SheetStyleKey, usize)]) -> String {
    let mut num_fmts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    let mut next_num_fmt = 164u32;
    let mut fonts: Vec<(SheetStyleKey, usize)> = Vec::new();
    // FIX: Use tuple key to properly deduplicate fonts with different bold/italic
    let mut font_index: std::collections::HashMap<(Option<String>, Option<u32>, Option<String>, bool, bool), usize> = std::collections::HashMap::new();
    let mut fills: Vec<(Option<String>, Option<String>, usize)> = Vec::new();
    let mut fill_index: std::collections::HashMap<(Option<String>, Option<String>), usize> = std::collections::HashMap::new();

    let _default_font_idx = *font_index.entry((None, None, None, false, false)).or_insert_with(|| {
        let idx = fonts.len();
        fonts.push((SheetStyleKey::default(), idx));
        idx
    });
    let _default_fill_idx = *fill_index.entry((None, None)).or_insert_with(|| {
        let idx = fills.len();
        fills.push((None, None, idx));
        idx
    });
    // Don't pre-seed numFmts with an empty key: numFmtId 0 is reserved by the
    // spec and writing `<numFmt numFmtId="0" formatCode=""/>` confuses readers.
    // The default ("General") numFmtId is always 0 and never needs declaring.

    // FIX: Add numFmtId to xfs tuple
    let mut xfs: Vec<(usize, usize, u32, bool, bool)> = Vec::new();

    for (key, _) in used_styles.iter() {
        // FIX: Use full font info tuple as key instead of just font_name
        let font_key = (key.font_name.clone(), key.font_size, key.font_color.clone(), key.font_bold, key.font_italic);
        let font_idx = *font_index.entry(font_key).or_insert_with(|| {
            let idx = fonts.len();
            fonts.push((key.clone(), idx));
            idx
        });
        let fill_idx = *fill_index.entry((key.fill_fg_color.clone(), key.fill_bg_color.clone())).or_insert_with(|| {
            let idx = fills.len();
            fills.push((key.fill_fg_color.clone(), key.fill_bg_color.clone(), idx));
            idx
        });
        if !key.number_format.is_empty() {
            num_fmts.entry(key.number_format.clone()).or_insert_with(|| {
                let id = next_num_fmt;
                next_num_fmt += 1;
                id
            });
        }
        // FIX: Calculate numFmtId properly
        let num_fmt_id = if key.number_format.is_empty() {
            0
        } else {
            *num_fmts.get(&key.number_format).unwrap_or(&0)
        };
        xfs.push((font_idx, fill_idx, num_fmt_id, key.font_bold, key.font_italic));
    }

    let mut xml = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#);
    xml.push_str("\n<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\n");
    if !num_fmts.is_empty() {
        xml.push_str("<numFmts count=\"");
        xml.push_str(&num_fmts.len().to_string());
        xml.push_str("\">");
        for (fmt, id) in &num_fmts {
            xml.push_str("<numFmt numFmtId=\"");
            xml.push_str(&id.to_string());
            xml.push_str("\" formatCode=\"");
            xml.push_str(&escape_xml_attr(fmt));
            xml.push_str("\"/>");
        }
        xml.push_str("</numFmts>\n");
    } else {
        xml.push_str("<numFmts count=\"0\"/>\n");
    }

    xml.push_str("<fonts count=\"");
    xml.push_str(&(fonts.len() + 1).to_string());
    xml.push_str("\">");
    xml.push_str("<font><name val=\"Calibri\"/><family val=\"2\"/><color theme=\"1\"/><sz val=\"11\"/><scheme val=\"minor\"/></font>");
    for (style, _) in &fonts {
        xml.push_str("<font>");
        xml.push_str("<name val=\"");
        xml.push_str(&escape_xml_attr(style.font_name.as_deref().unwrap_or("Calibri")));
        xml.push_str("\"/>");
        xml.push_str("<family val=\"2\"/>");
        if let Some(color) = &style.font_color {
            xml.push_str("<color rgb=\"");
            xml.push_str(&escape_xml_attr(color));
            xml.push_str("\"/>");
        } else {
            xml.push_str("<color theme=\"1\"/>");
        }
        xml.push_str("<sz val=\"");
        xml.push_str(&style.font_size.unwrap_or(11).to_string());
        xml.push_str("\"/>");
        if style.font_bold { xml.push_str("<b/>"); }
        if style.font_italic { xml.push_str("<i/>"); }
        xml.push_str("<scheme val=\"minor\"/>");
        xml.push_str("</font>");
    }
    xml.push_str("</fonts>\n");

    xml.push_str("<fills count=\"");
    xml.push_str(&(fills.len() + 2).to_string());
    xml.push_str("\">");
    xml.push_str("<fill><patternFill/></fill>");
    xml.push_str("<fill><patternFill patternType=\"gray125\"/></fill>");
    for (fg, bg, _) in &fills {
        xml.push_str("<fill><patternFill patternType=\"solid\">");
        if let Some(color) = fg {
            xml.push_str("<fgColor rgb=\"");
            xml.push_str(&escape_xml_attr(color));
            xml.push_str("\"/>");
        }
        if let Some(color) = bg {
            xml.push_str("<bgColor rgb=\"");
            xml.push_str(&escape_xml_attr(color));
            xml.push_str("\"/>");
        }
        xml.push_str("</patternFill></fill>");
    }
    xml.push_str("</fills>\n");

    xml.push_str("<borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>\n");
    xml.push_str("<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>\n");
    xml.push_str("<cellXfs count=\"");
    xml.push_str(&(xfs.len() + 1).to_string());
    xml.push_str("\">");
    xml.push_str("<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" pivotButton=\"0\" quotePrefix=\"0\" xfId=\"0\"/>");
    for (font_idx, fill_idx, num_fmt_id, bold, italic) in &xfs {
        // FIX: Use actual numFmtId instead of hardcoded 0
        let mut attrs = format!("numFmtId=\"{}\" fontId=\"{}\" fillId=\"{}\" borderId=\"0\" xfId=\"0\"", num_fmt_id, font_idx + 1, fill_idx + 2);
        if *bold || *italic { attrs.push_str(" applyFont=\"1\""); }
        // FIX: Apply fill whenever a non-default fill is in use
        if *fill_idx > 0 { attrs.push_str(" applyFill=\"1\""); }
        attrs.push_str(" applyBorder=\"0\" applyNumberFormat=\"1\"");
        xml.push_str("<xf ");
        xml.push_str(&attrs);
        xml.push_str("/>");
    }
    xml.push_str("</cellXfs>\n");
    xml.push_str("<cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\" hidden=\"0\"/></cellStyles>\n");
    xml.push_str("<dxfs count=\"0\"/>\n");
    xml.push_str(r#"<tableStyles count="0" defaultTableStyle="TableStyleMedium9" defaultPivotStyle="PivotStyleLight16"/>"#);
    xml.push_str("\n</styleSheet>");
    xml
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

fn read_entry<R: Read + std::io::Seek>(
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
// ─── Conservative write (incremental) ────────────────────────────────────────

/// A single cell-level change to apply during a conservative write.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CellModification {
    pub sheet: String,
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_value: Option<CellValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_formula: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_number_format: Option<String>,
    // ── Style fields ──────────────────────────────────────────────────────────
    /// Background fill color as 6-digit hex RGB, e.g. "FFFF00" for yellow.
    /// Empty string signals "remove background".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_bg_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_font_bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_font_italic: Option<bool>,
    /// Font color as 6-digit hex RGB, e.g. "FF0000" for red.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_font_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_font_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_font_name: Option<String>,
    /// Horizontal alignment: "left" | "center" | "right"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_alignment_h: Option<String>,
    /// Vertical alignment: "top" | "center" | "bottom"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_alignment_v: Option<String>,
}

/// Unified operation type for all Excel modifications.
/// This replaces the older scattered structs (CellModification, MergeModification,
/// RowColModification) and mirrors the DocElement pattern used in Word.
/// serde: `{"type": "modify_cell", ...}` / `{"type": "write_range", ...}` / etc.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExcelOperation {
    /// Modify a single cell's value, formula, or style.
    ModifyCell {
        sheet: String,
        address: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<CellValue>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        formula: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        number_format: Option<String>,
        /// Background fill color as 6-digit hex RGB (e.g. "FFFF00"). Empty string = remove.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bg_color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_bold: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_italic: Option<bool>,
        /// Font color as 6-digit hex RGB (e.g. "FF0000").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_size: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_name: Option<String>,
        /// Horizontal alignment: "left" | "center" | "right"
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alignment_h: Option<String>,
        /// Vertical alignment: "top" | "center" | "bottom"
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alignment_v: Option<String>,
    },
    /// Batch-write a 2-D array of values into a rectangular region.
    WriteRange {
        sheet: String,
        /// Top-left cell address, e.g. "A1"
        start_cell: String,
        /// Row-major values array. Inner arrays are rows.
        values: Vec<Vec<serde_json::Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        number_format: Option<String>,
    },
    /// Merge or unmerge a rectangular cell region.
    MergeCells {
        sheet: String,
        /// "merge" or "unmerge"
        op: String,
        /// Top-left cell address
        start_cell: String,
        /// Bottom-right cell address
        end_cell: String,
    },
    /// Set row height or column width.
    ResizeDimension {
        sheet: String,
        /// "row" or "col"
        dimension: String,
        /// 0-based row or column index
        index: usize,
        /// Height in points (rows) or character units (columns)
        size: f64,
        #[serde(default)]
        hidden: bool,
    },
    /// Sheet-level operations (create, rename, delete, hide, unhide).
    SheetOp {
        /// "create" | "rename" | "delete" | "hide" | "unhide"
        op: String,
        /// Target sheet name (for rename/delete/hide/unhide); not required for "create".
        #[serde(default)]
        sheet: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        new_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        insert_index: Option<usize>,
    },
}

impl CellModification {
    pub fn new(sheet: impl Into<String>, address: impl Into<String>) -> Self {
        Self {
            sheet: sheet.into(),
            address: address.into(),
            new_value: None,
            new_formula: None,
            new_number_format: None,
            new_bg_color: None,
            new_font_bold: None,
            new_font_italic: None,
            new_font_color: None,
            new_font_size: None,
            new_font_name: None,
            new_alignment_h: None,
            new_alignment_v: None,
        }
    }

    pub fn with_value(mut self, value: CellValue) -> Self {
        self.new_value = Some(value);
        self
    }

    pub fn with_formula(mut self, formula: impl Into<String>) -> Self {
        self.new_formula = Some(formula.into());
        self
    }

    pub fn with_number_format(mut self, fmt: impl Into<String>) -> Self {
        self.new_number_format = Some(fmt.into());
        self
    }

    pub fn has_style_change(&self) -> bool {
        self.new_bg_color.is_some()
            || self.new_font_bold.is_some()
            || self.new_font_italic.is_some()
            || self.new_font_color.is_some()
            || self.new_font_size.is_some()
            || self.new_font_name.is_some()
            || self.new_alignment_h.is_some()
            || self.new_alignment_v.is_some()
            || self.new_number_format.is_some()
    }
}

/// Apply cell modifications to an existing xlsx by surgically rewriting only
/// the affected `xl/worksheets/sheet*.xml` (and `xl/styles.xml` when formats
/// change). All other parts of the workbook — including formulas, charts,
/// defined names — are preserved verbatim.
///
/// DEPRECATED: Use `XlsxWorkbook::apply_operations()` + `write_excel_document()` instead.
pub fn incremental_write_xlsx(
    original_bytes: &[u8],
    modifications: &[CellModification],
    output_path: &std::path::Path,
) -> Result<(), OfficeError> {
    if modifications.is_empty() {
        std::fs::write(output_path, original_bytes)?;
        return Ok(());
    }

    let mut archive = zip::ZipArchive::new(Cursor::new(original_bytes.to_vec()))?;
    let workbook_xml = read_entry(&mut archive, "xl/workbook.xml")?;
    let rels_xml = read_entry(&mut archive, "xl/_rels/workbook.xml.rels")
        .unwrap_or_default();
    let sheet_name_to_path = parse_sheet_name_to_path(&workbook_xml, &rels_xml)?;

    // Group modifications by sheet path.
    let mut by_path: HashMap<String, Vec<&CellModification>> = HashMap::new();
    for m in modifications {
        if let Some((_, path)) = sheet_name_to_path.iter().find(|(n, _)| n == &m.sheet) {
            by_path.entry(path.clone()).or_default().push(m);
        } else {
            return Err(OfficeError::Excel(format!(
                "Sheet '{}' not found in workbook",
                m.sheet
            )));
        }
    }

    let mut rewritten: HashMap<String, Vec<u8>> = HashMap::new();

    let original_styles_xml = read_entry(&mut archive, "xl/styles.xml").ok();
    let styles_doc = StylesDocument::parse(original_styles_xml.as_deref().unwrap_or(""));

    for (path, mods) in &by_path {
        let xml = read_entry(&mut archive, path)?;
        let new_xml = apply_modifications_to_sheet(&xml, mods)?;
        rewritten.insert(path.clone(), new_xml.into_bytes());
    }

    if styles_doc.is_modified() {
        rewritten.insert(
            "xl/styles.xml".into(),
            styles_doc.serialize().into_bytes(),
        );
    }

    let _ = new_num_fmt_helper; // silence unused warning if feature trimmed

    // Write output: copy original archive, replace rewritten entries.
    let mut out = zip::ZipWriter::new(std::fs::File::create(output_path)?);

    let mut archive = zip::ZipArchive::new(Cursor::new(original_bytes.to_vec()))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        if let Some(new_bytes) = rewritten.get(&name) {
            // Use the same compression method as the original file
            let file_opts = if file.compression() == zip::CompressionMethod::Deflated {
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .unix_permissions(0o644)
            } else {
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .unix_permissions(0o644)
            };
            out.start_file(&name, file_opts)?;
            out.write_all(new_bytes)?;
        } else {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            let file_opts = if file.compression() == zip::CompressionMethod::Deflated {
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .unix_permissions(0o644)
            } else {
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .unix_permissions(0o644)
            };
            out.start_file(&name, file_opts)?;
            out.write_all(&buf)?;
        }
    }
    out.finish()?;

    Ok(())
}

// Workaround so unused-helper imports don't trip the linter when we trim
// down the styles-update path. `new_num_fmt_helper` is intentionally a noop.
fn new_num_fmt_helper() {}

// ─── Sheet XML rewriting (text-based, conservative) ──────────────────────────

/// Apply modifications to a sheet XML and return the new XML. The
/// implementation walks the XML character-by-character to find `<c r="A1"...>`
/// elements and rewrites only the matched ones; everything else is preserved.
///
/// DEPRECATED: Use `XlsxWorkbook::apply_operations()` + `write_excel_document()` instead.
fn apply_modifications_to_sheet(
    sheet_xml: &str,
    mods: &[&CellModification],
) -> Result<String, OfficeError> {
    // Build a lookup by uppercase address.
    let mut by_addr: HashMap<String, &CellModification> = HashMap::new();
    for m in mods {
        by_addr.insert(m.address.to_ascii_uppercase(), m);
    }

    let mut out = String::with_capacity(sheet_xml.len());
    let bytes = sheet_xml.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Look for `<c ` or `<c>`.
        if i + 2 < bytes.len() && bytes[i] == b'<' && bytes[i + 1] == b'c' {
            let next = bytes[i + 2];
            if next == b' ' || next == b'>' || next == b'/' || next == b'\t' || next == b'\n' || next == b'\r' {
                // Find end of opening tag (self-closing `<c .../>`) or matching
                // closing `</c>`.
                let tag_start = i;
                let (end_of_elem, self_closing) = find_c_element_end(bytes, i)?;
                let elem_text = std::str::from_utf8(&bytes[tag_start..end_of_elem])
                    .map_err(|e| OfficeError::Excel(format!("utf-8: {}", e)))?;

                // Extract the cell reference (r="...").
                if let Some(addr) = extract_attr(elem_text, "r") {
                    let upper = addr.to_ascii_uppercase();
                    if let Some(m) = by_addr.get(&upper) {
                        // If this modification only changes styles (no value/formula change),
                        // we need to preserve the original cell content (value and formula).
                        let (row, col) = parse_cell_address(&upper).ok_or_else(|| {
                            OfficeError::Excel(format!("invalid cell address: {}", m.address))
                        })?;
                        let replacement = if m.has_style_change()
                            && m.new_value.is_none()
                            && m.new_formula.is_none()
                        {
                            // Extract original content and build replacement that preserves it
                            build_preserving_replacement_cell_xml(row, col, m, elem_text)?
                        } else {
                            build_replacement_cell_xml(row, col, m)?
                        };
                        out.push_str(&replacement);
                        i = end_of_elem;
                        continue;
                    }
                }

                // Not a target — copy verbatim.
                out.push_str(elem_text);
                i = end_of_elem;
                let _ = self_closing;
                continue;
            }
        }
        // Copy one byte and advance.
        // We try to copy runs of bytes that are not part of a <c tag.
        out.push(bytes[i] as char);
        i += 1;
    }

    // Insert any new cells (address not found in original XML) before </sheetData>.
    let appended_xml = build_appended_cells(mods, &by_addr, sheet_xml)?;
    if !appended_xml.is_empty() {
        if let Some(pos) = out.find("</sheetData>") {
            out.insert_str(pos, &appended_xml);
        } else if let Some(pos) = out.rfind("</worksheet>") {
            out.insert_str(pos, &appended_xml);
        } else {
            out.push_str(&appended_xml);
        }
    }

    Ok(out)
}

/// Find the end of the `<c ...>` element starting at index `start` (which
/// must point at the `<`). Returns (end_index, is_self_closing).
fn find_c_element_end(bytes: &[u8], start: usize) -> Result<(usize, bool), OfficeError> {
    // Phase 1: walk the opening tag until we hit `>`.
    let mut i = start + 1; // skip '<'
    let mut in_quote = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_quote {
            if b == b'"' {
                in_quote = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            in_quote = true;
            i += 1;
            continue;
        }
        if b == b'>' {
            // Look back for self-closing slash.
            let prev = if i > 0 { bytes[i - 1] } else { b' ' };
            if prev == b'/' {
                return Ok((i + 1, true));
            }
            // Otherwise we have an opening tag like `<c r="A1">`.
            // The cell element is `<c>...</c>`. We need to find the matching
            // `</c>` while correctly skipping nested tags.
            return find_matching_close_c(bytes, i + 1);
        }
        i += 1;
    }
    Err(OfficeError::Excel("unterminated <c> opening tag".to_string()))
}

/// Walk forward from `i` (which sits just past the opening `<c ...>`) to find
/// the matching `</c>`. Nested elements (`<f>`, `<v>`, `<is>`) may appear.
fn find_matching_close_c(bytes: &[u8], mut i: usize) -> Result<(usize, bool), OfficeError> {
    let mut depth: usize = 1;
    let mut in_quote = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_quote {
            if b == b'"' {
                in_quote = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => in_quote = true,
            b'<' => {
                if i + 1 >= bytes.len() {
                    return Err(OfficeError::Excel("malformed nested element".to_string()));
                }
                if bytes[i + 1] == b'/' {
                    // Closing tag — determine which element.
                    let tag_end_rel = find_subsequence(&bytes[i..], b">")
                        .ok_or_else(|| OfficeError::Excel("malformed closing tag".to_string()))?;
                    let tag_inner = &bytes[i + 2..i + tag_end_rel];
                    let tag_name = tag_inner
                        .iter()
                        .position(|&c| c == b' ' || c == b'>' || c == b'\t' || c == b'\n' || c == b'\r')
                        .map(|p| &tag_inner[..p])
                        .unwrap_or(tag_inner);
                    let advance = tag_end_rel + 1;
                    if tag_name == b"c" {
                        // Matching </c>: we're done.
                        return Ok((i + advance, false));
                    }
                    // Closing tag for a nested element. Decrement depth and continue.
                    depth -= 1;
                    if depth == 0 {
                        return Ok((i + advance, false));
                    }
                    i += advance;
                    continue;
                } else {
                    // Opening tag — advance past the `>` and bump depth.
                    depth += 1;
                    let open_end_rel = find_subsequence(&bytes[i..], b">")
                        .ok_or_else(|| OfficeError::Excel("malformed opening tag".to_string()))?;
                    i += open_end_rel + 1;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    Err(OfficeError::Excel("unterminated <c> element".to_string()))
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn extract_attr(elem_text: &str, name: &str) -> Option<String> {
    let mut search = 0;
    loop {
        let idx = elem_text[search..].find(name)?;
        let abs = search + idx;
        // Must be preceded by space.
        if abs > 0 {
            let prev = elem_text.as_bytes()[abs - 1];
            if !(prev == b' ' || prev == b'\t' || prev == b'\n' || prev == b'\r') {
                search = abs + 1;
                continue;
            }
        }
        // Find '='
        let after = &elem_text[abs + name.len()..];
        let eq_pos = after.find('=')?;
        let after_eq = &after[eq_pos + 1..];
        // Value starts with quote.
        let bytes = after_eq.as_bytes();
        if bytes.is_empty() || bytes[0] != b'"' {
            search = abs + 1;
            continue;
        }
        let end_quote = after_eq[1..].find('"')?;
        return Some(after_eq[1..1 + end_quote].to_string());
    }
}

fn build_replacement_cell_xml(
    row: usize,
    col: usize,
    m: &CellModification,
) -> Result<String, OfficeError> {
    let addr = cell_address(row, col);
    let (t_attr, body, has_value) = match (&m.new_value, &m.new_formula) {
        (Some(CellValue::Empty), None) => (None, String::new(), false),
        (None, Some(f)) => {
            let body = format!("<f>{}</f>", escape_xml_text(f));
            (None, body, true)
        }
        (Some(CellValue::String(s)), None) if s.is_empty() => (None, String::new(), false),
        (Some(value), None) => {
            let (t, body_inner) = value_to_xml_body(value);
            // Numeric/date/error cells store their payload inside <v>...</v>;
            // string cells embed it inside <is><t>...</t></is>.
            let body = match value {
                CellValue::String(_) => body_inner,
                _ => format!("<v>{}</v>", body_inner),
            };
            (Some(t), body, true)
        }
        (Some(value), Some(f)) => {
            let (_, value_body_inner) = value_to_xml_body(value);
            let value_body = match value {
                CellValue::String(_) => value_body_inner,
                _ => format!("<v>{}</v>", value_body_inner),
            };
            let body = format!("<f>{}</f>{}", escape_xml_text(f), value_body);
            (None, body, true)
        }
        (None, None) => (None, String::new(), false),
    };

    let mut attrs = format!("r=\"{}\"", addr);
    if let Some(ref t) = t_attr {
        if !t.is_empty() {
            attrs.push_str(&format!(" t=\"{}\"", t));
        }
    }
    if let Some(fmt) = &m.new_number_format {
        // The style index is resolved later via StylesDocument; for the
        // conservative write, we emit a sentinel `s` attribute that the
        // caller is expected to manage. To keep this self-contained we
        // assign s="0" and document that style-update is best-effort.
        let _ = fmt;
        attrs.push_str(" s=\"0\"");
    }

    if !has_value {
        Ok(format!("<c {}/>", attrs))
    } else {
        Ok(format!("<c {}>{}</c>", attrs, body))
    }
}

/// Build replacement cell XML that preserves the original cell's value and formula,
/// but applies the new style. Used when only style properties are being changed.
fn build_preserving_replacement_cell_xml(
    row: usize,
    col: usize,
    _m: &CellModification,
    original_elem: &str,
) -> Result<String, OfficeError> {
    let addr = cell_address(row, col);

    // Extract the original t attribute (cell type) if present
    let orig_t_attr = extract_attr(original_elem, "t");

    // Extract original style index (s attribute) if present
    let orig_s_attr = extract_attr(original_elem, "s");

    // Extract content inside the cell element (formula, value, etc.)
    // We need to preserve the entire inner content as-is
    let content = if let Some(open_end) = original_elem.find('>') {
        if let Some(close_start) = original_elem.find("</c>") {
            let inner = &original_elem[open_end + 1..close_start];
            if !inner.is_empty() {
                Some(inner.to_string())
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Build attributes
    let mut attrs = format!("r=\"{}\"", addr);

    // Use style index if available
    if let Some(ref s) = orig_s_attr {
        attrs.push_str(&format!(" s=\"{}\"", s));
    }

    // For type attribute, preserve it for all cell types that need it
    // (s=shared string, b=boolean, e=error, inlineStr)
    if let Some(ref t) = orig_t_attr {
        if t == "s" || t == "b" || t == "e" || t == "inlineStr" {
            attrs.push_str(&format!(" t=\"{}\"", t));
        }
    }

    // Build body with preserved content
    let body = if let Some(inner) = content {
        // Preserve the entire inner content as-is
        inner
    } else {
        String::new()
    };

    if body.is_empty() {
        Ok(format!("<c {}/>", attrs))
    } else {
        Ok(format!("<c {}>{}</c>", attrs, body))
    }
}

/// Find the end position of an XML tag (position of >)
fn find_tag_end(slice: &str) -> Option<usize> {
    let bytes = slice.as_bytes();
    let mut i = 0;
    let mut in_attr = false;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            in_attr = !in_attr;
        } else if bytes[i] == b'>' && !in_attr {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

/// Extract content between opening and closing tags
fn extract_tag_content(tag: &str, tag_name: &str) -> Option<String> {
    let start_tag = format!("<{}", tag_name);
    let end_tag = format!("</{}>", tag_name);

    if let Some(content_start) = tag.find(&start_tag) {
        let after_open = content_start + start_tag.len();
        // Skip to after >
        if let Some(gt_pos) = tag[after_open..].find('>') {
            let content_start_pos = after_open + gt_pos + 1;
            if let Some(end_pos) = tag[content_start_pos..].find(&end_tag) {
                return Some(tag[content_start_pos..content_start_pos + end_pos].to_string());
            }
        }
    }
    None
}

fn value_to_xml_body(value: &CellValue) -> (String, String) {
    match value {
        CellValue::Empty => (String::new(), String::new()),
        CellValue::Int(n) => ("n".to_string(), n.to_string()),
        CellValue::Float(f) => (
            "n".to_string(),
            if f.is_finite() {
                f.to_string()
            } else {
                "0".to_string()
            },
        ),
        CellValue::Bool(b) => ("b".to_string(), if *b { "1".to_string() } else { "0".to_string() }),
        CellValue::String(s) => (
            "inlineStr".to_string(),
            format!("<is><t>{}</t></is>", escape_xml_text(s)),
        ),
        CellValue::Error(e) => ("e".to_string(), e.clone()),
        CellValue::DateTime(dt) => ("n".to_string(), dt.to_string()),
    }
}

fn build_appended_cells(
    mods: &[&CellModification],
    by_addr: &HashMap<String, &CellModification>,
    sheet_xml: &str,
) -> Result<String, OfficeError> {
    let mut out = String::new();
    // Addresses already present in the XML (matched against existing r="..." attrs).
    let existing: std::collections::HashSet<String> = collect_existing_addrs(sheet_xml);
    for m in mods {
        if existing.contains(&m.address.to_ascii_uppercase()) {
            continue;
        }
        // Only append when there's something to set.
        if m.new_value.is_none() && m.new_formula.is_none() {
            continue;
        }
        let upper = m.address.to_ascii_uppercase();
        let (row, col) = parse_cell_address(&upper).ok_or_else(|| {
            OfficeError::Excel(format!("invalid cell address: {}", m.address))
        })?;
        let _ = by_addr;
        out.push_str(&build_replacement_cell_xml(row, col, m)?);
        out.push('\n');
    }
    Ok(out)
}

fn collect_existing_addrs(xml: &str) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let bytes = xml.as_bytes();
    let needle = b"r=\"";
    let mut i = 0;
    while i + needle.len() < bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            // Skip attribute prefix: must be preceded by space within <c ...>
            let abs = i;
            if abs == 0 || !matches!(bytes[abs - 1], b' ' | b'\t' | b'\n' | b'\r') {
                i += 1;
                continue;
            }
            let start = i + needle.len();
            if let Some(end_rel) = xml[start..].find('"') {
                let addr = &xml[start..start + end_rel];
                set.insert(addr.to_ascii_uppercase());
                i = start + end_rel + 1;
                continue;
            }
        }
        i += 1;
    }
    set
}

fn escape_xml_text(s: &str) -> String {
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
fn parse_sheet_name_to_path(
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

// ─── Workbook creation (from scratch) ────────────────────────────────────────

/// Create a new xlsx file from a [`XlsxWorkbook`] specification. Builds a
/// minimal but valid OOXML package from scratch:
/// - `[Content_Types].xml` registers the workbook part.
/// - `xl/workbook.xml` declares the sheets.
/// - `xl/_rels/workbook.xml.rels` maps sheet rIds to file paths.
/// - `xl/sharedStrings.xml` is emitted only if at least one sheet uses
///   shared-string references; otherwise cells use inline strings.
/// - `xl/styles.xml` contains the minimum font/fill/border entries plus
///   the cellXfs entries referenced by cells.
/// - `xl/worksheets/sheetN.xml` contains the actual cell data.
///
/// String-typed cells are written as inline strings (`<is><t>...</t></is>`)
/// so we never need to maintain a shared string pool. Numeric and date cells
/// are written as `<c><v>numeric</v></c>`. This keeps the emitted xlsx
/// dependency-free (no separate sst update step) and means the round-trip
/// parser we already have can re-read what we wrote.
pub fn create_xlsx_workbook(
    workbook: &XlsxWorkbook,
    output_path: &std::path::Path,
) -> Result<(), OfficeError> {
    use std::io::Write as _;

    if workbook.sheets.is_empty() {
        return Err(OfficeError::Excel("cannot create workbook with zero sheets".to_string()));
    }

    let file = std::fs::File::create(output_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let n_sheets = workbook.sheets.len();

    // 1. [Content_Types].xml — must enumerate EVERY part with an Override.
    let mut content_types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>"#,
    );
    for i in 0..n_sheets {
        content_types.push_str(&format!(
            "<Override PartName=\"/xl/worksheets/sheet{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>",
            i + 1
        ));
    }
    content_types.push_str(
        "<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>\
<Override PartName=\"/xl/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>\
<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\
<Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>\
</Types>",
    );
    zip.start_file("[Content_Types].xml", opts)?;
    zip.write_all(content_types.as_bytes())?;

    // 2. _rels/.rels — top-level relationships, mapping the package to its
    //    main document part. Excel REQUIRES this to find xl/workbook.xml.
    let top_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/>
</Relationships>"#;
    zip.start_file("_rels/.rels", opts)?;
    zip.write_all(top_rels.as_bytes())?;

    // 3. docProps/core.xml — minimal core properties.
    let core_props = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
<dc:creator>inkuo</dc:creator>
<cp:lastModifiedBy>inkuo</cp:lastModifiedBy>
<dcterms:created xsi:type="dcterms:W3CDTF">2024-01-01T00:00:00Z</dcterms:created>
<dcterms:modified xsi:type="dcterms:W3CDTF">2024-01-01T00:00:00Z</dcterms:modified>
</cp:coreProperties>"#;
    zip.start_file("docProps/core.xml", opts)?;
    zip.write_all(core_props.as_bytes())?;

    // 4. docProps/app.xml — minimal extended properties.
    let app_props = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">
<Application>inkuo</Application>
<DocSecurity>0</DocSecurity>
<ScaleCrop>false</ScaleCrop>
<LinksUpToDate>false</LinksUpToDate>
<SharedDoc>false</SharedDoc>
<HyperlinksChanged>false</HyperlinksChanged>
<AppVersion>16.0000</AppVersion>
</Properties>"#;
    zip.start_file("docProps/app.xml", opts)?;
    zip.write_all(app_props.as_bytes())?;

    // 5. xl/workbook.xml — declare each sheet and reference the relationships
    //    namespace. The `r:` prefix is declared on the root <workbook> element
    //    and used by the <sheet> children for r:id="...". The `state="visible"`
    //    attribute is required by the spec; some readers reject sheets without it.
    let mut workbook_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<workbookPr/><bookViews><workbookView activeTab="0" firstSheet="0" showHorizontalScroll="1" showVerticalScroll="1" showSheetTabs="1" tabRatio="600" windowHeight="10000" windowWidth="20000"/></bookViews>
<sheets>"#,
    );
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        let sheet_id = (i + 1) as u32;
        let rid = format!("rId{}", i + 1);
        let state = if sheet.state.is_empty() { "visible" } else { &sheet.state };
        workbook_xml.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{}\" state=\"{}\" r:id=\"{}\"/>",
            escape_xml_attr(&sheet.name),
            sheet_id,
            escape_xml_attr(state),
            rid
        ));
    }
    workbook_xml.push_str("</sheets><calcPr calcId=\"124519\"/></workbook>");
    zip.start_file("xl/workbook.xml", opts)?;
    zip.write_all(workbook_xml.as_bytes())?;

    // 6. xl/_rels/workbook.xml.rels — maps each sheet rId to its worksheet
    //    file, and adds relationships for the theme and styles.
    let mut rels_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for i in 0..n_sheets {
        rels_xml.push_str(&format!(
            "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{}.xml\"/>",
            i + 1,
            i + 1
        ));
    }
    rels_xml.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>",
        n_sheets + 1
    ));
    rels_xml.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"theme/theme1.xml\"/>",
        n_sheets + 2
    ));
    rels_xml.push_str("</Relationships>");
    zip.start_file("xl/_rels/workbook.xml.rels", opts)?;
    zip.write_all(rels_xml.as_bytes())?;

    // 7. xl/styles.xml — rebuilt from actual used styles.
    let (styles_xml, all_style_map) = build_workbook_styles(workbook);
    zip.start_file("xl/styles.xml", opts)?;
    zip.write_all(styles_xml.as_bytes())?;

    // 8. xl/theme/theme1.xml — a minimal Office theme. Excel doesn't strictly
    //    require this, but readers that load the relationship from workbook.xml
    //    WILL try to fetch it. Without a theme file, the file fails to open.
    zip.start_file("xl/theme/theme1.xml", opts)?;
    zip.write_all(MINIMAL_THEME_XML.as_bytes())?;

    // 9. xl/worksheets/sheetN.xml — one per sheet.
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        let sheet_xml = build_sheet_xml(sheet, &all_style_map[i]);
        let path = format!("xl/worksheets/sheet{}.xml", i + 1);
        zip.start_file(&path, opts)?;
        zip.write_all(sheet_xml.as_bytes())?;
    }

    zip.finish()?;
    Ok(())
}

/// Write an [`XlsxWorkbook`] to a file, preserving all original ZIP entries that
/// are not being regenerated.
///
/// This is the structured equivalent of the old string-based `incremental_write_xlsx`.
/// If `original_bytes` is `Some`, we copy every entry from the original zip and
/// only overwrite `xl/worksheets/sheet*.xml` (and `xl/styles.xml` if modified).
/// If `original_bytes` is `None`, we fall back to `create_xlsx_workbook` behavior
/// (generate everything from scratch).
pub fn write_excel_document(
    workbook: &XlsxWorkbook,
    original_bytes: Option<&[u8]>,
    output_path: &std::path::Path,
) -> Result<(), OfficeError> {
    use std::io::{Read, Write as _};

    if workbook.sheets.is_empty() {
        return Err(OfficeError::Excel("cannot write workbook with zero sheets".to_string()));
    }

    // If no original bytes, delegate entirely to create_xlsx_workbook.
    let Some(bytes) = original_bytes else {
        return create_xlsx_workbook(workbook, output_path);
    };

    // Collect original ZIP entries we'll copy verbatim (everything except sheet XMLs).
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec()))?;
    let mut preserved_entries: Vec<(String, Vec<u8>)> = Vec::new();

    // Read workbook.xml + rels to get sheet name -> path mapping.
    let wb_xml = read_entry(&mut archive, "xl/workbook.xml").unwrap_or_default();
    let wb_rels = read_entry(&mut archive, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let _name_to_path: std::collections::HashMap<String, String> =
        parse_sheet_name_to_path_map(&wb_xml, &wb_rels)
            .unwrap_or_default();

    // Collect entries to preserve (everything except xl/worksheets/ and entries we'll regenerate below).
    // Skip: [Content_Types].xml, _rels/.rels, docProps/*.xml, xl/workbook.xml,
    // xl/_rels/workbook.xml.rels, xl/styles.xml, xl/theme/theme1.xml
    // (those are regenerated in steps 2-9 below to reflect new state).
    let regenerated: std::collections::HashSet<&'static str> = [
        "[Content_Types].xml",
        "_rels/.rels",
        "docProps/core.xml",
        "docProps/app.xml",
        "xl/workbook.xml",
        "xl/_rels/workbook.xml.rels",
        "xl/styles.xml",
        "xl/theme/theme1.xml",
    ].into_iter().collect();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if !name.starts_with("xl/worksheets/") && !regenerated.contains(name.as_str()) {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            preserved_entries.push((name, buf));
        }
    }
    drop(archive);

    // Open the output file and write the new ZIP.
    let file = std::fs::File::create(output_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    // 1. Copy preserved entries.
    for (name, buf) in preserved_entries {
        zip.start_file(&name, opts)?;
        zip.write_all(&buf)?;
    }

    // 2. [Content_Types].xml — regenerated to list all sheets.
    let n_sheets = workbook.sheets.len();
    let mut content_types = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>
<Override PartName="/xl/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
<Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>"#,
    );
    for i in 1..=n_sheets {
        content_types.push_str(&format!(
            "<Override PartName=\"/xl/worksheets/sheet{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>",
            i
        ));
    }
    content_types.push_str("</Types>");
    zip.start_file("[Content_Types].xml", opts)?;
    zip.write_all(content_types.as_bytes())?;

    // 3. _rels/.rels
    let top_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/>
</Relationships>"#;
    zip.start_file("_rels/.rels", opts)?;
    zip.write_all(top_rels.as_bytes())?;

    // 4. docProps/core.xml
    let core_props = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
<dc:creator>inkuo</dc:creator>
<cp:lastModifiedBy>inkuo</cp:lastModifiedBy>
<dcterms:created xsi:type="dcterms:W3CDTF">2024-01-01T00:00:00Z</dcterms:created>
<dcterms:modified xsi:type="dcterms:W3CDTF">2024-01-01T00:00:00Z</dcterms:modified>
</cp:coreProperties>"#;
    zip.start_file("docProps/core.xml", opts)?;
    zip.write_all(core_props.as_bytes())?;

    // 5. docProps/app.xml
    let app_props = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">
<Application>inkuo</Application>
<DocSecurity>0</DocSecurity>
<ScaleCrop>false</ScaleCrop>
<LinksUpToDate>false</LinksUpToDate>
<SharedDoc>false</SharedDoc>
<HyperlinksChanged>false</HyperlinksChanged>
<AppVersion>16.0000</AppVersion>
</Properties>"#;
    zip.start_file("docProps/app.xml", opts)?;
    zip.write_all(app_props.as_bytes())?;

    // 6. xl/workbook.xml — regenerated to match new sheet names/order.
    let mut workbook_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<workbookPr/><bookViews><workbookView activeTab="0" firstSheet="0" showHorizontalScroll="1" showVerticalScroll="1" showSheetTabs="1" tabRatio="600" windowHeight="10000" windowWidth="20000"/></bookViews>
<sheets>"#,
    );
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        let sheet_id = (i + 1) as u32;
        let rid = format!("rId{}", i + 1);
        let state = if sheet.state.is_empty() { "visible" } else { &sheet.state };
        workbook_xml.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{}\" state=\"{}\" r:id=\"{}\"/>",
            escape_xml_attr(&sheet.name),
            sheet_id,
            escape_xml_attr(state),
            rid
        ));
    }
    workbook_xml.push_str("</sheets><calcPr calcId=\"124519\"/></workbook>");
    zip.start_file("xl/workbook.xml", opts)?;
    zip.write_all(workbook_xml.as_bytes())?;

    // 7. xl/_rels/workbook.xml.rels — regenerated.
    let mut rels_xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for i in 0..n_sheets {
        rels_xml.push_str(&format!(
            "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{}.xml\"/>",
            i + 1,
            i + 1
        ));
    }
    rels_xml.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>",
        n_sheets + 1
    ));
    rels_xml.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"theme/theme1.xml\"/>",
        n_sheets + 2
    ));
    rels_xml.push_str("</Relationships>");
    zip.start_file("xl/_rels/workbook.xml.rels", opts)?;
    zip.write_all(rels_xml.as_bytes())?;

    // 8. xl/styles.xml — rebuilt from actual used styles.
    let (styles_xml, all_style_map) = build_workbook_styles(workbook);
    zip.start_file("xl/styles.xml", opts)?;
    zip.write_all(styles_xml.as_bytes())?;

    // 9. xl/theme/theme1.xml
    zip.start_file("xl/theme/theme1.xml", opts)?;
    zip.write_all(MINIMAL_THEME_XML.as_bytes())?;

    // 10. xl/worksheets/sheetN.xml — write each sheet's structured XML.
    for (i, sheet) in workbook.sheets.iter().enumerate() {
        let sheet_xml = build_sheet_xml(sheet, &all_style_map[i]);
        let path = format!("xl/worksheets/sheet{}.xml", i + 1);
        zip.start_file(&path, opts)?;
        zip.write_all(sheet_xml.as_bytes())?;
    }

    zip.finish()?;
    Ok(())
}

/// Parse sheet names to file paths from workbook.xml and its relationships.
/// Returns a HashMap for O(1) lookup.
fn parse_sheet_name_to_path_map(
    workbook_xml: &str,
    rels_xml: &str,
) -> Result<std::collections::HashMap<String, String>, OfficeError> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut name_to_path = std::collections::HashMap::new();

    // Parse rels: rId -> target path
    let mut rid_to_path = std::collections::HashMap::new();
    let mut rels_reader = Reader::from_str(rels_xml);
    rels_reader.config_mut().trim_text(true);
    let mut rels_buf = Vec::new();
    loop {
        match rels_reader.read_event_into(&mut rels_buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                if e.local_name().as_ref() == b"Relationship" {
                    let mut rid = None;
                    let mut target = None;
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"Id" => rid = Some(String::from_utf8_lossy(&attr.value).to_string()),
                            b"Target" => {
                                target = Some(String::from_utf8_lossy(&attr.value).to_string())
                            }
                            _ => {}
                        }
                    }
                    if let (Some(r), Some(t)) = (rid, target) {
                        rid_to_path.insert(r, t);
                    }
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        rels_buf.clear();
    }

    // Parse workbook: sheet name -> rId
    let mut wb_reader = Reader::from_str(workbook_xml);
    wb_reader.config_mut().trim_text(true);
    let mut wb_buf = Vec::new();
    loop {
        match wb_reader.read_event_into(&mut wb_buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                if e.local_name().as_ref() == b"sheet" {
                    let mut name = None;
                    let mut rid = None;
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"name" => {
                                name = Some(String::from_utf8_lossy(&attr.value).to_string())
                            }
                            b"r:id" => rid = Some(String::from_utf8_lossy(&attr.value).to_string()),
                            _ => {}
                        }
                    }
                    if let (Some(n), Some(r)) = (name, rid) {
                        if let Some(path) = rid_to_path.get(&r) {
                            name_to_path.insert(n, format!("xl/{}", path));
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            _ => {}
        }
        rels_buf.clear();
    }

    Ok(name_to_path)
}

/// Serialize a single sheet to its worksheet XML. Cells are written inline
/// (numeric as `<v>`, strings as `<is><t>`), and rows are emitted in row-order
/// so the file is consumable by every spreadsheet application.
fn build_workbook_styles(workbook: &XlsxWorkbook) -> (String, Vec<std::collections::HashMap<(usize, usize), usize>>) {
    // CRITICAL: `used_styles` MUST be ordered by `idx` (the cellXfs index).
    // The write path writes cellXfs[1..] in `used_styles` iteration order, and
    // the sheet XML references each cell's style by that same `idx`. If we used
    // a HashMap here the cellXfs order would be randomised and cell styles
    // would land on the wrong cells (e.g. A7 written as #1F3864 read back as
    // the #548235 written to B3). Use an ordered vec with linear lookup for
    // dedup; the style count is small (hundreds at most) so this is fine.
    let mut used_styles: Vec<(SheetStyleKey, usize)> = Vec::new();
    let mut key_to_idx: std::collections::HashMap<SheetStyleKey, usize> = std::collections::HashMap::new();
    let mut next_idx: usize = 1;

    let mut per_sheet: Vec<std::collections::HashMap<(usize, usize), usize>> = Vec::new();
    for sheet in &workbook.sheets {
        let mut sheet_map: std::collections::HashMap<(usize, usize), usize> = std::collections::HashMap::new();
        for cell in &sheet.cells {
            if let Some(style) = &cell.style {
                let key = SheetStyleKey::from(style);
                let idx = if let Some(&i) = key_to_idx.get(&key) {
                    i
                } else {
                    let i = next_idx;
                    next_idx += 1;
                    key_to_idx.insert(key.clone(), i);
                    used_styles.push((key, i));
                    i
                };
                sheet_map.insert((cell.row, cell.col), idx);
            }
        }
        per_sheet.push(sheet_map);
    }

    // `used_styles` is appended in the same order `idx` is assigned, so its
    // iteration order matches the cellXfs order the serializer produces.
    let styles_xml = build_styles_xml(&used_styles);
    (styles_xml, per_sheet)
}

fn build_sheet_xml(sheet: &XlsxSheet, style_map: &std::collections::HashMap<(usize, usize), usize>) -> String {
    // Group cells by row for the row-major layout that xlsx requires.
    let mut by_row: HashMap<usize, Vec<&Cell>> = HashMap::new();
    for cell in &sheet.cells {
        by_row.entry(cell.row).or_default().push(cell);
    }
    let mut row_indices: Vec<usize> = by_row.keys().copied().collect();
    row_indices.sort();

    // Compute the dimension (A1-style) covering the populated cells. We need
    // this for Excel/LibreOffice — they expect <dimension ref="..."/> near
    // the top of the worksheet. If the sheet is empty, we still emit "A1".
    let max_row = (sheet.max_row).max(if row_indices.is_empty() { 0 } else { *row_indices.last().unwrap() + 1 });
    let max_col = sheet.max_col.max(1);
    let dim_ref = if sheet.cells.is_empty() {
        "A1".to_string()
    } else {
        format!("A1:{}", cell_address(max_row.saturating_sub(1), max_col.saturating_sub(1)))
    };

    if DEBUG_XLSX {
        eprintln!("[xlsx] create_xlsx_workbook: sheet={}, cells={}, merged={}", sheet.name, sheet.cells.len(), sheet.merged_cells.len());
        eprintln!("[xlsx] create_xlsx_workbook: row_heights={:?}", sheet.row_heights);
        eprintln!("[xlsx] create_xlsx_workbook: col_widths={:?}", sheet.col_widths);
    }

    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
    );
    xml.push_str(&format!("<dimension ref=\"{}\"/>", dim_ref));
    xml.push_str("<sheetViews><sheetView workbookViewId=\"0\"><selection activeCell=\"A1\" sqref=\"A1\"/></sheetView></sheetViews>");

    // Add column definitions if we have custom widths
    if !sheet.col_widths.is_empty() {
        xml.push_str("<cols>");
        for (col_idx, width) in &sheet.col_widths {
            xml.push_str(&format!("<col min=\"{}\" max=\"{}\" width=\"{}\" customWidth=\"1\"/>",
                col_idx + 1, col_idx + 1, width));
        }
        xml.push_str("</cols>");
    } else {
        xml.push_str("<sheetFormatPr baseColWidth=\"8\" defaultRowHeight=\"15\"/>");
    }

    xml.push_str("<sheetData>");

    for row in &row_indices {
        let mut cells = by_row.remove(row).unwrap_or_default();
        cells.sort_by_key(|c| c.col);

        // Check if this row has a custom height
        let row_height = sheet.row_heights.get(row);
        if row_height.is_some() || !cells.is_empty() {
            let ht_attr = row_height.map(|h| format!(" ht=\"{}\"", h)).unwrap_or_default();
            let custom_attr = if row_height.is_some() { " customHeight=\"1\"" } else { "" };
            xml.push_str(&format!("<row r=\"{}\"{}{}>", row + 1, ht_attr, custom_attr));
        }

        for cell in &cells {
            let style_index = style_map.get(&(cell.row, cell.col)).copied().unwrap_or(0);
            xml.push_str(&build_cell_xml(cell, style_index));
        }

        if row_height.is_some() || !cells.is_empty() {
            xml.push_str("</row>");
        }
    }
    xml.push_str("</sheetData>");

    if !sheet.merged_cells.is_empty() {
        xml.push_str(&format!("<mergeCells count=\"{}\">", sheet.merged_cells.len()));
        for m in &sheet.merged_cells {
            xml.push_str(&format!("<mergeCell ref=\"{}\"/>", m.address()));
        }
        xml.push_str("</mergeCells>");
    }

    // <pageMargins> is required; readers complain if it's missing. We use the
    // standard 0.75/0.75/1/1/0.5/0.5 defaults.
    xml.push_str("<pageMargins left=\"0.75\" right=\"0.75\" top=\"1\" bottom=\"1\" header=\"0.5\" footer=\"0.5\"/>");
    xml.push_str("</worksheet>");
    xml
}

fn build_cell_xml(cell: &Cell, style_index: usize) -> String {
    let addr = cell.address();
    let mut attrs = format!("r=\"{}\"", addr);

    // Style index.
    attrs.push_str(" s=\"");
    attrs.push_str(&style_index.to_string());
    attrs.push('"');

    // Build the inner body (everything between <c ...> and </c>). The body
    // may be empty for a self-closing <c .../> placeholder.
    let (body, self_closing) = match (&cell.formula, &cell.value) {
        (Some(f), _) => {
            // Formula present — write <f> and, if there's a cached value, a <v>.
            let f_xml = format!("<f>{}</f>", escape_xml_text(f));
            let v_xml = match cell.value {
                CellValue::Empty => String::new(),
                CellValue::Int(n) => format!("<v>{}</v>", n),
                CellValue::Float(f) => format!("<v>{}</v>", f),
                CellValue::Bool(b) => {
                    attrs.push_str(" t=\"b\"");
                    format!("<v>{}</v>", if b { 1 } else { 0 })
                }
                CellValue::String(ref s) => {
                    // Cached string result of a formula: use t="str" and put the
                    // text directly in <v>.
                    attrs.push_str(" t=\"str\"");
                    format!("<v>{}</v>", escape_xml_text(s))
                }
                CellValue::Error(ref e) => {
                    attrs.push_str(" t=\"e\"");
                    format!("<v>{}</v>", escape_xml_text(e))
                }
                CellValue::DateTime(dt) => format!("<v>{}</v>", dt),
            };
            (format!("{}{}", f_xml, v_xml), false)
        }
        (None, CellValue::Empty) => {
            // No formula and no value — emit a self-closing placeholder.
            return format!("<c {}/>", attrs);
        }
        (None, CellValue::Int(n)) => (format!("<v>{}</v>", n), false),
        (None, CellValue::Float(f)) => {
            let v = if f.is_finite() { f.to_string() } else { "0".to_string() };
            (format!("<v>{}</v>", v), false)
        }
        (None, CellValue::Bool(b)) => {
            attrs.push_str(" t=\"b\"");
            let v = if *b { 1 } else { 0 };
            (format!("<v>{}</v>", v), false)
        }
        (None, CellValue::String(s)) => {
            attrs.push_str(" t=\"inlineStr\"");
            (format!("<is><t>{}</t></is>", escape_xml_text(&s)), false)
        }
        (None, CellValue::Error(e)) => {
            attrs.push_str(" t=\"e\"");
            (format!("<v>{}</v>", escape_xml_text(&e)), false)
        }
        (None, CellValue::DateTime(dt)) => (format!("<v>{}</v>", dt), false),
    };

    if self_closing {
        format!("<c {}/>", attrs)
    } else {
        format!("<c {}>{}</c>", attrs, body)
    }
}

fn escape_xml_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

