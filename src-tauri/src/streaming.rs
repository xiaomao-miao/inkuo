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

/// Stream event types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub enum StreamEventType {
    /// Text content delta
    Text,
    /// Summary of changes
    Summary,
    /// Error occurred
    Error,
    /// Tool call started (also used as first-time arrival: a new tool call appeared in stream)
    ToolCallStart,
    /// Tool call arguments delta (sent repeatedly as the AI streams the JSON argument string)
    ToolCallArgsDelta,
    /// Tool call completed
    ToolCallEnd,
    /// Tool execution result
    ToolResult,
    /// Thinking/processing indicator
    Thinking,
    /// Final completion
    Done,
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
    /// Event type: "text", "error", "tool_call_start", "tool_result", "done"
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
    pub fn text(session_id: &str, message_id: &str, content: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
            event_type: "text".to_string(),
            content: Some(content.to_string()),
            summary: None,
            tool_call_id: None,
            tool_name: None,
            tool_args: None,
            final_content: None,
            error: None,
            search_results: None,
            done: false,
            file_path: None,
            original_content: None,
            new_content: None,
            diff_summary: None,
            office_file_modified: None,
        }
    }

    pub fn error(session_id: &str, message_id: &str, error: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
            event_type: "error".to_string(),
            content: None,
            summary: None,
            tool_call_id: None,
            tool_name: None,
            tool_args: None,
            final_content: None,
            error: Some(error.to_string()),
            search_results: None,
            done: true,
            file_path: None,
            original_content: None,
            new_content: None,
            diff_summary: None,
            office_file_modified: None,
        }
    }

    pub fn tool_call_start(
        session_id: &str,
        message_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        args: &str,
    ) -> Self {
        Self {
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
            event_type: "tool_call_start".to_string(),
            content: None,
            summary: None,
            tool_call_id: Some(tool_call_id.to_string()),
            tool_name: Some(tool_name.to_string()),
            tool_args: Some(args.to_string()),
            final_content: None,
            error: None,
            search_results: None,
            done: false,
            file_path: None,
            original_content: None,
            new_content: None,
            diff_summary: None,
            office_file_modified: None,
        }
    }

    pub fn tool_result(
        session_id: &str,
        message_id: &str,
        tool_call_id: &str,
        result: &str,
        is_error: bool,
    ) -> Self {
        Self {
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
            event_type: "tool_result".to_string(),
            // When error, only put in error field, not content, to avoid duplication
            content: if is_error { None } else { Some(result.to_string()) },
            summary: None,
            tool_call_id: Some(tool_call_id.to_string()),
            tool_name: None,
            tool_args: None,
            final_content: None,
            error: if is_error { Some(result.to_string()) } else { None },
            search_results: None,
            done: false,
            file_path: None,
            original_content: None,
            new_content: None,
            diff_summary: None,
            office_file_modified: None,
        }
    }

    pub fn done(session_id: &str, message_id: &str, final_content: Option<&str>) -> Self {
        Self {
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
            event_type: "done".to_string(),
            content: None,
            summary: None,
            tool_call_id: None,
            tool_name: None,
            tool_args: None,
            final_content: final_content.map(String::from),
            error: None,
            search_results: None,
            done: true,
            file_path: None,
            original_content: None,
            new_content: None,
            diff_summary: None,
            office_file_modified: None,
        }
    }
}

/// Emit a stream payload to the frontend. Logs a warning on failure.
pub fn emit(app: &AppHandle, payload: StreamPayload) {
    if let Err(error) = app.emit("ai://stream", payload) {
        tracing::warn!("Failed to emit ai://stream event: {}", error);
    }
}
