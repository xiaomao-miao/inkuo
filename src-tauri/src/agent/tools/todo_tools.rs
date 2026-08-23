//! `update_todo` meta-tool.
//!
//! Lets the model publish a structured task list (kept in its own context
//! and surfaced to the user via the standard `tool_call_start` /
//! `tool_result` stream events). It does NOT touch the filesystem or any
//! sandboxed state — the registry stub always returns an error, and the
//! real implementation lives in `AgentExecutor`, which updates both the
//! frontend stream and the session-owned prompt state.
//!
//! # Schema (v2 — incremental, not snapshot)
//!
//! The previous version took a full `items: [{id, content, status}]`
//! array that the model was expected to keep in sync. In practice models
//! regularly got it wrong: they would forget to flip `in_progress` to
//! `completed`, or carry `completed` statuses into a fresh list, or
//! forget to set the next step to `in_progress` at all. The panel would
//! then show a stale "currently working on item 1" forever.
//!
//! v2 splits the tool into three explicit, small actions so each call
//! has a single, unambiguous intent that the model can produce at the
//! exact moment it makes sense — without having to reconstruct a whole
//! snapshot or remember which id was `in_progress` last time:
//!
//!   - `set` — replace the list (used at the *start* of a multi-step
//!     task to publish the planned steps). The list is just an array of
//!     one-line strings; the panel numbers them and renders the first
//!     one as `in_progress`, the rest as `pending`.
//!
//!   - `advance` — atomic "I just finished the current step, move on".
//!     Flips the current `in_progress` item to `completed` and the
//!     first remaining `pending` item to `in_progress`. No-op if every
//!     item is already `completed` or the list is empty.
//!
//!   - `complete_current` — flip the current `in_progress` item to
//!     `completed` without promoting the next one. Used when the model
//!     wants to mark progress but not yet commit to the next step
//!     (rare — `advance` is the common path).
//!
//! The state machine is owned by inkuo, not the model. The frontend mirrors
//! it for display, while `AgentSession` injects the same authoritative rows
//! into every following provider request. The model only needs to say "I
//! just finished step 1" or "I'm starting a new plan with these 4 steps".
//!
//! # Backward compatibility
//!
//! v1 calls (full `items: [{id, content, status}]` snapshots) are still
//! accepted. The agent loop coerces the old shape to the new one before
//! publishing to the UI: a v1 snapshot replaces the list as if it were
//! `set`, with one row per item, and statuses are preserved where they
//! make sense (otherwise the first non-`completed` row becomes
//! `in_progress`). See `AgentSession::apply_todo_arguments`.

use crate::agent::tools::{ToolDefinition, ToolError, ToolOpResult, ToolParameters};
use serde_json::Value;

/// One-line task description (the only thing the model needs to write).
/// No `id`, no `status` — the frontend derives both from the row's
/// position in the list and the action the model called.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TodoItem {
    /// Stable id within a single list snapshot. Populated by the frontend
    /// from the row index (`"1"`, `"2"`, …) so React can `key` rows
    /// across re-renders; models writing in v2 format don't have to
    /// supply this.
    #[serde(default)]
    pub id: String,
    /// One-line human description of the task. Shown verbatim in the UI.
    pub content: String,
    /// One of `pending`, `in_progress`, `completed`. Derived by the
    /// frontend from the action that produced the snapshot, never read
    /// from the model's input in v2.
    pub status: String,
}

/// Update todo meta-tool.
pub struct UpdateTodoTool;

impl UpdateTodoTool {
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "update_todo",
            "更新任务列表",
            "Publish or advance the operational task list. The same state is rendered in the panel and injected into every following model request, so it drives execution rather than acting as decorative UI. Each call takes exactly one action:\n\n• action='set' + items=[strings] — publish a new list. First row becomes in_progress (you've started it), the rest are pending. Call this ONCE at the start of a multi-step task. An empty array explicitly clears the plan.\n\n• action='advance' — atomic 'I just finished the current step, move on'. Flips the current in_progress row to completed and the first pending row to in_progress. Call this EXACTLY ONCE per finished step. This is the workhorse call — produce one after every meaningful unit of work, not only at the end.\n\n• action='complete_current' — flip the current in_progress row to completed without promoting the next one. Rarely needed; prefer 'advance'.\n\nDo NOT pass status or id fields — inkuo owns those. Do NOT use one call to set the whole list to completed at the end; advance one step at a time so the user sees live progress.",
            ToolParameters::new(
                vec!["action"],
                vec![
                    (
                        "action",
                        "string",
                        Some("One of 'set' (publish a new list — call once at the start), 'advance' (mark current step done, promote next to in_progress — call after each step), 'complete_current' (mark current done, don't promote — rare)."),
                    ),
                    (
                        "items",
                        "array",
                        Some("Array of one-line strings describing the steps. Required for action='set' (ignored otherwise). Empty array clears the active plan."),
                    ),
                ],
            ),
        )
    }

    pub async fn execute(&self, _args: Value, _workspace: Option<String>) -> ToolOpResult<String> {
        // Intercepted by AgentExecutor before registry dispatch; this
        // stub exists only so the unified registry stays uniform.
        Err(ToolError::ExecutionError(
            "update_todo is handled by the agent loop, not the registry".to_string(),
        ))
    }
}
