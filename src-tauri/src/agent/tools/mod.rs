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
            // Path doesn't exist yet — this is OK for write operations.
            // Validate that the *resolved* parent directory lives inside the
            // workspace, so a path like `/workspace/../../etc/passwd` (whose
            // canonicalized parent is `/etc`) can't sneak through. Without
            // this check the function would `return Ok(())` here and bypass
            // the sandbox entirely, which is a path-traversal vulnerability.
            let parent = Path::new(path).parent().ok_or_else(|| {
                ToolError::PathValidationError(format!(
                    "Cannot determine parent directory for path: {}",
                    path
                ))
            })?;
            let canonical_parent = std::fs::canonicalize(parent).map_err(|e| {
                ToolError::PathValidationError(format!(
                    "Parent directory does not exist or is inaccessible: {} ({})",
                    parent.display(),
                    e
                ))
            })?;
            if !canonical_parent.starts_with(&canonical_workspace) {
                return Err(ToolError::PathValidationError(format!(
                    "Path '{}' is outside the workspace directory '{}'. \
                    Access is denied for security reasons.",
                    path, workspace_root
                )));
            }
            return Ok(());
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
mod meta_tools; // get_tool_help + delegate_to
mod todo_tools; // update_todo (read-only meta-tool; see agent_loop::try_handle_meta_tool)
mod plan_tools;  // create_plan  (read-only meta-tool; see agent_loop::try_handle_meta_tool)
pub mod ask_user_tools; // ask_user   (meta-tool; see agent_loop::try_handle_meta_tool)

pub use file_tools::{ReadFileTool, WriteFileTool, EditFileTool, CreateDirTool, MoveFileTool};
pub use search_tools::{ListDirTool, GlobTool, GrepTool};
pub use office_tools::{
    ReadOfficeFileTool, CreateWordDocTool, CompareWordDocsTool, ModifyExcelTool,
    CreateExcelTool, InspectOfficeTool,
};
pub use database_tools::DatabaseSearchTool;
pub use meta_tools::{GetToolHelpTool, DelegateToTool};
pub use todo_tools::{UpdateTodoTool, TodoItem};
pub use plan_tools::{CreatePlanTool, CreatePlanArgs, PlanFileTouch};
pub use ask_user_tools::AskUserTool;

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
    ModifyExcel(office_tools::ModifyExcelTool),
    CreateExcel(office_tools::CreateExcelTool),
    InspectOffice(office_tools::InspectOfficeTool),
    DatabaseSearch(database_tools::DatabaseSearchTool),
    // Meta tools (intercepted by the agent loop; execute() returns an error
    // if reached directly).
    GetToolHelp(meta_tools::GetToolHelpTool),
    DelegateTo(meta_tools::DelegateToTool),
    UpdateTodo(todo_tools::UpdateTodoTool),
    CreatePlan(plan_tools::CreatePlanTool),
    AskUser(ask_user_tools::AskUserTool),
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
            ToolExecutor::ModifyExcel(_) => "modify_excel",
            ToolExecutor::CreateExcel(_) => "create_excel",
            ToolExecutor::InspectOffice(_) => "inspect_office",
            ToolExecutor::DatabaseSearch(_) => "database_search",
            ToolExecutor::GetToolHelp(_) => "get_tool_help",
            ToolExecutor::DelegateTo(_) => "delegate_to",
            ToolExecutor::UpdateTodo(_) => "update_todo",
            ToolExecutor::CreatePlan(_) => "create_plan",
            ToolExecutor::AskUser(_) => "ask_user",
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
            ToolExecutor::ModifyExcel(t) => t.definition(),
            ToolExecutor::CreateExcel(t) => t.definition(),
            ToolExecutor::InspectOffice(t) => t.definition(),
            ToolExecutor::DatabaseSearch(t) => t.definition(),
            ToolExecutor::GetToolHelp(t) => t.definition(),
            ToolExecutor::DelegateTo(t) => t.definition(),
            ToolExecutor::UpdateTodo(t) => t.definition(),
            ToolExecutor::CreatePlan(t) => t.definition(),
            ToolExecutor::AskUser(t) => t.definition(),
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
            ToolExecutor::ModifyExcel(t) => t.execute(arguments, workspace).await,
            ToolExecutor::CreateExcel(t) => t.execute(arguments, workspace).await,
            ToolExecutor::InspectOffice(t) => t.execute(arguments, workspace).await,
            ToolExecutor::DatabaseSearch(t) => t.execute(arguments, workspace).await,
            ToolExecutor::GetToolHelp(t) => t.execute(arguments, workspace).await,
            ToolExecutor::DelegateTo(t) => t.execute(arguments, workspace).await,
            ToolExecutor::UpdateTodo(t) => t.execute(arguments, workspace).await,
            ToolExecutor::CreatePlan(t) => t.execute(arguments, workspace).await,
            ToolExecutor::AskUser(t) => t.execute(arguments, workspace).await,
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
            // `update_todo` is a meta-tool — its registry stub always
            // errors out, and the actual implementation lives in
            // `agent_loop::try_handle_meta_tool`. Registering it here
            // makes it visible to the model in Plan / Ask mode so the
            // user can see planning progress, even though Plan mode
            // never actually executes the listed steps.
            ToolExecutor::UpdateTodo(UpdateTodoTool),
            // `create_plan` is a plan-mode-only meta-tool — same pattern
            // as `update_todo`: the registry stub errors out, and
            // `agent_loop::try_handle_meta_tool` does the real work.
            ToolExecutor::CreatePlan(plan_tools::CreatePlanTool),
            // `ask_user` is a meta-tool that suspends the agent loop until
            // the user picks an answer from the UI. Same pattern: registry
            // stub errors, real work in `agent_loop::try_handle_meta_tool`.
            ToolExecutor::AskUser(AskUserTool),
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
            ToolExecutor::ModifyExcel(ModifyExcelTool),
            ToolExecutor::CreateExcel(CreateExcelTool),
            ToolExecutor::InspectOffice(InspectOfficeTool),
            // DatabaseSearchTool added lazily via with_app_handle()
            // Meta tools (intercepted in agent loop, but still registered so
            // they appear in tool catalogs and can be schema-validated).
            ToolExecutor::GetToolHelp(GetToolHelpTool),
            ToolExecutor::DelegateTo(DelegateToTool),
            ToolExecutor::AskUser(ask_user_tools::AskUserTool),
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

    /// Return the names of every registered tool, in insertion order.
    /// Used by `commands_agent::ai_agent_stream` to compute the
    /// `allowed_tools` allowlist when feature toggles (e.g. strict-KB)
    /// are enabled. We don't iterate the registry twice — one pass over
    /// `definitions` keys is enough.
    pub fn tool_names(&self) -> Vec<String> {
        self.definitions.keys().cloned().collect()
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.executors.contains_key(name)
    }

    /// Return a list of tool definitions filtered by `allowed`. The
    /// underlying registry (and `AppHandle`) is shared unchanged — the
    /// filter is a view, not a copy, so sub-agents automatically inherit
    /// any tools added later (e.g. lazy `database_search`).
    ///
    /// Sub-agents that want to call an unfiltered tool name will still
    /// resolve at runtime, but the LLM never *sees* it, so this is safe.
    pub fn filtered_definitions(&self, allowed: &[String]) -> Vec<ToolDefinition> {
        self.definitions
            .iter()
            .filter(|(name, _)| allowed.iter().any(|a| a == *name))
            .map(|(_, def)| def.clone())
            .collect()
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
            "write_file" | "edit_file" | "create_word_doc" | "create_dir"
            | "modify_excel" | "create_excel" | "move_file"
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
                    // Emit file-change so the file tree refreshes even when the
                    // in-process inotify watcher misses the write.
                    if let (Some(app), Some(path)) = (self.app_handle.as_ref(), file_path) {
                        use crate::file_watcher::{emit_file_change, FileChangeEvent};
                        let existed = std::path::Path::new(path).exists();
                        let event = if existed {
                            FileChangeEvent::Modified { path: path.to_string() }
                        } else {
                            FileChangeEvent::Created { path: path.to_string() }
                        };
                        emit_file_change(app, event);
                    }

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
