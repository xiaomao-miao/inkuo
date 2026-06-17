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
                let json = serde_json::to_string(&workbook)
                    .map_err(|e| ToolError::ExecutionError(format!("JSON serialization failed: {}", e)))?;
                serde_json::json!({
                    "file_type": "xlsx",
                    "file_name": path_obj.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
                    "sheets": workbook.sheets.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
                    "text_content": text_content,
                    "json_content": json,
                    "note": "Modify json_content to update spreadsheet data."
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
