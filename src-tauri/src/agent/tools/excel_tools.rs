//! Excel file tools for AI-assisted spreadsheet operations.
//!
//! Provides fine-grained tools for:
//! - Reading specific cell ranges (token-efficient)
//! - Reading sheet metadata (row heights, col widths, merged cells)
//! - Formatting cells (background color, font, alignment)
//! - Merging / unmerging cells
//! - Resizing rows and columns
//! - Batch-writing values to ranges
//! - Managing sheets (create, rename, delete, hide)

use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;

use super::{ToolDefinition, ToolError, ToolParameters, validate_workspace_path};
use crate::office::{
    read_xlsx_structured,
    cell_address,
    parse_cell_address,
    CellModification,
    CellValue,
    XlsxSheet,
    MergeModification,
    MergeOp,
    RowColModification,
    merge_cells_xlsx,
    resize_rows_cols_xlsx,
    create_sheet_xlsx,
    rename_sheet_xlsx,
    delete_sheet_xlsx,
    set_sheet_state_xlsx,
    incremental_write_xlsx,
};

fn validate_xlsx_path(path: &str) -> Result<(), ToolError> {
    let path_obj = std::path::Path::new(path);
    if path_obj.extension().and_then(|e| e.to_str()).unwrap_or("") != "xlsx" {
        return Err(ToolError::InvalidArguments(
            "(excel tool)".to_string(),
            "Only .xlsx files are supported".into(),
        ));
    }
    Ok(())
}

/// Parse a column letter (e.g. "A", "Z", "AA", "ABC") into a 0-based column index.
fn parse_col_letter(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes.iter().any(|b| !b.is_ascii_alphabetic()) {
        return None;
    }
    let mut col: usize = 0;
    for &b in bytes {
        col = col * 26 + (b.to_ascii_uppercase() - b'A' + 1) as usize;
    }
    Some(col.saturating_sub(1))
}

// ─── read_excel_range ─────────────────────────────────────────────────────────

/// Read a specific range of cells from an Excel sheet, returning values,
/// formulas, and styles. Much more token-efficient than reading the whole sheet.
pub struct ReadExcelRangeTool;

impl ReadExcelRangeTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "read_excel_range",
            "读取 Excel 区域",
            "Read a specific rectangular range of cells from an Excel (.xlsx) sheet. Returns values, formulas, and styles for each cell. Use this instead of read_office_file when you only need a portion of the sheet.",
            ToolParameters::new(
                vec!["path", "sheet", "range"],
                vec![
                    ("path", "string", Some("Absolute path to the .xlsx file")),
                    ("sheet", "string", Some("Sheet name (case-sensitive)")),
                    ("range", "string", Some("Cell range in A1:B3 form, e.g. \"A1:D10\". Single cell like \"B2\" is also valid. Use \"1:10\" for entire rows, \"A:A\" for entire columns.")),
                    ("include_styles", "string", Some("Comma-separated list of style properties to include: bg_color, font_color, font_bold, font_italic, font_size, font_name, alignment_h, alignment_v, number_format. Default: bg_color,font_color,number_format")),
                ],
            ),
        )
    }

    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("read_excel_range".to_string(), "path must be a string".into()))?;
        let sheet_name = arguments["sheet"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("read_excel_range".to_string(), "sheet must be a string".into()))?;
        let range_str = arguments["range"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("read_excel_range".to_string(), "range must be a string".into()))?;
        let style_str = arguments["include_styles"].as_str().unwrap_or("bg_color,font_color,number_format");

        validate_workspace_path(path, &workspace)?;
        validate_xlsx_path(path)?;

        let bytes = tokio::fs::read(path).await
            .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;
        let workbook = read_xlsx_structured(&bytes)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to parse xlsx: {}", e)))?;

        let sheet = workbook.sheets.iter()
            .find(|s| s.name == sheet_name)
            .ok_or_else(|| ToolError::InvalidArguments(
                "sheet".to_string(),
                format!("Sheet '{}' not found. Available: {:?}", sheet_name,
                    workbook.sheets.iter().map(|s| &s.name).collect::<Vec<_>>()),
            ))?;

        // Parse range
        let ((sr, sc), (er, ec)) = parse_range_string(range_str, sheet)?;

        let style_fields: HashSet<&str> = style_str.split(',').map(|s| s.trim()).collect();

        // Build result
        let mut cells_out: Vec<serde_json::Value> = Vec::new();
        for cell in &sheet.cells {
            if cell.row >= sr && cell.row <= er && cell.col >= sc && cell.col <= ec {
                let addr = cell_address(cell.row, cell.col);
                let display = if let Some(ref f) = cell.formula {
                    format!("={}", f)
                } else {
                    cell.value.as_string_for_display()
                };

                let raw_type = match &cell.value {
                    CellValue::Empty => "empty",
                    CellValue::Int(_) => "int",
                    CellValue::Float(_) => "float",
                    CellValue::Bool(_) => "bool",
                    CellValue::String(_) => "string",
                    CellValue::Error(_) => "error",
                    CellValue::DateTime(_) => "datetime",
                };

                let mut cell_obj = serde_json::json!({
                    "address": addr,
                    "row": cell.row,
                    "col": cell.col,
                    "value": display,
                    "raw_type": raw_type,
                });

                if let Some(ref f) = cell.formula {
                    cell_obj["formula"] = serde_json::json!(f);
                }

                if let Some(ref style) = cell.style {
                    if style_fields.contains("bg_color") {
                        if let Some(ref bg) = style.fill_fg_color {
                            cell_obj["bg_color"] = serde_json::json!(bg);
                        }
                    }
                    if style_fields.contains("font_color") {
                        if let Some(ref fc) = style.font_color {
                            cell_obj["font_color"] = serde_json::json!(fc);
                        }
                    }
                    if style_fields.contains("font_bold") {
                        cell_obj["font_bold"] = serde_json::json!(style.font_bold);
                    }
                    if style_fields.contains("font_italic") {
                        cell_obj["font_italic"] = serde_json::json!(style.font_italic);
                    }
                    if style_fields.contains("font_size") {
                        if let Some(fs) = style.font_size {
                            cell_obj["font_size"] = serde_json::json!(fs);
                        }
                    }
                    if style_fields.contains("font_name") {
                        if let Some(ref fn_) = style.font_name {
                            cell_obj["font_name"] = serde_json::json!(fn_);
                        }
                    }
                    if style_fields.contains("alignment_h") {
                        if let Some(ref ah) = style.alignment_h {
                            cell_obj["alignment_h"] = serde_json::json!(ah);
                        }
                    }
                    if style_fields.contains("alignment_v") {
                        if let Some(ref av) = style.alignment_v {
                            cell_obj["alignment_v"] = serde_json::json!(av);
                        }
                    }
                    if style_fields.contains("number_format") && !style.number_format.is_empty() {
                        cell_obj["number_format"] = serde_json::json!(&style.number_format);
                    }
                }

                cells_out.push(cell_obj);
            }
        }

        let result = serde_json::json!({
            "path": path,
            "file_name": std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
            "sheet": sheet_name,
            "range": {
                "start": cell_address(sr, sc),
                "end": cell_address(er, ec),
                "rows": er - sr + 1,
                "cols": ec - sc + 1,
            },
            "cell_count": cells_out.len(),
            "cells": cells_out,
        });

        serde_json::to_string(&result)
            .map_err(|e| ToolError::ExecutionError(format!("JSON serialization failed: {}", e)))
    }
}

impl Default for ReadExcelRangeTool {
    fn default() -> Self { Self::new() }
}

/// Parse a range string into ((start_row, start_col), (end_row, end_col)) using 0-based indices.
fn parse_range_string(range_str: &str, sheet: &XlsxSheet) -> Result<((usize, usize), (usize, usize)), ToolError> {
    if range_str.contains(':') {
        let parts: Vec<&str> = range_str.split(':').collect();
        if parts.len() != 2 {
            return Err(ToolError::InvalidArguments("range".to_string(), format!("Invalid range '{}'", range_str)));
        }

        // Row-only range like "1:10"
        if parts[0].chars().all(|c| c.is_ascii_digit()) {
            let row_start: usize = parts[0].parse()
                .map_err(|_| ToolError::InvalidArguments("range".to_string(), format!("Invalid row number '{}'", parts[0])))?;
            let row_end: usize = parts[1].parse()
                .map_err(|_| ToolError::InvalidArguments("range".to_string(), format!("Invalid row number '{}'", parts[1])))?;
            let sr = row_start.saturating_sub(1);
            let er = row_end.saturating_sub(1);
            let sc = 0;
            let ec = sheet.max_col.saturating_sub(1);
            if sr > er {
                return Err(ToolError::InvalidArguments("range".to_string(), "Invalid range: start row > end row".into()));
            }
            return Ok(((sr, sc), (er, ec)));
        }

        // Column-only range like "A:A"
        if parts[1].chars().all(|c| c.is_ascii_alphabetic()) {
            let cs = parse_col_letter(parts[0])
                .ok_or_else(|| ToolError::InvalidArguments("range".to_string(), format!("Invalid column '{}'", parts[0])))?;
            let ce = parse_col_letter(parts[1])
                .ok_or_else(|| ToolError::InvalidArguments("range".to_string(), format!("Invalid column '{}'", parts[1])))?;
            if cs > ce {
                return Err(ToolError::InvalidArguments("range".to_string(), "Invalid range: start col > end col".into()));
            }
            return Ok(((0, cs), (sheet.max_row.saturating_sub(1), ce)));
        }

        // Standard A1:B3 range
        let (sr, sc) = parse_cell_address(parts[0])
            .ok_or_else(|| ToolError::InvalidArguments("range".to_string(), format!("Invalid address '{}'", parts[0])))?;
        let (er, ec) = parse_cell_address(parts[1])
            .ok_or_else(|| ToolError::InvalidArguments("range".to_string(), format!("Invalid address '{}'", parts[1])))?;
        if sr > er || sc > ec {
            return Err(ToolError::InvalidArguments("range".to_string(), "Invalid range: start is after end".into()));
        }
        Ok(((sr, sc), (er, ec)))
    } else {
        // Single cell
        let (r, c) = parse_cell_address(range_str)
            .ok_or_else(|| ToolError::InvalidArguments("range".to_string(), format!("Invalid cell address '{}'", range_str)))?;
        Ok(((r, c), (r, c)))
    }
}

// ─── read_excel_metadata ───────────────────────────────────────────────────────

/// Read sheet-level metadata: merged cells, used range, formulas, etc.
pub struct ReadExcelMetadataTool;

impl ReadExcelMetadataTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "read_excel_metadata",
            "读取 Excel 元数据",
            "Read sheet-level metadata from an Excel (.xlsx) file without returning cell values. Returns merged cell ranges, used range, cell count per sheet, formula count, and sheet names. Use this for a quick overview before reading specific ranges.",
            ToolParameters::new(
                vec!["path"],
                vec![
                    ("path", "string", Some("Absolute path to the .xlsx file")),
                    ("sheet", "string", Some("Optional: specific sheet name to get metadata for. If omitted, returns metadata for all sheets.")),
                ],
            ),
        )
    }

    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("read_excel_metadata".to_string(), "path must be a string".into()))?;
        let sheet_filter = arguments["sheet"].as_str();

        validate_workspace_path(path, &workspace)?;
        validate_xlsx_path(path)?;

        let bytes = tokio::fs::read(path).await
            .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;
        let workbook = read_xlsx_structured(&bytes)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to parse xlsx: {}", e)))?;

        let sheets: Vec<_> = if let Some(name) = sheet_filter {
            workbook.sheets.iter().filter(|s| s.name == name).collect()
        } else {
            workbook.sheets.iter().collect()
        };

        if sheet_filter.is_some() && sheets.is_empty() {
            return Err(ToolError::InvalidArguments(
                "sheet".to_string(),
                format!("Sheet '{}' not found. Available: {:?}", sheet_filter.unwrap(),
                    workbook.sheets.iter().map(|s| &s.name).collect::<Vec<_>>()),
            ));
        }

        let sheet_meta: Vec<serde_json::Value> = sheets.iter().map(|s| {
            let formula_cells: Vec<_> = s.cells.iter()
                .filter(|c| c.formula.is_some())
                .map(|c| {
                    serde_json::json!({
                        "address": c.address(),
                        "formula": c.formula.as_ref().unwrap(),
                    })
                })
                .collect();

            let merged_info: Vec<serde_json::Value> = s.merged_cells.iter().map(|m| {
                serde_json::json!({
                    "address": m.address(),
                    "start_row": m.start_row,
                    "start_col": m.start_col,
                    "end_row": m.end_row,
                    "end_col": m.end_col,
                    "rows": m.end_row - m.start_row + 1,
                    "cols": m.end_col - m.start_col + 1,
                })
            }).collect();

            serde_json::json!({
                "name": s.name,
                "state": s.state,
                "max_row": s.max_row,
                "max_col": s.max_col,
                "used_range": format!("A1:{}", cell_address(s.max_row.saturating_sub(1), s.max_col.saturating_sub(1))),
                "cell_count": s.cells.len(),
                "formula_count": formula_cells.len(),
                "merged_cells": merged_info,
                "formulas": formula_cells,
            })
        }).collect();

        let result = serde_json::json!({
            "path": path,
            "file_name": std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
            "sheet_count": workbook.sheets.len(),
            "sheets": sheet_meta,
        });

        serde_json::to_string(&result)
            .map_err(|e| ToolError::ExecutionError(format!("JSON serialization failed: {}", e)))
    }
}

impl Default for ReadExcelMetadataTool {
    fn default() -> Self { Self::new() }
}

// ─── write_excel_range ─────────────────────────────────────────────────────────

/// Write values to a rectangular range in an Excel sheet.
pub struct WriteExcelRangeTool;

impl WriteExcelRangeTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "write_excel_range",
            "写入 Excel 区域",
            "Write a 2D array of values into a rectangular range of an Excel (.xlsx) sheet. Values are written row by row starting from start_cell. Much more efficient than calling modify_excel for each cell individually.",
            ToolParameters::new(
                vec!["path", "sheet", "start_cell", "values"],
                vec![
                    ("path", "string", Some("Absolute path to the .xlsx file to modify")),
                    ("sheet", "string", Some("Sheet name (case-sensitive)")),
                    ("start_cell", "string", Some("Top-left cell of the target range in A1 form, e.g. \"A1\" or \"C5\"")),
                    ("values", "array", Some("2D array of rows. Each row is an array of cell values.\n\
                         Each value is an object: {type: \"string\"|\"int\"|\"float\"|\"bool\"|\"datetime\"|\"error\"|\"empty\", value?: any}.\n\
                         Example: [[{type:\"string\",value:\"Name\"}, {type:\"string\",value:\"Age\"}],\n\
                         [{type:\"string\",value:\"Alice\"}, {type:\"int\",value:30}]]\n\
                         writes a 2x2 table starting from start_cell.")),
                    ("number_format", "string", Some("Optional: Excel number format to apply to all written cells, e.g. \"0.00\" or \"yyyy-mm-dd\"")),
                ],
            ),
        )
    }

    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("write_excel_range".to_string(), "path must be a string".into()))?;
        let sheet_name = arguments["sheet"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("write_excel_range".to_string(), "sheet must be a string".into()))?;
        let start_cell = arguments["start_cell"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("write_excel_range".to_string(), "start_cell must be a string".into()))?;
        let number_format = arguments["number_format"].as_str();

        validate_workspace_path(path, &workspace)?;
        validate_xlsx_path(path)?;

        let values = arguments["values"].as_array()
            .ok_or_else(|| ToolError::InvalidArguments("write_excel_range".to_string(), "values must be an array".into()))?;

        if values.is_empty() {
            return Err(ToolError::InvalidArguments("write_excel_range".to_string(), "values array is empty".into()));
        }

        let (start_row, start_col) = parse_cell_address(start_cell)
            .ok_or_else(|| ToolError::InvalidArguments("start_cell".to_string(), format!("Invalid cell address '{}'", start_cell)))?;

        let mut modifications: Vec<CellModification> = Vec::new();
        for (row_offset, row) in values.iter().enumerate() {
            let row_arr = row.as_array().ok_or_else(|| ToolError::InvalidArguments(
                "values".to_string(), format!("values[{}] must be an array", row_offset)
            ))?;
            for (col_offset, cell) in row_arr.iter().enumerate() {
                let cell_obj = cell.as_object().ok_or_else(|| ToolError::InvalidArguments(
                    "values".to_string(),
                    format!("values[{}][{}] must be an object {{type, value?}}", row_offset, col_offset)
                ))?;
                let kind = cell_obj.get("type").and_then(|t| t.as_str())
                    .ok_or_else(|| ToolError::InvalidArguments(
                        "values".to_string(),
                        format!("values[{}][{}] missing 'type' field", row_offset, col_offset)
                    ))?;
                let raw_value = cell_obj.get("value");

                let target_row = start_row + row_offset;
                let target_col = start_col + col_offset;
                let target_addr = cell_address(target_row, target_col);

                let cell_value = parse_cell_value_from_json(kind, raw_value)?;
                let mut m = CellModification::new(sheet_name, &target_addr);
                m.new_value = Some(cell_value);
                if let Some(fmt) = number_format {
                    m.new_number_format = Some(fmt.to_string());
                }
                modifications.push(m);
            }
        }

        let bytes = tokio::fs::read(path).await
            .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;
        let path_obj = std::path::Path::new(path);
        let tmp_path = path_obj.with_extension("xlsx.tmp");

        incremental_write_xlsx(&bytes, &modifications, &tmp_path)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to write xlsx: {}", e)))?;
        tokio::fs::rename(&tmp_path, &path).await
            .map_err(|e| ToolError::IoError(format!("Failed to rename temp file: {}", e)))?;

        Ok(format!(
            "Successfully wrote {} cell(s) to {}!{} in {}",
            modifications.len(),
            sheet_name,
            start_cell,
            path
        ))
    }
}

impl Default for WriteExcelRangeTool {
    fn default() -> Self { Self::new() }
}

fn parse_cell_value_from_json(kind: &str, raw_value: Option<&Value>) -> Result<CellValue, ToolError> {
    match kind {
        "empty" => Ok(CellValue::Empty),
        "int" => {
            let n = raw_value.and_then(|v| v.as_i64())
                .ok_or_else(|| ToolError::InvalidArguments("value".to_string(), "int.value missing or not an integer".into()))?;
            Ok(CellValue::Int(n))
        }
        "float" => {
            let n = raw_value.and_then(|v| v.as_f64())
                .ok_or_else(|| ToolError::InvalidArguments("value".to_string(), "float.value missing or not a number".into()))?;
            Ok(CellValue::Float(n))
        }
        "bool" => {
            let b = raw_value.and_then(|v| v.as_bool())
                .ok_or_else(|| ToolError::InvalidArguments("value".to_string(), "bool.value missing or not a boolean".into()))?;
            Ok(CellValue::Bool(b))
        }
        "string" => {
            let s = raw_value.and_then(|v| v.as_str())
                .ok_or_else(|| ToolError::InvalidArguments("value".to_string(), "string.value missing or not a string".into()))?;
            Ok(CellValue::String(s.to_string()))
        }
        "datetime" => {
            let n = raw_value.and_then(|v| v.as_f64())
                .ok_or_else(|| ToolError::InvalidArguments("value".to_string(), "datetime.value missing or not a number".into()))?;
            Ok(CellValue::DateTime(n))
        }
        "error" => {
            let s = raw_value.and_then(|v| v.as_str())
                .unwrap_or("#ERR");
            Ok(CellValue::Error(s.to_string()))
        }
        other => Err(ToolError::InvalidArguments("type".to_string(), format!("unknown value type '{}'", other))),
    }
}

// ─── format_excel_cells ───────────────────────────────────────────────────────

/// A style operation for format_excel_cells.
#[derive(Debug, Deserialize)]
struct FormatCell {
    address: String,
    #[serde(default)]
    bg_color: Option<String>,
    #[serde(default)]
    font_color: Option<String>,
    #[serde(default)]
    bold: Option<bool>,
    #[serde(default)]
    italic: Option<bool>,
    #[serde(default)]
    font_size: Option<u32>,
    #[serde(default)]
    font_name: Option<String>,
    #[serde(default)]
    alignment_h: Option<String>,
    #[serde(default)]
    alignment_v: Option<String>,
    #[serde(default)]
    number_format: Option<String>,
}

/// Apply cell formatting (background color, font styles, alignment) to cells.
pub struct FormatExcelCellsTool;

impl FormatExcelCellsTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "format_excel_cells",
            "格式化 Excel 单元格",
            "Apply cell formatting (background color, font styles, alignment, number format) to specific cells in an Excel (.xlsx) file. All other cells and content are preserved.",
            ToolParameters::new(
                vec!["path", "sheet", "cells"],
                vec![
                    ("path", "string", Some("Absolute path to the .xlsx file to modify")),
                    ("sheet", "string", Some("Sheet name (case-sensitive)")),
                    ("cells", "array", Some("Array of cell formatting specs. Each entry: {address, bg_color?, font_color?, bold?, italic?, font_size?, font_name?, alignment_h?, alignment_v?, number_format?}.\n\
                         - address: cell address in A1 form (e.g. \"B3\")\n\
                         - bg_color: background fill color as 6-digit hex RGB without #, e.g. \"FFFF00\" for yellow. \"none\" removes background.\n\
                         - font_color: font color as 6-digit hex RGB, e.g. \"FF0000\" for red\n\
                         - bold: true/false for bold font\n\
                         - italic: true/false for italic font\n\
                         - font_size: font size in points, e.g. 12\n\
                         - font_name: font family, e.g. \"Calibri\"\n\
                         - alignment_h: \"left\" | \"center\" | \"right\"\n\
                         - alignment_v: \"top\" | \"center\" | \"bottom\"\n\
                         - number_format: Excel format string, e.g. \"0.00%\", \"yyyy-mm-dd\", \"#,##0\"\n\
                         Only include fields you want to change.")),
                ],
            ),
        )
    }

    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("format_excel_cells".to_string(), "path must be a string".into()))?;
        let sheet_name = arguments["sheet"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("format_excel_cells".to_string(), "sheet must be a string".into()))?;

        validate_workspace_path(path, &workspace)?;
        validate_xlsx_path(path)?;

        let cells_json = arguments["cells"].as_array()
            .ok_or_else(|| ToolError::InvalidArguments("format_excel_cells".to_string(), "cells must be an array".into()))?;

        if cells_json.is_empty() {
            return Err(ToolError::InvalidArguments("format_excel_cells".to_string(), "cells array is empty".into()));
        }

        let mut parsed: Vec<FormatCell> = Vec::new();
        for (i, v) in cells_json.iter().enumerate() {
            let cell: FormatCell = serde_json::from_value(v.clone())
                .map_err(|e| ToolError::InvalidArguments(
                    "format_excel_cells".to_string(),
                    format!("cells[{}]: {}", i, e),
                ))?;
            if parse_cell_address(&cell.address).is_none() {
                return Err(ToolError::InvalidArguments(
                    "format_excel_cells".to_string(),
                    format!("cells[{}]: invalid address '{}'", i, cell.address),
                ));
            }
            parsed.push(cell);
        }

        // Read to verify sheet exists and get existing cells
        let bytes = tokio::fs::read(path).await
            .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;
        let workbook = read_xlsx_structured(&bytes)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to parse xlsx: {}", e)))?;

        let sheet = workbook.sheets.iter()
            .find(|s| s.name == sheet_name)
            .ok_or_else(|| ToolError::InvalidArguments(
                "sheet".to_string(),
                format!("Sheet '{}' not found. Available: {:?}",
                    sheet_name, workbook.sheets.iter().map(|s| &s.name).collect::<Vec<_>>()),
            ))?;

        let existing: HashSet<String> = sheet.cells.iter()
            .map(|c| c.address())
            .collect();

        let mut modifications: Vec<CellModification> = Vec::new();
        for fc in &parsed {
            let addr = fc.address.to_ascii_uppercase();
            let mut m = CellModification::new(sheet_name, &addr);

            if let Some(ref bg) = fc.bg_color {
                m.new_bg_color = if bg == "none" {
                    Some(String::new()) // signals "remove"
                } else {
                    Some(bg.clone())
                };
            }
            if let Some(ref fc_val) = fc.font_color {
                m.new_font_color = Some(fc_val.clone());
            }
            if let Some(bold) = fc.bold {
                m.new_font_bold = Some(bold);
            }
            if let Some(italic) = fc.italic {
                m.new_font_italic = Some(italic);
            }
            if let Some(fs) = fc.font_size {
                m.new_font_size = Some(fs);
            }
            if let Some(ref fn_) = fc.font_name {
                m.new_font_name = Some(fn_.clone());
            }
            if let Some(ref ah) = fc.alignment_h {
                m.new_alignment_h = Some(ah.clone());
            }
            if let Some(ref av) = fc.alignment_v {
                m.new_alignment_v = Some(av.clone());
            }
            if let Some(ref nf) = fc.number_format {
                m.new_number_format = Some(nf.clone());
            }

            // If cell doesn't exist, provide a placeholder value
            if !existing.contains(&addr) {
                m.new_value = Some(CellValue::Empty);
            }

            modifications.push(m);
        }

        let path_obj = std::path::Path::new(path);
        let tmp_path = path_obj.with_extension("xlsx.tmp");
        incremental_write_xlsx(&bytes, &modifications, &tmp_path)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to write xlsx: {}", e)))?;
        tokio::fs::rename(&tmp_path, &path).await
            .map_err(|e| ToolError::IoError(format!("Failed to rename temp file: {}", e)))?;

        let formatted: Vec<String> = parsed.iter().map(|fc| fc.address.clone()).collect();
        Ok(format!(
            "Successfully formatted {} cell(s): {}",
            formatted.len(),
            formatted.join(", ")
        ))
    }
}

impl Default for FormatExcelCellsTool {
    fn default() -> Self { Self::new() }
}

// ─── merge_excel_cells ─────────────────────────────────────────────────────────

/// Merge or unmerge a range of cells in an Excel sheet.
pub struct MergeExcelCellsTool;

#[derive(Debug, Deserialize)]
struct MergeOperation {
    #[serde(rename = "type")]
    op_type: String,
    range: String,
}

impl MergeExcelCellsTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "merge_excel_cells",
            "合并 Excel 单元格",
            "Merge or unmerge a range of cells in an Excel (.xlsx) sheet. Merging combines multiple cells into one (the top-left cell holds the value). Unmerging splits a merged region back into individual cells.",
            ToolParameters::new(
                vec!["path", "sheet", "operations"],
                vec![
                    ("path", "string", Some("Absolute path to the .xlsx file to modify")),
                    ("sheet", "string", Some("Sheet name (case-sensitive)")),
                    ("operations", "array", Some("Array of merge operations. Each entry: {type: \"merge\"|\"unmerge\", range: \"A1:C3\"}. Example: [{type: \"merge\", range: \"A1:D1\"}, {type: \"unmerge\", range: \"B2:C2\"}]")),
                ],
            ),
        )
    }

    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("merge_excel_cells".to_string(), "path must be a string".into()))?;
        let sheet_name = arguments["sheet"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("merge_excel_cells".to_string(), "sheet must be a string".into()))?;

        validate_workspace_path(path, &workspace)?;
        validate_xlsx_path(path)?;

        let ops_json = arguments["operations"].as_array()
            .ok_or_else(|| ToolError::InvalidArguments("merge_excel_cells".to_string(), "operations must be an array".into()))?;

        if ops_json.is_empty() {
            return Err(ToolError::InvalidArguments("merge_excel_cells".to_string(), "operations array is empty".into()));
        }

        let mut ops: Vec<MergeOperation> = Vec::new();
        for (i, v) in ops_json.iter().enumerate() {
            let op: MergeOperation = serde_json::from_value(v.clone())
                .map_err(|e| ToolError::InvalidArguments(
                    "merge_excel_cells".to_string(),
                    format!("operations[{}]: {}", i, e),
                ))?;
            if !matches!(op.op_type.as_str(), "merge" | "unmerge") {
                return Err(ToolError::InvalidArguments(
                    "merge_excel_cells".to_string(),
                    format!("operations[{}]: type must be 'merge' or 'unmerge', got '{}'", i, op.op_type),
                ));
            }
            if parse_range_addr(&op.range).is_none() {
                return Err(ToolError::InvalidArguments(
                    "merge_excel_cells".to_string(),
                    format!("operations[{}]: invalid range '{}'", i, op.range),
                ));
            }
            ops.push(op);
        }

        let bytes = tokio::fs::read(path).await
            .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;
        let path_obj = std::path::Path::new(path);
        let tmp_path = path_obj.with_extension("xlsx.tmp");

        let merge_mods: Vec<_> = ops.iter().map(|op| {
            let ((sr, sc), (er, ec)) = parse_range_addr(&op.range).unwrap();
            MergeModification {
                sheet: sheet_name.to_string(),
                op: if op.op_type == "merge" { MergeOp::Merge } else { MergeOp::Unmerge },
                start_row: sr,
                start_col: sc,
                end_row: er,
                end_col: ec,
            }
        }).collect();

        merge_cells_xlsx(&bytes, &merge_mods, &tmp_path)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to merge cells: {}", e)))?;
        tokio::fs::rename(&tmp_path, &path).await
            .map_err(|e| ToolError::IoError(format!("Failed to rename temp file: {}", e)))?;

        let summaries: Vec<String> = ops.iter()
            .map(|op| format!("{}({})", op.op_type, op.range))
            .collect();
        Ok(format!(
            "Successfully processed {} merge operation(s): {}",
            ops.len(),
            summaries.join(", ")
        ))
    }
}

impl Default for MergeExcelCellsTool {
    fn default() -> Self { Self::new() }
}

fn parse_range_addr(s: &str) -> Option<((usize, usize), (usize, usize))> {
    let (start, end) = s.split_once(':')?;
    let (sr, sc) = parse_cell_address(start)?;
    let (er, ec) = parse_cell_address(end)?;
    Some(((sr, sc), (er, ec)))
}

// ─── resize_excel_rows_cols ────────────────────────────────────────────────────

/// A row or column dimension change.
#[derive(Debug, Deserialize)]
struct DimensionChange {
    #[serde(rename = "type")]
    dim_type: String,
    index: String,
    size: f64,
    #[serde(default)]
    hidden: Option<bool>,
}

/// Set row heights and column widths in an Excel sheet.
pub struct ResizeExcelRowsColsTool;

impl ResizeExcelRowsColsTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "resize_excel_rows_cols",
            "调整 Excel 行高列宽",
            "Set row heights and column widths in an Excel (.xlsx) sheet. Can also hide/show rows and columns.",
            ToolParameters::new(
                vec!["path", "sheet", "changes"],
                vec![
                    ("path", "string", Some("Absolute path to the .xlsx file to modify")),
                    ("sheet", "string", Some("Sheet name (case-sensitive)")),
                    ("changes", "array", Some("Array of dimension changes. Each entry: {type: \"row\"|\"col\", index, size, hidden?: bool}.\n\
                         - type: \"row\" or \"col\"\n\
                         - index: for rows, 1-based row number (e.g. \"1\", \"5\"); for columns, letter(s) or range (e.g. \"A\", \"C:F\")\n\
                         - size: height in points for rows (e.g. 20.0), width in Excel character units for columns (e.g. 15.0)\n\
                         - hidden: optional, set to true to hide the row/column\n\
                         Example: [{type: \"row\", index: \"1\", size: 30}, {type: \"col\", index: \"A:C\", size: 20}]")),
                ],
            ),
        )
    }

    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("resize_excel_rows_cols".to_string(), "path must be a string".into()))?;
        let sheet_name = arguments["sheet"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("resize_excel_rows_cols".to_string(), "sheet must be a string".into()))?;

        validate_workspace_path(path, &workspace)?;
        validate_xlsx_path(path)?;

        let changes_json = arguments["changes"].as_array()
            .ok_or_else(|| ToolError::InvalidArguments("resize_excel_rows_cols".to_string(), "changes must be an array".into()))?;

        if changes_json.is_empty() {
            return Err(ToolError::InvalidArguments("resize_excel_rows_cols".to_string(), "changes array is empty".into()));
        }

        let mut row_changes: Vec<RowColModification> = Vec::new();
        let mut col_changes: Vec<RowColModification> = Vec::new();

        for (i, v) in changes_json.iter().enumerate() {
            let change: DimensionChange = serde_json::from_value(v.clone())
                .map_err(|e| ToolError::InvalidArguments(
                    "resize_excel_rows_cols".to_string(),
                    format!("changes[{}]: {}", i, e),
                ))?;

            match change.dim_type.as_str() {
                "row" => {
                    let row_num: usize = change.index.parse()
                        .map_err(|_| ToolError::InvalidArguments(
                            "resize_excel_rows_cols".to_string(),
                            format!("changes[{}]: invalid row number '{}'", i, change.index),
                        ))?;
                    row_changes.push(RowColModification {
                        index: row_num.saturating_sub(1),
                        size: change.size,
                        hidden: change.hidden.unwrap_or(false),
                    });
                }
                "col" => {
                    if change.index.contains(':') {
                        let parts: Vec<&str> = change.index.split(':').collect();
                        if parts.len() != 2 {
                            return Err(ToolError::InvalidArguments(
                                "resize_excel_rows_cols".to_string(),
                                format!("changes[{}]: invalid column range '{}'", i, change.index),
                            ));
                        }
                        let cs = parse_col_letter(parts[0])
                            .ok_or_else(|| ToolError::InvalidArguments(
                                "resize_excel_rows_cols".to_string(),
                                format!("changes[{}]: invalid column '{}'", i, parts[0]),
                            ))?;
                        let ce = parse_col_letter(parts[1])
                            .ok_or_else(|| ToolError::InvalidArguments(
                                "resize_excel_rows_cols".to_string(),
                                format!("changes[{}]: invalid column '{}'", i, parts[1]),
                            ))?;
                        for ci in cs..=ce {
                            col_changes.push(RowColModification {
                                index: ci,
                                size: change.size,
                                hidden: change.hidden.unwrap_or(false),
                            });
                        }
                    } else {
                        let ci = parse_col_letter(&change.index)
                            .ok_or_else(|| ToolError::InvalidArguments(
                                "resize_excel_rows_cols".to_string(),
                                format!("changes[{}]: invalid column '{}' (use letter like 'A', 'B', or range 'A:C')", i, change.index),
                            ))?;
                        col_changes.push(RowColModification {
                            index: ci,
                            size: change.size,
                            hidden: change.hidden.unwrap_or(false),
                        });
                    }
                }
                other => {
                    return Err(ToolError::InvalidArguments(
                        "resize_excel_rows_cols".to_string(),
                        format!("changes[{}]: type must be 'row' or 'col', got '{}'", i, other),
                    ));
                }
            }
        }

        let bytes = tokio::fs::read(path).await
            .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;
        let path_obj = std::path::Path::new(path);
        let tmp_path = path_obj.with_extension("xlsx.tmp");

        resize_rows_cols_xlsx(&bytes, sheet_name, &row_changes, &col_changes, &tmp_path)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to resize: {}", e)))?;
        tokio::fs::rename(&tmp_path, &path).await
            .map_err(|e| ToolError::IoError(format!("Failed to rename temp file: {}", e)))?;

        Ok(format!(
            "Resized {} row(s) and {} column(s) in {} sheet '{}'",
            row_changes.len(), col_changes.len(), path, sheet_name
        ))
    }
}

impl Default for ResizeExcelRowsColsTool {
    fn default() -> Self { Self::new() }
}

// ─── manage_excel_sheets ──────────────────────────────────────────────────────

/// A sheet management operation.
#[derive(Debug, Deserialize)]
struct SheetOperation {
    #[serde(rename = "type")]
    op_type: String,
    #[serde(default)]
    sheet: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    index: Option<usize>,
}

/// Create, rename, delete, hide, or unhide sheets in an Excel workbook.
pub struct ManageExcelSheetsTool;

impl ManageExcelSheetsTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "manage_excel_sheets",
            "管理 Excel 工作表",
            "Create, rename, delete, hide, or unhide worksheets in an Excel (.xlsx) file.",
            ToolParameters::new(
                vec!["path", "operations"],
                vec![
                    ("path", "string", Some("Absolute path to the .xlsx file to modify")),
                    ("operations", "array", Some("Array of sheet operations. Each entry: {type: \"create\"|\"rename\"|\"delete\"|\"hide\"|\"unhide\", sheet?: string, name?: string, index?: number}.\n\
                         - type: operation type\n\
                         - sheet: existing sheet name (for rename source, delete, hide, unhide)\n\
                         - name: new name (for rename or create)\n\
                         - index: 0-based insertion index for new sheet (default: at end)\n\
                         Operations are applied in order.\n\
                         Example: [{type: \"create\", name: \"Summary\", index: 0}, {type: \"rename\", sheet: \"Sheet1\", name: \"Data\"}]")),
                ],
            ),
        )
    }

    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("manage_excel_sheets".to_string(), "path must be a string".into()))?;

        validate_workspace_path(path, &workspace)?;
        validate_xlsx_path(path)?;

        let ops_json = arguments["operations"].as_array()
            .ok_or_else(|| ToolError::InvalidArguments("manage_excel_sheets".to_string(), "operations must be an array".into()))?;

        if ops_json.is_empty() {
            return Err(ToolError::InvalidArguments("manage_excel_sheets".to_string(), "operations array is empty".into()));
        }

        let mut ops: Vec<SheetOperation> = Vec::new();
        for (i, v) in ops_json.iter().enumerate() {
            let op: SheetOperation = serde_json::from_value(v.clone())
                .map_err(|e| ToolError::InvalidArguments(
                    "manage_excel_sheets".to_string(),
                    format!("operations[{}]: {}", i, e),
                ))?;
            if !matches!(op.op_type.as_str(), "create" | "rename" | "delete" | "hide" | "unhide") {
                return Err(ToolError::InvalidArguments(
                    "manage_excel_sheets".to_string(),
                    format!("operations[{}]: type must be 'create', 'rename', 'delete', 'hide', or 'unhide', got '{}'", i, op.op_type),
                ));
            }
            ops.push(op);
        }

        let path_obj = std::path::Path::new(path);
        let mut current_bytes = tokio::fs::read(&path).await
            .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;

        for (i, op) in ops.iter().enumerate() {
            let tmp_path = path_obj.with_extension(format!("xlsx.tmp.{}", i));
            match op.op_type.as_str() {
                "create" => {
                    let name = op.name.as_ref()
                        .ok_or_else(|| ToolError::InvalidArguments(
                            "manage_excel_sheets".to_string(),
                            format!("operations[{}]: 'create' requires a 'name' field", i),
                        ))?;
                    create_sheet_xlsx(&current_bytes, name, op.index.unwrap_or(usize::MAX), &tmp_path)
                        .map_err(|e| ToolError::ExecutionError(format!("operations[{}] failed: {}", i, e)))?;
                }
                "rename" => {
                    let sheet = op.sheet.as_ref()
                        .ok_or_else(|| ToolError::InvalidArguments(
                            "manage_excel_sheets".to_string(),
                            format!("operations[{}]: 'rename' requires a 'sheet' field", i),
                        ))?;
                    let name = op.name.as_ref()
                        .ok_or_else(|| ToolError::InvalidArguments(
                            "manage_excel_sheets".to_string(),
                            format!("operations[{}]: 'rename' requires a 'name' field", i),
                        ))?;
                    rename_sheet_xlsx(&current_bytes, sheet, name, &tmp_path)
                        .map_err(|e| ToolError::ExecutionError(format!("operations[{}] failed: {}", i, e)))?;
                }
                "delete" => {
                    let sheet = op.sheet.as_ref()
                        .ok_or_else(|| ToolError::InvalidArguments(
                            "manage_excel_sheets".to_string(),
                            format!("operations[{}]: 'delete' requires a 'sheet' field", i),
                        ))?;
                    delete_sheet_xlsx(&current_bytes, sheet, &tmp_path)
                        .map_err(|e| ToolError::ExecutionError(format!("operations[{}] failed: {}", i, e)))?;
                }
                "hide" | "unhide" => {
                    let sheet = op.sheet.as_ref()
                        .ok_or_else(|| ToolError::InvalidArguments(
                            "manage_excel_sheets".to_string(),
                            format!("operations[{}]: '{}' requires a 'sheet' field", i, op.op_type),
                        ))?;
                    let new_state = if op.op_type == "hide" { "hidden" } else { "visible" };
                    set_sheet_state_xlsx(&current_bytes, sheet, new_state, &tmp_path)
                        .map_err(|e| ToolError::ExecutionError(format!("operations[{}] failed: {}", i, e)))?;
                }
                _ => unreachable!(),
            }

            tokio::fs::rename(&tmp_path, &path).await
                .map_err(|e| ToolError::IoError(format!("Failed to apply operation[{}]: {}", i, e)))?;
            current_bytes = tokio::fs::read(&path).await
                .map_err(|e| ToolError::IoError(format!("Failed to re-read after operation[{}]: {}", i, e)))?;
        }

        let summaries: Vec<String> = ops.iter().map(|op| {
            match op.op_type.as_str() {
                "create" => format!("create '{}'", op.name.as_deref().unwrap_or("?")),
                "rename" => format!("rename '{}' to '{}'", op.sheet.as_deref().unwrap_or("?"), op.name.as_deref().unwrap_or("?")),
                "delete" => format!("delete '{}'", op.sheet.as_deref().unwrap_or("?")),
                "hide" => format!("hide '{}'", op.sheet.as_deref().unwrap_or("?")),
                "unhide" => format!("unhide '{}'", op.sheet.as_deref().unwrap_or("?")),
                _ => op.op_type.clone(),
            }
        }).collect();

        Ok(format!(
            "Successfully performed {} sheet operation(s): {}",
            ops.len(),
            summaries.join("; ")
        ))
    }
}

impl Default for ManageExcelSheetsTool {
    fn default() -> Self { Self::new() }
}
