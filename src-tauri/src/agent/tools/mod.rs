//! Tool definitions and registry
//!
//! Provides:
//! - ToolDefinition: JSON Schema for tool parameters
//! - ToolResult: Execution result wrapper
//! - ToolRegistry: Central tool registration and execution

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
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
    #[error("Path validation failed: {0}")]
    PathValidationError(String),
}

pub type ToolOpResult<T> = Result<T, ToolError>;

/// Validates that a path is within the workspace boundary (security check).
/// This does NOT check if the path exists - use validate_path_exists for that.
///
/// Uses canonicalization to resolve relative paths and symlinks,
/// ensuring the final resolved path is within the workspace.
pub fn validate_workspace_path(path: &str, workspace: &Option<String>) -> Result<(), ToolError> {
    let Some(workspace_root) = workspace else {
        return Ok(());
    };

    let canonical_workspace = match std::fs::canonicalize(workspace_root) {
        Ok(p) => p,
        Err(_) => {
            return Err(ToolError::PathValidationError(
                format!("Workspace path does not exist: {}", workspace_root)
            ));
        }
    };

    // For security, we need to resolve the actual path to catch symlinks
    // But we allow the path to not exist yet (for write operations)
    let canonical_requested = match std::fs::canonicalize(Path::new(path)) {
        Ok(p) => p,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Path doesn't exist yet - this is OK for write operations
            // But we still need to validate that the PARENT directory is within workspace
            if let Some(parent) = Path::new(path).parent() {
                match std::fs::canonicalize(parent) {
                    Ok(p) => p,
                    Err(_) => {
                        return Err(ToolError::PathValidationError(
                            format!("Parent directory does not exist or is inaccessible: {}", parent.display())
                        ));
                    }
                }
            } else {
                return Err(ToolError::PathValidationError(
                    format!("Cannot determine parent directory for path: {}", path)
                ));
            }
        }
        Err(e) => {
            return Err(ToolError::PathValidationError(
                format!("Path is inaccessible: {} ({})", path, e)
            ));
        }
    };

    if !canonical_requested.starts_with(&canonical_workspace) {
        return Err(ToolError::PathValidationError(
            format!(
                "Path '{}' is outside the workspace directory '{}'. Access is denied for security reasons.",
                path, workspace_root
            )
        ));
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParameter {
    #[serde(rename = "type")]
    pub param_type: String,
    pub description: Option<String>,
    #[serde(default)]
    pub default: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParameters {
    #[serde(rename = "type")]
    pub params_type: String,
    pub properties: BTreeMap<String, ToolParameter>,
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(default, rename = "additionalProperties")]
    pub additional_properties: bool,
}

impl ToolParameters {
    pub fn new(required: Vec<&str>, properties: Vec<(&str, &str, Option<&str>)>) -> Self {
        let mut props = BTreeMap::new();
        for (name, param_type, description) in properties {
            let trimmed_name = name.trim();
            assert!(
                trimmed_name == name,
                "Tool parameter names must not contain leading or trailing whitespace: {:?}",
                name
            );
            let previous = props.insert(
                name.to_string(),
                ToolParameter {
                    param_type: param_type.to_string(),
                    description: description.map(String::from),
                    default: None,
                },
            );
            assert!(previous.is_none(), "Duplicate tool parameter name: {}", name);
        }

        let required: Vec<String> = required.iter().map(|s| s.to_string()).collect();
        for name in &required {
            let trimmed_name = name.trim();
            assert!(
                trimmed_name == name,
                "Required tool parameter names must not contain leading or trailing whitespace: {:?}",
                name
            );
            assert!(
                props.contains_key(name),
                "Required tool parameter '{}' is missing from properties",
                name
            );
        }

        Self {
            params_type: "object".to_string(),
            properties: props,
            required,
            additional_properties: false,
        }
    }
}

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
                label_zh: None,
            },
        }
    }

    pub fn new_with_label(name: &str, label_zh: &str, description: &str, parameters: ToolParameters) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: ToolFunction {
                name: name.to_string(),
                description: description.to_string(),
                parameters,
                label_zh: Some(label_zh.to_string()),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: ToolParameters,
    /// Chinese label shown in the frontend UI. Optional.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_zh: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub output: String,
    pub is_error: bool,
    pub original_content: Option<String>,
    pub new_content: Option<String>,
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

// Re-export tool structs and their enum variants for the unified ToolExecutor
mod file_tools;
mod search_tools;
mod office_tools;
mod database_tools;

pub use file_tools::{ReadFileTool, WriteFileTool, EditFileTool, CreateDirTool, MoveFileTool};
pub use search_tools::{ListDirTool, GlobTool, GrepTool};
pub use office_tools::{ReadOfficeFileTool, CreateWordDocTool, CompareWordDocsTool, GetDocxInfoTool, ModifyExcelTool, GetExcelInfoTool, CreateExcelTool};
pub use database_tools::DatabaseSearchTool;

/// Unified executor enum combining all tool implementations
pub enum ToolExecutor {
    ReadFile(file_tools::ReadFileTool),
    WriteFile(file_tools::WriteFileTool),
    EditFile(file_tools::EditFileTool),
    CreateDir(file_tools::CreateDirTool),
    MoveFile(file_tools::MoveFileTool),
    ListDir(search_tools::ListDirTool),
    Glob(search_tools::GlobTool),
    Grep(search_tools::GrepTool),
    ReadOfficeFile(office_tools::ReadOfficeFileTool),
    CreateWordDoc(office_tools::CreateWordDocTool),
    CompareWordDocs(office_tools::CompareWordDocsTool),
    GetDocxInfo(office_tools::GetDocxInfoTool),
    ModifyExcel(office_tools::ModifyExcelTool),
    GetExcelInfo(office_tools::GetExcelInfoTool),
    CreateExcel(office_tools::CreateExcelTool),
    DatabaseSearch(database_tools::DatabaseSearchTool),
}

impl ToolExecutor {
    pub fn name(&self) -> &'static str {
        match self {
            ToolExecutor::ReadFile(_) => "read_file",
            ToolExecutor::WriteFile(_) => "write_file",
            ToolExecutor::EditFile(_) => "edit_file",
            ToolExecutor::CreateDir(_) => "create_dir",
            ToolExecutor::MoveFile(_) => "move_file",
            ToolExecutor::ListDir(_) => "list_dir",
            ToolExecutor::Glob(_) => "glob",
            ToolExecutor::Grep(_) => "grep",
            ToolExecutor::ReadOfficeFile(_) => "read_office_file",
            ToolExecutor::CreateWordDoc(_) => "create_word_doc",
            ToolExecutor::CompareWordDocs(_) => "compare_word_docs",
            ToolExecutor::GetDocxInfo(_) => "get_docx_info",
            ToolExecutor::ModifyExcel(_) => "modify_excel",
            ToolExecutor::GetExcelInfo(_) => "get_excel_info",
            ToolExecutor::CreateExcel(_) => "create_excel",
            ToolExecutor::DatabaseSearch(_) => "database_search",
        }
    }

    pub fn definition(&self) -> ToolDefinition {
        match self {
            ToolExecutor::ReadFile(t) => t.definition(),
            ToolExecutor::WriteFile(t) => t.definition(),
            ToolExecutor::EditFile(t) => t.definition(),
            ToolExecutor::CreateDir(t) => t.definition(),
            ToolExecutor::MoveFile(t) => t.definition(),
            ToolExecutor::ListDir(t) => t.definition(),
            ToolExecutor::Glob(t) => t.definition(),
            ToolExecutor::Grep(t) => t.definition(),
            ToolExecutor::ReadOfficeFile(t) => t.definition(),
            ToolExecutor::CreateWordDoc(t) => t.definition(),
            ToolExecutor::CompareWordDocs(t) => t.definition(),
            ToolExecutor::GetDocxInfo(t) => t.definition(),
            ToolExecutor::ModifyExcel(t) => t.definition(),
            ToolExecutor::GetExcelInfo(t) => t.definition(),
            ToolExecutor::CreateExcel(t) => t.definition(),
            ToolExecutor::DatabaseSearch(t) => t.definition(),
        }
    }

    pub async fn execute(&self, arguments: Value, workspace: Option<String>) -> Result<String, ToolError> {
        match self {
            ToolExecutor::ReadFile(t) => t.execute(arguments, workspace).await,
            ToolExecutor::WriteFile(t) => t.execute(arguments, workspace).await,
            ToolExecutor::EditFile(t) => t.execute(arguments, workspace).await,
            ToolExecutor::CreateDir(t) => t.execute(arguments, workspace).await,
            ToolExecutor::MoveFile(t) => t.execute(arguments, workspace).await,
            ToolExecutor::ListDir(t) => t.execute(arguments, workspace).await,
            ToolExecutor::Glob(t) => t.execute(arguments, workspace).await,
            ToolExecutor::Grep(t) => t.execute(arguments, workspace).await,
            ToolExecutor::ReadOfficeFile(t) => t.execute(arguments, workspace).await,
            ToolExecutor::CreateWordDoc(t) => t.execute(arguments, workspace).await,
            ToolExecutor::CompareWordDocs(t) => t.execute(arguments, workspace).await,
            ToolExecutor::GetDocxInfo(t) => t.execute(arguments, workspace).await,
            ToolExecutor::ModifyExcel(t) => t.execute(arguments, workspace).await,
            ToolExecutor::GetExcelInfo(t) => t.execute(arguments, workspace).await,
            ToolExecutor::CreateExcel(t) => t.execute(arguments, workspace).await,
            ToolExecutor::DatabaseSearch(t) => t.execute(arguments, workspace).await,
        }
    }
}

pub struct ToolRegistry {
    definitions: HashMap<String, ToolDefinition>,
    executors: HashMap<String, ToolExecutor>,
    workspace: Option<String>,
    /// AppHandle needed by tools like database_search; set once at call site.
    app_handle: Option<tauri::AppHandle>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            definitions: HashMap::new(),
            executors: HashMap::new(),
            workspace: None,
            app_handle: None,
        };
        registry.register_builtin_tools();
        registry
    }

    pub fn new_read_only() -> Self {
        let mut registry = Self {
            definitions: HashMap::new(),
            executors: HashMap::new(),
            workspace: None,
            app_handle: None,
        };
        let tools: Vec<ToolExecutor> = vec![
            ToolExecutor::ReadFile(ReadFileTool),
            ToolExecutor::ListDir(ListDirTool),
            ToolExecutor::Glob(GlobTool),
            ToolExecutor::Grep(GrepTool),
            ToolExecutor::ReadOfficeFile(ReadOfficeFileTool),
        ];

        for tool in tools {
            let name = tool.name().to_string();
            let def = tool.definition();
            registry.definitions.insert(name.clone(), def);
            registry.executors.insert(name, tool);
        }
        registry
    }

    pub fn set_workspace(&mut self, workspace: Option<String>) {
        self.workspace = workspace;
    }

    pub fn get_workspace(&self) -> Option<&String> {
        self.workspace.as_ref()
    }

    pub fn set_app_handle(&mut self, app: tauri::AppHandle) {
        self.app_handle = Some(app.clone());
        // Lazily add database_search now that we have the AppHandle
        if !self.has_tool("database_search") {
            let tool = ToolExecutor::DatabaseSearch(DatabaseSearchTool::new(app));
            let name = tool.name().to_string();
            let def = tool.definition();
            self.definitions.insert(name.clone(), def);
            self.executors.insert(name, tool);
        }
    }

    fn register_builtin_tools(&mut self) {
        // Note: AppHandle must be provided at call site via ToolRegistry::with_app_handle()
        let tools: Vec<ToolExecutor> = vec![
            ToolExecutor::ReadFile(ReadFileTool),
            ToolExecutor::WriteFile(WriteFileTool),
            ToolExecutor::EditFile(EditFileTool),
            ToolExecutor::CreateDir(CreateDirTool),
            ToolExecutor::MoveFile(MoveFileTool),
            ToolExecutor::ListDir(ListDirTool),
            ToolExecutor::Glob(GlobTool),
            ToolExecutor::Grep(GrepTool),
            ToolExecutor::ReadOfficeFile(ReadOfficeFileTool),
            ToolExecutor::CreateWordDoc(CreateWordDocTool),
            ToolExecutor::CompareWordDocs(CompareWordDocsTool),
            ToolExecutor::GetDocxInfo(GetDocxInfoTool),
            ToolExecutor::ModifyExcel(ModifyExcelTool),
            ToolExecutor::GetExcelInfo(GetExcelInfoTool),
            ToolExecutor::CreateExcel(CreateExcelTool),
            // DatabaseSearchTool added lazily via with_app_handle()
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

        let workspace = self.workspace.clone();

        let is_file_modification = matches!(
            tool_call.name.as_str(),
            "write_file" | "edit_file" | "create_word_doc" | "create_dir" | "modify_excel" | "create_excel"
        );

        let file_path = is_file_modification
            .then(|| tool_call.arguments.get("path").and_then(|v| v.as_str()))
            .flatten();

        // Only read original content if we need it and it's not already provided
        let original_content: Option<String> = if is_file_modification {
            if let Some(path) = file_path {
                if let Err(e) = validate_workspace_path(path, &workspace) {
                    return ToolResult::error(&tool_call.id, e.to_string());
                }
                // Only read if file exists
                if std::path::Path::new(path).exists() {
                    tokio::fs::read_to_string(path).await.ok()
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        match executor.execute(tool_call.arguments.clone(), workspace).await {
            Ok(output) => {
                if is_file_modification {
                    // Don't read file again - reuse original_content for diff
                    // The file has been written, we don't need to read it again
                    ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        output,
                        is_error: false,
                        original_content,
                        new_content: None, // Will be computed by caller if needed
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

pub type SharedToolRegistry = Arc<RwLock<ToolRegistry>>;
