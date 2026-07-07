//! `ask_user` tool — pause the agent loop and ask the user a question.
//!
//! The suspension / wake-up flow:
//!
//!   1. `agent_loop::try_handle_meta_tool` sees "ask_user", creates a
//!      `tokio::sync::oneshot` channel, stores the `Sender` in
//!      `ASK_USER_PENDING` keyed by `tool_call_id`, emits an `ask_user`
//!      stream event, then awaits the `Receiver`.
//!   2. The frontend renders an `AskUserCard`.  When the user picks an
//!      option (or types a free-form answer), the JS side calls the
//!      `answer_ask_user` Tauri command.
//!   3. `commands_agent::answer_ask_user` calls `deliver_answer`, which
//!      removes the sender from the map and sends the answer string.
//!   4. The await in step 1 resolves and execution continues.

use crate::agent::tools::{ToolDefinition, ToolError, ToolOpResult, ToolParameters};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::oneshot;

// ── Global pending-answer registry ───────────────────────────────────────────

use std::sync::LazyLock;

pub static ASK_USER_PENDING: LazyLock<Mutex<HashMap<String, oneshot::Sender<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Store a pending oneshot `Sender` keyed by `tool_call_id` and return
/// the matching `Receiver`.  The agent loop awaits the receiver while the
/// frontend holds the initiative.
pub fn register_pending(tool_call_id: &str) -> oneshot::Receiver<String> {
    let (tx, rx) = oneshot::channel();
    ASK_USER_PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(tool_call_id.to_string(), tx);
    rx
}

/// Send the user's answer to a suspended `ask_user` call.
///
/// Returns `Ok(())` on success or `Err(msg)` when no matching pending call
/// is found (e.g. the agent was cancelled in the meantime).
pub fn deliver_answer(tool_call_id: &str, answer: String) -> Result<(), String> {
    let sender = ASK_USER_PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(tool_call_id);

    match sender {
        Some(tx) => {
            let _ = tx.send(answer);
            Ok(())
        }
        None => Err(format!(
            "No pending ask_user for tool_call_id '{}'",
            tool_call_id
        )),
    }
}

/// Remove a pending entry without delivering (used on cancellation).
pub fn cancel_pending(tool_call_id: &str) {
    ASK_USER_PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(tool_call_id);
}

// ── Tool stub ─────────────────────────────────────────────────────────────────

/// Registry stub — the real execution is intercepted by
/// `agent_loop::try_handle_meta_tool`.
pub struct AskUserTool;

impl AskUserTool {
    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "ask_user",
            "向用户提问",
            "Pause execution and ask the user a question with a list of suggested options. \
             The user may select one of the options, page through more options via the \
             '换一批' button, or type a custom free-form answer. \
             Use this tool when you need an explicit choice from the user before proceeding. \
             Provide 2–20 short, distinct string options. \
             The tool blocks until the user submits an answer.",
            ToolParameters::new(
                vec!["question", "options"],
                vec![
                    (
                        "question",
                        "string",
                        Some("The question to ask. Be concise and specific."),
                    ),
                    (
                        "options",
                        "array",
                        Some(
                            "Array of short string options (2–20 items). \
                             Each element is a plain string that the user can click to select.",
                        ),
                    ),
                    (
                        "allow_custom",
                        "boolean",
                        Some(
                            "When true (default) the user can also type a free-form answer. \
                             Set to false if only the listed options are acceptable.",
                        ),
                    ),
                ],
            ),
        )
    }

    pub async fn execute(&self, _args: Value, _workspace: Option<String>) -> ToolOpResult<String> {
        // The real work is done in `agent_loop::try_handle_meta_tool`.
        Err(ToolError::ExecutionError(
            "ask_user is handled by the agent loop, not the registry".to_string(),
        ))
    }
}
