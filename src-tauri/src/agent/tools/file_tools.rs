//! File manipulation tools: read_file, write_file, edit_file

use serde_json::Value;
use std::path::Path;

use super::{ToolDefinition, ToolError, ToolParameters, validate_workspace_path};

pub fn definition() -> ToolDefinition {
    ToolDefinition::new_with_label(
        "read_file",
        "读取文件",
        "Read the complete contents of a file from the filesystem.",
        ToolParameters::new(
            vec!["path"],
            vec![
                ("path", "string", Some("Absolute path to the file to read")),
                ("offset", "integer", Some("Line number to start reading from (0-indexed). Default: 0")),
                ("limit", "integer", Some("Maximum number of lines to read. Default: all lines")),
            ],
        ),
    )
}

pub async fn execute(arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
    let path = arguments["path"]
        .as_str()
        .ok_or_else(|| ToolError::InvalidArguments("read_file".to_string(), "path must be a string".into()))?;

    validate_workspace_path(path, &workspace)?;

    let offset = arguments["offset"].as_u64().unwrap_or(0) as usize;
    let limit = arguments["limit"].as_u64();

    tokio::fs::read_to_string(path)
        .await
        .map_err(|e| ToolError::IoError(format!("Failed to read file {}: {}", path, e)))
        .map(|content| {
            let lines: Vec<&str> = content.lines().collect();
            if offset >= lines.len() {
                return String::new();
            }
            let end = limit.map(|l| (offset + l as usize).min(lines.len())).unwrap_or(lines.len());
            lines[offset..end].join("\n")
        })
}

pub struct ReadFileTool;

impl ReadFileTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition { definition() }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        execute(arguments, workspace).await
    }
}

impl Default for ReadFileTool {
    fn default() -> Self { Self::new() }
}

// ─── WriteFile ────────────────────────────────────────────────────────────────

pub struct WriteFileTool;

impl WriteFileTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "write_file",
            "写入文件",
            "Create a new file or overwrite an existing file with the given content.",
            ToolParameters::new(
                vec!["path", "content"],
                vec![
                    ("path", "string", Some("Absolute path where the file should be created")),
                    ("content", "string", Some("The complete content to write to the file")),
                ],
            ),
        )
    }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("write_file".to_string(), "path must be a string".into()))?;

        validate_workspace_path(path, &workspace)?;

        let content = arguments["content"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("write_file".to_string(), "content must be a string".into()))?;

        if let Some(parent) = Path::new(path).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::IoError(format!("Failed to create directory: {}", e)))?;
        }

        tokio::fs::write(path, content)
            .await
            .map_err(|e| ToolError::IoError(format!("Failed to write file {}: {}", path, e)))?;

        Ok(format!("File '{}' written successfully", path))
    }
}

impl Default for WriteFileTool {
    fn default() -> Self { Self::new() }
}

// ─── CreateDir ─────────────────────────────────────────────────────────────────

pub struct CreateDirTool;

impl CreateDirTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "create_dir",
            "创建目录",
            "Create a new directory. Creates parent directories as needed (like mkdir -p).",
            ToolParameters::new(
                vec!["path"],
                vec![
                    ("path", "string", Some("Absolute path of the directory to create")),
                ],
            ),
        )
    }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("create_dir".to_string(), "path must be a string".into()))?;

        validate_workspace_path(path, &workspace)?;

        tokio::fs::create_dir_all(path)
            .await
            .map_err(|e| ToolError::IoError(format!("Failed to create directory {}: {}", path, e)))?;

        Ok(format!("Directory '{}' created successfully", path))
    }
}

impl Default for CreateDirTool {
    fn default() -> Self { Self::new() }
}

// ─── EditFile ──────────────────────────────────────────────────────────────

pub struct EditFileTool;

impl EditFileTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "edit_file",
            "编辑文件",
            "Edit a specific portion of an existing file by replacing old_text with new_text.",
            ToolParameters::new(
                vec!["path", "old_text", "new_text"],
                vec![
                    ("path", "string", Some("Absolute path to the file to edit")),
                    ("old_text", "string", Some("The exact text to find and replace. Must match exactly including whitespace and newlines.")),
                    ("new_text", "string", Some("The replacement text")),
                ],
            ),
        )
    }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("edit_file".to_string(), "path must be a string".into()))?;

        validate_workspace_path(path, &workspace)?;

        let old_text = arguments["old_text"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("edit_file".to_string(), "old_text must be a string".into()))?;

        let new_text = arguments["new_text"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("edit_file".to_string(), "new_text must be a string".into()))?;

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| ToolError::IoError(format!("Failed to read file {}: {}", path, e)))?;

        if !content.contains(old_text) {
            return Err(ToolError::InvalidArguments(
                "edit_file".to_string(),
                format!("old_text not found in file. Make sure to provide the exact text including whitespace and newlines.\n\nSearched for:\n{}", old_text),
            ));
        }

        let new_content = content.replace(old_text, new_text);

        tokio::fs::write(path, &new_content)
            .await
            .map_err(|e| ToolError::IoError(format!("Failed to write file {}: {}", path, e)))?;

        Ok(format!("File '{}' edited successfully", path))
    }
}

impl Default for EditFileTool {
    fn default() -> Self { Self::new() }
}

// ─── MoveFile ─────────────────────────────────────────────────────────────────

pub struct MoveFileTool;

impl MoveFileTool {
    pub fn new() -> Self { Self }
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "move_file",
            "移动文件",
            "Move or rename a file or directory.",
            ToolParameters::new(
                vec!["source", "destination"],
                vec![
                    ("source", "string", Some("Absolute path of the file or directory to move")),
                    ("destination", "string", Some("Absolute path of the destination (can be new name for rename)")),
                ],
            ),
        )
    }
    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        let source = arguments["source"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("move_file".to_string(), "source must be a string".into()))?;

        let destination = arguments["destination"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("move_file".to_string(), "destination must be a string".into()))?;

        validate_workspace_path(source, &workspace)?;
        validate_workspace_path(destination, &workspace)?;

        if !Path::new(source).exists() {
            return Err(ToolError::IoError(format!("Source path does not exist: {}", source)));
        }

        if let Some(parent) = Path::new(destination).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::IoError(format!("Failed to create destination directory: {}", e)))?;
        }

        tokio::fs::rename(source, destination)
            .await
            .map_err(|e| ToolError::IoError(format!("Failed to move file from '{}' to '{}': {}", source, destination, e)))?;

        Ok(format!("Moved '{}' to '{}' successfully", source, destination))
    }
}

impl Default for MoveFileTool {
    fn default() -> Self { Self::new() }
}
