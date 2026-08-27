//! `InspectOfficeTool` — read-only inspection for `.docx` and `.xlsx`.
//!
//! Owns:
//!   - The `InspectOfficeTool` impl (new / definition / execute)
//!   - Two small helpers `parse_inspect_col_letter` / `parse_inspect_range`
//!     used by the XLSX branch.
//!
//! Pulled out of `office/mod.rs` because the file's main loop already
//! dispatches by tool name; giving `InspectOfficeTool` its own file
//! makes it easy to extend the inspection surface without reopening
//! the (still-large) orchestrator file.

use std::collections::HashSet;

use serde_json::Value;

use super::{ToolDefinition, ToolError, ToolParameters, validate_workspace_path};

pub struct InspectOfficeTool;

impl InspectOfficeTool {
    pub fn new() -> Self { Self }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "inspect_office",
            "查看 Office 文件",
            "Inspect a Word (.docx) or Excel (.xlsx) file. Returns a summary at the level chosen by `mode`. Use this before doing any edits to gauge file size and structure.\n\n\
             - format=docx, mode=info: paragraph / table / word / character counts.\n\
             - format=docx, mode=elements: list all element IDs (paragraphs, tables, images) with their text content preview. Use this to get IDs for use with create_word_doc's deletes[] parameter.\n\
             - format=xlsx, mode=info: workbook / sheet / cell / formula counts.\n\
             - format=xlsx, mode=metadata: per-sheet merged ranges, used range, and full formula list.\n\
             - format=xlsx, mode=range: cells in a specific A1:B3 range (requires `sheet` + `range`).",
            ToolParameters::new(
                vec!["path", "format", "mode"],
                vec![
                    ("path", "string", Some("Absolute path to the .docx or .xlsx file")),
                    ("format", "string", Some("\"docx\" or \"xlsx\" (must match the file extension)")),
                    ("mode", "string", Some("Inspection depth: \"info\" | \"elements\" for .docx; \"info\" | \"metadata\" | \"range\" for .xlsx.")),
                    ("sheet", "string", Some("format=xlsx + mode=range or mode=metadata: sheet name (case-sensitive). Optional for mode=metadata (returns all sheets).")),
                    ("range", "string", Some("format=xlsx + mode=range: A1:B3-style cell range, e.g. \"A1:D10\". Single cell \"B2\", row \"1:10\", column \"A:A\" also valid.")),
                    ("include_styles", "string", Some("format=xlsx + mode=range: comma-separated style properties. Default: bg_color,font_color,number_format")),
                ],
            ),
        )
    }

    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("inspect_office".to_string(), "path must be a string".into()))?;
        let format = arguments["format"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("inspect_office".to_string(), "format must be a string".into()))?;
        let mode = arguments["mode"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("inspect_office".to_string(), "mode must be a string".into()))?;

        validate_workspace_path(path, &workspace)?;

        let path_obj = std::path::Path::new(path);
        let ext = path_obj.extension().and_then(|e| e.to_str()).unwrap_or("");

        match (format, ext) {
            ("docx", "docx") => {}
            ("xlsx", "xlsx") => {}
            (f, e) if f != e => {
                return Err(ToolError::InvalidArguments(
                    "inspect_office".to_string(),
                    format!("format='{}' does not match file extension '.{}'", f, e),
                ));
            }
            _ => {
                return Err(ToolError::InvalidArguments(
                    "inspect_office".to_string(),
                    format!("Unsupported format '{}' or extension '.{}'", format, ext),
                ));
            }
        }

        match format {
            "docx" => match mode {
                "info" => inspect_docx_info(path, path_obj).await,
                "elements" => inspect_docx_elements(path, path_obj).await,
                other => Err(ToolError::InvalidArguments(
                    "inspect_office".to_string(),
                    format!("For format=docx, mode must be 'info' or 'elements' (got '{}')", other),
                )),
            },
            "xlsx" => match mode {
                "info" => inspect_xlsx_info(path, path_obj).await,
                "metadata" => inspect_xlsx_metadata(path, &arguments).await,
                "range" => inspect_xlsx_range(path, &arguments).await,
                other => Err(ToolError::InvalidArguments(
                    "inspect_office".to_string(),
                    format!("For format=xlsx, mode must be one of info/metadata/range (got '{}')", other),
                )),
            },
            _ => Err(ToolError::InvalidArguments(
                "inspect_office".to_string(),
                format!("Unknown format '{}' (expected docx or xlsx)", format),
            )),
        }
    }
}

impl Default for InspectOfficeTool {
    fn default() -> Self { Self::new() }
}

// ─── inspect_office helpers ──────────────────────────────────────────────────

async fn inspect_docx_info(path: &str, path_obj: &std::path::Path) -> Result<String, ToolError> {
    let bytes = tokio::fs::read(path).await
        .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;
    let doc = crate::office::read_word_document(&bytes)
        .map_err(|e| ToolError::ExecutionError(format!("Failed to parse docx: {}", e)))?;

    let mut total_chars: usize = 0;
    let mut word_count: usize = 0;
    let mut styles_used: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for p in &doc.paragraphs {
        let t = &p.text;
        total_chars += t.chars().count();
        word_count += t.split_whitespace().count();
        if let Some(ref s) = p.style {
            styles_used.insert(s.clone());
        }
    }
    for tbl in &doc.tables {
        for row in &tbl.rows {
            for cell in &row.cells {
                total_chars += cell.text.chars().count();
                word_count += cell.text.split_whitespace().count();
            }
        }
    }

    let entries = crate::office::shared::read_all_zip_entries(&bytes).ok();
    let (has_headers, has_footers, has_images) = if let Some(map) = entries {
        let mut h = false;
        let mut f = false;
        let mut imgs = 0usize;
        for name in map.keys() {
            if name.starts_with("word/header") && name.ends_with(".xml") { h = true; }
            if name.starts_with("word/footer") && name.ends_with(".xml") { f = true; }
            if name.starts_with("word/media/") { imgs += 1; }
        }
        (h, f, imgs > 0)
    } else {
        (false, false, false)
    };

    let file_name = path_obj.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
    let result = serde_json::json!({
        "file_name": file_name,
        "path": path,
        "format": "docx",
        "mode": "info",
        "paragraph_count": doc.paragraphs.len(),
        "table_count": doc.tables.len(),
        "word_count": word_count,
        "total_characters": total_chars,
        "styles_used": styles_used.into_iter().collect::<Vec<_>>(),
        "has_headers": has_headers,
        "has_footers": has_footers,
        "has_images": has_images,
        "file_size_bytes": bytes.len(),
    });
    Ok(result.to_string())
}

/// List all document elements with their IDs and content preview.
/// Used to get element IDs for use with create_word_doc's deletes[] parameter.
async fn inspect_docx_elements(path: &str, path_obj: &std::path::Path) -> Result<String, ToolError> {
    let bytes = tokio::fs::read(path).await
        .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;
    let doc = crate::office::read_word_document(&bytes)
        .map_err(|e| ToolError::ExecutionError(format!("Failed to parse docx: {}", e)))?;

    // Collect paragraph elements (excluding position marker paragraphs)
    let paragraphs: Vec<serde_json::Value> = doc.paragraphs.iter()
        .filter(|p| !p.id.starts_with("__tbl_pos_") && !p.id.starts_with("__img_pos_") && !p.id.starts_with("__sect_break_"))
        .map(|p| {
            // Truncate text preview to 100 chars for readability
            // Use char_indices to avoid splitting multi-byte characters (e.g., Chinese)
            let text_preview = if p.text.chars().count() > 100 {
                let end_byte = p.text.char_indices()
                    .nth(100)
                    .map(|(idx, _)| idx)
                    .unwrap_or(p.text.len());
                format!("{}...", &p.text[..end_byte])
            } else {
                p.text.clone()
            };
            serde_json::json!({
                "type": "paragraph",
                "id": p.id,
                "text_preview": text_preview,
                "text_length": p.text.len(),
                "style": p.style,
                "runs_count": p.runs.as_ref().map(|r| r.len()).unwrap_or(0),
            })
        })
        .collect();

    // Collect table elements
    let tables: Vec<serde_json::Value> = doc.tables.iter()
        .map(|t| {
            // Collect header text for preview
            // Use char_indices to avoid splitting multi-byte characters (e.g., Chinese)
            let header_preview: Vec<String> = t.rows.first()
                .map(|row| row.cells.iter().map(|c| {
                    if c.text.chars().count() > 30 {
                        let end_byte = c.text.char_indices()
                            .nth(30)
                            .map(|(idx, _)| idx)
                            .unwrap_or(c.text.len());
                        format!("{}...", &c.text[..end_byte])
                    } else {
                        c.text.clone()
                    }
                }).collect())
                .unwrap_or_default();

            serde_json::json!({
                "type": "table",
                "id": t.id,
                "row_count": t.rows.len(),
                "col_count": t.rows.first().map(|r| r.cells.len()).unwrap_or(0),
                "header_preview": header_preview,
                "has_cell_paragraphs": !t.cell_paragraphs.is_empty(),
            })
        })
        .collect();

    // Collect image elements
    let images: Vec<serde_json::Value> = doc.images.iter()
        .map(|img| {
            serde_json::json!({
                "type": "image",
                "id": img.id,
                "width_emu": img.width_emu,
                "height_emu": img.height_emu,
                "source_path": img.path,
            })
        })
        .collect();

    let file_name = path_obj.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
    let result = serde_json::json!({
        "file_name": file_name,
        "path": path,
        "format": "docx",
        "mode": "elements",
        "paragraph_count": paragraphs.len(),
        "table_count": tables.len(),
        "image_count": images.len(),
        "paragraphs": paragraphs,
        "tables": tables,
        "images": images,
    });
    Ok(result.to_string())
}

async fn inspect_xlsx_info(path: &str, path_obj: &std::path::Path) -> Result<String, ToolError> {
    let bytes = tokio::fs::read(path).await
        .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;
    let workbook = crate::office::read_xlsx_structured(&bytes)
        .map_err(|e| ToolError::ExecutionError(format!("Failed to parse xlsx: {}", e)))?;

    let sheet_summaries: Vec<serde_json::Value> = workbook.sheets.iter().map(|s| {
        serde_json::json!({
            "name": s.name,
            "state": s.state,
            "max_row": s.max_row,
            "max_col": s.max_col,
            "cell_count": s.cells.len(),
            "merged_count": s.merged_cells.len(),
            "cells_with_formulas": s.cells.iter().filter(|c| c.formula.is_some()).count(),
        })
    }).collect();

    let total_cells: usize = workbook.sheets.iter().map(|s| s.cells.len()).sum();
    let total_formulas: usize = workbook.sheets.iter()
        .map(|s| s.cells.iter().filter(|c| c.formula.is_some()).count())
        .sum();

    let file_name = path_obj.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
    let result = serde_json::json!({
        "file_name": file_name,
        "path": path,
        "format": "xlsx",
        "mode": "info",
        "sheet_count": workbook.sheets.len(),
        "total_cells": total_cells,
        "total_formulas": total_formulas,
        "sheets": sheet_summaries,
        "file_size_bytes": bytes.len(),
    });
    Ok(result.to_string())
}

async fn inspect_xlsx_metadata(path: &str, arguments: &Value) -> Result<String, ToolError> {
    let sheet_filter = arguments["sheet"].as_str();
    let bytes = tokio::fs::read(path).await
        .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;
    let workbook = crate::office::read_xlsx_structured(&bytes)
        .map_err(|e| ToolError::ExecutionError(format!("Failed to parse xlsx: {}", e)))?;

    let sheets: Vec<_> = if let Some(name) = sheet_filter {
        workbook.sheets.iter().filter(|s| s.name == name).collect()
    } else {
        workbook.sheets.iter().collect()
    };

    if sheet_filter.is_some() && sheets.is_empty() {
        return Err(ToolError::InvalidArguments(
            "inspect_office".to_string(),
            format!("Sheet '{}' not found. Available: {:?}", sheet_filter.unwrap(),
                workbook.sheets.iter().map(|s| &s.name).collect::<Vec<_>>()),
        ));
    }

    let sheet_meta: Vec<serde_json::Value> = sheets.iter().map(|s| {
        let formula_cells: Vec<_> = s.cells.iter()
            .filter(|c| c.formula.is_some())
            .map(|c| {
                serde_json::json!({
                    "address": crate::office::cell_address(c.row, c.col),
                    // Filter above guarantees `formula` is Some, but a
                    // bare unwrap would still panic if the invariant
                    // ever drifts — use expect with an actionable
                    // message instead.
                    "formula": c.formula.as_deref().expect("formula filter above"),
                })
            })
            .collect();

        let merged_info: Vec<serde_json::Value> = s.merged_cells.iter().map(|m| {
            serde_json::json!({
                "address": crate::office::cell_address(m.start_row, m.start_col),
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
            "used_range": format!("A1:{}", crate::office::cell_address(s.max_row.saturating_sub(1), s.max_col.saturating_sub(1))),
            "cell_count": s.cells.len(),
            "formula_count": formula_cells.len(),
            "merged_cells": merged_info,
            "formulas": formula_cells,
        })
    }).collect();

    let result = serde_json::json!({
        "file_name": std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
        "path": path,
        "format": "xlsx",
        "mode": "metadata",
        "sheet_count": workbook.sheets.len(),
        "sheets": sheet_meta,
    });
    serde_json::to_string(&result)
        .map_err(|e| ToolError::ExecutionError(format!("JSON serialization failed: {}", e)))
}

async fn inspect_xlsx_range(path: &str, arguments: &Value) -> Result<String, ToolError> {
    let sheet_name = arguments["sheet"].as_str()
        .ok_or_else(|| ToolError::InvalidArguments("inspect_office".to_string(), "sheet is required for mode=range".into()))?;
    let range_str = arguments["range"].as_str()
        .ok_or_else(|| ToolError::InvalidArguments("inspect_office".to_string(), "range is required for mode=range".into()))?;
    let style_str = arguments["include_styles"].as_str().unwrap_or("bg_color,font_color,number_format");

    let bytes = tokio::fs::read(path).await
        .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;
    let workbook = crate::office::read_xlsx_structured(&bytes)
        .map_err(|e| ToolError::ExecutionError(format!("Failed to parse xlsx: {}", e)))?;

    let sheet = workbook.sheets.iter()
        .find(|s| s.name == sheet_name)
        .ok_or_else(|| ToolError::InvalidArguments(
            "inspect_office".to_string(),
            format!("Sheet '{}' not found. Available: {:?}", sheet_name,
                workbook.sheets.iter().map(|s| &s.name).collect::<Vec<_>>()),
        ))?;

    let ((sr, sc), (er, ec)) = parse_inspect_range(range_str, sheet)?;

    let style_fields: HashSet<&str> = style_str.split(',').map(|s| s.trim()).collect();

    let mut cells_out: Vec<serde_json::Value> = Vec::new();
    for cell in &sheet.cells {
        if cell.row >= sr && cell.row <= er && cell.col >= sc && cell.col <= ec {
            let addr = crate::office::cell_address(cell.row, cell.col);
            let display = if let Some(ref f) = cell.formula {
                format!("={}", f)
            } else {
                cell.value.as_string_for_display()
            };

            let raw_type = match &cell.value {
                crate::office::CellValue::Empty => "empty",
                crate::office::CellValue::Int(_) => "int",
                crate::office::CellValue::Float(_) => "float",
                crate::office::CellValue::Bool(_) => "bool",
                crate::office::CellValue::String(_) => "string",
                crate::office::CellValue::Error(_) => "error",
                crate::office::CellValue::DateTime(_) => "datetime",
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
        "format": "xlsx",
        "mode": "range",
        "sheet": sheet_name,
        "range": {
            "start": crate::office::cell_address(sr, sc),
            "end": crate::office::cell_address(er, ec),
            "rows": er - sr + 1,
            "cols": ec - sc + 1,
        },
        "cell_count": cells_out.len(),
        "cells": cells_out,
    });

    serde_json::to_string(&result)
        .map_err(|e| ToolError::ExecutionError(format!("JSON serialization failed: {}", e)))
}

fn parse_inspect_col_letter(s: &str) -> Option<usize> {
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

fn parse_inspect_range(range_str: &str, sheet: &crate::office::XlsxSheet) -> Result<((usize, usize), (usize, usize)), ToolError> {
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
            let cs = parse_inspect_col_letter(parts[0])
                .ok_or_else(|| ToolError::InvalidArguments("range".to_string(), format!("Invalid column '{}'", parts[0])))?;
            let ce = parse_inspect_col_letter(parts[1])
                .ok_or_else(|| ToolError::InvalidArguments("range".to_string(), format!("Invalid column '{}'", parts[1])))?;
            if cs > ce {
                return Err(ToolError::InvalidArguments("range".to_string(), "Invalid range: start col > end col".into()));
            }
            return Ok(((0, cs), (sheet.max_row.saturating_sub(1), ce)));
        }

        // Standard A1:B3 range
        let (sr, sc) = crate::office::parse_cell_address(parts[0])
            .ok_or_else(|| ToolError::InvalidArguments("range".to_string(), format!("Invalid address '{}'", parts[0])))?;
        let (er, ec) = crate::office::parse_cell_address(parts[1])
            .ok_or_else(|| ToolError::InvalidArguments("range".to_string(), format!("Invalid address '{}'", parts[1])))?;
        if sr > er || sc > ec {
            return Err(ToolError::InvalidArguments("range".to_string(), "Invalid range: start is after end".into()));
        }
        Ok(((sr, sc), (er, ec)))
    } else {
        // Single cell
        let (r, c) = crate::office::parse_cell_address(range_str)
            .ok_or_else(|| ToolError::InvalidArguments("range".to_string(), format!("Invalid cell address '{}'", range_str)))?;
        Ok(((r, c), (r, c)))
    }
}
