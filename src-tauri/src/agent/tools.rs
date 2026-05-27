//! Tool definitions and registry for agent tool calling
//!
//! This module provides:
//! - ToolDefinition: JSON Schema for tool parameters
//! - ToolResult: Execution result wrapper
//! - ToolExecutor enum: Enum-based tool implementation
//! - ToolRegistry: Central tool registration and execution
//! - Built-in tools: read_file, write_file, edit_file, list_dir, glob, grep

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ToolError {
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
    #[error("Invalid arguments for tool {0}: {1}")]
    InvalidArguments(String, String),
    #[error("Execution error: {0}")]
    ExecutionError(String),
    #[error("IO error: {0}")]
    IoError(String),
}

/// Tool parameter schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParameter {
    #[serde(rename = "type")]
    pub param_type: String,
    pub description: Option<String>,
    #[serde(default)]
    pub default: Option<Value>,
}

/// Tool parameters schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParameters {
    #[serde(rename = "type")]
    pub params_type: String,
    pub properties: HashMap<String, ToolParameter>,
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default)]
    pub additionalProperties: bool,
}

impl ToolParameters {
    pub fn new(required: Vec<&str>, properties: Vec<(&str, &str, Option<&str>)>) -> Self {
        let mut props = HashMap::new();
        for (name, param_type, description) in properties {
            props.insert(
                name.to_string(),
                ToolParameter {
                    param_type: param_type.to_string(),
                    description: description.map(String::from),
                    default: None,
                },
            );
        }
        Self {
            params_type: "object".to_string(),
            properties: props,
            required: required.iter().map(|s| s.to_string()).collect(),
            additionalProperties: false,
        }
    }
}

/// Tool definition following OpenAI function calling format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: ToolFunction,
}

impl ToolDefinition {
    pub fn new(name: &str, description: &str, parameters: ToolParameters) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: name.to_string(),
                description: description.to_string(),
                parameters,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: ToolParameters,
}

/// Tool call request from AI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Tool execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub output: String,
    pub is_error: bool,
    /// Original file content before modification (for diff calculation)
    pub original_content: Option<String>,
    /// New content after modification (for diff calculation)
    pub new_content: Option<String>,
    /// Path of modified file (if applicable)
    pub file_path: Option<String>,
}

impl ToolResult {
    pub fn success(tool_call_id: &str, output: impl Into<String>) -> Self {
        Self {
            tool_call_id: tool_call_id.to_string(),
            output: output.into(),
            is_error: false,
            original_content: None,
            new_content: None,
            file_path: None,
        }
    }

    pub fn error(tool_call_id: &str, error: impl Into<String>) -> Self {
        Self {
            tool_call_id: tool_call_id.to_string(),
            output: error.into(),
            is_error: true,
            original_content: None,
            new_content: None,
            file_path: None,
        }
    }

    /// Create a success result for file modifications with diff info
    pub fn file_modified(
        tool_call_id: &str,
        output: impl Into<String>,
        file_path: impl Into<String>,
        original_content: impl Into<String>,
        new_content: impl Into<String>,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.to_string(),
            output: output.into(),
            is_error: false,
            original_content: Some(original_content.into()),
            new_content: Some(new_content.into()),
            file_path: Some(file_path.into()),
        }
    }
}

// ============================================================================
// Tool Executor Enum
// ============================================================================

/// Enum-based tool executor for dyn compatibility
pub enum ToolExecutor {
    ReadFile(ReadFileTool),
    WriteFile(WriteFileTool),
    EditFile(EditFileTool),
    ListDir(ListDirTool),
    Glob(GlobTool),
    Grep(GrepTool),
}

impl ToolExecutor {
    pub fn name(&self) -> &str {
        match self {
            ToolExecutor::ReadFile(_) => "read_file",
            ToolExecutor::WriteFile(_) => "write_file",
            ToolExecutor::EditFile(_) => "edit_file",
            ToolExecutor::ListDir(_) => "list_dir",
            ToolExecutor::Glob(_) => "glob",
            ToolExecutor::Grep(_) => "grep",
        }
    }

    pub fn definition(&self) -> ToolDefinition {
        match self {
            ToolExecutor::ReadFile(t) => t.definition(),
            ToolExecutor::WriteFile(t) => t.definition(),
            ToolExecutor::EditFile(t) => t.definition(),
            ToolExecutor::ListDir(t) => t.definition(),
            ToolExecutor::Glob(t) => t.definition(),
            ToolExecutor::Grep(t) => t.definition(),
        }
    }

    pub async fn execute(&self, arguments: Value) -> Result<String, ToolError> {
        match self {
            ToolExecutor::ReadFile(t) => t.execute(arguments).await,
            ToolExecutor::WriteFile(t) => t.execute(arguments).await,
            ToolExecutor::EditFile(t) => t.execute(arguments).await,
            ToolExecutor::ListDir(t) => t.execute(arguments).await,
            ToolExecutor::Glob(t) => t.execute(arguments).await,
            ToolExecutor::Grep(t) => t.execute(arguments).await,
        }
    }
}

// ============================================================================
// Built-in Tools
// ============================================================================

/// Read file tool
pub struct ReadFileTool;

impl ReadFileTool {
    pub fn new() -> Self {
        Self
    }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "read_file",
            "Read the complete contents of a file from the filesystem. Use this when you need to see what's in a file.",
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

    pub async fn execute(&self, arguments: Value) -> Result<String, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("read_file".to_string(), "path must be a string".into()))?;

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
}

impl Default for ReadFileTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Write file tool
pub struct WriteFileTool;

impl WriteFileTool {
    pub fn new() -> Self {
        Self
    }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "write_file",
            "Create a new file or overwrite an existing file with the given content. Use this when you need to create or completely replace a file.",
            ToolParameters::new(
                vec!["path", "content"],
                vec![
                    ("path", "string", Some("Absolute path where the file should be created")),
                    ("content", "string", Some("The complete content to write to the file")),
                ],
            ),
        )
    }

    pub async fn execute(&self, arguments: Value) -> Result<String, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("write_file".to_string(), "path must be a string".into()))?;

        let content = arguments["content"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("write_file".to_string(), "content must be a string".into()))?;

        // Ensure parent directory exists
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
    fn default() -> Self {
        Self::new()
    }
}

/// Edit file tool - replace specific text in a file
pub struct EditFileTool;

impl EditFileTool {
    pub fn new() -> Self {
        Self
    }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "edit_file",
            "Edit a specific portion of an existing file by replacing old_text with new_text. Use this when you need to make targeted changes to a file without replacing its entire contents. Make sure the old_text you provide matches exactly what's in the file.",
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

    pub async fn execute(&self, arguments: Value) -> Result<String, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("edit_file".to_string(), "path must be a string".into()))?;

        let old_text = arguments["old_text"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("edit_file".to_string(), "old_text must be a string".into()))?;

        let new_text = arguments["new_text"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("edit_file".to_string(), "new_text must be a string".into()))?;

        // Read current content
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| ToolError::IoError(format!("Failed to read file {}: {}", path, e)))?;

        // Check if old_text exists
        if !content.contains(old_text) {
            return Err(ToolError::InvalidArguments(
                "edit_file".to_string(),
                format!("old_text not found in file. Make sure to provide the exact text including whitespace and newlines.\n\nSearched for:\n{}", old_text),
            ));
        }

        // Replace all occurrences
        let new_content = content.replace(old_text, new_text);

        // Write back
        tokio::fs::write(path, &new_content)
            .await
            .map_err(|e| ToolError::IoError(format!("Failed to write file {}: {}", path, e)))?;

        Ok(format!("File '{}' edited successfully", path))
    }
}

impl Default for EditFileTool {
    fn default() -> Self {
        Self::new()
    }
}

/// List directory tool
pub struct ListDirTool;

impl ListDirTool {
    pub fn new() -> Self {
        Self
    }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "list_dir",
            "List the contents of a directory. Use this to see what files and subdirectories exist in a folder.",
            ToolParameters::new(
                vec!["path"],
                vec![
                    ("path", "string", Some("Absolute path to the directory to list")),
                ],
            ),
        )
    }

    pub async fn execute(&self, arguments: Value) -> Result<String, ToolError> {
        let path = arguments["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("list_dir".to_string(), "path must be a string".into()))?;

        let mut entries = Vec::new();

        let mut dir = tokio::fs::read_dir(path)
            .await
            .map_err(|e| ToolError::IoError(format!("Failed to read directory {}: {}", path, e)))?;

        while let Some(entry) = dir.next_entry().await.map_err(|e| ToolError::IoError(e.to_string()))? {
            let file_type = entry.file_type().await.map_err(|e| ToolError::IoError(e.to_string()))?;
            let name = entry.file_name().to_string_lossy().to_string();

            let entry_type = if file_type.is_dir() {
                "[DIR]"
            } else if file_type.is_symlink() {
                "[SYMLINK]"
            } else {
                "[FILE]"
            };

            entries.push(format!("{} {}", entry_type, name));
        }

        entries.sort();
        entries.insert(0, format!("Contents of '{}':", path));
        Ok(entries.join("\n"))
    }
}

impl Default for ListDirTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Glob tool - find files matching a pattern
pub struct GlobTool;

impl GlobTool {
    pub fn new() -> Self {
        Self
    }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "glob",
            "Find all files matching a glob pattern. Use this to search for files by name patterns. Common patterns: **/*.rs for all Rust files, **/*.md for all markdown files, src/** for everything in src folder.",
            ToolParameters::new(
                vec!["pattern", "base_dir"],
                vec![
                    ("pattern", "string", Some("Glob pattern to match (e.g., '**/*.rs', 'src/**/*.{ts,tsx}')")),
                    ("base_dir", "string", Some("Base directory to search from")),
                ],
            ),
        )
    }

    pub async fn execute(&self, arguments: Value) -> Result<String, ToolError> {
        let pattern = arguments["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("glob".to_string(), "pattern must be a string".into()))?;

        let base_dir = arguments["base_dir"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("glob".to_string(), "base_dir must be a string".into()))?;

        let matches = glob::glob(
            &if pattern.starts_with('/') {
                pattern.to_string()
            } else {
                format!("{}/{}", base_dir.trim_end_matches('/'), pattern)
            },
        )
        .map_err(|e| ToolError::ExecutionError(format!("Glob error: {}", e)))?;

        let mut files: Vec<String> = Vec::new();

        for entry in matches {
            match entry {
                Ok(path) => {
                    // Convert to relative path from base_dir
                    if let Ok(rel) = path.strip_prefix(base_dir) {
                        files.push(rel.to_string_lossy().to_string().trim_start_matches('/').to_string());
                    } else {
                        files.push(path.to_string_lossy().to_string());
                    }
                }
                Err(e) => {
                    tracing::warn!("Glob match error: {}", e);
                }
            }
        }

        files.sort();
        if files.is_empty() {
            Ok(format!("No files matching pattern '{}' found in '{}'", pattern, base_dir))
        } else {
            Ok(format!("Found {} file(s):\n{}", files.len(), files.join("\n")))
        }
    }
}

impl Default for GlobTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Grep tool - search for text in files
pub struct GrepTool;

impl GrepTool {
    pub fn new() -> Self {
        Self
    }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "grep",
            "Search for lines containing a pattern in files. Use this to find specific text or code across multiple files. Supports basic regex patterns.",
            ToolParameters::new(
                vec!["pattern", "paths"],
                vec![
                    ("pattern", "string", Some("Text pattern or regex to search for")),
                    ("paths", "array", Some("Array of file/directory paths to search in")),
                    ("case_sensitive", "boolean", Some("Whether search should be case sensitive. Default: false")),
                ],
            ),
        )
    }

    pub async fn execute(&self, arguments: Value) -> Result<String, ToolError> {
        let pattern = arguments["pattern"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("grep".to_string(), "pattern must be a string".into()))?;

        let paths = arguments["paths"]
            .as_array()
            .ok_or_else(|| ToolError::InvalidArguments("grep".to_string(), "paths must be an array".into()))?;

        let case_sensitive = arguments["case_sensitive"].as_bool().unwrap_or(false);

        if paths.is_empty() {
            return Err(ToolError::InvalidArguments("grep".to_string(), "paths array cannot be empty".into()));
        }

        let mut results: Vec<String> = Vec::new();
        let pattern_lower = if case_sensitive {
            pattern.to_string()
        } else {
            pattern.to_lowercase()
        };

        for path_val in paths {
            let path = path_val.as_str().ok_or_else(|| {
                ToolError::InvalidArguments("grep".to_string(), "path must be a string".into())
            })?;

            let path_obj = Path::new(path);

            if path_obj.is_file() {
                let content = tokio::fs::read_to_string(path)
                    .await
                    .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path, e)))?;

                let search_content = if case_sensitive {
                    content.clone()
                } else {
                    content.to_lowercase()
                };

                for (line_num, line) in search_content.lines().enumerate() {
                    if line.contains(&pattern_lower) {
                        let original_line = content.lines().nth(line_num).unwrap_or("");
                        results.push(format!("{}:{}: {}", path, line_num + 1, original_line));
                    }
                }
            } else if path_obj.is_dir() {
                grep_directory_traverse(path, &pattern_lower, case_sensitive, &mut results)
                    .await?;
            }
        }

        if results.is_empty() {
            Ok(format!("No matches found for '{}'", pattern))
        } else {
            Ok(format!("Found {} match(es):\n{}", results.len(), results.join("\n")))
        }
    }
}

impl Default for GrepTool {
    fn default() -> Self {
        Self::new()
    }
}

// Helper function for grep directory traversal
async fn grep_directory_traverse(
    dir: &str,
    pattern: &str,
    case_sensitive: bool,
    results: &mut Vec<String>,
) -> Result<(), ToolError> {
    let mut dir_entries = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| ToolError::IoError(format!("Failed to read directory {}: {}", dir, e)))?;

    while let Some(entry) = dir_entries.next_entry().await.map_err(|e| ToolError::IoError(e.to_string()))? {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files and common ignore directories
        if name.starts_with('.') || name == "node_modules" || name == "target" || name == ".git" {
            continue;
        }

        let file_type = entry.file_type().await.map_err(|e| ToolError::IoError(e.to_string()))?;

        if file_type.is_dir() {
            Box::pin(grep_directory_traverse(
                &path.to_string_lossy(),
                pattern,
                case_sensitive,
                results,
            ))
            .await?;
        } else if file_type.is_file() {
            // Only search text files (by extension heuristic)
            let is_text = name.ends_with(".rs")
                || name.ends_with(".ts")
                || name.ends_with(".tsx")
                || name.ends_with(".js")
                || name.ends_with(".jsx")
                || name.ends_with(".py")
                || name.ends_with(".md")
                || name.ends_with(".json")
                || name.ends_with(".txt")
                || name.ends_with(".css")
                || name.ends_with(".html")
                || name.ends_with(".yaml")
                || name.ends_with(".yml")
                || name.ends_with(".toml");

            if is_text {
                let content = tokio::fs::read_to_string(&path)
                    .await
                    .map_err(|e| ToolError::IoError(format!("Failed to read {}: {}", path.display(), e)))?;

                let search_content = if case_sensitive {
                    content.clone()
                } else {
                    content.to_lowercase()
                };

                let path_str = path.to_string_lossy().to_string();
                for (line_num, line) in search_content.lines().enumerate() {
                    if line.contains(pattern) {
                        let original_line = content.lines().nth(line_num).unwrap_or("");
                        results.push(format!("{}:{}: {}", path_str, line_num + 1, original_line));
                    }
                }
            }
        }
    }

    Ok(())
}

// ============================================================================
// Tool Registry
// ============================================================================

/// Central registry for all available tools
pub struct ToolRegistry {
    definitions: HashMap<String, ToolDefinition>,
    executors: HashMap<String, ToolExecutor>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            definitions: HashMap::new(),
            executors: HashMap::new(),
        };
        registry.register_builtin_tools();
        registry
    }

    pub fn new_read_only() -> Self {
        let mut registry = Self {
            definitions: HashMap::new(),
            executors: HashMap::new(),
        };
        let tools: Vec<ToolExecutor> = vec![
            ToolExecutor::ReadFile(ReadFileTool::new()),
            ToolExecutor::ListDir(ListDirTool::new()),
            ToolExecutor::Glob(GlobTool::new()),
            ToolExecutor::Grep(GrepTool::new()),
        ];

        for tool in tools {
            let name = tool.name().to_string();
            let def = tool.definition();
            registry.definitions.insert(name.clone(), def);
            registry.executors.insert(name, tool);
        }
        registry
    }

    fn register_builtin_tools(&mut self) {
        let tools: Vec<ToolExecutor> = vec![
            ToolExecutor::ReadFile(ReadFileTool::new()),
            ToolExecutor::WriteFile(WriteFileTool::new()),
            ToolExecutor::EditFile(EditFileTool::new()),
            ToolExecutor::ListDir(ListDirTool::new()),
            ToolExecutor::Glob(GlobTool::new()),
            ToolExecutor::Grep(GrepTool::new()),
        ];

        for tool in tools {
            let name = tool.name().to_string();
            let def = tool.definition();
            self.definitions.insert(name.clone(), def);
            self.executors.insert(name, tool);
        }
    }

    pub fn register(&mut self, executor: ToolExecutor) {
        let name = executor.name().to_string();
        let def = executor.definition();
        self.definitions.insert(name.clone(), def);
        self.executors.insert(name, executor);
    }

    pub fn get_definition(&self, name: &str) -> Option<&ToolDefinition> {
        self.definitions.get(name)
    }

    pub fn get_all_definitions(&self) -> Vec<ToolDefinition> {
        self.definitions.values().cloned().collect()
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.executors.contains_key(name)
    }

    pub async fn execute(&self, tool_call: &ToolCall) -> ToolResult {
        let executor = match self.executors.get(&tool_call.name) {
            Some(ex) => ex,
            None => {
                return ToolResult::error(
                    &tool_call.id,
                    format!("Tool '{}' not found. Available tools: {:?}", tool_call.name, self.definitions.keys().collect::<Vec<_>>()),
                );
            }
        };

        // Check if this is a file modification tool
        let is_file_modification = matches!(
            tool_call.name.as_str(),
            "write_file" | "edit_file"
        );

        // For file modification tools, we need to capture original content
        let original_content: Option<String> = if is_file_modification {
            if let Some(path) = tool_call.arguments.get("path").and_then(|v| v.as_str()) {
                tokio::fs::read_to_string(path).await.ok()
            } else {
                None
            }
        } else {
            None
        };

        match executor.execute(tool_call.arguments.clone()).await {
            Ok(output) => {
                if is_file_modification {
                    let file_path = tool_call.arguments.get("path").and_then(|v| v.as_str());
                    let new_content = if let Some(path) = file_path {
                        tokio::fs::read_to_string(path).await.ok()
                    } else {
                        None
                    };

                    ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        output,
                        is_error: false,
                        original_content,
                        new_content,
                        file_path: file_path.map(String::from),
                    }
                } else {
                    ToolResult::success(&tool_call.id, output)
                }
            }
            Err(e) => ToolResult::error(&tool_call.id, e.to_string()),
        }
    }

    pub async fn execute_many(&self, tool_calls: &[ToolCall]) -> Vec<ToolResult> {
        let mut results = Vec::new();
        for tool_call in tool_calls {
            results.push(self.execute(tool_call).await);
        }
        results
    }
}

// Thread-safe wrapper for sharing registry across async tasks
pub type SharedToolRegistry = Arc<RwLock<ToolRegistry>>;

/// Create a full-featured tool registry (for agent mode)
pub fn create_tool_registry() -> SharedToolRegistry {
    Arc::new(RwLock::new(ToolRegistry::new()))
}

/// Create a read-only tool registry (for ask/plan mode)
pub fn create_read_only_tool_registry() -> SharedToolRegistry {
    Arc::new(RwLock::new(ToolRegistry::new_read_only()))
}
