use serde::{Deserialize, Serialize};

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
    /// Tool call started
    ToolCallStart,
    /// Tool call completed
    ToolCallEnd,
    /// Tool execution result
    ToolResult,
    /// Thinking/processing indicator
    Thinking,
    /// Final completion
    Done,
    /// Mode switch suggestion from AI
    ModeSwitch,
}

/// Re-exports from diff module for external use
pub use crate::diff::{FileDiffSummary, StreamDiffHunk, StreamDiffChange};

/// Alias for streaming diff summary
pub type StreamDiffSummary = FileDiffSummary;

/// Stream payload with tool call support
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    /// Whether this is the final event
    pub done: bool,

    // === File modification diff fields ===
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

    // === Mode switch suggestion fields ===
    /// Suggested mode to switch to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_mode: Option<String>,
    /// Reason for the mode switch suggestion
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_switch_reason: Option<String>,
}

impl Default for StreamPayload {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            message_id: String::new(),
            event_type: String::new(),
            content: None,
            summary: None,
            tool_call_id: None,
            tool_name: None,
            tool_args: None,
            final_content: None,
            error: None,
            done: false,
            file_path: None,
            original_content: None,
            new_content: None,
            diff_summary: None,
            suggested_mode: None,
            mode_switch_reason: None,
        }
    }
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
            done: false,
            file_path: None,
            original_content: None,
            new_content: None,
            diff_summary: None,
            suggested_mode: None,
            mode_switch_reason: None,
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
            done: true,
            file_path: None,
            original_content: None,
            new_content: None,
            diff_summary: None,
            suggested_mode: None,
            mode_switch_reason: None,
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
            done: false,
            file_path: None,
            original_content: None,
            new_content: None,
            diff_summary: None,
            suggested_mode: None,
            mode_switch_reason: None,
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
            content: Some(result.to_string()),
            summary: None,
            tool_call_id: Some(tool_call_id.to_string()),
            tool_name: None,
            tool_args: None,
            final_content: None,
            error: if is_error { Some(result.to_string()) } else { None },
            done: false,
            file_path: None,
            original_content: None,
            new_content: None,
            diff_summary: None,
            suggested_mode: None,
            mode_switch_reason: None,
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
            done: true,
            file_path: None,
            original_content: None,
            new_content: None,
            diff_summary: None,
            suggested_mode: None,
            mode_switch_reason: None,
        }
    }

    pub fn mode_switch(
        session_id: &str,
        message_id: &str,
        suggested_mode: &str,
        reason: &str,
    ) -> Self {
        Self {
            session_id: session_id.to_string(),
            message_id: message_id.to_string(),
            event_type: "mode_switch".to_string(),
            content: None,
            summary: None,
            tool_call_id: None,
            tool_name: None,
            tool_args: None,
            final_content: None,
            error: None,
            done: false,
            file_path: None,
            original_content: None,
            new_content: None,
            diff_summary: None,
            suggested_mode: Some(suggested_mode.to_string()),
            mode_switch_reason: Some(reason.to_string()),
        }
    }
}
