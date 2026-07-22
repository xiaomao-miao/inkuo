//! Office file tools: read_office_file, create_word_doc

use serde::Deserialize;
use serde_json::Value;

use super::{ToolDefinition, ToolError, ToolParameters, validate_workspace_path};

mod create_word_doc;
pub use create_word_doc::CreateWordDocTool;

mod inspect_office;
pub use inspect_office::InspectOfficeTool;

pub struct ReadOfficeFileTool;

impl ReadOfficeFileTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "read_office_file",
            "读取 Office 文件",
            "Read a Word (.docx) or Excel (.xlsx) file and extract its content as readable text.",
            ToolParameters::new(
                vec!["path"],
                vec![
                    ("path", "string", Some("Absolute path to the .docx or .xlsx file to read")),
                ],
            ),
        )
    }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("read_office_file".to_string(), "path must be a string".into()))?;

        validate_workspace_path(path, &workspace)?;

        let path_obj = std::path::Path::new(path);
        let extension = path_obj
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if !matches!(extension.as_str(), "docx" | "xlsx") {
            return Err(ToolError::InvalidArguments(
                "read_office_file".to_string(),
                format!("Unsupported file type: '{}'. Only .docx and .xlsx files are supported.", extension),
            ));
        }

        let _bytes = tokio::fs::read(path)
            .await
            .map_err(|e| ToolError::IoError(format!("Failed to read file {}: {}", path, e)))?;

        let result = crate::office::read_office_file(path_obj)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to parse office file: {}", e)))?;

        let (file_type, text_content) = result;

        let response = match file_type {
            crate::office::OfficeFileType::Word(doc) => {
                let elements = crate::office::word_document_to_elements(&doc);
                serde_json::json!({
                    "file_type": "docx",
                    "file_name": path_obj.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
                    "text_content": text_content,
                    "elements": elements,
                    "note": "Use create_word_doc with elements[] to modify this document. Elements with an 'id' will replace existing ones. Elements without an 'id' will be appended."
                }).to_string()
            }
            crate::office::OfficeFileType::Excel(workbook) => {
                // Re-parse from the file on disk to access the structured
                // representation (formulas, merged ranges, sheet metadata).
                // The `OfficeFileType::Excel` payload here only carries the
                // legacy flat 2D view used for `text_content`.
                let structured = match crate::office::read_xlsx_structured(&_bytes) {
                    Ok(s) => Some(s),
                    Err(_) => None,
                };
                let sheets_summary: Vec<serde_json::Value> = structured
                    .as_ref()
                    .map(|sw| {
                        sw.sheets
                            .iter()
                            .map(|s| {
                                serde_json::json!({
                                    "name": s.name,
                                    "max_row": s.max_row,
                                    "max_col": s.max_col,
                                    "cell_count": s.cells.len(),
                                    "merged_count": s.merged_cells.len(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                // Build a per-sheet "values" array (string grid) from the
                // structured data — much more useful for the AI than the
                // legacy headers/rows split which assumed a single header row.
                let values_summary: Vec<serde_json::Value> = structured
                    .as_ref()
                    .map(|sw| {
                        sw.sheets
                            .iter()
                            .map(|s| {
                            let mut grid: Vec<Vec<String>> =
                                if s.max_row == 0 || s.max_col == 0 {
                                    vec![]
                                } else {
                                    vec![vec![String::new(); s.max_col]; s.max_row]
                                };
                                for c in &s.cells {
                                    if c.row < grid.len() && c.col < (grid.get(0).map(|r| r.len()).unwrap_or(0)) {
                                        grid[c.row][c.col] = if let Some(f) = &c.formula {
                                            format!("={}", f)
                                        } else {
                                            c.value.as_string_for_display()
                                        };
                                    }
                                }
                                serde_json::json!({
                                    "name": s.name,
                                    "values": grid,
                                    "merged_cells": s.merged_cells.iter().map(|m| m.address()).collect::<Vec<_>>(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let json = serde_json::to_string(&workbook)
                    .map_err(|e| ToolError::ExecutionError(format!("JSON serialization failed: {}", e)))?;
                serde_json::json!({
                    "file_type": "xlsx",
                    "file_name": path_obj.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
                    "sheets": workbook.sheets.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
                    "sheets_summary": sheets_summary,
                    "values": values_summary,
                    "text_content": text_content,
                    "json_content": json,
                    "note": "Use modify_excel to change specific cells. Each modification targets one cell address and preserves all other workbook content (formulas, styles, charts, etc.)."
                }).to_string()
            }
        };

        Ok(response)
    }
}

impl Default for ReadOfficeFileTool {
    fn default() -> Self { Self::new() }
}


// ─── compare_word_docs ─────────────────────────────────────────────────────────

/// Compare two .docx files and return a structured diff.
///
/// Strategy: load both docs, build a map `id -> text` for each, then
/// categorise every element id as added (only in B), removed (only in A),
/// or modified (in both, but text differs). Paragraphs are matched by their
/// stable `p<N>` id; tables by `t<N>` id.
pub struct CompareWordDocsTool;

impl CompareWordDocsTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "compare_word_docs",
            "比较 Word 文档差异",
            "Compare two Word (.docx) files and return a structured diff of added, removed and modified paragraphs and tables.",
            ToolParameters::new(
                vec!["path1", "path2"],
                vec![
                    ("path1", "string", Some("Absolute path to the first .docx file (the 'before' version)")),
                    ("path2", "string", Some("Absolute path to the second .docx file (the 'after' version)")),
                ],
            ),
        )
    }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path1 = arguments["path1"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("compare_word_docs".to_string(), "path1 must be a string".into()))?;
        let path2 = arguments["path2"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("compare_word_docs".to_string(), "path2 must be a string".into()))?;
        validate_workspace_path(path1, &workspace)?;
        validate_workspace_path(path2, &workspace)?;

        let bytes1 = tokio::fs::read(path1).await
            .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path1, e)))?;
        let bytes2 = tokio::fs::read(path2).await
            .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path2, e)))?;

        let doc1 = crate::office::read_word_document(&bytes1)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to parse {}: {}", path1, e)))?;
        let doc2 = crate::office::read_word_document(&bytes2)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to parse {}: {}", path2, e)))?;

        // Build id -> (kind, text) maps for each side.
        let mut left: std::collections::BTreeMap<String, (String, String)> = std::collections::BTreeMap::new();
        for p in &doc1.paragraphs {
            left.insert(p.id.clone(), ("paragraph".into(), p.text.clone()));
        }
        for t in &doc1.tables {
            let flat = t.rows.iter()
                .map(|r| r.cells.iter().map(|c| c.text.clone()).collect::<Vec<_>>().join(" | "))
                .collect::<Vec<_>>()
                .join("\n");
            left.insert(t.id.clone(), ("table".into(), flat));
        }

        let mut right: std::collections::BTreeMap<String, (String, String)> = std::collections::BTreeMap::new();
        for p in &doc2.paragraphs {
            right.insert(p.id.clone(), ("paragraph".into(), p.text.clone()));
        }
        for t in &doc2.tables {
            let flat = t.rows.iter()
                .map(|r| r.cells.iter().map(|c| c.text.clone()).collect::<Vec<_>>().join(" | "))
                .collect::<Vec<_>>()
                .join("\n");
            right.insert(t.id.clone(), ("table".into(), flat));
        }

        let mut added: Vec<serde_json::Value> = Vec::new();
        let mut removed: Vec<serde_json::Value> = Vec::new();
        let mut modified: Vec<serde_json::Value> = Vec::new();

        for (id, (kind, text)) in &right {
            match left.get(id) {
                None => added.push(serde_json::json!({"id": id, "kind": kind, "text": text})),
                Some((_, old_text)) if old_text != text => {
                    modified.push(serde_json::json!({"id": id, "kind": kind, "old_text": old_text, "new_text": text}));
                }
                _ => {}
            }
        }
        for (id, (kind, text)) in &left {
            if !right.contains_key(id) {
                removed.push(serde_json::json!({"id": id, "kind": kind, "text": text}));
            }
        }

        let total_changes = added.len() + removed.len() + modified.len();
        let summary = format!(
            "{} added, {} removed, {} modified (total {} changes)",
            added.len(), removed.len(), modified.len(), total_changes
        );

        let result = serde_json::json!({
            "added": added,
            "removed": removed,
            "modified": modified,
            "summary": summary,
        });
        Ok(result.to_string())
    }
}

impl Default for CompareWordDocsTool {
    fn default() -> Self { Self::new() }
}

// ─── modify_excel ─────────────────────────────────────────────────────────────

/// Surgical cell-level editor for Excel workbooks. The workbook is parsed into memory,
/// a sequence of structured operations is applied, and the result is written back.
/// All unmodified content (formulas, styles, charts, images) is preserved.
pub struct ModifyExcelTool;

impl ModifyExcelTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "modify_excel",
            "修改 Excel 单元格",
            "Modify an Excel (.xlsx) file by applying a sequence of structured operations. The workbook is parsed into memory, operations are applied, and the result is written back preserving all unmodified content (formulas, styles, charts, images). Use read_office_file first to confirm sheet names and current values.",
            ToolParameters::new(
                vec!["path", "operations"],
                vec![
                    ("path", "string", Some("Absolute path to the .xlsx file to modify")),
                    ("operations", "array", Some(
                        "Array of operation objects. Each entry has a 'type' field distinguishing the variant:\n\
                         - {type: \"modify_cell\", sheet, address, value?, formula?, number_format?, bg_color?, font_bold?, font_italic?, font_color?, font_size?, font_name?, alignment_h?, alignment_v?}\n\
                           Modify a single cell's value, formula, or style. address is A1 form (e.g. \"B3\").\n\
                         - {type: \"write_range\", sheet, start_cell, values: [[...], ...], number_format?}\n\
                           Batch-write a 2-D array of values starting at start_cell (e.g. \"A1\").\n\
                         - {type: \"merge_cells\", sheet, op: \"merge\"|\"unmerge\", start_cell, end_cell}\n\
                           Merge or unmerge a rectangular region (e.g. start_cell=\"A1\", end_cell=\"C3\").\n\
                         - {type: \"resize_dimension\", sheet, dimension: \"row\"|\"col\", index: 0-based, size, hidden?}\n\
                           Set row height (points) or column width (character units), or hide/show.\n\
                         - {type: \"sheet_op\", op: \"create\"|\"rename\"|\"delete\"|\"hide\"|\"unhide\", sheet, new_name?, insert_index?}\n\
                           Manage sheets. create requires new_name; rename requires new_name; delete requires sheet to exist."
                    )),
                ],
            ),
        )
    }

    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("modify_excel".to_string(), "path must be a string".into()))?
            .to_string();
        validate_workspace_path(&path, &workspace)?;

        let path_obj = std::path::Path::new(&path);
        if path_obj.extension().and_then(|e| e.to_str()).unwrap_or("") != "xlsx" {
            return Err(ToolError::InvalidArguments(
                "modify_excel".to_string(),
                "Only .xlsx files are supported".into(),
            ));
        }

        let ops_json = arguments["operations"].as_array()
            .ok_or_else(|| ToolError::InvalidArguments("modify_excel".to_string(), "operations must be an array".into()))?;

        if ops_json.is_empty() {
            return Err(ToolError::InvalidArguments(
                "modify_excel".to_string(),
                "operations array is empty".into(),
            ));
        }

        let operations: Vec<crate::office::ExcelOperation> = ops_json
            .iter()
            .map(|v| serde_json::from_value(v.clone())
                .map_err(|e| ToolError::InvalidArguments("modify_excel".to_string(), format!("Invalid operation: {}", e))))
            .collect::<Result<Vec<_>, _>>()?;

        let bytes = tokio::fs::read(&path).await
            .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;

        let tmp_path = path_obj.with_extension("xlsx.tmp");

        // RAII guard: delete tmp_path on function exit (success or failure).
        struct TempGuard(std::path::PathBuf);
        impl Drop for TempGuard {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _guard = TempGuard(tmp_path.clone());

        let mut workbook = crate::office::read_xlsx_structured(&bytes)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to parse xlsx: {}", e)))?;
        workbook.apply_operations(operations)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to apply operations: {}", e)))?;
        crate::office::write_excel_document(&workbook, Some(&bytes), &tmp_path)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to write xlsx: {}", e)))?;
        tokio::fs::rename(&tmp_path, &path).await
            .map_err(|e| ToolError::IoError(format!("Failed to replace original file: {}", e)))?;

        let count = ops_json.len();
        Ok(format!("Successfully applied {} operation(s) to: {}", count, path))
    }
}

impl Default for ModifyExcelTool {
    fn default() -> Self { Self::new() }
}

// ─── create_excel ────────────────────────────────────────────────────────────

/// A cell value passed to [`CreateExcelTool`]. Mirrors [`crate::office::CellValue`]
/// so AI callers can express typed values without re-implementing the parser.
#[derive(Debug, Clone, Deserialize)]
struct CreateExcelCellValue {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    value: Option<serde_json::Value>,
}

impl CreateExcelCellValue {
    fn into_cell_value(self) -> Result<crate::office::CellValue, String> {
        use crate::office::CellValue;
        match self.kind.as_str() {
            "empty" => Ok(CellValue::Empty),
            "int" => {
                let n = self.value.as_ref()
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| "int.value missing".to_string())?;
                Ok(CellValue::Int(n))
            }
            "float" => {
                let n = self.value.as_ref()
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| "float.value missing".to_string())?;
                Ok(CellValue::Float(n))
            }
            "bool" => {
                let b = self.value.as_ref()
                    .and_then(|v| v.as_bool())
                    .ok_or_else(|| "bool.value missing".to_string())?;
                Ok(CellValue::Bool(b))
            }
            "string" => {
                let s = self.value.as_ref()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "string.value missing".to_string())?;
                Ok(CellValue::String(s.to_string()))
            }
            "datetime" => {
                let n = self.value.as_ref()
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| "datetime.value missing".to_string())?;
                Ok(CellValue::DateTime(n))
            }
            "error" => {
                let s = self.value.as_ref()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "error.value missing".to_string())?;
                Ok(CellValue::Error(s.to_string()))
            }
            other => Err(format!("unknown value type '{}'", other)),
        }
    }
}

/// A single cell entry used by [`CreateExcelTool`].
#[derive(Debug, Clone, Deserialize)]
struct CreateExcelCell {
    /// A1-style cell address, e.g. "B3".
    address: String,
    /// Cell value. JSON shape: {"type": "int|float|bool|string|datetime|error|empty", "value": ...}.
    /// May be omitted when `formula` is provided.
    #[serde(default)]
    value: Option<CreateExcelCellValue>,
    /// Optional formula text (without leading "="), e.g. "SUM(A1:A10)".
    #[serde(default)]
    formula: Option<String>,
}

/// Sheet specification accepted by [`CreateExcelTool`].
#[derive(Debug, Clone, Deserialize)]
struct CreateExcelSheet {
    /// Display name for the sheet (max 31 chars in xlsx).
    name: String,
    /// Cell entries. Optional — useful when the sheet starts empty.
    #[serde(default)]
    cells: Vec<CreateExcelCell>,
    /// Optional list of merged ranges in A1:B3 form.
    #[serde(default)]
    merged: Vec<String>,
}

/// Create a new .xlsx file from scratch. The user provides sheet names and
/// their cells; we assemble a valid OOXML package and write it atomically.
pub struct CreateExcelTool;

impl CreateExcelTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "create_excel",
            "创建 Excel 文件",
            "Create a new Excel (.xlsx) file with the given sheets and cells. The file is created atomically; if a file already exists at the path it is overwritten.",
            ToolParameters::new(
                vec!["path", "sheets"],
                vec![
                    ("path", "string", Some("Absolute path where the new .xlsx file will be written")),
                    ("sheets", "array", Some(
                        "Array of sheet definitions. Each entry: {name, cells?, merged?}.\n\
                         - name: sheet name (1-31 chars, must be unique)\n\
                         - cells: array of {address, value?, formula?}\n\
                         - merged: optional array of \"A1:B3\" range strings\n\
                         - value: {type, value} where type is one of: empty|int|float|bool|string|datetime|error\n\
                         - formula: formula text without leading '=' (e.g. \"SUM(A1:A10)\")\n\
                         At least one sheet is required. The first sheet becomes the active sheet."
                    )),
                ],
            ),
        )
    }

    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("create_excel".to_string(), "path must be a string".into()))?
            .to_string();
        validate_workspace_path(&path, &workspace)?;

        let sheets_json = arguments["sheets"].as_array()
            .ok_or_else(|| ToolError::InvalidArguments("create_excel".to_string(), "sheets must be an array".into()))?;

        if sheets_json.is_empty() {
            return Err(ToolError::InvalidArguments(
                "create_excel".to_string(),
                "sheets array is empty (at least one sheet is required)".into(),
            ));
        }

        // Build the structured workbook.
        let mut sheets: Vec<crate::office::XlsxSheet> = Vec::new();
        let mut names_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (idx, s) in sheets_json.iter().enumerate() {
            let sheet_input: CreateExcelSheet = serde_json::from_value(s.clone())
                .map_err(|e| ToolError::InvalidArguments("create_excel".to_string(), format!("Invalid sheets[{}]: {}", idx, e)))?;
            if sheet_input.name.is_empty() {
                return Err(ToolError::InvalidArguments("create_excel".to_string(), format!("sheets[{}].name is empty", idx)));
            }
            if sheet_input.name.len() > 31 {
                return Err(ToolError::InvalidArguments(
                    "create_excel".to_string(),
                    format!("sheets[{}].name is too long: '{}' (max 31 chars)", idx, sheet_input.name),
                ));
            }
            if !names_seen.insert(sheet_input.name.clone()) {
                return Err(ToolError::InvalidArguments(
                    "create_excel".to_string(),
                    format!("Duplicate sheet name: '{}'", sheet_input.name),
                ));
            }

            let mut cells: Vec<crate::office::Cell> = Vec::new();
            let mut max_row = 0usize;
            let mut max_col = 0usize;
            for (ci, c) in sheet_input.cells.iter().enumerate() {
                let (row, col) = crate::office::parse_cell_address(&c.address)
                    .ok_or_else(|| ToolError::InvalidArguments(
                        "create_excel".to_string(),
                        format!("sheets[{}].cells[{}].address is invalid: '{}'", idx, ci, c.address),
                    ))?;
                let value = match c.value.clone() {
                    Some(v) => Some(v.into_cell_value().map_err(|e| ToolError::InvalidArguments(
                        "create_excel".to_string(),
                        format!("sheets[{}].cells[{}].value: {}", idx, ci, e),
                    ))?),
                    None => Some(crate::office::CellValue::Empty),
                };
                if row + 1 > max_row { max_row = row + 1; }
                if col + 1 > max_col { max_col = col + 1; }
                cells.push(crate::office::Cell {
                    row,
                    col,
                    value: value.unwrap_or(crate::office::CellValue::Empty),
                    formula: c.formula.clone(),
                    style: None,
                });
            }

            let mut merged_cells: Vec<crate::office::MergedRange> = Vec::new();
            for (mi, m) in sheet_input.merged.iter().enumerate() {
                let (start, end) = m.split_once(':').ok_or_else(|| ToolError::InvalidArguments(
                    "create_excel".to_string(),
                    format!("sheets[{}].merged[{}] must be in A1:B3 form: '{}'", idx, mi, m),
                ))?;
                let (sr, sc) = crate::office::parse_cell_address(start).ok_or_else(|| ToolError::InvalidArguments(
                    "create_excel".to_string(),
                    format!("sheets[{}].merged[{}] start is invalid: '{}'", idx, mi, start),
                ))?;
                let (er, ec) = crate::office::parse_cell_address(end).ok_or_else(|| ToolError::InvalidArguments(
                    "create_excel".to_string(),
                    format!("sheets[{}].merged[{}] end is invalid: '{}'", idx, mi, end),
                ))?;
                merged_cells.push(crate::office::MergedRange { start_row: sr, start_col: sc, end_row: er, end_col: ec });
                if er + 1 > max_row { max_row = er + 1; }
                if ec + 1 > max_col { max_col = ec + 1; }
            }

            sheets.push(crate::office::XlsxSheet {
                name: sheet_input.name,
                state: "visible".to_string(),
                cells,
                merged_cells,
                max_row,
                max_col,
                row_heights: std::collections::HashMap::new(),
                col_widths: std::collections::HashMap::new(),
            });
        }

        let workbook = crate::office::XlsxWorkbook {
            sheets,
            shared_strings: Vec::new(),
        };

        // Ensure parent directory exists, then write atomically.
        let path_obj = std::path::Path::new(&path);
        if let Some(parent) = path_obj.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                tokio::fs::create_dir_all(parent).await
                    .map_err(|e| ToolError::IoError(format!("Failed to create parent dir: {}", e)))?;
            }
        }
        let tmp_path = path_obj.with_extension("xlsx.tmp");
        crate::office::create_xlsx_workbook(&workbook, &tmp_path)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to create xlsx: {}", e)))?;
        tokio::fs::rename(&tmp_path, &path).await
            .map_err(|e| ToolError::IoError(format!("Failed to move temp file into place: {}", e)))?;

        let total_cells: usize = workbook.sheets.iter().map(|s| s.cells.len()).sum();
        let sheet_names: Vec<String> = workbook.sheets.iter().map(|s| s.name.clone()).collect();
        Ok(format!(
            "Created {} with {} sheet(s) and {} cell(s): {}",
            path,
            sheet_names.len(),
            total_cells,
            sheet_names.join(", "),
        ))
    }
}

impl Default for CreateExcelTool {
    fn default() -> Self { Self::new() }
}

