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
use thiserror::Error;
use tokio::sync::RwLock;

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

// Re-exported from `crate::security` so that the agent tool impls and
// the future general-purpose commands share a single implementation.
// Adding a wrapper here would make every tool pick up the security fix
// for free; if you need a tool-specific error wrapper, layer it on
// top via the SecurityError enum.
pub use crate::security::SecurityError;

/// Validates that a path is within the workspace boundary (security check).
/// Delegates to `crate::security::validate_workspace_path` for the actual
/// implementation and wraps errors in the tool-specific `ToolError` so
/// downstream code keeps the same signature it had before the security
/// module was extracted.
pub fn validate_workspace_path(path: &str, workspace: &Option<String>) -> Result<(), ToolError> {
    crate::security::validate_workspace_path(path, workspace).map_err(|e| match e {
        SecurityError::PathValidation(msg) => ToolError::PathValidationError(msg),
    })
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
            assert!(
                previous.is_none(),
                "Duplicate tool parameter name: {}",
                name
            );
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

    pub fn new_with_label(
        name: &str,
        label_zh: &str,
        description: &str,
        parameters: ToolParameters,
    ) -> Self {
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

// ── ToolResult ─────────────────────────────────────────────────────────────────

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
pub mod asset_registry;
mod convert_tools; // svg_to_png / md_to_word / word_to_pdf  (document_converter sub-agent)
mod database_tools;
mod file_tools;
pub mod image_gen_tools; // generate_image (AI image generation)
mod media_tools; // read_image / read_pdf  (binary workspace files for multimodal LLMs)
mod mermaid_tools; // render_mermaid  (in-process merman renderer, mermaid.js 11.15 parity)
mod meta_tools; // get_tool_help + delegate_to
mod office;
mod pptx; // create_pptx (packs SVGs into editable .pptx; see office_pptx_expert)
mod pptx_anim; // create_pptx_animation + add_pptx_animation
mod sandbox_tools; // dependency-free, allowlisted diagnostics (feature-gated)
mod search_tools;
mod svg_tools; // create_svg  (AI-authored standalone .svg files)
mod todo_tools; // update_todo (read-only meta-tool; see agent_loop::try_handle_meta_tool)
mod visual_inspection_tools; // Office/PPT render -> bounded multimodal page assets
mod web_search_tool; // web_search (external encyclopedia lookup; today Baike) // binary side-channel: stores asset://<id> entries so LLM context never sees base64

// ── Re-exports ─────────────────────────────────────────────────────────────────

pub use convert_tools::{MdToWordTool, SvgToPngTool, WordToPdfTool};
pub use database_tools::DatabaseSearchTool;
pub use file_tools::{CreateDirTool, EditFileTool, MoveFileTool, ReadFileTool, WriteFileTool};
pub use image_gen_tools::{GenerateImageOutcome, GenerateImageTool};
pub use media_tools::{ReadImageTool, ReadPdfTool};
pub use mermaid_tools::RenderMermaidTool;
pub use meta_tools::{DelegateToTool, GetToolHelpTool};
pub use office::{
    CompareWordDocsTool, CreateExcelTool, CreateWordDocTool, InspectOfficeTool, ModifyExcelTool,
    ReadOfficeFileTool,
};
pub use pptx::{CreatePptxOutcome, CreatePptxTool};
pub use pptx_anim::{AddAnimationTool, CreatePptxAnimationTool};
pub use sandbox_tools::SandboxCommandTool;
pub use search_tools::{GlobTool, GrepTool, ListDirTool};
pub use svg_tools::{CreateSvgOutcome, CreateSvgTool};
pub use todo_tools::{TodoItem, UpdateTodoTool};
pub use visual_inspection_tools::RenderOfficePreviewTool;
pub use web_search_tool::WebSearchTool;

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
    ReadOfficeFile(office::ReadOfficeFileTool),
    CreateWordDoc(office::CreateWordDocTool),
    CompareWordDocs(office::CompareWordDocsTool),
    ModifyExcel(office::ModifyExcelTool),
    CreateExcel(office::CreateExcelTool),
    InspectOffice(office::InspectOfficeTool),
    RenderMermaid(mermaid_tools::RenderMermaidTool),
    CreateSvg(svg_tools::CreateSvgTool),
    CreatePptx(pptx::CreatePptxTool),
    CreatePptxAnimation(pptx_anim::CreatePptxAnimationTool),
    AddPptxAnimation(pptx_anim::AddAnimationTool),
    DatabaseSearch(database_tools::DatabaseSearchTool),
    WebSearch(web_search_tool::WebSearchTool),
    ReadImage(media_tools::ReadImageTool),
    ReadPdf(media_tools::ReadPdfTool),
    GenerateImage(image_gen_tools::GenerateImageTool),
    // Document converter (svg_to_png / md_to_word / word_to_pdf). Lives
    // in `document_converter` sub-agent profile; main agent must
    // delegate_to it to reach any of these tools.
    SvgToPng(convert_tools::SvgToPngTool),
    MdToWord(convert_tools::MdToWordTool),
    WordToPdf(convert_tools::WordToPdfTool),
    SandboxCommand(sandbox_tools::SandboxCommandTool),
    RenderOfficePreview(visual_inspection_tools::RenderOfficePreviewTool),
    // Meta tools (intercepted by the agent loop; execute() returns an error
    // if reached directly).
    GetToolHelp(meta_tools::GetToolHelpTool),
    DelegateTo(meta_tools::DelegateToTool),
    UpdateTodo(todo_tools::UpdateTodoTool),
}

// ── ToolExecutor ────────────────────────────────────────────────────────────────

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
            ToolExecutor::RenderMermaid(_) => "render_mermaid",
            ToolExecutor::CreateSvg(_) => "create_svg",
            ToolExecutor::CreatePptx(_) => "create_pptx",
            ToolExecutor::CreatePptxAnimation(_) => "create_pptx_animation",
            ToolExecutor::AddPptxAnimation(_) => "add_pptx_animation",
            ToolExecutor::DatabaseSearch(_) => "database_search",
            ToolExecutor::WebSearch(_) => "web_search",
            ToolExecutor::ReadImage(_) => "read_image",
            ToolExecutor::ReadPdf(_) => "read_pdf",
            ToolExecutor::GenerateImage(_) => "generate_image",
            ToolExecutor::SvgToPng(_) => "svg_to_png",
            ToolExecutor::MdToWord(_) => "md_to_word",
            ToolExecutor::WordToPdf(_) => "word_to_pdf",
            ToolExecutor::SandboxCommand(_) => "run_sandbox_command",
            ToolExecutor::RenderOfficePreview(_) => "render_office_preview",
            ToolExecutor::GetToolHelp(_) => "get_tool_help",
            ToolExecutor::DelegateTo(_) => "delegate_to",
            ToolExecutor::UpdateTodo(_) => "update_todo",
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
            ToolExecutor::RenderMermaid(t) => t.definition(),
            ToolExecutor::CreateSvg(t) => t.definition(),
            ToolExecutor::CreatePptx(t) => t.definition(),
            ToolExecutor::CreatePptxAnimation(t) => t.definition(),
            ToolExecutor::AddPptxAnimation(t) => t.definition(),
            ToolExecutor::DatabaseSearch(t) => t.definition(),
            ToolExecutor::WebSearch(t) => t.definition(),
            ToolExecutor::ReadImage(t) => t.definition(),
            ToolExecutor::ReadPdf(t) => t.definition(),
            ToolExecutor::GenerateImage(t) => t.definition(),
            ToolExecutor::SvgToPng(t) => t.definition(),
            ToolExecutor::MdToWord(t) => t.definition(),
            ToolExecutor::WordToPdf(t) => t.definition(),
            ToolExecutor::SandboxCommand(t) => t.definition(),
            ToolExecutor::RenderOfficePreview(t) => t.definition(),
            ToolExecutor::GetToolHelp(t) => t.definition(),
            ToolExecutor::DelegateTo(t) => t.definition(),
            ToolExecutor::UpdateTodo(t) => t.definition(),
        }
    }

    pub async fn execute(
        &self,
        arguments: Value,
        workspace: Option<String>,
    ) -> Result<String, ToolError> {
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
            // `render_mermaid` returns a richer outcome (carries the
            // output file path so the registry can stamp `file_path` on
            // the ToolResult and trigger the frontend's `file-written`
            // event). Convert back to a plain String here; the registry
            // re-stitches the file_path below in `ToolRegistry::execute`.
            ToolExecutor::RenderMermaid(t) => {
                let outcome = t.execute(arguments, workspace).await?;
                Ok(outcome.output)
            }
            // `create_svg` returns a richer outcome that carries the
            // output file path (so the registry can stamp `file_path` on
            // the `ToolResult` and trigger the frontend's `file-written`
            // event) and the raw svg_source (so the chat panel can
            // inline-preview the SVG without an extra `read_file` trip).
            // Convert back to a plain String here; the registry re-stitches
            // the file_path below in `ToolRegistry::execute`. The richer
            // `svg_source` is exposed via the `output` JSON the LLM sees
            // — the frontend can re-parse it if it wants the inline
            // preview.
            ToolExecutor::CreateSvg(t) => {
                let outcome = t.execute(arguments, workspace).await?;
                Ok(outcome.output)
            }
            // `create_pptx` returns a richer outcome that carries the output
            // file path (so the registry can stamp `file_path` on the
            // `ToolResult` and trigger the frontend's `file-written` event).
            // Convert back to a plain String here; the registry re-stitches
            // the file_path below in `ToolRegistry::execute`.
            ToolExecutor::CreatePptx(t) => {
                let outcome = t.execute(arguments, workspace).await?;
                Ok(outcome.output)
            }
            ToolExecutor::CreatePptxAnimation(t) => {
                let outcome = t.execute(arguments, workspace).await?;
                Ok(serde_json::to_string(&outcome).unwrap_or(outcome.output))
            }
            ToolExecutor::AddPptxAnimation(t) => t.execute(arguments, workspace).await,
            ToolExecutor::DatabaseSearch(t) => t.execute(arguments, workspace).await,
            ToolExecutor::WebSearch(t) => t.execute(arguments, workspace).await,
            ToolExecutor::ReadImage(t) => t.execute(arguments, workspace).await,
            ToolExecutor::ReadPdf(t) => t.execute(arguments, workspace).await,
            ToolExecutor::GenerateImage(t) => {
                let outcome = t.execute(arguments, workspace).await?;
                Ok(outcome.output)
            }
            ToolExecutor::SvgToPng(t) => {
                let outcome = t.execute(arguments, workspace).await?;
                Ok(outcome.output)
            }
            ToolExecutor::MdToWord(t) => {
                let outcome = t.execute(arguments, workspace).await?;
                Ok(outcome.output)
            }
            ToolExecutor::WordToPdf(t) => {
                let outcome = t.execute(arguments, workspace).await?;
                Ok(outcome.output)
            }
            ToolExecutor::SandboxCommand(t) => t.execute(arguments, workspace).await,
            ToolExecutor::RenderOfficePreview(t) => t.execute(arguments, workspace).await,
            ToolExecutor::GetToolHelp(t) => t.execute(arguments, workspace).await,
            ToolExecutor::DelegateTo(t) => t.execute(arguments, workspace).await,
            ToolExecutor::UpdateTodo(t) => t.execute(arguments, workspace).await,
        }
    }
}

pub struct ToolRegistry {
    definitions: HashMap<String, ToolDefinition>,
    executors: HashMap<String, ToolExecutor>,
    /// AppHandle needed by tools like database_search; set once at call site.
    app_handle: Option<tauri::AppHandle>,
}

// ── ToolRegistry ────────────────────────────────────────────────────────────────

impl ToolRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            definitions: HashMap::new(),
            executors: HashMap::new(),
            app_handle: None,
        };
        registry.register_builtin_tools();
        registry
    }

    pub fn new_read_only() -> Self {
        let mut registry = Self {
            definitions: HashMap::new(),
            executors: HashMap::new(),
            app_handle: None,
        };
        let tools: Vec<ToolExecutor> = vec![
            ToolExecutor::ReadFile(ReadFileTool),
            ToolExecutor::ListDir(ListDirTool),
            ToolExecutor::Glob(GlobTool),
            ToolExecutor::Grep(GrepTool),
            ToolExecutor::ReadOfficeFile(ReadOfficeFileTool),
            // `web_search` is registered because it's a read-only lookup. Whether
            // the LLM actually sees it is decided per-turn by the
            // `web_search` feature toggle via
            // `feature_toggles::effective_tool_set` — when the toggle is
            // off, the tool is filtered out of the allowlist and the
            // model has no way to call it. A placeholder is registered
            // here and swapped for the real implementation when the
            // AppHandle is wired up via `set_app_handle`.
            ToolExecutor::WebSearch(WebSearchTool::placeholder()),
            // `update_todo` is a meta-tool — its registry stub always
            // errors out, while AgentExecutor owns the session-plan update.
            ToolExecutor::UpdateTodo(UpdateTodoTool),
        ];

        for tool in tools {
            let name = tool.name().to_string();
            let def = tool.definition();
            registry.definitions.insert(name.clone(), def);
            registry.executors.insert(name, tool);
        }
        registry
    }

    pub fn set_app_handle(&mut self, app: tauri::AppHandle) {
        self.app_handle = Some(app.clone());
        // Lazily add database_search now that we have the AppHandle
        if !self.has_tool("database_search") {
            let tool = ToolExecutor::DatabaseSearch(DatabaseSearchTool::new(app.clone()));
            let name = tool.name().to_string();
            let def = tool.definition();
            self.definitions.insert(name.clone(), def);
            self.executors.insert(name, tool);
        }
        // Same lazy pattern for web_search: the tool needs the AppHandle
        // to reach the cloud client + the `Settings` cache, both of
        // which are accessed via the running Tauri app. The placeholder
        // registered by `register_builtin_tools` errors out if reached
        // before this hook fires; after this point we replace it with the
        // real tool. (Today every call site sets the AppHandle at startup
        // so this should always overwrite the placeholder — the existing
        // entry is a defence against a future caller forgetting.)
        let replacement = ToolExecutor::WebSearch(WebSearchTool::new(app));
        let name = replacement.name().to_string();
        let def = replacement.definition();
        self.definitions.insert(name.clone(), def);
        self.executors.insert(name, replacement);
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
            ToolExecutor::RenderMermaid(RenderMermaidTool::default()),
            // `create_svg` lets the agent author a self-contained `.svg`
            // file. Output lands in the workspace; the registry emits a
            // `file-change` event so the sidebar tree refreshes and the
            // in-app SVG viewer can auto-open the new file.
            ToolExecutor::CreateSvg(CreateSvgTool::default()),
            // `create_pptx` packs a list of `.svg` files into a single
            // `.pptx` deck where every shape is native OOXML (editable in
            // PowerPoint / Keynote / WPS). Output lands in the workspace
            // and triggers the same `file-change` event as the other
            // file-modifying tools.
            ToolExecutor::CreatePptx(CreatePptxTool::default()),
            ToolExecutor::CreatePptxAnimation(pptx_anim::CreatePptxAnimationTool::new()),
            ToolExecutor::AddPptxAnimation(pptx_anim::AddAnimationTool::new()),
            // DatabaseSearchTool added lazily via with_app_handle()
            // `web_search` is registered here with a placeholder tool;
            // `set_app_handle()` swaps in the real implementation once
            // the Tauri app handle is available. The placeholder never
            // actually executes because every call site seeds the
            // AppHandle before any agent turn.
            ToolExecutor::WebSearch(WebSearchTool::placeholder()),
            ToolExecutor::ReadImage(ReadImageTool),
            ToolExecutor::ReadPdf(ReadPdfTool),
            ToolExecutor::GenerateImage(image_gen_tools::GenerateImageTool::default()),
            // Document-format converters. Lives in the
            // `document_converter` sub-agent profile; registered here
            // alongside the other file-emitting tools so the registry
            // knows how to stamp `file_path` on the `ToolResult` and
            // emit the frontend `file-written` event.
            ToolExecutor::SvgToPng(convert_tools::SvgToPngTool::new()),
            ToolExecutor::MdToWord(convert_tools::MdToWordTool::new()),
            ToolExecutor::WordToPdf(convert_tools::WordToPdfTool::new()),
            // Allowlisted, dependency-free diagnostics. Visibility is gated
            // by the user-controlled `sandbox` feature toggle.
            ToolExecutor::SandboxCommand(SandboxCommandTool),
            // Rendered Word/PPT pages become private workspace-owned assets;
            // the agent loop attaches their pixels on the following model turn.
            ToolExecutor::RenderOfficePreview(RenderOfficePreviewTool),
            // Meta tools (intercepted in agent loop, but still registered so
            // they appear in tool catalogs and can be schema-validated).
            ToolExecutor::GetToolHelp(GetToolHelpTool),
            ToolExecutor::DelegateTo(DelegateToTool),
            ToolExecutor::UpdateTodo(UpdateTodoTool),
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
    /// The AgentSession also enforces this allowlist before dispatch, so a
    /// model-returned hidden tool name cannot bypass feature toggles or a
    /// sub-agent profile.
    pub fn filtered_definitions(&self, allowed: &[String]) -> Vec<ToolDefinition> {
        self.definitions
            .iter()
            .filter(|(name, _)| allowed.iter().any(|a| a == *name))
            .map(|(_, def)| def.clone())
            .collect()
    }

    /// Execute a tool within the workspace owned by the calling agent
    /// session. The registry is process-shared, but workspace authority is
    /// deliberately supplied per invocation so concurrent sessions can
    /// never overwrite one another's filesystem boundary.
    pub async fn execute_in_workspace(
        &self,
        tool_call: &ToolCall,
        workspace: Option<&str>,
    ) -> ToolResult {
        let executor = match self.executors.get(&tool_call.name) {
            Some(ex) => ex,
            None => {
                return ToolResult::error(
                    &tool_call.id,
                    format!(
                        "Tool '{}' not found. Available tools: {:?}",
                        tool_call.name,
                        self.definitions.keys().collect::<Vec<_>>()
                    ),
                );
            }
        };

        let workspace = workspace.filter(|path| !path.trim().is_empty());
        let may_run_without_workspace = matches!(
            tool_call.name.as_str(),
            "web_search" | "get_tool_help" | "delegate_to" | "update_todo"
        );
        if workspace.is_none() && !may_run_without_workspace {
            return ToolResult::error(
                &tool_call.id,
                format!(
                    "Tool '{}' requires a non-empty active workspace. Open or create a workspace before using file, Office, knowledge-base, image, conversion, or sandbox tools.",
                    tool_call.name
                ),
            );
        }

        let workspace = workspace.map(str::to_owned);

        // `render_mermaid` and `create_svg` are both special cases among
        // file-modifying tools: their output path lives in `output_path`
        // (not `path`) and the tool itself constructs an `Outcome`-style
        // struct carrying the path so we can stamp `file_path` on the
        // `ToolResult` for the frontend's `file-written` event. Branch on
        // the tool name first so we don't accidentally apply the generic
        // `path` lookup below to either of them.
        if tool_call.name == "render_mermaid"
            || tool_call.name == "create_svg"
            || tool_call.name == "create_pptx"
            || tool_call.name == "create_pptx_animation"
            || tool_call.name == "add_pptx_animation"
            || tool_call.name == "generate_image"
            || tool_call.name == "svg_to_png"
            || tool_call.name == "md_to_word"
            || tool_call.name == "word_to_pdf"
        {
            let output_path = tool_call
                .arguments
                .get("output_path")
                .and_then(|v| v.as_str())
                .map(String::from);
            return match executor
                .execute(tool_call.arguments.clone(), workspace)
                .await
            {
                Ok(output) => {
                    if let (Some(app), Some(path)) =
                        (self.app_handle.as_ref(), output_path.as_deref())
                    {
                        use crate::file_watcher::{emit_file_change, FileChangeEvent};
                        let existed = std::path::Path::new(path).exists();
                        let event = if existed {
                            FileChangeEvent::Modified {
                                path: path.to_string(),
                            }
                        } else {
                            FileChangeEvent::Created {
                                path: path.to_string(),
                            }
                        };
                        emit_file_change(app, event);
                    }
                    // `create_pptx` writes a valid draft before static QA is
                    // known. Keep the file path/event so the workspace can
                    // display that draft, but surface hard QA failures as a
                    // blocking tool result. This forces the specialist back
                    // into its SVG revision loop instead of treating
                    // `needs_revision` as ordinary success.
                    let is_error = special_file_tool_output_is_error(&tool_call.name, &output);
                    ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        output,
                        is_error,
                        original_content: None,
                        new_content: None,
                        file_path: output_path,
                    }
                }
                Err(e) => ToolResult::error(&tool_call.id, e.to_string()),
            };
        }

        let is_file_modification = matches!(
            tool_call.name.as_str(),
            "write_file"
                | "edit_file"
                | "create_word_doc"
                | "create_dir"
                | "modify_excel"
                | "create_excel"
                | "move_file"
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

        match executor
            .execute(tool_call.arguments.clone(), workspace)
            .await
        {
            Ok(output) => {
                if is_file_modification {
                    // Emit file-change so the file tree refreshes even when the
                    // in-process inotify watcher misses the write.
                    if let (Some(app), Some(path)) = (self.app_handle.as_ref(), file_path) {
                        use crate::file_watcher::{emit_file_change, FileChangeEvent};
                        let existed = std::path::Path::new(path).exists();
                        let event = if existed {
                            FileChangeEvent::Modified {
                                path: path.to_string(),
                            }
                        } else {
                            FileChangeEvent::Created {
                                path: path.to_string(),
                            }
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

    pub async fn execute_many_in_workspace(
        &self,
        tool_calls: &[ToolCall],
        workspace: Option<&str>,
    ) -> Vec<ToolResult> {
        let mut results = Vec::new();
        for tool_call in tool_calls {
            results.push(self.execute_in_workspace(tool_call, workspace).await);
        }
        results
    }
}

pub type SharedToolRegistry = Arc<RwLock<ToolRegistry>>;

fn special_file_tool_output_is_error(tool_name: &str, output: &str) -> bool {
    tool_name == "create_pptx" && pptx::output_requires_revision(output)
}

#[cfg(test)]
mod revision_gate_tests {
    use super::{special_file_tool_output_is_error, ToolCall, ToolRegistry};

    #[test]
    fn create_pptx_needs_revision_becomes_a_tool_error() {
        let output = r#"{"status":"needs_revision","quality":{"passed":false}}"#;
        assert!(special_file_tool_output_is_error("create_pptx", output));
        assert!(!special_file_tool_output_is_error("create_svg", output));
        assert!(!special_file_tool_output_is_error(
            "create_pptx",
            r#"{"status":"ok","quality":{"passed":true}}"#,
        ));
    }

    #[tokio::test]
    async fn registry_preserves_revision_draft_path_but_marks_result_as_error() {
        let directory =
            std::env::temp_dir().join(format!("inkuo-registry-pptx-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let svg_path = directory.join("slide.svg");
        let output_path = directory.join("draft.pptx");
        std::fs::write(
            &svg_path,
            r#"<svg viewBox="0 0 1280 720"><text x="80" y="100" font-size="20">Tiny title</text><text x="80" y="220" font-size="12">Tiny body</text></svg>"#,
        )
        .unwrap();
        let tool_call = ToolCall {
            id: "pptx-gate".to_string(),
            name: "create_pptx".to_string(),
            arguments: serde_json::json!({
                "svg_paths": [svg_path.to_string_lossy()],
                "output_path": output_path.to_string_lossy(),
                "title": "Revision draft",
            }),
        };

        let result = ToolRegistry::new()
            .execute_in_workspace(&tool_call, directory.to_str())
            .await;

        assert!(result.is_error, "needs_revision must be a tool error");
        assert_eq!(result.file_path.as_deref(), output_path.to_str());
        assert!(
            output_path.is_file(),
            "valid draft package must be preserved"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&result.output).unwrap()["status"],
            "needs_revision"
        );
        std::fs::remove_dir_all(directory).ok();
    }
}
