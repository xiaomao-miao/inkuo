//! Office file tools: read_office_file, write_office_file

use serde_json::Value;

use super::{ToolDefinition, ToolError, ToolParameters, validate_workspace_path};

pub struct ReadOfficeFileTool;

impl ReadOfficeFileTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "read_office_file",
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
                format!(
                    "Unsupported file type: '{}'. Only .docx and .xlsx files are supported.",
                    extension
                ),
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
                let json = serde_json::to_string(&doc)
                    .map_err(|e| ToolError::ExecutionError(format!("JSON serialization failed: {}", e)))?;
                serde_json::json!({
                    "file_type": "docx",
                    "file_name": path_obj.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
                    "text_content": text_content,
                    "json_content": json,
                    "note": "To modify this document, produce a modified JSON representation and use the frontend to save."
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
                    "note": "To modify this spreadsheet, produce a modified JSON representation."
                }).to_string()
            }
        };

        Ok(response)
    }
}

impl Default for ReadOfficeFileTool {
    fn default() -> Self { Self::new() }
}

pub struct WriteOfficeFileTool;

impl WriteOfficeFileTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "write_office_file",
            "Write a modified Word (.docx) or Excel (.xlsx) file from a JSON representation.",
            ToolParameters::new(
                vec!["path", "json_content"],
                vec![
                    ("path", "string", Some("Absolute path to the office file to write")),
                    ("json_content", "string", Some("JSON representation of the modified document")),
                ],
            ),
        )
    }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("write_office_file".to_string(), "path must be a string".into()))?;

        let json_content = arguments["json_content"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("write_office_file".to_string(), "json_content must be a string".into()))?;

        validate_workspace_path(path, &workspace)?;

        let path_obj = std::path::Path::new(path);
        let extension = path_obj
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if !matches!(extension.as_str(), "docx" | "xlsx") {
            return Err(ToolError::InvalidArguments(
                "write_office_file".to_string(),
                format!(
                    "Unsupported file type: '{}'. Only .docx and .xlsx files are supported.",
                    extension
                ),
            ));
        }

        match extension.as_str() {
            "xlsx" => {
                let _: crate::office::ExcelWorkbook = serde_json::from_str(json_content)
                    .map_err(|e| ToolError::InvalidArguments("write_office_file".to_string(), format!("Invalid Excel JSON: {}", e)))?;
            }
            "docx" => {
                let _: crate::office::WordDocument = serde_json::from_str(json_content)
                    .map_err(|e| ToolError::InvalidArguments("write_office_file".to_string(), format!("Invalid Word JSON: {}", e)))?;
            }
            _ => unreachable!(),
        }

        crate::office::write_office_file(path_obj, json_content)
            .map_err(|e| ToolError::ExecutionError(format!("Failed to write office file: {}", e)))?;

        Ok(format!("Successfully wrote office file: {}", path))
    }
}

impl Default for WriteOfficeFileTool {
    fn default() -> Self { Self::new() }
}
