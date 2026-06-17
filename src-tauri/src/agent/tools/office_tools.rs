//! Office file tools: read_office_file, create_word_doc

use serde::Deserialize;
use serde_json::Value;

use super::{ToolDefinition, ToolError, ToolParameters, validate_workspace_path};

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
                                    vec![vec![String::new(); s.max_col.max(1)]; s.max_row.max(1)];
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

// ─── create_word_doc ───────────────────────────────────────────────────────────

/// A formatted text segment within a paragraph.
#[derive(Debug, Clone, Deserialize)]
struct DocTextRun {
    text: String,
    #[serde(default)]
    bold: Option<bool>,
    #[serde(default)]
    italic: Option<bool>,
    #[serde(default)]
    underline: Option<bool>,
    #[serde(default)]
    strikethrough: Option<bool>,
    #[serde(default)]
    font_size: Option<u32>,   // half-points, e.g. 24 = 12pt
    #[serde(default)]
    color: Option<String>,    // hex RGB, e.g. "FF0000"
    #[serde(default)]
    font_name: Option<String>,
    #[serde(default)]
    highlight: Option<String>,
}

/// A paragraph in the document.
#[derive(Debug, Deserialize)]
struct DocParagraph {
    /// Unique ID. If provided, replaces the existing element with this ID.
    /// If absent, creates a new element (appended or inserted).
    #[serde(default)]
    id: Option<String>,
    /// The paragraph text.
    text: String,
    /// Paragraph style: "Heading1" (large blue), "Heading2", "Heading3", "Normal".
    #[serde(default)]
    style: Option<String>,
    /// Rich text runs for inline formatting.
    #[serde(default)]
    runs: Option<Vec<DocTextRun>>,
    /// List/numbering reference: {num_id: u32, level: u32}.
    #[serde(default)]
    numbering: Option<NumberingInput>,
    /// Insert position relative to anchor_id: "before", "after", "end".
    /// Only used when id is absent (new element).
    #[serde(default)]
    position: Option<String>,
    /// Anchor element ID for insertion. Only used when id is absent.
    #[serde(default)]
    anchor_id: Option<String>,
    /// If true, delete the element with this id instead.
    #[serde(default, rename = "action")]
    delete_action: Option<String>,
}

/// Same shape as `NumberingRef` but deserialized from the wire-format JSON.
#[derive(Debug, Clone, Deserialize)]
struct NumberingInput {
    num_id: u32,
    #[serde(default)]
    level: u32,
}

impl From<NumberingInput> for crate::office::NumberingRef {
    fn from(n: NumberingInput) -> Self {
        crate::office::NumberingRef { num_id: n.num_id, level: n.level }
    }
}

/// A table in the document.
#[derive(Debug, Deserialize)]
struct DocTable {
    /// Unique ID. If provided, replaces the existing table with this ID.
    #[serde(default)]
    id: Option<String>,
    /// Column header labels (becomes the first table row).
    header: Vec<String>,
    /// Data rows (each row is an array of cell values).
    rows: Vec<Vec<String>>,
    /// Insert position: "before", "after", "end".
    #[serde(default)]
    position: Option<String>,
    /// Anchor element ID for insertion.
    #[serde(default)]
    anchor_id: Option<String>,
    /// If true, delete this table instead.
    #[serde(default, rename = "action")]
    delete_action: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateWordDocParams {
    /// Absolute path of the .docx file to create or modify.
    path: String,
    /// Document title for newly created documents (ignored when modifying existing).
    #[serde(default)]
    title: Option<String>,
    /// Structured document elements (paragraphs and tables) for new content or modifications.
    /// - With `id`: replaces the existing element with that ID
    /// - Without `id` + with `anchor_id` + `position`: inserts at that position
    /// - Without `id` and `anchor_id`: appends to end
    #[serde(default)]
    elements: Option<Vec<serde_json::Value>>,
    /// IDs of elements to delete.
    #[serde(default)]
    deletes: Option<Vec<String>>,
    /// Deprecated: use elements[]. Kept for backward compatibility.
    #[serde(default)]
    paragraphs: Option<Vec<DocParagraph>>,
    /// Deprecated: use elements[]. Kept for backward compatibility.
    #[serde(default)]
    tables: Option<Vec<DocTable>>,
    /// Deprecated: use elements[]. Path to an existing .docx to append content to.
    #[serde(default)]
    append_to: Option<String>,
    /// When true, the content in `elements[]` is appended to the end of the existing
    /// document without reading/modifying its current structure. Useful for progressive
    /// document building — call repeatedly as you generate content section by section.
    /// Takes effect only when the file already exists.
    #[serde(default)]
    append: Option<bool>,
}

pub struct CreateWordDocTool;

impl CreateWordDocTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
            ToolDefinition::new_with_label(
                "create_word_doc",
                "创建 Word 文档",
                "Create, modify, or append a Word (.docx) document. Pass elements[] with paragraph and table objects. Use IDs to modify existing content; omit IDs to append new content. Use anchor_id + position to insert at a specific location.",
            ToolParameters::new(
                vec!["path"],
                vec![
                    ("path", "string", Some("Absolute path of the .docx file to create or modify")),
                    ("title", "string", Some("Document title (for new files only; ignored when modifying existing)")),
                    ("elements", "array", Some(
                        "Array of element objects. Paragraph: {id?, text?, style?, runs?, position?, anchor_id?}. Table: {id?, header, rows, position?, anchor_id?}.\n\
                         Elements with id replace existing ones; without id are appended or inserted at anchor_id+position. Use action:'delete' with id to delete.\n\
                         When modifying (id present), any field omitted from text/style/runs is preserved from the original — this is how 'edit just the text' works without losing formatting.\n\
                         runs shape: array of {text, bold?, italic?, underline?, font_size? (half-points, e.g. 24=12pt), color? (hex RGB, e.g. 'FF0000'), font_name?}.\n\
                         Supplying runs fully replaces the paragraph's run list."
                    )),
                    ("deletes", "array", Some("Array of element IDs to delete.")),
                ],
            ),
        )
    }

    fn to_font_run(r: DocTextRun) -> crate::office::FontRun {
        crate::office::FontRun {
            text: r.text,
            bold: r.bold.unwrap_or(false),
            italic: r.italic.unwrap_or(false),
            underline: r.underline.unwrap_or(false),
            strikethrough: r.strikethrough.unwrap_or(false),
            font_size: r.font_size,
            color: r.color,
            font_name: r.font_name,
            highlight: r.highlight,
        }
    }

    fn parse_paragraph(v: &serde_json::Value) -> Result<Option<crate::office::DocElement>, String> {
        if v["action"].as_str() == Some("delete") {
            if let Some(id) = v["id"].as_str() {
                return Ok(Some(crate::office::DocElement::Paragraph {
                    id: id.to_string(),
                    text: String::new(),
                    omit_text: false,
                    style: None,
                    runs: None,
                    numbering: None,
                }));
            }
            return Err("delete action requires an id".to_string());
        }

        let id = v["id"].as_str().map(|s| s.to_string());

        // The `text` field is optional when modifying an existing paragraph
        // (id is set). Omitting it tells the backend to keep the original
        // text. We record that intent via `omit_text` so `WordDocument::modify`
        // can do the right merge.
        let has_text_key = v.as_object().map(|o| o.contains_key("text")).unwrap_or(false);
        let text = v["text"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let omit_text = !has_text_key;

        let style = v["style"].as_str().map(|s| s.to_string());

        let has_runs_key = v.as_object().map(|o| o.contains_key("runs")).unwrap_or(false);
        let runs: Option<Vec<_>> = if has_runs_key {
            v["runs"].as_array().map(|arr| {
                arr.iter().filter_map(|r| {
                    let text = r["text"].as_str().unwrap_or("").to_string();
                    if text.is_empty() { return None; }
                    Some(crate::office::FontRun {
                        text,
                        bold: r["bold"].as_bool().unwrap_or(false),
                        italic: r["italic"].as_bool().unwrap_or(false),
                        underline: r["underline"].as_bool().unwrap_or(false),
                        strikethrough: r["strikethrough"].as_bool().unwrap_or(false),
                        font_size: r["font_size"].as_u64().map(|n| n as u32),
                        color: r["color"].as_str().map(|s| s.to_string()),
                        font_name: r["font_name"].as_str().map(|s| s.to_string()),
                        highlight: r["highlight"].as_str().map(|s| s.to_string()),
                    })
                }).collect()
            })
        } else {
            None
        };

        let numbering: Option<crate::office::NumberingRef> = v["numbering"].as_object().and_then(|obj| {
            let num_id = obj.get("num_id")?.as_u64()? as u32;
            let level = obj.get("level").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            Some(crate::office::NumberingRef { num_id, level })
        });

        Ok(Some(crate::office::DocElement::Paragraph {
            id: id.unwrap_or_else(|| format!("__new_p{}", uuid_simple())),
            text,
            omit_text,
            style,
            runs,
            numbering,
        }))
    }

    fn parse_table(v: &serde_json::Value) -> Result<Option<crate::office::DocElement>, String> {
        if v["action"].as_str() == Some("delete") {
            if let Some(id) = v["id"].as_str() {
                return Ok(Some(crate::office::DocElement::Table {
                    id: id.to_string(),
                    position: 0,
                    header: vec![],
                    rows: vec![],
                }));
            }
            return Err("delete action requires an id".to_string());
        }

        let id = v["id"].as_str().map(|s| s.to_string());
        let header: Vec<String> = v["header"].as_array()
            .map(|arr| arr.iter().filter_map(|c| c.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        let rows: Vec<Vec<String>> = v["rows"].as_array()
            .map(|arr| arr.iter().filter_map(|r| {
                r.as_array().map(|cells| {
                    cells.iter().filter_map(|c| c.as_str().map(|s| s.to_string())).collect()
                })
            }).collect())
            .unwrap_or_default();

        Ok(Some(crate::office::DocElement::Table {
            id: id.unwrap_or_else(|| format!("__new_t{}", uuid_simple())),
            position: 0,
            header,
            rows,
        }))
    }

    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let params: CreateWordDocParams = serde_json::from_value(arguments)
            .map_err(|e| ToolError::InvalidArguments("create_word_doc".to_string(), format!("Invalid parameters: {}", e)))?;

        validate_workspace_path(&params.path, &workspace)?;

        let path_obj = std::path::Path::new(&params.path);
        if path_obj.extension().and_then(|e| e.to_str()).unwrap_or("") != "docx" {
            return Err(ToolError::InvalidArguments("create_word_doc".to_string(), "Only .docx files are supported".into()));
        }

        // Collect operations from elements[]
        let mut modifies = Vec::new();
        let mut new_elements = Vec::new();
        let mut deletes = Vec::new();

        if let Some(ref elems) = params.elements {
            for v in elems {
                let is_delete = v["action"].as_str() == Some("delete");
                let has_id = v["id"].is_string();
                let has_anchor = v["anchor_id"].is_string();

                if is_delete {
                    if let Some(id) = v["id"].as_str() {
                        deletes.push(id.to_string());
                    }
                    continue;
                }

                let elem_type = v["type"].as_str().unwrap_or("paragraph");
                let result = if elem_type == "table" {
                    Self::parse_table(v)
                } else {
                    Self::parse_paragraph(v)
                };

                let elem = result.map_err(|e| ToolError::InvalidArguments("create_word_doc".to_string(), e))?;

                if let Some(e) = elem {
                    if has_id {
                        modifies.push(e);
                    } else if has_anchor {
                        new_elements.push(e);
                    } else {
                        new_elements.push(e);
                    }
                }
            }
        }

        // Backward compat: convert old paragraphs/tables format
        if let Some(ref paras) = params.paragraphs {
            for p in paras {
                if p.delete_action.as_deref() == Some("delete") {
                    if let Some(ref id) = p.id {
                        deletes.push(id.clone());
                    }
                } else {
                    let elem = crate::office::DocElement::Paragraph {
                        id: p.id.clone().unwrap_or_else(|| format!("__new_p{}", uuid_simple())),
                        text: p.text.clone(),
                        omit_text: false,
                        style: p.style.clone(),
                        runs: p.runs.as_ref().map(|rvec| rvec.iter().map(|r| Self::to_font_run(r.clone())).collect()),
                        numbering: p.numbering.clone().map(crate::office::NumberingRef::from),
                    };
                    if p.id.is_some() {
                        modifies.push(elem);
                    } else {
                        new_elements.push(elem);
                    }
                }
            }
        }

        if let Some(ref tbls) = params.tables {
            for t in tbls {
                if t.delete_action.as_deref() == Some("delete") {
                    if let Some(ref id) = t.id {
                        deletes.push(id.clone());
                    }
                } else {
                    let elem = crate::office::DocElement::Table {
                        id: t.id.clone().unwrap_or_else(|| format!("__new_t{}", uuid_simple())),
                        position: 0,
                        header: t.header.clone(),
                        rows: t.rows.clone(),
                    };
                    if t.id.is_some() {
                        modifies.push(elem);
                    } else {
                        new_elements.push(elem);
                    }
                }
            }
        }

        // Determine if this is purely a new-file creation
        let file_exists = path_obj.exists();
        let has_operations = !modifies.is_empty() || !deletes.is_empty() || !new_elements.is_empty();
        // New file only if: no file exists, OR no operations requested
        let is_pure_new_file = !file_exists || !has_operations;

        // Append/deprecated mode: append_to takes precedence for backward compat
        if let Some(ref append_path) = params.append_to {
            if std::path::Path::new(append_path).exists() {
                validate_workspace_path(append_path, &workspace)?;
                let bytes = tokio::fs::read(append_path)
                    .await
                    .map_err(|e| ToolError::IoError(format!("Failed to read existing doc: {}", e)))?;
                let mut existing = crate::office::read_word_document(&bytes)
                    .map_err(|e| ToolError::ExecutionError(format!("Failed to read existing doc: {}", e)))?;

                let mut new_paras = Vec::new();
                let mut new_tables = Vec::new();
                for e in new_elements {
                    match e {
                        crate::office::DocElement::Paragraph { id, text, style, runs, numbering, .. } => {
                            new_paras.push(crate::office::WordParagraph { id, text, style, runs, numbering });
                        }
                        crate::office::DocElement::Table { id, position: _, header, rows } => {
                            let mut table_rows = vec![];
                            if !header.is_empty() {
                                table_rows.push(crate::office::TableRow {
                                    cells: header.into_iter()
                                        .map(|text| crate::office::TableCell { text, col_span: 1, row_span: 1 })
                                        .collect()
                                });
                            }
                            for row in rows {
                                table_rows.push(crate::office::TableRow {
                                    cells: row.into_iter()
                                        .map(|text| crate::office::TableCell { text, col_span: 1, row_span: 1 })
                                        .collect()
                                });
                            }
                            new_tables.push(crate::office::WordTable { id, rows: table_rows });
                        }
                    }
                }
                existing.paragraphs.extend(new_paras);
                existing.tables.extend(new_tables);

                crate::office::write_word_document_to_path(&existing, path_obj, Some(&bytes))
                    .map_err(|e| ToolError::ExecutionError(format!("Failed to write doc: {}", e)))?;
                return Ok(format!("Successfully appended content to: {}", params.path));
            }
        }

        // Progressive append mode: append new elements to existing document without reading/modifying structure
        if params.append == Some(true) && file_exists && !new_elements.is_empty() {
            let bytes = tokio::fs::read(&params.path)
                .await
                .map_err(|e| ToolError::IoError(format!("Failed to read existing doc: {}", e)))?;
            let mut existing = crate::office::read_word_document(&bytes)
                .map_err(|e| ToolError::ExecutionError(format!("Failed to read existing doc: {}", e)))?;

            // Build a temporary document from just the new elements, then extract its parts
            let temp_doc = crate::office::WordDocument::from_elements(new_elements);
            let new_count = temp_doc.paragraphs.len() + temp_doc.tables.len();

            existing.paragraphs.extend(temp_doc.paragraphs);
            existing.tables.extend(temp_doc.tables);

            crate::office::write_word_document_to_path(&existing, path_obj, Some(&bytes))
                .map_err(|e| ToolError::ExecutionError(format!("Failed to append to doc: {}", e)))?;
            return Ok(format!("Successfully appended {} element(s) to: {}", new_count, params.path));
        }

        // Existing file with operations: modify/delete/insert
        if file_exists && !is_pure_new_file {
            let bytes = tokio::fs::read(&params.path)
                .await
                .map_err(|e| ToolError::IoError(format!("Failed to read existing doc: {}", e)))?;
            let mut existing = crate::office::read_word_document(&bytes)
                .map_err(|e| ToolError::ExecutionError(format!("Failed to read existing doc: {}", e)))?;

            // Determine insert anchor from elements that have anchor_id
            let mut anchor_id: Option<String> = None;
            if let Some(ref arr) = params.elements {
                for v in arr {
                    if v["anchor_id"].is_string() {
                        anchor_id = v["anchor_id"].as_str().map(|s| s.to_string());
                        break;
                    }
                }
            }

            existing.modify(modifies, deletes, anchor_id, new_elements);

            crate::office::write_word_document_to_path(&existing, path_obj, Some(&bytes))
                .map_err(|e| ToolError::ExecutionError(format!("Failed to write doc: {}", e)))?;
            return Ok(format!("Successfully modified document: {}", params.path));
        }

        // Existing file with no operations: no-op
        if file_exists {
            return Ok(format!("Document already exists, no changes requested: {}", params.path));
        }

        // New file mode: title + new_elements
        let mut elements_for_new: Vec<crate::office::DocElement> = Vec::new();

        if let Some(ref title) = params.title {
            if !title.is_empty() {
                elements_for_new.push(crate::office::DocElement::Paragraph {
                    id: format!("__new_p{}", uuid_simple()),
                    text: title.clone(),
                    omit_text: false,
                    style: Some("Title".to_string()),
                    runs: None,
                    numbering: None,
                });
            }
        }

        for e in new_elements {
            elements_for_new.push(e);
        }

        let doc = crate::office::WordDocument::from_elements(elements_for_new);
        crate::office::write_word_document_to_path(&doc, path_obj, None)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to write Word document: {}", e)))?;

        Ok(format!("Successfully created Word document: {}", params.path))
    }
}

impl Default for CreateWordDocTool {
    fn default() -> Self { Self::new() }
}

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    use std::sync::atomic::{AtomicU64, Ordering};
    thread_local! { static CNT: AtomicU64 = AtomicU64::new(0); }
    let cnt = CNT.with(|c| c.fetch_add(1, Ordering::Relaxed));
    format!("{}{}", now.as_nanos(), cnt)
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

// ─── get_docx_info ─────────────────────────────────────────────────────────────

/// Return a cheap-to-compute summary of a .docx file (paragraph/table counts,
/// character counts, presence of headers/footers/images). Use this before
/// `read_office_file` to decide whether the file is worth the full parse.
pub struct GetDocxInfoTool;

impl GetDocxInfoTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "get_docx_info",
            "获取 Word 文档信息",
            "Read summary metadata of a Word (.docx) file without returning its full content.",
            ToolParameters::new(
                vec!["path"],
                vec![
                    ("path", "string", Some("Absolute path to the .docx file")),
                ],
            ),
        )
    }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("get_docx_info".to_string(), "path must be a string".into()))?;
        validate_workspace_path(path, &workspace)?;

        let path_obj = std::path::Path::new(path);
        if path_obj.extension().and_then(|e| e.to_str()).unwrap_or("") != "docx" {
            return Err(ToolError::InvalidArguments("get_docx_info".to_string(), "Only .docx files are supported".into()));
        }

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
        // Include table cell text in the totals too.
        for tbl in &doc.tables {
            for row in &tbl.rows {
                for cell in &row.cells {
                    total_chars += cell.text.chars().count();
                    word_count += cell.text.split_whitespace().count();
                }
            }
        }

        // Inspect the zip for headers/footers/images without parsing their XML.
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
}

impl Default for GetDocxInfoTool {
    fn default() -> Self { Self::new() }
}

// ─── modify_excel ─────────────────────────────────────────────────────────────

/// Cell-level modification accepted by [`ModifyExcelTool`].
#[derive(Debug, Clone, Deserialize)]
struct ExcelCellInput {
    /// Sheet name (case-sensitive).
    sheet: String,
    /// Cell address in A1 form, e.g. "B3".
    address: String,
    /// New value. JSON shape:
    ///   {"type":"int","value":100}
    ///   {"type":"float","value":3.14}
    ///   {"type":"bool","value":true}
    ///   {"type":"string","value":"hello"}
    ///   {"type":"empty"}
    #[serde(default)]
    value: Option<serde_json::Value>,
    /// New formula (without leading "="), e.g. "SUM(A1:A10)".
    #[serde(default)]
    formula: Option<String>,
    /// Optional new number format, e.g. "0.00%" or "yyyy-mm-dd".
    #[serde(default)]
    number_format: Option<String>,
}

impl ExcelCellInput {
    fn into_modification(self) -> Result<crate::office::CellModification, String> {
        let mut m = crate::office::CellModification::new(self.sheet, self.address);
        if let Some(v) = self.value {
            m.new_value = Some(parse_cell_value_json(v)?);
        }
        m.new_formula = self.formula;
        m.new_number_format = self.number_format;
        Ok(m)
    }
}

fn parse_cell_value_json(v: serde_json::Value) -> Result<crate::office::CellValue, String> {
    use crate::office::CellValue;
    let obj = v.as_object().ok_or_else(|| "value must be an object {type, value}".to_string())?;
    let kind = obj.get("type").and_then(|t| t.as_str()).ok_or_else(|| "value missing 'type'".to_string())?;
    match kind {
        "empty" => Ok(CellValue::Empty),
        "int" => {
            let n = obj.get("value").and_then(|x| x.as_i64()).ok_or_else(|| "int.value missing".to_string())?;
            Ok(CellValue::Int(n))
        }
        "float" => {
            let n = obj.get("value").and_then(|x| x.as_f64()).ok_or_else(|| "float.value missing".to_string())?;
            Ok(CellValue::Float(n))
        }
        "bool" => {
            let b = obj.get("value").and_then(|x| x.as_bool()).ok_or_else(|| "bool.value missing".to_string())?;
            Ok(CellValue::Bool(b))
        }
        "string" => {
            let s = obj.get("value").and_then(|x| x.as_str()).ok_or_else(|| "string.value missing".to_string())?;
            Ok(CellValue::String(s.to_string()))
        }
        "datetime" => {
            let n = obj.get("value").and_then(|x| x.as_f64()).ok_or_else(|| "datetime.value missing".to_string())?;
            Ok(CellValue::DateTime(n))
        }
        "error" => {
            let s = obj.get("value").and_then(|x| x.as_str()).ok_or_else(|| "error.value missing".to_string())?;
            Ok(CellValue::Error(s.to_string()))
        }
        other => Err(format!("unknown value type '{}'", other)),
    }
}

/// Surgical cell-level editor for Excel workbooks. Unlike rewriting the entire
/// workbook, this tool only changes the listed cells and leaves every other
/// part of the file (formulas, styles, charts, defined names, themes, etc.)
/// intact.
pub struct ModifyExcelTool;

impl ModifyExcelTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "modify_excel",
            "修改 Excel 单元格",
            "Modify specific cells in an Excel (.xlsx) file by surgically rewriting only the changed cells. All other workbook content (formulas, styles, charts) is preserved. Use read_office_file first to confirm sheet names and current values.",
            ToolParameters::new(
                vec!["path", "modifications"],
                vec![
                    ("path", "string", Some("Absolute path to the .xlsx file to modify")),
                    ("modifications", "array", Some(
                        "Array of cell modifications. Each entry: {sheet, address, value?, formula?, number_format?}.\n\
                         - sheet: sheet name (e.g. \"Sales\")\n\
                         - address: cell address in A1 form (e.g. \"B3\")\n\
                         - value: {type, value} where type is one of: empty|int|float|bool|string|datetime|error\n\
                         - formula: formula text without leading '=' (e.g. \"SUM(A1:A10)\"). Setting a formula replaces any existing value.\n\
                         - number_format: optional Excel number format string (e.g. \"0.00%\", \"yyyy-mm-dd\")\n\
                         At least one of value/formula must be provided."
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

        let mods_json = arguments["modifications"].as_array()
            .ok_or_else(|| ToolError::InvalidArguments("modify_excel".to_string(), "modifications must be an array".into()))?;

        let path_obj = std::path::Path::new(&path);
        if path_obj.extension().and_then(|e| e.to_str()).unwrap_or("") != "xlsx" {
            return Err(ToolError::InvalidArguments(
                "modify_excel".to_string(),
                "Only .xlsx files are supported".into(),
            ));
        }

        let mut inputs: Vec<ExcelCellInput> = Vec::new();
        for v in mods_json {
            let parsed: ExcelCellInput = serde_json::from_value(v.clone())
                .map_err(|e| ToolError::InvalidArguments("modify_excel".to_string(), format!("Invalid modification entry: {}", e)))?;
            inputs.push(parsed);
        }
        let modifications: Vec<crate::office::CellModification> = inputs
            .into_iter()
            .map(|i| i.into_modification().map_err(|e| ToolError::InvalidArguments("modify_excel".to_string(), e)))
            .collect::<Result<Vec<_>, _>>()?;

        if modifications.is_empty() {
            return Err(ToolError::InvalidArguments(
                "modify_excel".to_string(),
                "modifications array is empty".into(),
            ));
        }

        let bytes = tokio::fs::read(&path).await
            .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;

        // Write to a sibling .tmp first, then atomically replace the original.
        let tmp_path = path_obj.with_extension("xlsx.tmp");
        crate::office::incremental_write_xlsx(&bytes, &modifications, &tmp_path)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to modify xlsx: {}", e)))?;
        tokio::fs::rename(&tmp_path, &path).await
            .map_err(|e| ToolError::IoError(format!("Failed to replace original file: {}", e)))?;

        let summary: Vec<String> = modifications.iter().map(|m| {
            let mut parts = vec![format!("{}!{}", m.sheet, m.address)];
            if let Some(v) = &m.new_value {
                parts.push(format!("value={}", v.as_string_for_display()));
            }
            if let Some(f) = &m.new_formula {
                parts.push(format!("formula={}", f));
            }
            if let Some(fmt) = &m.new_number_format {
                parts.push(format!("format={}", fmt));
            }
            parts.join(" ")
        }).collect();

        Ok(format!(
            "Successfully modified {} cell(s) in {}: {}",
            modifications.len(),
            path,
            summary.join("; ")
        ))
    }
}

impl Default for ModifyExcelTool {
    fn default() -> Self { Self::new() }
}

// ─── get_excel_info ───────────────────────────────────────────────────────────

/// Cheap-to-compute summary of a .xlsx file. Use this before `read_office_file`
/// to decide whether the workbook is worth the full structured parse.
pub struct GetExcelInfoTool;

impl GetExcelInfoTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "get_excel_info",
            "获取 Excel 工作簿信息",
            "Read summary metadata of an Excel (.xlsx) file without returning its full content.",
            ToolParameters::new(
                vec!["path"],
                vec![
                    ("path", "string", Some("Absolute path to the .xlsx file")),
                ],
            ),
        )
    }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"].as_str()
            .ok_or_else(|| ToolError::InvalidArguments("get_excel_info".to_string(), "path must be a string".into()))?;
        validate_workspace_path(path, &workspace)?;

        let path_obj = std::path::Path::new(path);
        if path_obj.extension().and_then(|e| e.to_str()).unwrap_or("") != "xlsx" {
            return Err(ToolError::InvalidArguments("get_excel_info".to_string(), "Only .xlsx files are supported".into()));
        }

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

        let result = serde_json::json!({
            "file_name": path_obj.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
            "path": path,
            "sheet_count": workbook.sheets.len(),
            "total_cells": total_cells,
            "total_formulas": total_formulas,
            "sheets": sheet_summaries,
            "file_size_bytes": bytes.len(),
        });
        Ok(result.to_string())
    }
}

impl Default for GetExcelInfoTool {
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
