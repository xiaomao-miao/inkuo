//! `create_plan` meta-tool — plan mode only.
//!
//! This tool is registered in the read-only registry so the model can see
//! it, but its actual implementation lives in `agent_loop::try_handle_meta_tool`.
//! The registry stub always returns an error; the agent loop handles the
//! real work: writing the plan to `<workspace>/.inkuo/plans/<id>.md` and
//! emitting a `plan_result` stream event so the frontend can render the
//! PlanCard immediately.
//!
//! Only available in plan mode (ask mode does NOT get this tool).

use crate::agent::tools::{ToolDefinition, ToolError, ToolOpResult, ToolParameters};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanFileTouch {
    pub path: String,
    pub intent: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePlanArgs {
    /// Markdown prose describing the plan in detail.
    pub content: String,
    /// One-sentence summary of the goal and overall strategy.
    pub plan_summary: String,
    /// List of files that will be read, created, modified, deleted, or renamed.
    #[serde(default)]
    pub files_to_touch: Vec<PlanFileTouch>,
    /// Risk level: "low" | "medium" | "high"
    pub risk: String,
    /// Optional note on what makes this risky.
    #[serde(default)]
    pub risk_reason: Option<String>,
}

pub struct CreatePlanTool;

impl CreatePlanTool {
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "create_plan",
            "创建计划",
            "Create a structured plan and persist it to disk. Call this once when you have finished analyzing and have a complete plan ready for the user to review.\n\nThe plan is saved to `<workspace>/.inkuo/plans/<id>.md` so the user can grep or open it later. The frontend renders it as a PlanCard with the files list, risk badge, and Apply/Adjust buttons.\n\nCall this tool ONLY in plan mode when you are confident the plan is complete. Do NOT call it multiple times per planning session — call it once with the full plan.",
            ToolParameters::new(
                vec!["content", "plan_summary", "risk"],
                vec![
                    (
                        "content",
                        "string",
                        Some("Full plan description in Markdown prose. Include analysis, reasoning, step-by-step breakdown, and any caveats. This is what the user sees in the PlanCard's collapsible details section."),
                    ),
                    (
                        "plan_summary",
                        "string",
                        Some("One-sentence goal and strategy. Shown as the plan card's subtitle."),
                    ),
                    (
                        "files_to_touch",
                        "array",
                        Some("Array of file entries. Each entry: {path: string, intent: 'read'|'create'|'modify'|'delete'|'rename', reason: string}. Can be empty for simple requests."),
                    ),
                    (
                        "risk",
                        "string",
                        Some("Risk level: 'low' (reads or additive changes), 'medium' (significant rewrites), 'high' (any delete or rename)."),
                    ),
                    (
                        "risk_reason",
                        "string",
                        Some("Optional brief note explaining the risk assessment."),
                    ),
                ],
            ),
        )
    }

    pub async fn execute(&self, _args: Value, _workspace: Option<String>) -> ToolOpResult<String> {
        // Intercepted by the agent loop (see `try_handle_meta_tool`); this
        // stub exists only so the unified registry stays uniform.
        Err(ToolError::ExecutionError(
            "create_plan is handled by the agent loop, not the registry".to_string(),
        ))
    }
}
