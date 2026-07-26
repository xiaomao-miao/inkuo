//! Meta-tools that the agent loop uses to manage itself.
//!
//! - `get_tool_help`: load detailed spec for a named business category.
//!   The actual spec lookup lives in `prompts::find_tool_spec`.
//! - `delegate_to`: construct a sub-session (different prompt + tool set +
//!   max_iter cap) and run a task there. Returns a string summary back to
//!   the main agent's tool-call slot. Heavy lifting is in `agent_loop::run`.

use crate::agent::tools::{ToolDefinition, ToolError, ToolOpResult, ToolParameters};
use serde_json::Value;

/// Loads a category-scoped tool spec into the LLM's context on demand.
pub struct GetToolHelpTool;

impl GetToolHelpTool {
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "get_tool_help",
            "加载工具帮助",
            "Load detailed tool usage instructions for a business category. Use this when you need full parameter / behavior details beyond the one-line summary in the system prompt. Categories: `general` (read/write/edit/grep/glob/database_search), `word` (.docx via office_word_expert), `excel` (.xlsx via office_excel_expert), `pptx` (.pptx via office_pptx_expert), `markdown` (long-form .md writing), `media` (read_image / read_pdf), `svg` (create_svg style guide). The spec is injected into your context as the tool result and is not shown to the user.",
            ToolParameters::new(
                vec!["category"],
                vec![
                    ("category", "string", Some("Business category. One of: general, word, excel, pptx, markdown, media, svg.")),
                ],
            ),
        )
    }

    pub async fn execute(&self, _args: Value, _workspace: Option<String>) -> ToolOpResult<String> {
        // Intercepted by the agent loop (see `try_handle_meta_tool`); this
        // stub exists only so the unified registry stays uniform.
        Err(ToolError::ExecutionError(
            "get_tool_help is handled by the agent loop, not the registry".to_string(),
        ))
    }
}

/// Launches a sub-agent run and returns its summary as the tool result.
pub struct DelegateToTool;

impl DelegateToTool {
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "delegate_to",
            "委派给子代理",
            "Delegate a task to a sub-agent. Sub-agent has its own system prompt and tool set. Returns the sub-agent's final summary. Pass `expert` (profile name), `task` (description), and optional `context` (extra instructions). Available experts: office_word_expert (Word .docx), office_excel_expert (Excel .xlsx), office_pptx_expert (PowerPoint .pptx — packs existing .svg files only, cannot edit in place), md_writer (long Markdown), researcher (read-only search), batch_editor (multi-file same-rule edits), code_expert (code features/bugs/refactor), flowchart_expert (Mermaid → PNG/SVG/PDF), word_image_expert (insert PNG/JPEG/GIF into .docx).",
            ToolParameters::new(
                vec!["expert", "task"],
                vec![
                    (
                        "expert",
                        "string",
                        Some("Profile name. One of: office_word_expert, office_excel_expert, office_pptx_expert, md_writer, researcher, batch_editor, code_expert, flowchart_expert, word_image_expert."),
                    ),
                    (
                        "task",
                        "string",
                        Some("Description of the task for the sub-agent to complete."),
                    ),
                    (
                        "context",
                        "string",
                        Some("Optional extra context for the sub-agent."),
                    ),
                ],
            ),
        )
    }

    pub async fn execute(&self, _args: Value, _workspace: Option<String>) -> ToolOpResult<String> {
        // Intercepted by the agent loop (see `try_handle_meta_tool`); this
        // stub exists only so the unified registry stays uniform.
        Err(ToolError::ExecutionError(
            "delegate_to is handled by the agent loop, not the registry".to_string(),
        ))
    }
}
