use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamPayload {
    pub session_id: String,
    pub message_id: String,
    pub event_type: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_content: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    pub done: bool,
}
