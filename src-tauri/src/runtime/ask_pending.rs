//! Pending ask-user registry.
//!
//! When the agent loop encounters an `ask_user` tool call it serialises
//! the *whole* session plus the question schema into a process-global map
//! keyed by `session_id`, emits a `tool_paused` stream event, and returns
//! from `AgentExecutor::run` early with `AgentError::PausedForUser`.
//!
//! The frontend renders the question card and replies via the new
//! `ai_agent_resume` Tauri command, which `take`s the entry, injects a
//! synthetic `Message::Tool` carrying the answers back into the
//! conversation, and resumes the loop from where it left off.
//!
//! Why a global registry instead of an `mpsc` channel keyed by session:
//!   - Resume is request/response — the command fires when the user
//!     clicks Submit, not before. An async channel would need to be
//!     plumbed through the entire agent loop.
//!   - The entry holds the owned `AgentSession` for the duration of the
//!     pause; the original `ai_agent_stream` future has already
//!     returned, so the registry is the only place that session can
//!     live.
//!   - Concurrent resumes for the same session are impossible in
//!     practice (the user can only click one button at a time) but the
//!     `Mutex` keeps that an explicit error if it ever happens.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::agent::AgentSession;
use crate::ai::AIConfig;

/// One option the model offered the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserOption {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One question in an `ask_user` invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskUserQuestion {
    pub question: String,
    pub options: Vec<AskUserOption>,
    #[serde(default)]
    pub multiSelect: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
}

/// What we hold while waiting for the user. The owned `AgentSession`
/// is what makes resume possible — the entire conversation history
/// rides along inside the entry.
pub struct PendingAsk {
    pub session_id: String,
    pub message_id: String,
    pub tool_call_id: String,
    pub request_id: String,
    pub questions: Vec<AskUserQuestion>,
    /// Owned session snapshot taken from the agent loop right before it
    /// returned `AgentError::PausedForUser`. The resume command takes
    /// this back by value and calls `executor.run` on it directly.
    pub session: AgentSession,
    /// Cached AIConfig so the resume command can rebuild an
    /// `AgentExecutor` without needing the original `AIConfigInput`
    /// from the frontend.
    pub ai_config: AIConfig,
}

/// Process-global table of paused agent sessions. Keyed by `session_id`
/// — only one pause can be active per session at a time, which is the
/// only sensible shape (the model produces one `ask_user` call per
/// iteration).
static PENDING_ASK: Lazy<Mutex<HashMap<String, PendingAsk>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Park `entry` under `entry.session_id`. Replaces any prior pending
/// ask for the same session — the previous one is dropped (along with
/// its `AgentSession`); this matches the behaviour the user expects if
/// they somehow trigger two pauses back-to-back.
pub fn put(entry: PendingAsk) {
    PENDING_ASK.lock().insert(entry.session_id.clone(), entry);
}

/// Take the pending ask for `session_id`, removing it from the table.
/// Returns `None` if no pause is currently active.
pub fn take(session_id: &str) -> Option<PendingAsk> {
    PENDING_ASK.lock().remove(session_id)
}

/// Look up the pending ask without removing it. Used by debug paths
/// and by the cancel-button handler.
pub fn peek(session_id: &str) -> Option<String> {
    PENDING_ASK.lock().get(session_id).map(|e| e.request_id.clone())
}
