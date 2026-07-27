use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeSearchResult {
    pub chunk_id: String,
    pub document_id: String,
    pub content: String,
    pub score: f32,
    pub document_title: String,
    pub file_path: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
}

/// Re-exports from diff module for external use
pub use crate::diff::{FileDiffSummary, StreamDiffHunk, StreamDiffChange};

/// Stream payload with tool call support
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StreamPayload {
    /// Session identifier
    pub session_id: String,
    /// Message identifier
    pub message_id: String,
    /// Event type: "text", "reasoning", "error", "tool_call_start", "tool_result", "done"
    pub event_type: String,

    /// Text content delta (for text events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Summary of changes (for summary events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// Tool call identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    /// Tool name (for tool_call_start events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,

    /// Tool arguments as JSON string (for tool_call_start events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_args: Option<String>,

    /// Final content (for done events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_content: Option<String>,

    /// Error message (for error events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Knowledge search results for knowledge mode answers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_results: Option<Vec<KnowledgeSearchResult>>,

    /// Whether this is the final event
    pub done: bool,

    /// Streamed file modification payload fields
    /// File path that was modified
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,

    /// Original content before modification (for diff calculation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_content: Option<String>,

    /// New content after modification (for diff calculation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_content: Option<String>,

    /// Diff summary (file name, line counts, hunks) for UI display
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_summary: Option<FileDiffSummary>,

    /// Office file that was modified (path -> format: "xlsx" or "docx")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub office_file_modified: Option<OfficeFileModified>,
}

/// Metadata about an Office file that was modified
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfficeFileModified {
    pub path: String,
    pub format: String,
}

impl StreamPayload {
    fn with_event(mut self, event_type: &str) -> Self {
        self.event_type = event_type.to_string();
        self
    }

    fn with_ids(mut self, session_id: &str, message_id: &str) -> Self {
        self.session_id = session_id.to_string();
        self.message_id = message_id.to_string();
        self
    }

    /// A streaming text delta. Caller is responsible for emitting the final
    /// `final_text` / `done` event when the stream completes.
    // ── Constructors ─────────────────────────────────────────────────────────────

    pub fn text(session_id: &str, message_id: &str, content: &str) -> Self {
        Self::default()
            .with_ids(session_id, message_id)
            .with_event("text")
            .with(|p| p.content = Some(content.to_string()))
    }

    /// Terminal error event (done=true).
    pub fn error(session_id: &str, message_id: &str, error: &str) -> Self {
        Self::default()
            .with_ids(session_id, message_id)
            .with_event("error")
            .with(|p| {
                p.error = Some(error.to_string());
                p.done = true;
            })
    }

    /// Cancellation event (done=true, error=cancelled). Distinct from a real
    /// error so the frontend can treat it as a non-fatal terminal state.
    pub fn cancelled(session_id: &str, message_id: &str) -> Self {
        Self::default()
            .with_ids(session_id, message_id)
            .with_event("error")
            .with(|p| {
                p.error = Some("cancelled".to_string());
                p.done = true;
            })
    }

    pub fn tool_call_start(
        session_id: &str,
        message_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        args: &str,
    ) -> Self {
        Self::default()
            .with_ids(session_id, message_id)
            .with_event("tool_call_start")
            .with(|p| {
                p.tool_call_id = Some(tool_call_id.to_string());
                p.tool_name = Some(tool_name.to_string());
                p.tool_args = Some(args.to_string());
            })
    }

    pub fn tool_result(
        session_id: &str,
        message_id: &str,
        tool_call_id: &str,
        result: &str,
        is_error: bool,
    ) -> Self {
        Self::default()
            .with_ids(session_id, message_id)
            .with_event("tool_result")
            .with(|p| {
                p.tool_call_id = Some(tool_call_id.to_string());
                if is_error {
                    p.error = Some(result.to_string());
                } else {
                    p.content = Some(result.to_string());
                }
            })
    }

    /// Final event for a plain chat stream.
    pub fn done(session_id: &str, message_id: &str, final_content: Option<&str>) -> Self {
        Self::default()
            .with_ids(session_id, message_id)
            .with_event("done")
            .with(|p| {
                p.final_content = final_content.map(String::from);
                p.done = true;
            })
    }

    /// Final event for an edit stream: summary + final content.
    pub fn summary(
        session_id: &str,
        message_id: &str,
        summary: &str,
        final_content: &str,
    ) -> Self {
        Self::default()
            .with_ids(session_id, message_id)
            .with_event("summary")
            .with(|p| {
                p.summary = Some(summary.to_string());
                p.final_content = Some(final_content.to_string());
                p.done = true;
            })
    }

    /// Final event for a knowledge-mode chat stream. Carries the final answer
    /// plus the search results so the frontend can render the references.
    pub fn final_text_with_results(
        session_id: &str,
        message_id: &str,
        final_content: &str,
        search_results: Vec<KnowledgeSearchResult>,
    ) -> Self {
        Self::default()
            .with_ids(session_id, message_id)
            .with_event("text")
            .with(|p| {
                p.final_content = Some(final_content.to_string());
                p.search_results = Some(search_results);
                p.done = true;
            })
    }

    /// Sub-agent start event. Emitted before the sub-agent's first stream event
    /// so the frontend can initialize a collapsible "nested" message block.
    pub fn subagent_start(
        session_id: &str,
        parent_message_id: &str,
        sub_message_id: &str,
        expert: &str,
        label: &str,
        task: &str,
    ) -> Self {
        Self::default()
            .with_ids(session_id, parent_message_id)
            .with_event("subagent_start")
            .with(|p| {
                p.content = Some(task.to_string());
                p.final_content = Some(sub_message_id.to_string());
                p.summary = Some(expert.to_string());
                p.tool_args = Some(label.to_string());
                p.done = false;
            })
    }

    /// Sub-agent end event. Emitted when a sub-agent completes so the frontend
    /// can finalize the nested message block.
    pub fn subagent_end(
        session_id: &str,
        parent_message_id: &str,
        sub_message_id: &str,
    ) -> Self {
        Self::default()
            .with_ids(session_id, parent_message_id)
            .with_event("subagent_end")
            .with(|p| {
                p.content = Some(sub_message_id.to_string());
                p.done = false;
            })
    }

    fn with(mut self, f: impl FnOnce(&mut Self)) -> Self {
        f(&mut self);
        self
    }
}

/// Emit a stream payload to the frontend. Logs a warning on failure.
pub fn emit(app: &AppHandle, payload: StreamPayload) {
    if let Err(error) = app.emit("ai://stream", payload) {
        tracing::warn!("Failed to emit ai://stream event: {}", error);
    }
}
