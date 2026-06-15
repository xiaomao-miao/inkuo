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

        let bytes = tokio::fs::read(path)
            .await
            .map_err(|e| ToolError::IoError(format!("Failed to read file {}: {}", path, e)))?;

        let result = crate::office::read_office_file(path_obj)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to parse office file: {}", e)))?;

        let (file_type, text_content) = result;

        let response = match file_type {
            crate::office::OfficeFileType::Word(doc) => {
                let json = serde_json::to_string(&doc)
                    .map_err(|e| ToolError::ExecutionError(format!("JSON serialization failed: {}", e)))?;
                serde_json::json!({
                    "file_type": "docx",
                    "file_name": path_obj.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
                    "text_content": text_content,
                    "json_content": json,
                    "note": "Use create_word_doc to save modifications."
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
#[derive(Debug, Deserialize)]
struct DocTextRun {
    text: String,
    #[serde(default)]
    bold: Option<bool>,
    #[serde(default)]
    italic: Option<bool>,
    #[serde(default)]
    underline: Option<bool>,
    #[serde(default)]
    font_size: Option<u32>,   // half-points, e.g. 24 = 12pt
    #[serde(default)]
    color: Option<String>,    // hex RGB, e.g. "FF0000"
    #[serde(default)]
    font_name: Option<String>,
}

/// A paragraph in the document.
#[derive(Debug, Deserialize)]
struct DocParagraph {
    /// The paragraph text.
    text: String,
    /// Optional style: "Heading1" (large blue), "Heading2", "Heading3", "Normal".
    #[serde(default)]
    style: Option<String>,
    /// Optional rich text runs for inline formatting.
    #[serde(default)]
    runs: Option<Vec<DocTextRun>>,
}

/// A table in the document.
#[derive(Debug, Deserialize)]
struct DocTable {
    /// Column header labels (becomes the first table row).
    header: Vec<String>,
    /// Data rows (each row is an array of cell values).
    rows: Vec<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct CreateWordDocParams {
    /// Absolute path of the .docx file to create.
    path: String,
    /// Document title (shown as the first paragraph with Title style).
    title: String,
    /// Content paragraphs.
    paragraphs: Vec<DocParagraph>,
    /// Optional tables.
    tables: Option<Vec<DocTable>>,
    /// Optional: path to an existing .docx. If provided, appends content to that file.
    #[serde(default)]
    append_to: Option<String>,
}

pub struct CreateWordDocTool;

impl CreateWordDocTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
            ToolDefinition::new_with_label(
                "create_word_doc",
                "创建 Word 文档",
                "Create or append a Word (.docx) document from structured content. No JSON string needed — use this for all Word document creation and editing.",
            ToolParameters::new(
                vec!["path", "title", "paragraphs"],
                vec![
                    ("path", "string", Some("Absolute path of the .docx file to create")),
                    ("title", "string", Some("Document title text")),
                    ("paragraphs", "array", Some("Array of paragraph objects: {text, style:(Heading1/Heading2/Heading3/Normal), runs:[{text,bold,italic,underline,font_size,color}]}")),
                    ("tables", "array", Some("Optional: {header:[col_names], rows:[[r1c1,r1c2],...]}")),
                    ("append_to", "string", Some("Optional: path to an existing .docx. If provided, appends content to that file instead of creating new. Ignores title when used.")),
                ],
            ),
        )
    }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let params: CreateWordDocParams = serde_json::from_value(arguments)
            .map_err(|e| ToolError::InvalidArguments("create_word_doc".to_string(), format!("Invalid parameters: {}", e)))?;

        validate_workspace_path(&params.path, &workspace)?;

        let path_obj = std::path::Path::new(&params.path);
        if path_obj.extension().and_then(|e| e.to_str()).unwrap_or("") != "docx" {
            return Err(ToolError::InvalidArguments("create_word_doc".to_string(), "Only .docx files are supported".into()));
        }

        // Build paragraphs from DocParagraph -> WordParagraph
        let content_paras: Vec<_> = params.paragraphs.into_iter()
            .map(|p| {
                let style = p.style.unwrap_or_else(|| "Normal".to_string());
                let runs = p.runs.map(|rvec| rvec.into_iter().map(|r| crate::office::FontRun {
                    text: r.text,
                    bold: r.bold.unwrap_or(false),
                    italic: r.italic.unwrap_or(false),
                    underline: r.underline.unwrap_or(false),
                    font_size: r.font_size,
                    color: r.color,
                    font_name: r.font_name,
                }).collect());
                crate::office::WordParagraph { text: p.text, style: Some(style), runs }
            })
            .collect();

        // Build tables
        let tables: Vec<_> = params.tables
            .unwrap_or_default()
            .into_iter()
            .map(|t| {
                let header_cells: Vec<_> = t.header.into_iter()
                    .map(|text| crate::office::TableCell { text, col_span: 1, row_span: 1 })
                    .collect();
                let mut rows = vec![crate::office::TableRow { cells: header_cells }];
                for row_data in t.rows {
                    let cells: Vec<_> = row_data.into_iter()
                        .map(|text| crate::office::TableCell { text, col_span: 1, row_span: 1 })
                        .collect();
                    rows.push(crate::office::TableRow { cells });
                }
                crate::office::WordTable { rows }
            })
            .collect();

        // Append mode: read existing doc, extend content, write back
        if let Some(ref append_path) = params.append_to {
            if std::path::Path::new(append_path).exists() {
                validate_workspace_path(append_path, &workspace)?;
                let bytes = tokio::fs::read(append_path)
                    .await
                    .map_err(|e| ToolError::IoError(format!("Failed to read existing doc: {}", e)))?;
                let mut existing = crate::office::read_word_document(&bytes)
                    .map_err(|e| ToolError::ExecutionError(format!("Failed to read existing doc: {}", e)))?;
                existing.paragraphs.extend(content_paras);
                existing.tables.extend(tables);
                crate::office::write_word_document(&existing, path_obj)
                    .map_err(|e| ToolError::ExecutionError(format!("Failed to write appended doc: {}", e)))?;
                return Ok(format!("Successfully appended content to: {}", params.path));
            }
        }

        // New file mode
        let paragraphs = if params.title.is_empty() {
            content_paras
        } else {
            let mut all = vec![crate::office::WordParagraph {
                text: params.title,
                style: Some("Title".to_string()),
                runs: None,
            }];
            all.extend(content_paras);
            all
        };

        let doc = crate::office::WordDocument { paragraphs, tables };
        crate::office::write_word_document(&doc, path_obj)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to write Word document: {}", e)))?;

        Ok(format!("Successfully created Word document: {}", params.path))
    }
}

impl Default for CreateWordDocTool {
    fn default() -> Self { Self::new() }
}
