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

use serde_json::Value;
use std::collections::HashSet;

use super::{ToolDefinition, ToolError, ToolParameters, validate_workspace_path};
// The `Merge*`, `RowCol*`, and standalone `*_xlsx` helpers in `office::xlsx`
// are not wired into any tool yet — they sit behind `#![allow(dead_code)]`
// in the module. They're re-exported from `office/mod.rs` so callers can
// reach them when those features land. Mark the whole import group as
// `unused_imports` to keep CI noise down until the next Excel feature pass
// actually consumes them.
#[allow(unused_imports)]
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


