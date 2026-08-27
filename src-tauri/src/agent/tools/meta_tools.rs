//! Meta-tools that the agent loop uses to manage itself.
//!
//! - `get_tool_help`: load detailed spec for a named business category.
//!   The actual spec lookup lives in `prompts::find_tool_spec`.
//! - `delegate_to`: construct a sub-session (different prompt + tool set +
//!   max_iter cap) and run a task there. Returns a string summary back to
//!   the main agent's tool-call slot. Heavy lifting is in `agent_loop::run`.

use crate::agent::tools::{ToolDefinition, ToolError, ToolOpResult, ToolParameters};
use crate::runtime::ask_pending::{AskUserOption, AskUserQuestion};
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

/// Pauses the agent loop and asks the user to choose between options.
///
/// `ask_user` is a *meta-tool*: the registry stub below always errors
/// out, and the real implementation lives in
/// `agent_loop::try_handle_meta_tool`. The intercept path validates the
/// schema, stashes the session in `runtime::ask_pending`, emits a
/// `tool_paused` stream event, and returns early from the loop with
/// `AgentError::PausedForUser`. The frontend then renders the question
/// card and replies via the `ai_agent_resume` Tauri command.
pub struct AskUserTool;

impl AskUserTool {
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "ask_user",
            "询问用户",
            "Pause the run and ask the user to choose. Use ONLY when you need a concrete decision before you can proceed (e.g. the user explicitly said \"ask me\", or you've hit a fork in the road where picking one path would discard the others). DO NOT use this for clarifications you can resolve from the workspace, or for trivial preferences that have a sensible default.\n\nThe user can skip individual questions or cancel the entire call — handle either gracefully.\n\nSchema:\n- `questions`: array of 1–4 question objects.\n  - `question` (string, required): the question to display.\n  - `header` (string, optional): short chip shown above the question (max ~12 chars).\n  - `options` (array, required): 2–4 `{label, description?}` choices.\n  - `multiSelect` (bool, default false): allow picking more than one.\n\nThe user is shown the options as buttons plus a free-text \"Other\" input, so always pick options that are genuinely distinct choices. The tool result is a JSON object `{\"answers\": [{questionIndex, selectedLabels, customText}], \"cancelled\": bool}`.",
            ToolParameters::new(
                vec!["questions"],
                vec![
                    (
                        "questions",
                        "array",
                        Some("Array of 1–4 question objects. Each question: {question: string, header?: string, options: [{label, description?}], multiSelect?: bool}."),
                    ),
                ],
            ),
        )
    }

    pub async fn execute(&self, _args: Value, _workspace: Option<String>) -> ToolOpResult<String> {
        // Intercepted by the agent loop (see `try_handle_meta_tool`); this
        // stub exists only so the unified registry stays uniform.
        Err(ToolError::ExecutionError(
            "ask_user is handled by the agent loop, not the registry".to_string(),
        ))
    }
}

/// Helper: parse + validate `args["questions"]` into `Vec<AskUserQuestion>`.
///
/// Enforces the schema constraints the LLM is told about in the tool
/// description (1–4 questions, 2–4 options each). If validation fails,
/// returns a precise error string the model can self-correct on. This
/// runs *before* we ever pause, so a malformed call doesn't strand the
/// user on a broken question card.
pub fn parse_ask_user_questions(args: &Value) -> Result<Vec<AskUserQuestion>, String> {
    let questions_value = args
        .get("questions")
        .ok_or_else(|| "Missing required parameter: `questions`".to_string())?;
    let questions_array = questions_value
        .as_array()
        .ok_or_else(|| "`questions` must be an array".to_string())?;

    if questions_array.is_empty() || questions_array.len() > 4 {
        return Err(format!(
            "`questions` must contain 1–4 items, got {}",
            questions_array.len()
        ));
    }

    let mut out = Vec::with_capacity(questions_array.len());
    for (idx, q_value) in questions_array.iter().enumerate() {
        let question_text = q_value
            .get("question")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("questions[{idx}].question must be a non-empty string"))?
            .trim();
        if question_text.is_empty() {
            return Err(format!(
                "questions[{idx}].question must be a non-empty string"
            ));
        }

        let header = q_value
            .get("header")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let multi_select = q_value
            .get("multiSelect")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let options_value = q_value
            .get("options")
            .ok_or_else(|| format!("questions[{idx}].options is required"))?;
        let options_array = options_value
            .as_array()
            .ok_or_else(|| format!("questions[{idx}].options must be an array"))?;

        if options_array.len() < 2 || options_array.len() > 4 {
            return Err(format!(
                "questions[{idx}].options must contain 2–4 items, got {}",
                options_array.len()
            ));
        }

        let mut options = Vec::with_capacity(options_array.len());
        for (opt_idx, opt_value) in options_array.iter().enumerate() {
            let label = opt_value
                .get("label")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    format!("questions[{idx}].options[{opt_idx}].label must be a string")
                })?
                .trim();
            if label.is_empty() {
                return Err(format!(
                    "questions[{idx}].options[{opt_idx}].label must be non-empty"
                ));
            }
            let description = opt_value
                .get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            options.push(AskUserOption {
                label: label.to_string(),
                description,
            });
        }

        out.push(AskUserQuestion {
            question: question_text.to_string(),
            options,
            multiSelect: multi_select,
            header,
        });
    }

    Ok(out)
}
