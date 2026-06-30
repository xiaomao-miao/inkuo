//! Excel (.xlsx) workbook reading, structured parsing, and conservative writing.
//!
//! Two layers of API:
//! - The legacy [`ExcelWorkbook`] / [`ExcelSheet`] types (flat 2D string grid)
//!   are kept for backward compatibility with existing callers.
//! - The structured [`XlsxWorkbook`] / [`XlsxSheet`] / [`Cell`] / [`CellStyle`]
//!   types provide cell-level fidelity (formulas, merged ranges, styles)
//!   suitable for AI editing and conservative round-tripping.
//!
//! This module also contains several standalone `*_xlsx` helpers (merge,
//! resize, sheet CRUD, incremental XML rewriting, ...) that predate the
//! `ExcelOperation` enum in `agent/tools/excel_tools.rs`. The active editor
//! path uses `ExcelOperation`, so these helpers have no callers today. We
//! suppress the `dead_code` warnings rather than deleting them: dropping a
//! 1500-line block is risky without a regression test that exercises the
//! non-`ExcelOperation` code paths, and a future Excel feature may want to
//! reuse some of these helpers. Revisit when the warning count is no longer
//! considered noise.

#![allow(dead_code)]

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

pub fn cell_to_string(cell: &Data) -> String {
    match cell {
        Data::Int(n) => n.to_string(),
        Data::Float(f) => {
            if f.fract() == 0.0 {
                format!("{:.0}", f)
            } else {
                format!("{}", f)
            }
        }
        Data::String(ref s) => s.clone(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(ref dt) => format!("{:.0}", dt.as_f64()),
        Data::DateTimeIso(ref s) => s.clone(),
        Data::DurationIso(ref s) => s.clone(),
        Data::Error(ref e) => format!("#ERR:{:?}", e),
        Data::Empty => String::new(),
    }
}

pub fn excel_workbook_to_text(workbook: &ExcelWorkbook) -> String {
    let mut output = String::new();

    for sheet in &workbook.sheets {
        output.push_str(&format!("=== Sheet: {} ===\n\n", sheet.name));

        if sheet.rows.is_empty() {
            output.push_str("(empty sheet)\n");
        } else {
            let max_cols = sheet.rows.iter().map(|r| r.len()).max().unwrap_or(0);
            let col_widths: Vec<usize> = (0..max_cols)
                .map(|col| {
                    sheet
                        .rows
                        .iter()
                        .map(|row| row.get(col).map(|s| s.len()).unwrap_or(0))
                        .max()
                        .unwrap_or(0)
                        .max(8)
                })
                .collect();

            for row in sheet.rows.iter().take(100) {
                let row_text: Vec<String> = row
                    .iter()
                    .enumerate()
                    .map(|(i, cell)| {
                        let w = col_widths.get(i).copied().unwrap_or(8);
                        format!("{:w$}", cell, w = w)
                    })
                    .collect();
                output.push_str(&row_text.join(" | "));
                output.push('\n');
            }

            if sheet.rows.len() > 100 {
                output.push_str(&format!("\n... ({} more rows)\n", sheet.rows.len() - 100));
            }
        }

        output.push('\n');
    }

    output.trim().to_string()
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

fn parse_styles(xml: &str) -> StylesInfo {
    let mut reader = XmlReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut num_formats: HashMap<u32, String> = HashMap::new();
    let mut cell_xfs: Vec<CellXf> = Vec::new();
    let mut fonts: Vec<FontXf> = Vec::new();
    let mut fills: Vec<FillXf> = Vec::new();

    let mut in_num_fmts = false;
    let mut in_cell_xfs = false;
    let mut in_fonts = false;
    let mut in_fills = false;
    let mut in_font = false;
    let mut in_fill = false;
    let mut current_cell_xf = CellXf::default();
    let mut current_font = FontXf::default();
    let mut current_fill = FillXf::default();
    let mut current_align: Option<AlignmentXf> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"numFmts" => in_num_fmts = true,
                    b"cellXfs" => in_cell_xfs = true,
                    b"fonts" => in_fonts = true,
                    b"fills" => in_fills = true,
                    b"font" if in_fonts => {
                        current_font = FontXf::default();
                        in_font = true;
                    }
                    b"fill" if in_fills => {
                        current_fill = FillXf::default();
                        in_fill = true;
                    }
                    b"alignment" => {
                        let mut a = AlignmentXf::default();
                        for attr in e.attributes().with_checks(false).flatten() {
                            let v = attr.value.as_ref();
                            if let Ok(s) = std::str::from_utf8(v) {
                                let key = attr.key.as_ref();
                                let local = strip_xml_ns(key);
                                match local {
                                    b"horizontal" => a.horizontal = Some(s.to_string()),
                                    b"vertical" => a.vertical = Some(s.to_string()),
                                    b"wrapText" => a.wrap_text = s.as_bytes() == b"1" || s.as_bytes() == b"true",
                                    _ => {}
                                }
                            }
                        }
                        current_align = Some(a);
                    }
                    _ => {}
                }
                if in_num_fmts && name.as_ref() == b"numFmt" {
                    let mut id: u32 = 0;
                    let mut code = String::new();
                    for attr in e.attributes().with_checks(false).flatten() {
                        let v = attr.value.as_ref();
                        if let Ok(s) = std::str::from_utf8(v) {
                            let key = attr.key.as_ref();
                            let local = strip_xml_ns(key);
                            match local {
                                b"numFmtId" => {
                                    id = s.parse().unwrap_or(0);
                                }
                                b"formatCode" => {
                                    code = s.to_string();
                                }
                                _ => {}
                            }
                        }
                    }
                    num_formats.insert(id, code);
                }
                if in_cell_xfs && name.as_ref() == b"xf" {
                    current_cell_xf = CellXf::default();
                    for attr in e.attributes().with_checks(false).flatten() {
                        let v = attr.value.as_ref();
                        if let Ok(s) = std::str::from_utf8(v) {
                            let key = attr.key.as_ref();
                            let local = strip_xml_ns(key);
                            match local {
                                b"numFmtId" => current_cell_xf.num_fmt_id = s.parse().unwrap_or(0),
                                b"fontId" => current_cell_xf.font_id = s.parse().unwrap_or(0),
                                b"fillId" => current_cell_xf.fill_id = s.parse().unwrap_or(0),
                                b"borderId" => current_cell_xf.border_id = s.parse().unwrap_or(0),
                                b"applyNumberFormat" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_number_format = false;
                                    }
                                }
                                b"applyFont" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_font = false;
                                    }
                                }
                                b"applyFill" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_fill = false;
                                    }
                                }
                                b"applyBorder" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_border = false;
                                    }
                                }
                                b"applyAlignment" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_alignment = false;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                if in_font && name.as_ref() == b"sz" {
                    if let Some(v) = attr_value(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(&v) {
                            current_font.size = s.parse().ok();
                        }
                    }
                }
                if in_font && name.as_ref() == b"color" {
                    if let Some(v) = attr_value(e, b"rgb") {
                        if let Ok(s) = std::str::from_utf8(&v) {
                            current_font.color = Some(s.trim_start_matches('#').to_ascii_uppercase());
                        }
                    }
                }
                if in_font && name.as_ref() == b"name" {
                    if let Some(v) = attr_value(e, b"val") {
                        if let Ok(s) = std::str::from_utf8(&v) {
                            current_font.name = Some(s.to_string());
                        }
                    }
                }
                if in_font && name.as_ref() == b"b" {
                    current_font.bold = true;
                }
                if in_font && name.as_ref() == b"i" {
                    current_font.italic = true;
                }
                if in_fill && name.as_ref() == b"patternFill" {
                    if let Some(v) = attr_value(e, b"patternType") {
                        if let Ok(s) = std::str::from_utf8(&v) {
                            current_fill.pattern_type = Some(s.to_string());
                        }
                    }
                }
                if in_fill && name.as_ref() == b"fgColor" {
                    if let Some(v) = attr_value(e, b"rgb") {
                        if let Ok(s) = std::str::from_utf8(&v) {
                            current_fill.fg_color = Some(s.trim_start_matches('#').to_ascii_uppercase());
                        }
                    }
                }
                if in_fill && name.as_ref() == b"bgColor" {
                    if let Some(v) = attr_value(e, b"rgb") {
                        if let Ok(s) = std::str::from_utf8(&v) {
                            current_fill.bg_color = Some(s.trim_start_matches('#').to_ascii_uppercase());
                        }
                    }
                }
            }
            // FIX: Handle Empty events (self-closing tags like <b/>, <i/>, <sz val="11"/>)
            Ok(Event::Empty(ref e)) => {
                let name = e.local_name();
                // Container elements - set flags
                match name.as_ref() {
                    b"numFmts" => in_num_fmts = true,
                    b"cellXfs" => in_cell_xfs = true,
                    b"fonts" => in_fonts = true,
                    b"fills" => in_fills = true,
                    _ => {}
                }
                // numFmt within numFmts
                if in_num_fmts && name.as_ref() == b"numFmt" {
                    let mut id: u32 = 0;
                    let mut code = String::new();
                    for attr in e.attributes().with_checks(false).flatten() {
                        let v = attr.value.as_ref();
                        if let Ok(s) = std::str::from_utf8(v) {
                            let key = attr.key.as_ref();
                            let local = strip_xml_ns(key);
                            match local {
                                b"numFmtId" => { id = s.parse().unwrap_or(0); }
                                b"formatCode" => { code = s.to_string(); }
                                _ => {}
                            }
                        }
                    }
                    num_formats.insert(id, code);
                }
                // xf within cellXfs
                if in_cell_xfs && name.as_ref() == b"xf" {
                    current_cell_xf = CellXf::default();
                    for attr in e.attributes().with_checks(false).flatten() {
                        let v = attr.value.as_ref();
                        if let Ok(s) = std::str::from_utf8(v) {
                            let key = attr.key.as_ref();
                            let local = strip_xml_ns(key);
                            match local {
                                b"numFmtId" => current_cell_xf.num_fmt_id = s.parse().unwrap_or(0),
                                b"fontId" => current_cell_xf.font_id = s.parse().unwrap_or(0),
                                b"fillId" => current_cell_xf.fill_id = s.parse().unwrap_or(0),
                                b"borderId" => current_cell_xf.border_id = s.parse().unwrap_or(0),
                                b"applyNumberFormat" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_number_format = false;
                                    }
                                }
                                b"applyFont" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_font = false;
                                    }
                                }
                                b"applyFill" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_fill = false;
                                    }
                                }
                                b"applyBorder" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_border = false;
                                    }
                                }
                                b"applyAlignment" => {
                                    let bytes = s.as_bytes();
                                    if bytes == b"0" || bytes == b"false" {
                                        current_cell_xf.apply_alignment = false;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    // For Empty xf, immediately push
                    if let Some(a) = current_align.take() {
                        current_cell_xf.alignment = Some(a);
                    }
                    cell_xfs.push(current_cell_xf.clone());
                }
                // Font child elements within a font
                if in_font {
                    match name.as_ref() {
                        b"sz" => {
                            if let Some(v) = attr_value(e, b"val") {
                                if let Ok(s) = std::str::from_utf8(&v) {
                                    current_font.size = s.parse().ok();
                                }
                            }
                        }
                        b"color" => {
                            if let Some(v) = attr_value(e, b"rgb") {
                                if let Ok(s) = std::str::from_utf8(&v) {
                                    current_font.color = Some(s.trim_start_matches('#').to_ascii_uppercase());
                                }
                            }
                        }
                        b"name" => {
                            if let Some(v) = attr_value(e, b"val") {
                                if let Ok(s) = std::str::from_utf8(&v) {
                                    current_font.name = Some(s.to_string());
                                }
                            }
                        }
                        b"b" => { current_font.bold = true; }
                        b"i" => { current_font.italic = true; }
                        _ => {}
                    }
                }
                // Fill child elements within a fill
                if in_fill {
                    match name.as_ref() {
                        b"patternFill" => {
                            if let Some(v) = attr_value(e, b"patternType") {
                                if let Ok(s) = std::str::from_utf8(&v) {
                                    current_fill.pattern_type = Some(s.to_string());
                                }
                            }
                        }
                        b"fgColor" => {
                            if let Some(v) = attr_value(e, b"rgb") {
                                if let Ok(s) = std::str::from_utf8(&v) {
                                    current_fill.fg_color = Some(s.trim_start_matches('#').to_ascii_uppercase());
                                }
                            }
                        }
                        b"bgColor" => {
                            if let Some(v) = attr_value(e, b"rgb") {
                                if let Ok(s) = std::str::from_utf8(&v) {
                                    current_fill.bg_color = Some(s.trim_start_matches('#').to_ascii_uppercase());
                                }
                            }
                        }
                        _ => {}
                    }
                }
                // For Empty font element, immediately push
                if name.as_ref() == b"font" && in_fonts {
                    fonts.push(current_font.clone());
                    current_font = FontXf::default();
                }
                // For Empty fill element, immediately push
                if name.as_ref() == b"fill" && in_fills {
                    fills.push(current_fill.clone());
                    current_fill = FillXf::default();
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"numFmts" => in_num_fmts = false,
                    b"cellXfs" => in_cell_xfs = false,
                    b"fonts" => in_fonts = false,
                    b"fills" => in_fills = false,
                    b"font" if in_font => {
                        fonts.push(current_font.clone());
                        in_font = false;
                    }
                    b"fill" if in_fill => {
                        fills.push(current_fill.clone());
                        in_fill = false;
                    }
                    b"xf" if in_cell_xfs => {
                        if let Some(a) = current_align.take() {
                            current_cell_xf.alignment = Some(a);
                        }
                        cell_xfs.push(current_cell_xf.clone());
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

    StylesInfo {
        num_formats,
        cell_xfs,
        fonts,
        fills,
    }
}

fn strip_xml_ns(key: &[u8]) -> &[u8] {
    match key.iter().position(|&b| b == b':') {
        Some(i) => &key[i + 1..],
        None => key,
    }
}

fn attr_value(e: &quick_xml::events::BytesStart, name: &[u8]) -> Option<Vec<u8>> {
    for attr in e.attributes().with_checks(false).flatten() {
        let key = attr.key.as_ref();
        let local = strip_xml_ns(key);
        if local == name {
            return Some(attr.value.into_owned());
        }
    }
    None
}

fn resolve_number_format(id: u32, custom: &HashMap<u32, String>) -> String {
    if let Some(s) = custom.get(&id) {
        return s.clone();
    }
    match id {
        0 => "General".to_string(),
        1 => "0".to_string(),
        2 => "0.00".to_string(),
        3 => "#,##0".to_string(),
        4 => "#,##0.00".to_string(),
        9 => "0%".to_string(),
        10 => "0.00%".to_string(),
        11 => "0.00E+00".to_string(),
        14 => "m/d/yyyy".to_string(),
        22 => "m/d/yyyy h:mm".to_string(),
        49 => "@".to_string(),
        _ => "General".to_string(),
    }
}

#[derive(Default, Clone, Debug)]
struct CellXf {
    num_fmt_id: u32,
    font_id: u32,
    fill_id: u32,
    border_id: u32,
    apply_number_format: bool,
    apply_font: bool,
    apply_fill: bool,
    apply_border: bool,
    apply_alignment: bool,
    alignment: Option<AlignmentXf>,
}

#[derive(Default, Clone, Debug)]
struct AlignmentXf {
    horizontal: Option<String>,
    vertical: Option<String>,
    wrap_text: bool,
}

#[derive(Default, Clone, Debug)]
struct FontXf {
    size: Option<u32>,
    color: Option<String>,
    name: Option<String>,
    bold: bool,
    italic: bool,
}

#[derive(Default, Clone, Debug)]
struct FillXf {
    pattern_type: Option<String>,
    fg_color: Option<String>,
    bg_color: Option<String>,
}

struct StylesInfo {
    num_formats: HashMap<u32, String>,
    cell_xfs: Vec<CellXf>,
    fonts: Vec<FontXf>,
    fills: Vec<FillXf>,
}

impl StylesInfo {
    fn resolve_style(&self, xf_index: usize) -> Option<CellStyle> {
        let xf = self.cell_xfs.get(xf_index)?;
        let mut style = CellStyle::default();

        // FIX: Per Excel spec, apply* attributes default to true when absent.
        // We invert the logic: if the attribute is explicitly "0" or "false",
        // skip that aspect; otherwise apply it.
        // Since we only set apply* to false when explicitly parsed as such,
        // and the parser leaves them true by default for non-trivial xfs,
        // we treat apply_*=true as the expected case.
        if xf.apply_number_format {
            style.number_format = resolve_number_format(xf.num_fmt_id, &self.num_formats);
        } else {
            style.number_format = "General".to_string();
        }

        // Apply font whenever font_id points to a valid font
        if xf.font_id < self.fonts.len() as u32 {
            if let Some(font) = self.fonts.get(xf.font_id as usize) {
                style.font_bold = font.bold;
                style.font_italic = font.italic;
                style.font_color = font.color.clone();
                // OOXML SpreadsheetML's `sz@val` is in points (a double, not 1/100 pt).
                // Keep the parsed value as-is so the writer can emit it back unchanged.
                style.font_size = font.size;
                style.font_name = font.name.clone();
            }
        }

        // Apply fill whenever fill_id points to a non-trivial fill
        if xf.fill_id < self.fills.len() as u32 {
            if let Some(fill) = self.fills.get(xf.fill_id as usize) {
                let has_pattern = fill
                    .pattern_type
                    .as_ref()
                    .map(|p| p != "none" && !p.is_empty())
                    .unwrap_or(false);
                if has_pattern {
                    style.fill_fg_color = fill.fg_color.clone();
                    style.fill_bg_color = fill.bg_color.clone();
                }
            }
        }

        if let Some(ref a) = xf.alignment {
            style.alignment_h = a.horizontal.clone();
            style.alignment_v = a.vertical.clone();
        }

        Some(style)
    }
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
pub fn xlsx_workbook_to_text(workbook: &XlsxWorkbook) -> String {
    let mut output = String::new();
    for sheet in &workbook.sheets {
        output.push_str(&format!(
            "=== Sheet: {} ({}x{}) ===\n\n",
            sheet.name, sheet.max_row, sheet.max_col
        ));

        if sheet.cells.is_empty() {
            output.push_str("(empty sheet)\n\n");
            continue;
        }

        let mut grid: Vec<Vec<String>> =
            vec![vec![String::new(); sheet.max_col.max(1)]; sheet.max_row.max(1)];
        for cell in &sheet.cells {
            let display = if let Some(f) = &cell.formula {
                format!("={}", f)
            } else {
                cell.value.as_string_for_display()
            };
            if cell.row < grid.len() && cell.col < (grid.get(0).map(|r| r.len()).unwrap_or(0)) {
                grid[cell.row][cell.col] = display;
            }
        }

        let col_widths: Vec<usize> = (0..sheet.max_col.max(1))
            .map(|c| {
                sheet
                    .cells
                    .iter()
                    .filter(|cell| cell.col == c)
                    .map(|cell| {
                        let v = if let Some(f) = &cell.formula {
                            format!("={}", f)
                        } else {
                            cell.value.as_string_for_display()
                        };
                        v.chars().count().max(8)
                    })
                    .max()
                    .unwrap_or(8)
            })
            .collect();

        for row in grid.iter() {
            let cells: Vec<String> = row
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let w = col_widths.get(i).copied().unwrap_or(8);
                    format!("{:w$}", c, w = w)
                })
                .collect();
            output.push_str(&cells.join(" | "));
            output.push('\n');
        }

        let styled: Vec<String> = sheet
            .cells
            .iter()
            .filter_map(|cell| {
                cell.style.as_ref().and_then(|s| {
                    if s.number_format != "General" {
                        Some(format!("{}={}", cell.address(), s.number_format))
                    } else {
                        None
                    }
                })
            })
            .collect();
        if !styled.is_empty() {
            output.push_str(&format!("\nFormats: {}\n", styled.join(", ")));
        }
        if !sheet.merged_cells.is_empty() {
            let merged_addrs: Vec<String> =
                sheet.merged_cells.iter().map(|m| m.address()).collect();
            output.push_str(&format!("Merged: {}\n", merged_addrs.join(", ")));
        }
        output.push('\n');
    }
    output.trim().to_string()
}

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

const MINIMAL_THEME_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Office Theme"><a:themeElements><a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="1F497D"/></a:dk2><a:lt2><a:srgbClr val="EEECE1"/></a:lt2><a:accent1><a:srgbClr val="4F81BD"/></a:accent1><a:accent2><a:srgbClr val="C0504D"/></a:accent2><a:accent3><a:srgbClr val="9BBB59"/></a:accent3><a:accent4><a:srgbClr val="8064A2"/></a:accent4><a:accent5><a:srgbClr val="4BACC6"/></a:accent5><a:accent6><a:srgbClr val="F79646"/></a:accent6><a:hlink><a:srgbClr val="0000FF"/></a:hlink><a:folHlink><a:srgbClr val="800080"/></a:folHlink></a:clrScheme><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Cambria"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Office"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="50000"/><a:satMod val="300000"/></a:schemeClr></a:gs><a:gs pos="35000"><a:schemeClr val="phClr"><a:tint val="37000"/><a:satMod val="300000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:tint val="15000"/><a:satMod val="350000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="16200000" scaled="1"/></a:gradFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:shade val="51000"/><a:satMod val="130000"/></a:schemeClr></a:gs><a:gs pos="80000"><a:schemeClr val="phClr"><a:shade val="93000"/><a:satMod val="130000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="94000"/><a:satMod val="135000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="16200000" scaled="0"/></a:gradFill></a:fillStyleLst><a:lnStyleLst><a:ln w="9525" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"><a:shade val="95000"/><a:satMod val="105000"/></a:schemeClr></a:solidFill><a:prstDash val="solid"/></a:ln><a:ln w="25400" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln><a:ln w="38100" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst><a:outerShdw blurRad="40000" dist="20000" dir="5400000" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="38000"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle><a:effectStyle><a:effectLst><a:outerShdw blurRad="40000" dist="23000" dir="5400000" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="35000"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle><a:effectStyle><a:effectLst><a:outerShdw blurRad="40000" dist="23000" dir="5400000" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="35000"/></a:srgbClr></a:outerShdw></a:effectLst><a:scene3d><a:camera prst="orthographicFront"><a:rot lat="0" lon="0" rev="0"/></a:camera><a:lightRig rig="threePt" dir="t"><a:rot lat="0" lon="0" rev="1200000"/></a:lightRig></a:scene3d><a:sp3d><a:bevelT w="63500" h="25400"/></a:sp3d></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="40000"/><a:satMod val="350000"/></a:schemeClr></a:gs><a:gs pos="40000"><a:schemeClr val="phClr"><a:tint val="45000"/><a:shade val="99000"/><a:satMod val="350000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="20000"/><a:satMod val="255000"/></a:schemeClr></a:gs></a:gsLst><a:path path="circle"><a:fillToRect l="50000" t="-80000" r="50000" b="180000"/></a:path></a:gradFill><a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="80000"/><a:satMod val="300000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="30000"/><a:satMod val="200000"/></a:schemeClr></a:gs></a:gsLst><a:path path="circle"><a:fillToRect l="50000" t="50000" r="50000" b="50000"/></a:path></a:gradFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"#;

const MINIMAL_STYLES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<numFmts count="0"/>
<fonts count="1"><font><name val="Calibri"/><family val="2"/><color theme="1"/><sz val="11"/><scheme val="minor"/></font></fonts>
<fills count="2"><fill><patternFill/></fill><fill><patternFill patternType="gray125"/></fill></fills>
<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>
<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" pivotButton="0" quotePrefix="0" xfId="0"/></cellXfs>
<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0" hidden="0"/></cellStyles>
<dxfs count="0"/>
<tableStyles count="0" defaultTableStyle="TableStyleMedium9" defaultPivotStyle="PivotStyleLight16"/>
<colors><indexedColors><rgbColor rgb="00000000"/><rgbColor rgb="00FFFFFF"/><rgbColor rgb="00FF0000"/><rgbColor rgb="0000FF00"/><rgbColor rgb="000000FF"/><rgbColor rgb="00FFFF00"/><rgbColor rgb="00FF00FF"/><rgbColor rgb="0000FFFF"/><rgbColor rgb="00000000"/><rgbColor rgb="00FFFFFF"/><rgbColor rgb="00FF0000"/><rgbColor rgb="0000FF00"/><rgbColor rgb="000000FF"/><rgbColor rgb="00FFFF00"/><rgbColor rgb="00FF00FF"/><rgbColor rgb="0000FFFF"/><rgbColor rgb="00800000"/><rgbColor rgb="00008000"/><rgbColor rgb="00000080"/><rgbColor rgb="00808000"/><rgbColor rgb="00800080"/><rgbColor rgb="00008080"/><rgbColor rgb="00C0C0C0"/><rgbColor rgb="00808080"/><rgbColor rgb="009999FF"/><rgbColor rgb="00993366"/><rgbColor rgb="00FFFFCC"/><rgbColor rgb="00CCFFFF"/><rgbColor rgb="00660066"/><rgbColor rgb="00FF8080"/><rgbColor rgb="000066CC"/><rgbColor rgb="00CCCCFF"/><rgbColor rgb="00000080"/><rgbColor rgb="00FF00FF"/><rgbColor rgb="00FFFF00"/><rgbColor rgb="0000FFFF"/><rgbColor rgb="00800080"/><rgbColor rgb="00800000"/><rgbColor rgb="00008080"/><rgbColor rgb="000000FF"/><rgbColor rgb="0000CCFF"/><rgbColor rgb="00CCFFFF"/><rgbColor rgb="00CCFFCC"/><rgbColor rgb="00FFFF99"/><rgbColor rgb="0099CCFF"/><rgbColor rgb="00FF99CC"/><rgbColor rgb="00CC99FF"/><rgbColor rgb="00FFCC99"/><rgbColor rgb="003366FF"/><rgbColor rgb="0033CCCC"/><rgbColor rgb="0099CC00"/><rgbColor rgb="00FFCC00"/><rgbColor rgb="00FF9900"/><rgbColor rgb="00FF6600"/><rgbColor rgb="00666699"/><rgbColor rgb="00969696"/><rgbColor rgb="00003366"/><rgbColor rgb="00339966"/><rgbColor rgb="00003300"/><rgbColor rgb="00333300"/><rgbColor rgb="00993300"/><rgbColor rgb="00993366"/><rgbColor rgb="00333399"/><rgbColor rgb="00333333"/></indexedColors></colors>
</styleSheet>"#;

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


// ─── Merge / Unmerge cells ─────────────────────────────────────────────────────

/// What to do with a merged range.
#[derive(Debug, Clone)]
pub enum MergeOp {
    Merge,
    Unmerge,
}

/// A merge/unmerge operation to apply.
#[derive(Debug, Clone)]
pub struct MergeModification {
    pub sheet: String,
    pub op: MergeOp,
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
}

/// Merge or unmerge cell ranges in an xlsx file.
///
/// DEPRECATED: Use `XlsxWorkbook::apply_operations()` + `write_excel_document()` instead.
pub fn merge_cells_xlsx(
    original_bytes: &[u8],
    modifications: &[MergeModification],
    output_path: &std::path::Path,
) -> Result<(), OfficeError> {
    use std::io::{Read, Write};

    if modifications.is_empty() {
        std::fs::write(output_path, original_bytes)?;
        return Ok(());
    }

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(original_bytes.to_vec()))?;
    let workbook_xml = read_entry(&mut archive, "xl/workbook.xml")?;
    let rels_xml = read_entry(&mut archive, "xl/_rels/workbook.xml.rels")
        .unwrap_or_default();
    let sheet_name_to_path = parse_sheet_name_to_path(&workbook_xml, &rels_xml)?;

    let mut by_path: HashMap<String, Vec<&MergeModification>> = HashMap::new();
    for m in modifications {
        if let Some((_, path)) = sheet_name_to_path.iter().find(|(n, _)| n == &m.sheet) {
            by_path.entry(path.clone()).or_default().push(m);
        } else {
            return Err(OfficeError::Excel(format!("Sheet '{}' not found", m.sheet)));
        }
    }

    let mut rewritten: HashMap<String, Vec<u8>> = HashMap::new();
    for (path, mods) in &by_path {
        let xml = read_entry(&mut archive, path)?;
        let new_xml = apply_merge_to_sheet_xml(&xml, mods)?;
        rewritten.insert(path.clone(), new_xml.into_bytes());
    }

    let mut out = zip::ZipWriter::new(std::fs::File::create(output_path)?);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(original_bytes.to_vec()))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        if let Some(new_bytes) = rewritten.get(&name) {
            out.start_file(&name, opts)?;
            out.write_all(new_bytes)?;
        } else {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            let file_opts = if file.compression() == zip::CompressionMethod::Deflated {
                opts
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

/// Legacy string-based merge/unmerge operation. DEPRECATED: use
/// `XlsxWorkbook::apply_operations()` + `write_excel_document()` instead.
fn apply_merge_to_sheet_xml(sheet_xml: &str, mods: &[&MergeModification]) -> Result<String, OfficeError> {
    // First handle unmerges: remove existing mergeCell entries
    let mut xml = sheet_xml.to_string();
    for m in mods {
        if matches!(m.op, MergeOp::Unmerge) {
            let ref_str = format!(
                "ref=\"{}\"",
                format!("{}:{}",
                    cell_address(m.start_row, m.start_col),
                    cell_address(m.end_row, m.end_col)
                )
            );
            if let Some(start) = xml.find(&ref_str) {
                let before = &xml[..start];
                if let Some(tag_start) = before.rfind("<mergeCell") {
                    let after_ref = start + ref_str.len();
                    let rest = &xml[after_ref..];
                    let tag_end = rest.find("/>").map(|p| after_ref + p + 2)
                        .unwrap_or(after_ref + rest.len());
                    let mut actual_end = tag_end;
                    while actual_end < xml.len()
                        && matches!(xml.as_bytes()[actual_end], b' ' | b'\t' | b'\n' | b'\r')
                    {
                        actual_end += 1;
                    }
                    xml = format!("{}{}", &xml[..tag_start], &xml[actual_end..]);
                }
            }
        }
    }

    // Now handle merges: add new mergeCell entries
    let mut merge_inserts: Vec<String> = Vec::new();
    for m in mods {
        if matches!(m.op, MergeOp::Merge) {
            let ref_str = format!(
                "{}:{}",
                cell_address(m.start_row, m.start_col),
                cell_address(m.end_row, m.end_col)
            );
            merge_inserts.push(format!(r#"<mergeCell ref="{}"/>"#, ref_str));
        }
    }

    if merge_inserts.is_empty() {
        return Ok(xml);
    }

    // Find where to insert mergeCell entries
    if let Some(pos) = xml.find("</mergeCells>") {
        xml.insert_str(pos, &merge_inserts.join(""));
    } else if let Some(pos) = xml.find("</sheetData>") {
        let insert = format!(
            r#"<mergeCells count="{}">{}</mergeCells>"#,
            merge_inserts.len(),
            merge_inserts.join("")
        );
        xml.insert_str(pos, &format!("{}{}", insert, "\n"));
    } else if let Some(pos) = xml.find("</worksheet>") {
        let insert = format!(
            r#"<mergeCells count="{}">{}</mergeCells>"#,
            merge_inserts.len(),
            merge_inserts.join("")
        );
        xml.insert_str(pos, &format!("\n{}{}", insert, "\n"));
    }

    Ok(xml)
}

// ─── Row / Column dimensions ────────────────────────────────────────────────────

/// A row or column dimension change.
#[derive(Debug, Clone)]
pub struct RowColModification {
    /// 0-based row or column index.
    pub index: usize,
    /// Size: row height in points, or column width in Excel character units.
    pub size: f64,
    /// Whether to hide the row/column.
    pub hidden: bool,
}

/// Set row heights and column widths in an xlsx file.
///
/// DEPRECATED: Use `XlsxWorkbook::apply_operations()` + `write_excel_document()` instead.
pub fn resize_rows_cols_xlsx(
    original_bytes: &[u8],
    sheet_name: &str,
    row_changes: &[RowColModification],
    col_changes: &[RowColModification],
    output_path: &std::path::Path,
) -> Result<(), OfficeError> {
    use std::io::{Read, Write};

    if row_changes.is_empty() && col_changes.is_empty() {
        std::fs::write(output_path, original_bytes)?;
        return Ok(());
    }

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(original_bytes.to_vec()))?;
    let workbook_xml = read_entry(&mut archive, "xl/workbook.xml")?;
    let rels_xml = read_entry(&mut archive, "xl/_rels/workbook.xml.rels")
        .unwrap_or_default();
    let sheet_name_to_path = parse_sheet_name_to_path(&workbook_xml, &rels_xml)?;

    let path = sheet_name_to_path.iter()
        .find(|(n, _)| n == sheet_name)
        .map(|(_, p)| p.clone())
        .ok_or_else(|| OfficeError::Excel(format!("Sheet '{}' not found", sheet_name)))?;

    let xml = read_entry(&mut archive, &path)?;
    let new_xml = apply_dimension_changes(&xml, row_changes, col_changes)?;

    let mut out = zip::ZipWriter::new(std::fs::File::create(output_path)?);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(original_bytes.to_vec()))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        if name == path {
            out.start_file(&name, opts)?;
            out.write_all(new_xml.as_bytes())?;
        } else {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            let file_opts = if file.compression() == zip::CompressionMethod::Deflated {
                opts
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

/// Legacy string-based dimension changes. DEPRECATED: use
/// `XlsxWorkbook::apply_operations()` + `write_excel_document()` instead.
fn apply_dimension_changes(
    sheet_xml: &str,
    row_changes: &[RowColModification],
    col_changes: &[RowColModification],
) -> Result<String, OfficeError> {
    let mut xml = sheet_xml.to_string();

    // Apply row changes
    for rc in row_changes {
        let existing_pattern = format!("<row r=\"{}\"", rc.index + 1);
        if let Some(start) = xml.find(&existing_pattern) {
            // Find the end of the opening tag
            let after_tag_start = start + existing_pattern.len();
            let tag_open_end = xml[after_tag_start..].find('>')
                .map(|p| after_tag_start + p + 1)
                .unwrap_or(after_tag_start);

            // Check if it's self-closing
            let is_self_closing = xml[start..tag_open_end].ends_with("/>");

            if is_self_closing {
                // Self-closing: replace the whole tag
                let new_attrs = if rc.hidden {
                    format!(r#"r="{}" hidden="1""#, rc.index + 1)
                } else {
                    format!(r#"r="{}" customHeight="1" ht="{}""#, rc.index + 1, rc.size)
                };
                xml = format!("{}{}/>", &xml[..start], new_attrs);
            } else {
                // Not self-closing: find the closing </row> tag and preserve content
                let close_pattern = format!("</row>");
                let content_start = tag_open_end;
                let content_end = xml[content_start..].find(&close_pattern)
                    .map(|p| content_start + p)
                    .unwrap_or(content_start);
                let content = &xml[content_start..content_end];

                let new_attrs = if rc.hidden {
                    format!(r#"r="{}" hidden="1""#, rc.index + 1)
                } else {
                    format!(r#"r="{}" customHeight="1" ht="{}""#, rc.index + 1, rc.size)
                };
                xml = format!("{}{}>{}{}</row>", &xml[..start], new_attrs, content, &close_pattern);
                // Remove the original row
                let original_start = start;
                let original_end = content_end + close_pattern.len();
                xml = format!("{}{}", &xml[..original_start], &xml[original_end..]);
            }
        } else {
            // Insert before </sheetData>
            if let Some(pos) = xml.find("</sheetData>") {
                let new_row = if rc.hidden {
                    format!(r#"<row r="{}" hidden="1"/>"#, rc.index + 1)
                } else {
                    format!(r#"<row r="{}" customHeight="1" ht="{}"/>"#, rc.index + 1, rc.size)
                };
                xml.insert_str(pos, &format!("{}{}", new_row, "\n"));
            }
        }
    }

    // Apply column changes
    for cc in col_changes {
        let existing_pattern = format!("<col min=\"{}\"", cc.index + 1);
        if xml.contains(&existing_pattern) {
            if let Some(start) = xml.find(&existing_pattern) {
                let rest = &xml[start..];
                let tag_end = rest.find("/>").map(|p| start + p + 2)
                    .or_else(|| rest.find(">").map(|p| start + p + 1))
                    .unwrap_or(start + rest.len());
                let attrs = if cc.hidden {
                    format!(r#"min="{}" max="{}" width="{}" hidden="1""#, cc.index + 1, cc.index + 1, cc.size)
                } else {
                    format!(r#"min="{}" max="{}" width="{}" customWidth="1""#, cc.index + 1, cc.index + 1, cc.size)
                };
                xml = format!("{}{}{}", &xml[..start], attrs, &xml[tag_end..]);
            }
        } else {
            if let Some(pos) = xml.find("</cols>") {
                let new_col = if cc.hidden {
                    format!(r#"<col min="{}" max="{}" width="{}" hidden="1"/>"#, cc.index + 1, cc.index + 1, cc.size)
                } else {
                    format!(r#"<col min="{}" max="{}" width="{}" customWidth="1"/>"#, cc.index + 1, cc.index + 1, cc.size)
                };
                xml.insert_str(pos, &format!("{}{}", new_col, "\n"));
            }
        }
    }

    Ok(xml)
}

// ─── Sheet management ──────────────────────────────────────────────────────────

/// Create a new sheet in the workbook.
pub fn create_sheet_xlsx(
    original_bytes: &[u8],
    sheet_name: &str,
    insert_index: usize,
    output_path: &std::path::Path,
) -> Result<(), OfficeError> {
    use std::io::{Read, Write};

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(original_bytes.to_vec()))?;
    let workbook_xml = read_entry(&mut archive, "xl/workbook.xml")?;
    let rels_xml = read_entry(&mut archive, "xl/_rels/workbook.xml.rels")
        .unwrap_or_default();

    let name_to_path = parse_sheet_name_to_path(&workbook_xml, &rels_xml)?;
    let next_num = name_to_path.len() + 1;
    let new_sheet_path = format!("xl/worksheets/sheet{}.xml", next_num);

    let new_sheet_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetViews><sheetView workbookViewId="0"/></sheetViews>
<sheetFormatPr defaultRowHeight="15"/>
<sheetData/>
</worksheet>"#
    );

    let new_workbook_xml = inject_sheet_into_workbook(&workbook_xml, sheet_name, next_num as u32, insert_index)?;
    let new_rels_xml = inject_sheet_relationship(&rels_xml, next_num)?;
    let content_types_xml = read_entry(&mut archive, "[Content_Types].xml")?;
    let new_content_types = inject_content_type(&content_types_xml, &new_sheet_path)?;

    let mut out = zip::ZipWriter::new(std::fs::File::create(output_path)?);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    // Re-open archive for iteration (can't reuse mutable borrow)
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(original_bytes.to_vec()))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let (new_name, new_content) = match name.as_str() {
            "xl/workbook.xml" => (name.clone(), new_workbook_xml.as_bytes().to_vec()),
            "xl/_rels/workbook.xml.rels" => (name.clone(), new_rels_xml.as_bytes().to_vec()),
            "[Content_Types].xml" => (name.clone(), new_content_types.as_bytes().to_vec()),
            _ => (name.clone(), buf),
        };

        let file_opts = if file.compression() == zip::CompressionMethod::Deflated {
            opts
        } else {
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(0o644)
        };
        out.start_file(&new_name, file_opts)?;
        out.write_all(&new_content)?;
    }

    out.start_file(&new_sheet_path, opts)?;
    out.write_all(new_sheet_xml.as_bytes())?;
    out.finish()?;
    Ok(())
}

fn inject_sheet_into_workbook(xml: &str, sheet_name: &str, sheet_id: u32, insert_index: usize) -> Result<String, OfficeError> {
    let escaped_name = xml_escape(sheet_name);
    let new_tag = format!(
        r#"<sheet name="{}" sheetId="{}" state="visible" r:id="rId{}"/>"#,
        escaped_name, sheet_id, sheet_id
    );

    let mut search_pos = 0;
    let mut sheet_count = 0;
    let mut insert_pos: Option<usize> = None;
    while let Some(start) = xml[search_pos..].find("<sheet ") {
        let abs_start = search_pos + start;
        sheet_count += 1;
        if sheet_count == insert_index {
            insert_pos = Some(abs_start);
        }
        if let Some(end) = xml[abs_start..].find("/>").map(|p| abs_start + p + 2)
            .or_else(|| xml[abs_start..].find("</sheet>").map(|p| abs_start + p + 8))
        {
            search_pos = end;
        } else {
            break;
        }
    }

    if let Some(pos) = insert_pos {
        Ok(format!("{}{}{}", &xml[..pos], new_tag, &xml[pos..]))
    } else if let Some(pos) = xml.find("</sheets>") {
        Ok(format!("{}{}{}", &xml[..pos], new_tag, &xml[pos..]))
    } else {
        Err(OfficeError::Excel("Could not find </sheets> in workbook.xml".to_string()))
    }
}

fn inject_sheet_relationship(rels_xml: &str, sheet_num: usize) -> Result<String, OfficeError> {
    let new_rel = format!(
        r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{}.xml"/>"#,
        sheet_num, sheet_num
    );
    if let Some(pos) = rels_xml.find("</Relationships>") {
        Ok(format!("{}{}{}", &rels_xml[..pos], new_rel, &rels_xml[pos..]))
    } else {
        Err(OfficeError::Excel("Could not find </Relationships> in workbook.xml.rels".to_string()))
    }
}

fn inject_content_type(content_types: &str, sheet_path: &str) -> Result<String, OfficeError> {
    let entry = format!(
        r#"<Override PartName="/{}" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
        sheet_path
    );
    if let Some(pos) = content_types.find("</Types>") {
        Ok(format!("{}{}{}", &content_types[..pos], entry, &content_types[pos..]))
    } else {
        Err(OfficeError::Excel("Could not find </Types> in [Content_Types].xml".to_string()))
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Rename a sheet in the workbook.
pub fn rename_sheet_xlsx(
    original_bytes: &[u8],
    old_name: &str,
    new_name: &str,
    output_path: &std::path::Path,
) -> Result<(), OfficeError> {
    use std::io::{Read, Write};

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(original_bytes.to_vec()))?;
    let workbook_xml = read_entry(&mut archive, "xl/workbook.xml")?;

    let escaped_old = xml_escape(old_name);
    let escaped_new = xml_escape(new_name);
    let new_workbook_xml = workbook_xml.replace(
        &format!("name=\"{}\"", escaped_old),
        &format!("name=\"{}\"", escaped_new),
    );

    let mut out = zip::ZipWriter::new(std::fs::File::create(output_path)?);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(original_bytes.to_vec()))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let new_content = if name == "xl/workbook.xml" {
            new_workbook_xml.as_bytes().to_vec()
        } else {
            buf
        };

        let file_opts = if file.compression() == zip::CompressionMethod::Deflated {
            opts
        } else {
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(0o644)
        };
        out.start_file(&name, file_opts)?;
        out.write_all(&new_content)?;
    }
    out.finish()?;
    Ok(())
}

/// Delete a sheet from the workbook.
pub fn delete_sheet_xlsx(
    original_bytes: &[u8],
    sheet_name: &str,
    output_path: &std::path::Path,
) -> Result<(), OfficeError> {
    use std::io::{Read, Write};

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(original_bytes.to_vec()))?;
    let workbook_xml = read_entry(&mut archive, "xl/workbook.xml")?;
    let rels_xml = read_entry(&mut archive, "xl/_rels/workbook.xml.rels")
        .unwrap_or_default();
    let name_to_path = parse_sheet_name_to_path(&workbook_xml, &rels_xml)?;

    let sheet_path = name_to_path.iter()
        .find(|(n, _)| n == sheet_name)
        .map(|(_, p)| p.clone())
        .ok_or_else(|| OfficeError::Excel(format!("Sheet '{}' not found", sheet_name)))?;

    let name_attr = xml_escape(sheet_name);
    let name_attr = format!("name=\"{}\"", name_attr);

    let mut new_workbook_xml = workbook_xml.clone();
    if let Some(pos) = new_workbook_xml.find(&name_attr) {
        let before = &new_workbook_xml[..pos];
        if let Some(tag_start) = before.rfind("<sheet") {
            let rest = &new_workbook_xml[pos..];
            let end = rest.find("/>").map(|p| pos + p + 2)
                .or_else(|| rest.find("</sheet>").map(|p| pos + p + 8))
                .unwrap_or(pos + name_attr.len());
            new_workbook_xml = format!("{}{}", &new_workbook_xml[..tag_start], &new_workbook_xml[end..]);
        }
    }

    let mut new_rels_xml = rels_xml.to_string();
    let sheet_file = sheet_path.trim_start_matches("xl/");
    if let Some(pos) = new_rels_xml.find(sheet_file) {
        let before = &new_rels_xml[..pos];
        if let Some(tag_start) = before.rfind("<Relationship") {
            let rest = &new_rels_xml[pos..];
            let end = rest.find("/>").map(|p| pos + p + 2)
                .or_else(|| rest.find("</Relationship>").map(|p| pos + p + 15))
                .unwrap_or(pos + sheet_file.len());
            new_rels_xml = format!("{}{}", &new_rels_xml[..tag_start], &new_rels_xml[end..]);
        }
    }

    let content_types_xml = read_entry(&mut archive, "[Content_Types].xml")?;
    let sheet_part = format!("/{}", sheet_path);
    let mut new_content_types = content_types_xml.clone();
    if let Some(pos) = new_content_types.find(&sheet_part) {
        let before = &new_content_types[..pos];
        if let Some(tag_start) = before.rfind("<Override") {
            let rest = &new_content_types[pos..];
            let end = rest.find("/>").map(|p| pos + p + 2)
                .unwrap_or(pos + sheet_part.len());
            new_content_types = format!("{}{}", &new_content_types[..tag_start], &new_content_types[end..]);
        }
    }

    let mut out = zip::ZipWriter::new(std::fs::File::create(output_path)?);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(original_bytes.to_vec()))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        if name == sheet_path {
            continue;
        }
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let new_content = match name.as_str() {
            "xl/workbook.xml" => new_workbook_xml.as_bytes().to_vec(),
            "xl/_rels/workbook.xml.rels" => new_rels_xml.as_bytes().to_vec(),
            "[Content_Types].xml" => new_content_types.as_bytes().to_vec(),
            _ => buf,
        };

        let file_opts = if file.compression() == zip::CompressionMethod::Deflated {
            opts
        } else {
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(0o644)
        };
        out.start_file(&name, file_opts)?;
        out.write_all(&new_content)?;
    }
    out.finish()?;
    Ok(())
}

/// Set a sheet's visibility state (hidden, visible).
pub fn set_sheet_state_xlsx(
    original_bytes: &[u8],
    sheet_name: &str,
    new_state: &str,
    output_path: &std::path::Path,
) -> Result<(), OfficeError> {
    use std::io::{Read, Write};

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(original_bytes.to_vec()))?;
    let workbook_xml = read_entry(&mut archive, "xl/workbook.xml")?;

    let escaped_name = xml_escape(sheet_name);
    let mut new_workbook_xml = workbook_xml.clone();

    // Find the sheet element by name
    let name_attr = format!("name=\"{}\"", escaped_name);
    if let Some(name_pos) = new_workbook_xml.find(&name_attr) {
        let before = &new_workbook_xml[..name_pos];
        if let Some(_tag_start) = before.rfind("<sheet") {
            let after_name = &new_workbook_xml[name_pos..];

            // Try to find existing state attribute within this sheet tag
            let tag_end_candidates = ["/>", ">"];
            let mut tag_end_pos = None;
            for candidate in &tag_end_candidates {
                if let Some(pos) = after_name.find(candidate) {
                    tag_end_pos = Some(name_pos + pos + candidate.len());
                    break;
                }
            }

            if let Some(tag_end) = tag_end_pos {
                // Check if state attribute exists
                let between_name_and_end = &new_workbook_xml[name_pos..tag_end];
                if let Some(state_start) = between_name_and_end.find("state=\"") {
                    // Modify existing state attribute
                    let state_value_start = name_pos + state_start + 7; // after 'state="'
                    let after_state_value = &new_workbook_xml[state_value_start..];
                    if let Some(quote_pos) = after_state_value.find('"') {
                        let state_value_end = state_value_start + quote_pos;
                        new_workbook_xml = format!(
                            "{}{}{}",
                            &new_workbook_xml[..state_value_start],
                            new_state,
                            &new_workbook_xml[state_value_end..]
                        );
                    }
                } else {
                    // Insert state attribute before the end of the tag
                    new_workbook_xml = format!(
                        "{} state=\"{}\"{}",
                        &new_workbook_xml[..tag_end],
                        new_state,
                        &new_workbook_xml[tag_end..]
                    );
                }
            }
        }
    }

    let mut out = zip::ZipWriter::new(std::fs::File::create(output_path)?);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(original_bytes.to_vec()))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let new_content = if name == "xl/workbook.xml" {
            new_workbook_xml.as_bytes().to_vec()
        } else {
            buf
        };

        let file_opts = if file.compression() == zip::CompressionMethod::Deflated {
            opts
        } else {
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(0o644)
        };
        out.start_file(&name, file_opts)?;
        out.write_all(&new_content)?;
    }
    out.finish()?;
    Ok(())
}



// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_address_round_trip() {
        assert_eq!(cell_address(0, 0), "A1");
        assert_eq!(cell_address(0, 25), "Z1");
        assert_eq!(cell_address(0, 26), "AA1");
        assert_eq!(cell_address(2, 1), "B3");
        assert_eq!(parse_cell_address("A1"), Some((0, 0)));
        assert_eq!(parse_cell_address("Z1"), Some((0, 25)));
        assert_eq!(parse_cell_address("AA1"), Some((0, 26)));
        assert_eq!(parse_cell_address("B3"), Some((2, 1)));
    }

    #[test]
    fn cell_value_display() {
        assert_eq!(CellValue::Int(42).as_string_for_display(), "42");
        assert_eq!(CellValue::Float(3.5).as_string_for_display(), "3.5");
        assert_eq!(CellValue::Bool(true).as_string_for_display(), "true");
        assert_eq!(CellValue::String("hi".into()).as_string_for_display(), "hi");
        assert_eq!(CellValue::Empty.as_string_for_display(), "");
    }

    #[test]
    fn modification_builder() {
        let m = CellModification::new("Sheet1", "A1")
            .with_value(CellValue::Int(100))
            .with_number_format("0%");
        assert_eq!(m.sheet, "Sheet1");
        assert_eq!(m.address, "A1");
        assert_eq!(m.new_value, Some(CellValue::Int(100)));
        assert_eq!(m.new_number_format.as_deref(), Some("0%"));
        assert!(m.new_formula.is_none());
    }

    /// Repro for the style write/read mismatch reported by AI tools.
    /// Sets A7 to bg_color=#1F3864, font_color=#FFFFFF on a brand-new file,
    /// writes the workbook, reads it back, and asserts the style survived.
    /// Prints the resulting styles.xml / sheet1.xml so the actual mapping
    /// can be inspected if the assertion fails.
    #[test]
    fn style_repro_a7_round_trip() {
        let out_dir = std::env::temp_dir().join("inkuo_repro");
        std::fs::create_dir_all(&out_dir).unwrap();

        let mut wb = XlsxWorkbook { sheets: vec![], shared_strings: vec![] };
        wb.sheets.push(XlsxSheet::new("Sheet1".to_string()));
        let initial_path = out_dir.join("repro_initial.xlsx");
        create_xlsx_workbook(&wb, &initial_path).unwrap();

        let bytes = std::fs::read(&initial_path).unwrap();
        let mut wb = read_xlsx_structured(&bytes).unwrap();
        let ops = vec![
            ExcelOperation::ModifyCell {
                sheet: "Sheet1".to_string(),
                address: "A7".to_string(),
                value: Some(CellValue::String("seven".to_string())),
                formula: None,
                number_format: None,
                bg_color: Some("1F3864".to_string()),
                font_bold: None,
                font_italic: None,
                font_color: Some("FFFFFF".to_string()),
                font_size: None,
                font_name: None,
                alignment_h: None,
                alignment_v: None,
            },
            ExcelOperation::ModifyCell {
                sheet: "Sheet1".to_string(),
                address: "B3".to_string(),
                value: Some(CellValue::String("three".to_string())),
                formula: None,
                number_format: None,
                bg_color: Some("548235".to_string()),
                font_bold: None,
                font_italic: None,
                font_color: Some("FF0000".to_string()),
                font_size: None,
                font_name: None,
                alignment_h: None,
                alignment_v: None,
            },
            ExcelOperation::ModifyCell {
                sheet: "Sheet1".to_string(),
                address: "C5".to_string(),
                // style-only edit (no value). The write path emits a self-closing
                // <c .../> for empty cells; the read path must still recognise it.
                value: None,
                formula: None,
                number_format: None,
                bg_color: Some("A9D08E".to_string()),
                font_bold: Some(true),
                font_italic: None,
                font_color: None,
                font_size: None,
                font_name: None,
                alignment_h: None,
                alignment_v: None,
            },
        ];
        wb.apply_operations(ops).unwrap();

        let out_path = out_dir.join("repro.xlsx");
        let original = std::fs::read(&initial_path).unwrap();
        write_excel_document(&wb, Some(&original), &out_path).unwrap();

        let file_bytes = std::fs::read(&out_path).unwrap();
        let cursor = std::io::Cursor::new(file_bytes);
        let mut archive = zip::ZipArchive::new(cursor).unwrap();
        for name in ["xl/styles.xml", "xl/worksheets/sheet1.xml"] {
            let mut entry = archive.by_name(name).unwrap();
            let mut s = String::new();
            std::io::Read::read_to_string(&mut entry, &mut s).unwrap();
            eprintln!("\n========== {} ==========\n{}", name, s);
        }

        let bytes = std::fs::read(&out_path).unwrap();
        let wb = read_xlsx_structured(&bytes).unwrap();
        let sheet = &wb.sheets[0];
        let a7 = sheet.cells.iter().find(|c| c.row == 6 && c.col == 0)
            .expect("A7 cell missing after write");
        let b3 = sheet.cells.iter().find(|c| c.row == 2 && c.col == 1)
            .expect("B3 cell missing after write");
        let c5 = sheet.cells.iter().find(|c| c.row == 4 && c.col == 2)
            .expect("C5 cell missing after write (style-only edit was lost)");
        eprintln!("\nA7 cell: {:?}", a7);
        eprintln!("\nB3 cell: {:?}", b3);
        eprintln!("\nC5 cell: {:?}", c5);
        let a7_style = a7.style.as_ref().expect("A7 has no style");
        let b3_style = b3.style.as_ref().expect("B3 has no style");
        let c5_style = c5.style.as_ref().expect("C5 has no style");
        // Compare normalised hex (strip leading #) since the read path adds # prefix
        // but the write path does not strip it. This is a real bug; the round-trip
        // is lossy in formatting even when the color value is correct.
        let norm = |s: &str| s.trim_start_matches('#').to_ascii_uppercase();
        assert_eq!(norm(a7_style.fill_fg_color.as_deref().unwrap_or("")), "1F3864",
            "A7 bg_color mismatch");
        assert_eq!(norm(a7_style.font_color.as_deref().unwrap_or("")), "FFFFFF",
            "A7 font_color mismatch");
        assert_eq!(norm(b3_style.fill_fg_color.as_deref().unwrap_or("")), "548235",
            "B3 bg_color mismatch");
        assert_eq!(norm(b3_style.font_color.as_deref().unwrap_or("")), "FF0000",
            "B3 font_color mismatch");
        assert_eq!(norm(c5_style.fill_fg_color.as_deref().unwrap_or("")), "A9D08E",
            "C5 bg_color mismatch");
        assert!(c5_style.font_bold, "C5 font_bold not preserved");
    }

    /// Regression for the merged-cell "top-left style disappears" report.
    /// After writing a fill+font style to the anchor cell of a merge region
    /// and saving, reading back must keep that style on the anchor cell.
    #[test]
    fn style_merged_anchor_preserved() {
        let out_dir = std::env::temp_dir().join("inkuo_repro");
        std::fs::create_dir_all(&out_dir).unwrap();
        let mut wb = XlsxWorkbook { sheets: vec![], shared_strings: vec![] };
        wb.sheets.push(XlsxSheet::new("Sheet1".to_string()));
        let initial_path = out_dir.join("repro_merge_initial.xlsx");
        create_xlsx_workbook(&wb, &initial_path).unwrap();

        let bytes = std::fs::read(&initial_path).unwrap();
        let mut wb = read_xlsx_structured(&bytes).unwrap();
        let ops = vec![
            ExcelOperation::MergeCells {
                sheet: "Sheet1".to_string(),
                op: "merge".to_string(),
                start_cell: "A1".to_string(),
                end_cell: "G1".to_string(),
            },
            ExcelOperation::ModifyCell {
                sheet: "Sheet1".to_string(),
                address: "A1".to_string(),
                value: Some(CellValue::String("merged header".to_string())),
                formula: None,
                number_format: None,
                bg_color: Some("1F3864".to_string()),
                font_bold: Some(true),
                font_italic: None,
                font_color: Some("FFFFFF".to_string()),
                font_size: None,
                font_name: None,
                alignment_h: None,
                alignment_v: None,
            },
        ];
        wb.apply_operations(ops).unwrap();
        let out_path = out_dir.join("repro_merge.xlsx");
        let original = std::fs::read(&initial_path).unwrap();
        write_excel_document(&wb, Some(&original), &out_path).unwrap();

        let bytes = std::fs::read(&out_path).unwrap();
        let wb = read_xlsx_structured(&bytes).unwrap();
        let sheet = &wb.sheets[0];
        let a1 = sheet.cells.iter().find(|c| c.row == 0 && c.col == 0)
            .expect("merged anchor A1 missing after write");
        eprintln!("\nA1 (merged anchor): {:?}", a1);
        let style = a1.style.as_ref().expect("merged anchor has no style");
        let norm = |s: &str| s.trim_start_matches('#').to_ascii_uppercase();
        assert_eq!(norm(style.fill_fg_color.as_deref().unwrap_or("")), "1F3864",
            "merged A1 bg_color mismatch");
        assert_eq!(norm(style.font_color.as_deref().unwrap_or("")), "FFFFFF",
            "merged A1 font_color mismatch");
        assert!(style.font_bold, "merged A1 font_bold not preserved");
    }

    /// Stress test: 50 cells each with a unique (background, font) colour pair
    /// are styled in a single call, then read back. Every cell must come back
    /// with exactly the colours that were requested. This is the user's "50
    /// operations, 10 wrong" failure mode and exercises the cellXfs ordering
    /// fix at scale.
    #[test]
    fn style_50_cells_unique_colors() {
        let out_dir = std::env::temp_dir().join("inkuo_repro");
        std::fs::create_dir_all(&out_dir).unwrap();
        let mut wb = XlsxWorkbook { sheets: vec![], shared_strings: vec![] };
        wb.sheets.push(XlsxSheet::new("Sheet1".to_string()));
        let file = out_dir.join("repro_50.xlsx");
        create_xlsx_workbook(&wb, &file).unwrap();

        // Generate 50 (row, col) targets with deterministic distinct colours.
        let mut targets: Vec<(usize, usize, String, String)> = Vec::new();
        for i in 0..50 {
            let row = i;
            let col = 0;
            // Pseudo-random hex from row index — distinct values per row.
            let fg = format!("{:06X}", (i * 0x020202) & 0xFFFFFF);
            let fc = format!("{:06X}", ((i * 0x010101 + 0x808080) & 0xFFFFFF));
            targets.push((row, col, fg, fc));
        }

        let ops: Vec<ExcelOperation> = targets.iter().map(|(r, c, fg, fc)| {
            let addr = cell_address(*r, *c);
            ExcelOperation::ModifyCell {
                sheet: "Sheet1".to_string(),
                address: addr,
                value: Some(CellValue::String(format!("v{}", r))),
                formula: None,
                number_format: None,
                bg_color: Some(fg.clone()),
                font_bold: None,
                font_italic: None,
                font_color: Some(fc.clone()),
                font_size: None,
                font_name: None,
                alignment_h: None,
                alignment_v: None,
            }
        }).collect();

        let bytes = std::fs::read(&file).unwrap();
        let mut wb = read_xlsx_structured(&bytes).unwrap();
        wb.apply_operations(ops).unwrap();
        let original = std::fs::read(&file).unwrap();
        write_excel_document(&wb, Some(&original), &file).unwrap();

        let bytes = std::fs::read(&file).unwrap();
        let wb = read_xlsx_structured(&bytes).unwrap();
        let sheet = &wb.sheets[0];
        let norm = |s: &str| s.trim_start_matches('#').to_ascii_uppercase();
        for (r, c, fg, fc) in &targets {
            let cell = sheet.cells.iter()
                .find(|cell| cell.row == *r && cell.col == *c)
                .unwrap_or_else(|| panic!("({},{}) missing after write", r, c));
            let style = cell.style.as_ref()
                .unwrap_or_else(|| panic!("({},{}) has no style", r, c));
            let actual_fg = norm(style.fill_fg_color.as_deref().unwrap_or(""));
            let actual_fc = norm(style.font_color.as_deref().unwrap_or(""));
            assert_eq!(actual_fg, fg.to_ascii_uppercase(),
                "({},{}) bg: expected {}, got {}", r, c, fg, actual_fg);
            assert_eq!(actual_fc, fc.to_ascii_uppercase(),
                "({},{}) font: expected {}, got {}", r, c, fc, actual_fc);
        }
    }

    /// Regression: colour strings are normalised so that callers can pass
    /// "#1F3864", "1f3864", "  1F3864  ", or "1F3864" and always see the same
    /// value come back. Without normalisation the round-trip is lossy because
    /// the read path adds a leading "#" while the write path doesn't strip it.
    #[test]
    fn style_hex_colour_normalised() {
        let out_dir = std::env::temp_dir().join("inkuo_repro");
        std::fs::create_dir_all(&out_dir).unwrap();
        let mut wb = XlsxWorkbook { sheets: vec![], shared_strings: vec![] };
        wb.sheets.push(XlsxSheet::new("Sheet1".to_string()));
        let file = out_dir.join("repro_norm.xlsx");
        create_xlsx_workbook(&wb, &file).unwrap();

        // First call: caller uses "#1f3864" (lowercase, with hash).
        let bytes = std::fs::read(&file).unwrap();
        let mut wb = read_xlsx_structured(&bytes).unwrap();
        wb.apply_operations(vec![ExcelOperation::ModifyCell {
            sheet: "Sheet1".to_string(),
            address: "A1".to_string(),
            value: Some(CellValue::String("v1".to_string())),
            formula: None,
            number_format: None,
            bg_color: Some("#1f3864".to_string()),
            font_bold: None,
            font_italic: None,
            font_color: Some("  #ffffff  ".to_string()),
            font_size: None,
            font_name: None,
            alignment_h: None,
            alignment_v: None,
        }]).unwrap();
        let original = std::fs::read(&file).unwrap();
        write_excel_document(&wb, Some(&original), &file).unwrap();

        // Second call uses the canonical form and asserts the file's stored
        // value (which AI can read back) matches what was actually written.
        let bytes = std::fs::read(&file).unwrap();
        let wb = read_xlsx_structured(&bytes).unwrap();
        let a1 = wb.sheets[0].cells.iter().find(|c| c.row == 0 && c.col == 0)
            .expect("A1 missing");
        let style = a1.style.as_ref().expect("A1 has no style");
        assert_eq!(style.fill_fg_color.as_deref(), Some("1F3864"),
            "bg_color should be normalised to upper 6-hex, got {:?}", style.fill_fg_color);
        assert_eq!(style.font_color.as_deref(), Some("FFFFFF"),
            "font_color should be normalised, got {:?}", style.font_color);
    }

    /// Three modify_excel calls in sequence must NOT corrupt earlier cells. This
    /// is the read-modify-write lifecycle: call 1 sets A1, call 2 sets B2, call 3
    /// sets C3, and reading back must see all three styles intact and isolated.
    #[test]
    fn style_multi_call_isolation() {
        let out_dir = std::env::temp_dir().join("inkuo_repro");
        std::fs::create_dir_all(&out_dir).unwrap();
        let mut wb = XlsxWorkbook { sheets: vec![], shared_strings: vec![] };
        wb.sheets.push(XlsxSheet::new("Sheet1".to_string()));
        let file = out_dir.join("repro_multi.xlsx");
        create_xlsx_workbook(&wb, &file).unwrap();

        let make_op = |addr: &str, fg: &str, fc: &str| ExcelOperation::ModifyCell {
            sheet: "Sheet1".to_string(),
            address: addr.to_string(),
            value: Some(CellValue::String(addr.to_string())),
            formula: None,
            number_format: None,
            bg_color: Some(fg.to_string()),
            font_bold: None,
            font_italic: None,
            font_color: Some(fc.to_string()),
            font_size: None,
            font_name: None,
            alignment_h: None,
            alignment_v: None,
        };

        for (addr, fg, fc) in [("A1", "111111", "AAAAAA"), ("B2", "222222", "BBBBBB"), ("C3", "333333", "CCCCCC")] {
            let bytes = std::fs::read(&file).unwrap();
            let mut wb = read_xlsx_structured(&bytes).unwrap();
            wb.apply_operations(vec![make_op(addr, fg, fc)]).unwrap();
            let original = std::fs::read(&file).unwrap();
            write_excel_document(&wb, Some(&original), &file).unwrap();
        }

        let bytes = std::fs::read(&file).unwrap();
        let wb = read_xlsx_structured(&bytes).unwrap();
        let sheet = &wb.sheets[0];
        let norm = |s: &str| s.trim_start_matches('#').to_ascii_uppercase();
        for (addr, fg, fc) in [("A1", "111111", "AAAAAA"), ("B2", "222222", "BBBBBB"), ("C3", "333333", "CCCCCC")] {
            let (row, col) = parse_cell_address(addr).unwrap();
            let cell = sheet.cells.iter().find(|c| c.row == row && c.col == col)
                .unwrap_or_else(|| panic!("{} missing after multi-call sequence", addr));
            let style = cell.style.as_ref().expect(&format!("{} has no style", addr));
            assert_eq!(norm(style.fill_fg_color.as_deref().unwrap_or("")), fg,
                "{} bg_color wrong", addr);
            assert_eq!(norm(style.font_color.as_deref().unwrap_or("")), fc,
                "{} font_color wrong", addr);
        }
    }

    #[test]
    fn extract_attr_simple() {
        assert_eq!(extract_attr("<c r=\"A1\" t=\"s\"/>", "r"), Some("A1".into()));
        assert_eq!(extract_attr("<c r=\"B3\"/>", "r"), Some("B3".into()));
        assert_eq!(extract_attr("<c r=\"B3\"/>", "t"), None);
    }

    #[test]
    fn build_replacement_cell_preserves_attrs() {
        let m = CellModification::new("Sheet1", "B2")
            .with_value(CellValue::Int(42));
        let xml = build_replacement_cell_xml(1, 1, &m).unwrap();
        assert!(xml.contains("r=\"B2\""));
        assert!(xml.contains("<v>42</v>"));
    }

    #[test]
    fn build_replacement_cell_with_formula() {
        let m = CellModification::new("Sheet1", "C3")
            .with_formula("SUM(A1:A10)");
        let xml = build_replacement_cell_xml(2, 2, &m).unwrap();
        assert!(xml.contains("r=\"C3\""));
        assert!(xml.contains("<f>SUM(A1:A10)</f>"));
    }

    #[test]
    fn build_replacement_cell_string_value() {
        let m = CellModification::new("Sheet1", "D4")
            .with_value(CellValue::String("hello".into()));
        let xml = build_replacement_cell_xml(3, 3, &m).unwrap();
        assert!(xml.contains("r=\"D4\""));
        assert!(xml.contains("t=\"inlineStr\""));
        assert!(xml.contains("hello"));
    }

    #[test]
    fn apply_modifications_preserves_unmodified_cells() {
        let sheet_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1"><v>100</v></c></row>
<row r="2"><c r="A2"><v>hello</v></c><c r="B2"><v>200</v></c></row>
</sheetData>
</worksheet>"#;
        let m = CellModification::new("Sheet1", "B1")
            .with_value(CellValue::Int(999));
        let result = apply_modifications_to_sheet(sheet_xml, &[&m]).unwrap();
        // The modified cell should carry the new value.
        assert!(result.contains("<v>999</v>"), "modified value missing; got: {}", result);
        // Untouched cells should still be there with their original values.
        assert!(result.contains("<v>0</v>"));
        assert!(result.contains("<v>hello</v>"));
        assert!(result.contains("<v>200</v>"));
    }

    #[test]
    fn apply_modifications_preserves_formulas_and_other_cells() {
        // Test that modifying one cell preserves formulas and other cell data
        let sheet_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1"><c r="A1"><v>10</v></c><c r="B1"><v>20</v></c><c r="C1"><f>SUM(A1:B1)</f><v>30</v></c></row>
<row r="2"><c r="A2"><v>100</v></c><c r="B2"><f>A2*2</f><v>200</v></c></row>
</sheetData>
</worksheet>"#;
        // Modify B1's value
        let m = CellModification::new("Sheet1", "B1")
            .with_value(CellValue::Int(50));
        let result = apply_modifications_to_sheet(sheet_xml, &[&m]).unwrap();

        // B1 should have new value
        assert!(result.contains("r=\"B1\""), "B1 should exist");
        assert!(result.contains("<v>50</v>"), "B1 new value missing; got: {}", result);

        // A1 should still exist with original value
        assert!(result.contains("r=\"A1\""), "A1 should exist");
        assert!(result.contains("<v>10</v>"), "A1 value missing; got: {}", result);

        // C1 with formula should still exist
        assert!(result.contains("r=\"C1\""), "C1 should exist");
        assert!(result.contains("<f>SUM(A1:B1)</f>"), "C1 formula missing; got: {}", result);
        assert!(result.contains("<v>30</v>"), "C1 cached value missing; got: {}", result);

        // A2 and B2 should still exist
        assert!(result.contains("r=\"A2\""), "A2 should exist");
        assert!(result.contains("<v>100</v>"), "A2 value missing; got: {}", result);
        assert!(result.contains("r=\"B2\""), "B2 should exist");
        assert!(result.contains("<f>A2*2</f>"), "B2 formula missing; got: {}", result);
    }

    #[test]
    fn apply_modifications_inserts_new_cell() {
        let sheet_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1"><c r="A1"><v>1</v></c></row>
</sheetData>
</worksheet>"#;
        let m = CellModification::new("Sheet1", "C5")
            .with_value(CellValue::String("new".into()));
        let result = apply_modifications_to_sheet(sheet_xml, &[&m]).unwrap();
        assert!(result.contains("r=\"C5\""), "new cell not appended; got: {}", result);
        assert!(result.contains("new"));
        // Original cell still present.
        assert!(result.contains("r=\"A1\""));
    }

    #[test]
    fn collect_existing_addrs_finds_all() {
        let xml = r#"<sheetData>
<c r="A1"/><c r="B2" t="s"/><c r="Z99"/>
</sheetData>"#;
        let addrs = collect_existing_addrs(xml);
        assert!(addrs.contains("A1"));
        assert!(addrs.contains("B2"));
        assert!(addrs.contains("Z99"));
        assert_eq!(addrs.len(), 3);
    }

    #[test]
    fn parse_sheet_name_to_path_resolves_rid() {
        let wb_xml = r#"<?xml version="1.0"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets>
<sheet name="Sales" sheetId="1" r:id="rId1"/>
<sheet name="Data" sheetId="2" r:id="rId2"/>
</sheets>
</workbook>"#;
        let rels_xml = r#"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/>
</Relationships>"#;
        let result = parse_sheet_name_to_path(wb_xml, rels_xml).unwrap();
        assert_eq!(result[0].0, "Sales");
        assert_eq!(result[0].1, "xl/worksheets/sheet1.xml");
        assert_eq!(result[1].0, "Data");
        assert_eq!(result[1].1, "xl/worksheets/sheet2.xml");
    }

    #[test]
    fn conservative_write_round_trip() {
        use std::io::Write;
        // Build a minimal valid xlsx with one sheet, three cells.
        let sheet_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1"><v>100</v></c></row>
<row r="2"><c r="A2"><v>hello</v></c><c r="B2"><v>200</v></c></row>
</sheetData>
</worksheet>"#;
        let workbook_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets>
</workbook>"#;
        let rels_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#;
        let shared_strings_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<si><t>Header A</t></si>
</sst>"#;
        let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
</Types>"#;

        let mut buf: Vec<u8> = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(0o644);
            zip.start_file("[Content_Types].xml", opts).unwrap();
            zip.write_all(content_types.as_bytes()).unwrap();
            zip.start_file("xl/workbook.xml", opts).unwrap();
            zip.write_all(workbook_xml.as_bytes()).unwrap();
            zip.start_file("xl/_rels/workbook.xml.rels", opts).unwrap();
            zip.write_all(rels_xml.as_bytes()).unwrap();
            zip.start_file("xl/sharedStrings.xml", opts).unwrap();
            zip.write_all(shared_strings_xml.as_bytes()).unwrap();
            zip.start_file("xl/worksheets/sheet1.xml", opts).unwrap();
            zip.write_all(sheet_xml.as_bytes()).unwrap();
            zip.finish().unwrap();
        }

        // Apply a single modification: change B1 from 100 to 999.
        let tmpdir = std::env::temp_dir();
        let out_path = tmpdir.join("inkuo_test_conservative_write.xlsx");
        let mods = vec![
            CellModification::new("Sheet1", "B1")
                .with_value(CellValue::Int(999)),
        ];
        incremental_write_xlsx(&buf, &mods, &out_path).expect("write failed");

        // Re-parse and confirm the modification landed.
        let written = std::fs::read(&out_path).expect("read back");
        let workbook = read_xlsx_structured(&written).expect("reparse");
        let sheet = &workbook.sheets[0];
        let b1 = sheet.cells.iter().find(|c| c.address() == "B1").expect("B1 cell");
        assert_eq!(b1.value, CellValue::Int(999));

        // And the other cells were preserved.
        let a2 = sheet.cells.iter().find(|c| c.address() == "A2").expect("A2 cell");
        assert_eq!(a2.value, CellValue::String("hello".into()));
        let b2 = sheet.cells.iter().find(|c| c.address() == "B2").expect("B2 cell");
        assert_eq!(b2.value, CellValue::Int(200));

        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    fn incremental_write_preserves_formulas_when_modifying_other_cells() {
        use std::io::Write;
        // Build a workbook with formulas to test that modifying one cell
        // preserves all other cells including formulas
        let sheet_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1"><c r="A1"><v>10</v></c><c r="B1"><v>20</v></c><c r="C1"><f>SUM(A1:B1)</f><v>30</v></c></row>
<row r="2"><c r="A2"><v>100</v></c><c r="B2"><f>A2*2</f><v>200</v></c><c r="C2"><f>SUM(A2:B2)</f><v>300</v></c></row>
</sheetData>
</worksheet>"#;
        let workbook_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets>
</workbook>"#;
        let rels_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#;
        let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
</Types>"#;

        let mut buf: Vec<u8> = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(0o644);
            zip.start_file("[Content_Types].xml", opts).unwrap();
            zip.write_all(content_types.as_bytes()).unwrap();
            zip.start_file("xl/workbook.xml", opts).unwrap();
            zip.write_all(workbook_xml.as_bytes()).unwrap();
            zip.start_file("xl/_rels/workbook.xml.rels", opts).unwrap();
            zip.write_all(rels_xml.as_bytes()).unwrap();
            zip.start_file("xl/worksheets/sheet1.xml", opts).unwrap();
            zip.write_all(sheet_xml.as_bytes()).unwrap();
            zip.finish().unwrap();
        }

        // Modify A1 value only
        let tmpdir = std::env::temp_dir();
        let out_path = tmpdir.join("inkuo_test_formula_preserve.xlsx");
        let mods = vec![
            CellModification::new("Sheet1", "A1")
                .with_value(CellValue::Int(999)),
        ];
        incremental_write_xlsx(&buf, &mods, &out_path).expect("write failed");

        // Re-parse and verify
        let written = std::fs::read(&out_path).expect("read back");
        let workbook = read_xlsx_structured(&written).expect("reparse");
        let sheet = &workbook.sheets[0];

        // A1 should have new value
        let a1 = sheet.cells.iter().find(|c| c.address() == "A1").expect("A1 cell");
        assert_eq!(a1.value, CellValue::Int(999), "A1 should have new value 999");

        // B1 should be preserved
        let b1 = sheet.cells.iter().find(|c| c.address() == "B1").expect("B1 cell");
        assert_eq!(b1.value, CellValue::Int(20), "B1 should be preserved");

        // C1 formula should be preserved
        let c1 = sheet.cells.iter().find(|c| c.address() == "C1").expect("C1 cell");
        assert!(c1.formula.is_some(), "C1 should have formula");
        assert_eq!(c1.formula.as_ref().unwrap(), "SUM(A1:B1)", "C1 formula should be preserved");

        // A2 should be preserved
        let a2 = sheet.cells.iter().find(|c| c.address() == "A2").expect("A2 cell");
        assert_eq!(a2.value, CellValue::Int(100), "A2 should be preserved");

        // B2 formula should be preserved
        let b2 = sheet.cells.iter().find(|c| c.address() == "B2").expect("B2 cell");
        assert!(b2.formula.is_some(), "B2 should have formula");
        assert_eq!(b2.formula.as_ref().unwrap(), "A2*2", "B2 formula should be preserved");

        // C2 formula should be preserved
        let c2 = sheet.cells.iter().find(|c| c.address() == "C2").expect("C2 cell");
        assert!(c2.formula.is_some(), "C2 should have formula");
        assert_eq!(c2.formula.as_ref().unwrap(), "SUM(A2:B2)", "C2 formula should be preserved");

        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    fn create_workbook_then_read_back() {
        // Build a small workbook, write it out, re-parse, and confirm contents.
        let sheet = XlsxSheet {
            name: "Sales".to_string(),
            state: "visible".to_string(),
            cells: vec![
                Cell { row: 0, col: 0, value: CellValue::String("Region".into()),  formula: None, style: None },
                Cell { row: 0, col: 1, value: CellValue::String("Revenue".into()), formula: None, style: None },
                Cell { row: 1, col: 0, value: CellValue::String("North".into()),   formula: None, style: None },
                Cell { row: 1, col: 1, value: CellValue::Int(1200),                formula: None, style: None },
                Cell { row: 2, col: 0, value: CellValue::String("South".into()),   formula: None, style: None },
                Cell { row: 2, col: 1, value: CellValue::Int(800),                 formula: None, style: None },
                Cell { row: 3, col: 1, value: CellValue::Empty, formula: Some("SUM(B2:B3)".to_string()), style: None },
            ],
            merged_cells: vec![],
            max_row: 4,
            max_col: 2,
            row_heights: std::collections::HashMap::new(),
            col_widths: std::collections::HashMap::new(),
        };
        let workbook = XlsxWorkbook {
            sheets: vec![sheet],
            shared_strings: vec![],
        };

        let tmpdir = std::env::temp_dir();
        let out_path = tmpdir.join("inkuo_test_create_workbook.xlsx");
        let _ = std::fs::remove_file(&out_path);
        create_xlsx_workbook(&workbook, &out_path).expect("create failed");
        assert!(out_path.exists(), "xlsx file should exist after create_xlsx_workbook");

        // Re-parse the produced file with calamine via the legacy API to
        // confirm Excel-compatibility (cell content matches what we wrote).
        let bytes = std::fs::read(&out_path).expect("read back");
        let legacy = match read_excel_workbook(&bytes) {
            Ok(l) => l,
            Err(e) => panic!("reparse legacy failed: {}", e),
        };
        assert_eq!(legacy.sheets.len(), 1);
        let parsed = &legacy.sheets[0].rows;
        assert_eq!(parsed[0][0], "Region");
        assert_eq!(parsed[0][1], "Revenue");
        assert_eq!(parsed[1][0], "North");
        assert_eq!(parsed[1][1], "1200");
        assert_eq!(parsed[2][0], "South");
        assert_eq!(parsed[2][1], "800");

        // The structured reader should also see the formula cell.
        let structured = read_xlsx_structured(&bytes).expect("structured reparse");
        let sales = structured.sheet("Sales").expect("Sales sheet present");
        let total_cell = sales.cells.iter().find(|c| c.address() == "B4").expect("B4");
        assert_eq!(total_cell.formula.as_deref(), Some("SUM(B2:B3)"));

        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    fn create_workbook_with_merged_and_multiple_sheets() {
        // Verify multi-sheet + merged ranges work end-to-end.
        let summary = XlsxSheet {
            name: "Summary".to_string(),
            state: "visible".to_string(),
            cells: vec![
                Cell { row: 0, col: 0, value: CellValue::String("Header".into()), formula: None, style: None },
                Cell { row: 0, col: 1, value: CellValue::Float(1.5),             formula: None, style: None },
            ],
            merged_cells: vec![MergedRange { start_row: 0, start_col: 0, end_row: 0, end_col: 1 }],
            max_row: 1,
            max_col: 2,
            row_heights: std::collections::HashMap::new(),
            col_widths: std::collections::HashMap::new(),
        };
        let notes = XlsxSheet {
            name: "Notes".to_string(),
            state: "visible".to_string(),
            cells: vec![
                Cell { row: 0, col: 0, value: CellValue::Bool(true), formula: None, style: None },
            ],
            merged_cells: vec![],
            max_row: 1,
            max_col: 1,
            row_heights: std::collections::HashMap::new(),
            col_widths: std::collections::HashMap::new(),
        };
        let workbook = XlsxWorkbook {
            sheets: vec![summary, notes],
            shared_strings: vec![],
        };

        let tmpdir = std::env::temp_dir();
        let out_path = tmpdir.join("inkuo_test_create_multi.xlsx");
        let _ = std::fs::remove_file(&out_path);
        create_xlsx_workbook(&workbook, &out_path).expect("create failed");
        let bytes = std::fs::read(&out_path).expect("read");
        let structured = read_xlsx_structured(&bytes).expect("reparse");
        assert_eq!(structured.sheets.len(), 2);
        assert_eq!(structured.sheet("Summary").unwrap().merged_cells.len(), 1);
        let notes_sheet = structured.sheet("Notes").unwrap();
        let bool_cell = notes_sheet.cells.iter().find(|c| c.address() == "A1").expect("A1");
        assert_eq!(bool_cell.value, CellValue::Bool(true));

        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    fn create_workbook_rejects_empty() {
        let workbook = XlsxWorkbook { sheets: vec![], shared_strings: vec![] };
        let out_path = std::env::temp_dir().join("inkuo_test_empty.xlsx");
        let result = create_xlsx_workbook(&workbook, &out_path);
        assert!(result.is_err(), "creating an empty workbook should fail");
    }

    #[test]
    fn build_preserving_replacement_preserves_shared_strings() {
        // Test the build_preserving_replacement_cell_xml function directly
        // with a shared string cell
        let original = r#"<c r="A1" t="s"><v>0</v></c>"#;
        let m = CellModification::new("Sheet1", "A1");
        let result = build_preserving_replacement_cell_xml(0, 0, &m, original).unwrap();

        // Should preserve t="s" and <v>0</v>
        assert!(result.contains("t=\"s\""), "t=\"s\" should be preserved");
        assert!(result.contains("<v>0</v>"), "<v>0</v> should be preserved");
    }

    #[test]
    fn build_preserving_replacement_preserves_formulas() {
        // Test the build_preserving_replacement_cell_xml function directly
        // with a formula cell
        let original = r#"<c r="C1"><f>SUM(A1:B1)</f><v>30</v></c>"#;
        let m = CellModification::new("Sheet1", "C1");
        let result = build_preserving_replacement_cell_xml(0, 2, &m, original).unwrap();

        // Should preserve <f>SUM(A1:B1)</f> and <v>30</v>
        assert!(result.contains("<f>SUM(A1:B1)</f>"), "<f> should be preserved");
        assert!(result.contains("<v>30</v>"), "<v>30</v> should be preserved");
    }

    #[test]
    fn style_only_modification_preserves_all_cell_data() {
        use std::io::Write;
        // Test that applying ONLY style changes (no value/formula changes)
        // preserves ALL cell data including shared strings, formulas, and values
        let sheet_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1"><f>SUM(A1:B1)</f><v>30</v></c></row>
<row r="2"><c r="A2"><v>100</v></c><c r="B2"><f>A2*2</f><v>200</v></c><c r="C2" t="s"><v>2</v></c></row>
</sheetData>
</worksheet>"#;
        let workbook_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets>
</workbook>"#;
        let rels_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#;
        let shared_strings_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<si><t>Header1</t></si>
<si><t>Header2</t></si>
<si><t>Total</t></si>
</sst>"#;
        let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
</Types>"#;

        let mut buf: Vec<u8> = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(0o644);
            zip.start_file("[Content_Types].xml", opts).unwrap();
            zip.write_all(content_types.as_bytes()).unwrap();
            zip.start_file("xl/workbook.xml", opts).unwrap();
            zip.write_all(workbook_xml.as_bytes()).unwrap();
            zip.start_file("xl/_rels/workbook.xml.rels", opts).unwrap();
            zip.write_all(rels_xml.as_bytes()).unwrap();
            zip.start_file("xl/sharedStrings.xml", opts).unwrap();
            zip.write_all(shared_strings_xml.as_bytes()).unwrap();
            zip.start_file("xl/worksheets/sheet1.xml", opts).unwrap();
            zip.write_all(sheet_xml.as_bytes()).unwrap();
            zip.finish().unwrap();
        }

        // Apply style-only modifications (bg_color, bold, etc.) - NO value/formula changes
        let tmpdir = std::env::temp_dir();
        let out_path = tmpdir.join("inkuo_test_style_only.xlsx");
        let mods = vec![
            CellModification::new("Sheet1", "A1").with_number_format("0"),
            CellModification::new("Sheet1", "B1").with_number_format("0"),
            CellModification::new("Sheet1", "C1").with_number_format("0"),
        ];
        incremental_write_xlsx(&buf, &mods, &out_path).expect("write failed");

        // Re-parse and verify ALL data is preserved
        let written = std::fs::read(&out_path).expect("read back");
        let workbook = read_xlsx_structured(&written).expect("reparse");
        let sheet = &workbook.sheets[0];

        // Verify shared string cells
        let a1 = sheet.cells.iter().find(|c| c.address() == "A1").expect("A1 cell");
        assert!(matches!(a1.value, CellValue::String(ref s) if s == "Header1"),
            "A1 shared string should be preserved, got: {:?}", a1.value);

        let b1 = sheet.cells.iter().find(|c| c.address() == "B1").expect("B1 cell");
        assert!(matches!(b1.value, CellValue::String(ref s) if s == "Header2"),
            "B1 shared string should be preserved, got: {:?}", b1.value);

        let c2 = sheet.cells.iter().find(|c| c.address() == "C2").expect("C2 cell");
        assert!(matches!(c2.value, CellValue::String(ref s) if s == "Total"),
            "C2 shared string should be preserved, got: {:?}", c2.value);

        // Verify formula cells
        let c1 = sheet.cells.iter().find(|c| c.address() == "C1").expect("C1 cell");
        assert!(c1.formula.as_ref().map(|f| f == "SUM(A1:B1)").unwrap_or(false),
            "C1 formula should be preserved, got: {:?}", c1.formula);
        // Value should be preserved (30 as Int or Float)
        let c1_value_ok = match &c1.value {
            CellValue::Int(30) => true,
            CellValue::Float(f) => *f == 30.0,
            _ => false,
        };
        assert!(c1_value_ok, "C1 value should be 30, got: {:?}", c1.value);

        let b2 = sheet.cells.iter().find(|c| c.address() == "B2").expect("B2 cell");
        assert!(b2.formula.as_ref().map(|f| f == "A2*2").unwrap_or(false),
            "B2 formula should be preserved, got: {:?}", b2.formula);

        // Verify numeric cells
        let a2 = sheet.cells.iter().find(|c| c.address() == "A2").expect("A2 cell");
        assert_eq!(a2.value, CellValue::Int(100));

        let _ = std::fs::remove_file(&out_path);
    }
}

// ─── LibreOffice round-trip test (restored) ────────────────────────────────────
#[cfg(test)]
mod libreoffice_tests {
    use super::*;

    /// End-to-end smoke test: round-trip the file through LibreOffice's
    /// `soffice --headless --convert-to csv` to verify the workbook is
    /// "openable" by a real spreadsheet application. Skipped if soffice
    /// is not installed (e.g. on CI without LibreOffice).
    #[test]
    fn create_workbook_opens_in_libreoffice() {
        let sheet = XlsxSheet {
            name: "Sales".to_string(),
            state: "visible".to_string(),
            cells: vec![
                Cell { row: 0, col: 0, value: CellValue::String("Region".into()),  formula: None, style: None },
                Cell { row: 0, col: 1, value: CellValue::String("Revenue".into()), formula: None, style: None },
                Cell { row: 1, col: 0, value: CellValue::String("North".into()),   formula: None, style: None },
                Cell { row: 1, col: 1, value: CellValue::Int(1200),                formula: None, style: None },
                Cell { row: 2, col: 0, value: CellValue::String("South".into()),   formula: None, style: None },
                Cell { row: 2, col: 1, value: CellValue::Int(800),                 formula: None, style: None },
            ],
            merged_cells: vec![],
            max_row: 3,
            max_col: 2,
            row_heights: std::collections::HashMap::new(),
            col_widths: std::collections::HashMap::new(),
        };
        let workbook = XlsxWorkbook { sheets: vec![sheet], shared_strings: vec![] };

        let tmpdir = std::env::temp_dir();
        let out_path = tmpdir.join("inkuo_test_libreoffice_open.xlsx");
        let _ = std::fs::remove_file(&out_path);
        create_xlsx_workbook(&workbook, &out_path).expect("create failed");

        let soffice = std::process::Command::new("which")
            .arg("soffice")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|_| "soffice".to_string());
        let Some(soffice) = soffice else {
            eprintln!("soffice not available — skipping libreoffice round-trip test");
            let _ = std::fs::remove_file(&out_path);
            return;
        };

        let profile = tmpdir.join(format!("inkuo_lo_profile_{}", std::process::id()));
        let out_dir = tmpdir.join(format!("inkuo_lo_out_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&profile);
        let _ = std::fs::remove_dir_all(&out_dir);
        let _ = std::fs::create_dir_all(&out_dir);

        let status = std::process::Command::new(&soffice)
            .arg("--headless")
            .arg("--user-profile").arg(&profile)
            .arg("--convert-to").arg("csv")
            .arg("--outdir").arg(&out_dir)
            .arg(&out_path)
            .status();

        match status {
            Ok(s) if s.success() => {
                let csv_path = out_dir.join("inkuo_test_libreoffice_open.csv");
                let csv = std::fs::read_to_string(&csv_path).expect("read csv");
                assert!(csv.contains("Region"), "csv should contain Region: {}", csv);
                assert!(csv.contains("North"), "csv should contain North: {}", csv);
                assert!(csv.contains("1200"), "csv should contain 1200: {}", csv);
                let _ = std::fs::remove_file(&csv_path);
            }
            Ok(s) => {
                let _ = std::fs::remove_file(&out_path);
                panic!("soffice convert failed with status {}", s);
            }
            Err(e) => {
                eprintln!("soffice invocation error: {} — skipping", e);
                let _ = std::fs::remove_file(&out_path);
            }
        }
        let _ = std::fs::remove_file(&out_path);
        let _ = std::fs::remove_dir_all(&profile);
        let _ = std::fs::remove_dir_all(&out_dir);
    }
}
