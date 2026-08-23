//! Agent loop - Core execution engine for tool calling
//!
//! This module implements the agent execution loop:
//! 1. Send request to AI with tools schema
//! 2. Receive AI response (may contain tool_calls)
//! 3. Execute tools and collect results
//! 4. Continue loop until final response or max iterations
//!
//! The loop follows this pattern:
//! ```text
//! request → AI → [tool_calls?] → execute → AI → [tool_calls?] → ... → final
//! ```

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use super::multimodal::{
    push_visual_inspection_bounded, validate_image_request_budget,
    visual_inspections_from_tool_output, ImageAttachment, VisualInspectionInput,
};
use super::profile::AgentProfile;
use super::tools::{SharedToolRegistry, ToolCall, ToolResult};
// `find_tool_spec` is reached via the `pub use super::prompts::*` glob below
// — don't re-import it explicitly or we'll trigger the hidden_glob_reexports
// lint. Same goes for `list_profiles` and `resolve_profile`.
use crate::ai::{AIConfig, AIProvider};
use crate::diff;
use crate::streaming::{
    FileDiffSummary, OfficeFileModified, StreamDiffChange, StreamDiffHunk, StreamPayload,
};

use crate::agent::agent_helpers::{
    parse_tool_call_message, DeltaFunction, DeltaResponse, DeltaToolCall,
};

/// Check if a session has been cancelled
fn is_session_cancelled(session_id: &str) -> bool {
    crate::commands::is_stream_cancelled(session_id)
}

/// Clear cancellation flag for a session
fn clear_cancellation(session_id: &str) {
    let _ = crate::commands::clear_stream_cancelled(session_id);
}

/// Default iteration cap for tool-calling agent loops.
const DEFAULT_MAX_ITERATIONS: usize = 50;

/// Agent execution error
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("AI request failed: {0}")]
    AIError(String),
    #[error("Max iterations ({0}) reached")]
    MaxIterationsReached(usize),
    #[error("Tool execution failed: {0}")]
    ToolExecutionError(String),
    #[error("Invalid response format: {0}")]
    InvalidResponse(String),
    #[error("Cancelled by user")]
    Cancelled,
}

/// Callback type for streaming events
pub type EventCallback = Box<dyn Fn(StreamPayload) + Send + Sync>;

/// Message in the agent conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role")]
pub enum Message {
    #[serde(rename = "system")]
    System { content: String },
    #[serde(rename = "user")]
    User {
        content: String,
        /// Provider-neutral image payloads. Empty for ordinary text turns.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImageAttachment>,
        /// Images are deliberately sent for one API iteration only. The
        /// textual message remains in history, but pixels are dropped after a
        /// successful response to avoid repeatedly re-uploading 10-30 MiB on
        /// every later tool loop.
        #[serde(default)]
        images_once: bool,
    },
    #[serde(rename = "assistant")]
    Assistant {
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
        #[serde(default)]
        tool_calls: Option<Vec<ToolCallMessage>>,
    },
    #[serde(rename = "tool")]
    Tool {
        tool_call_id: String,
        content: String,
    },
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self::User {
            content: content.into(),
            images: Vec::new(),
            images_once: false,
        }
    }

    pub fn user_with_images(content: impl Into<String>, images: Vec<ImageAttachment>) -> Self {
        Self::User {
            content: content.into(),
            images,
            images_once: true,
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::System {
            content: content.into(),
        }
    }

    pub fn assistant(
        content: Option<String>,
        reasoning_content: Option<String>,
        tool_calls: Option<Vec<ToolCallMessage>>,
    ) -> Self {
        Self::Assistant {
            content,
            reasoning_content,
            tool_calls,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self::Tool {
            tool_call_id: tool_call_id.into(),
            content: content.into(),
        }
    }
}

/// Tool call as it appears in messages (for serialization)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ToolCallMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ToolCallFunction {
    pub name: String,
    pub arguments: String, // JSON string
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TodoPlanStatus {
    Pending,
    InProgress,
    Completed,
}

impl TodoPlanStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }
}

#[derive(Debug, Clone)]
struct TodoPlanItem {
    content: String,
    status: TodoPlanStatus,
}

/// Agent state for a single conversation
pub struct AgentSession {
    pub messages: Vec<Message>,
    pub max_iterations: usize,
    pub tool_registry: SharedToolRegistry,
    /// Filesystem authority for this session. This value is fixed when the
    /// session is constructed and is passed explicitly to every normal tool
    /// execution; it never lives in the process-global tool registry.
    workspace: Option<String>,
    /// Optional tool whitelist. When `Some`, only tools whose name is in
    /// this list are advertised to the LLM and accepted at execution time.
    /// `None` (the default) advertises the whole registry, matching the
    /// legacy behaviour of the main Agent session before feature toggles.
    /// See `feature_toggles::effective_tool_set` for how feature toggles
    /// (e.g. `kb_strict`) populate this.
    pub allowed_tools: Option<Vec<String>>,
    /// Optional per-expert iteration cap overrides, keyed by sub-agent
    /// profile name (e.g. `"office_excel_expert"`). When the main agent
    /// dispatches to a sub-agent via `delegate_to`, the override (if any)
    /// replaces the compile-time default in the sub-agent's profile.
    /// Missing keys fall back to the profile's compile-time default.
    /// Populated from the user's `expert_max_iterations` setting on the
    /// frontend; the Tauri command sanitises and clamps values to
    /// `[1, 200]`.
    pub expert_max_iterations: HashMap<String, usize>,
    /// Operational execution plan created by `update_todo`.
    ///
    /// Unlike the old UI-only snapshot, this state is rendered into the
    /// system prompt before every model iteration, so the active row really
    /// constrains what the model should do next.
    todo_plan: Vec<TodoPlanItem>,
}

impl AgentSession {
    pub fn new(tool_registry: SharedToolRegistry) -> Self {
        Self {
            messages: Vec::new(),
            max_iterations: DEFAULT_MAX_ITERATIONS,
            tool_registry,
            workspace: None,
            allowed_tools: None,
            expert_max_iterations: HashMap::new(),
            todo_plan: Vec::new(),
        }
    }

    /// Restrict both the tools advertised to the LLM and the tool calls the
    /// loop will execute. This runtime check is essential because a model can
    /// return a hidden tool name despite it being absent from the schema.
    /// Pass `None` (or use `new`) to allow everything.
    pub fn with_allowed_tools(mut self, allowed: Option<Vec<String>>) -> Self {
        self.allowed_tools = allowed;
        self
    }

    /// Bind this session to one workspace for its entire lifetime. Empty
    /// frontend values are treated as no workspace instead of becoming a
    /// permissive or invalid pseudo-root.
    pub fn with_workspace(mut self, workspace: Option<String>) -> Self {
        self.workspace = workspace.filter(|path| !path.trim().is_empty());
        self
    }

    pub(crate) fn workspace(&self) -> Option<&str> {
        self.workspace.as_deref()
    }

    fn tool_authorization_error(&self, tool_call: &ToolCall) -> Option<ToolResult> {
        let allowed = self
            .allowed_tools
            .as_ref()
            .map(|tools| tools.iter().any(|name| name == &tool_call.name))
            .unwrap_or(true);
        (!allowed).then(|| {
            ToolResult::error(
                &tool_call.id,
                format!(
                    "Tool '{}' is disabled for this session by the active mode, feature toggles, or sub-agent profile and was not executed.",
                    tool_call.name
                ),
            )
        })
    }

    /// Override per-expert iteration caps. The map is keyed by sub-agent
    /// profile name (e.g. `"office_excel_expert"`). Missing keys fall
    /// back to each profile's compile-time default.
    pub fn with_expert_max_iterations(mut self, overrides: HashMap<String, usize>) -> Self {
        self.expert_max_iterations = overrides;
        self
    }

    /// Look up the iteration cap override for a given expert profile.
    /// Returns `None` if the user didn't set a per-expert value (caller
    /// should then use the profile's compile-time default).
    pub fn expert_max_iterations_for(&self, expert_name: &str) -> Option<usize> {
        self.expert_max_iterations.get(expert_name).copied()
    }

    /// Construct a sub-agent session driven by `profile`.
    ///
    /// Shares the parent's `SharedToolRegistry` (tools/executors are shared —
    /// the profile's `allowed_tools` only restricts what's *advertised* to
    /// the LLM via `filtered_definitions`). Per-iteration cap is taken from
    /// the profile, defaulting to `DEFAULT_MAX_ITERATIONS` if zero.
    ///
    /// Used by `delegate_to` (sub-agent dispatch) and by the main Agent Mode
    /// entry point when the slim prompt is enabled.
    pub fn new_with_profile(profile: AgentProfile, tool_registry: SharedToolRegistry) -> Self {
        let max_iters = if profile.max_iterations == 0 {
            DEFAULT_MAX_ITERATIONS
        } else {
            profile.max_iterations
        };
        Self {
            messages: vec![Message::system(profile.system_prompt)],
            max_iterations: max_iters,
            tool_registry,
            workspace: None,
            allowed_tools: if profile.allowed_tools.is_empty() {
                None
            } else {
                Some(profile.allowed_tools)
            },
            expert_max_iterations: HashMap::new(),
            todo_plan: Vec::new(),
        }
    }

    /// Returns the tool definitions that should be advertised to the LLM
    /// for this session. For a profile-driven session the caller passes the
    /// profile's `allowed_tools` here; for legacy sessions, pass `None`.
    pub async fn tool_definitions_for_api(&self, allowed_tools: Option<&[String]>) -> Vec<Value> {
        let registry = self.tool_registry.read().await;
        let defs = match allowed_tools {
            Some(list) => registry.filtered_definitions(list),
            None => registry.get_all_definitions(),
        };
        defs.iter()
            .map(|t| serde_json::to_value(t).unwrap_or(Value::Null))
            .collect()
    }

    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// Enqueue tool-produced images as one user message after a complete
    /// assistant tool-call batch. `read_image` and `render_office_preview`
    /// both reach this live path through the standard visual-assets contract.
    pub fn enqueue_visual_inspections(
        &mut self,
        inputs: Vec<VisualInspectionInput>,
    ) -> Result<(), String> {
        if inputs.is_empty() {
            return Ok(());
        }
        let mut bounded = Vec::with_capacity(inputs.len());
        for input in inputs {
            push_visual_inspection_bounded(&mut bounded, input)
                .map_err(|error| error.to_string())?;
        }
        self.validate_pending_visual_inspections(&bounded)?;
        let labels = Self::visual_inspection_labels(&bounded);
        let images = bounded.into_iter().map(|input| input.attachment).collect();
        self.add_message(Message::user_with_images(
            format!(
                "Visual inspection input generated by completed tools: {}. Inspect the actual pixels now. Report only what is visibly supported; do not claim a visual check merely from file metadata. If this is a document/deck preview, evaluate clipping, overlap, legibility, hierarchy, alignment, spacing, contrast, and page/slide consistency before deciding whether another edit is required.",
                labels
            ),
            images,
        ));
        Ok(())
    }

    fn validate_pending_visual_inspections(
        &self,
        inputs: &[VisualInspectionInput],
    ) -> Result<(), String> {
        validate_image_request_budget(
            self.messages
                .iter()
                .filter_map(|message| match message {
                    Message::User { images, .. } => Some(images.as_slice()),
                    _ => None,
                })
                .flatten()
                .chain(inputs.iter().map(|input| &input.attachment)),
        )
        .map_err(|error| error.to_string())
    }

    fn visual_inspection_labels(inputs: &[VisualInspectionInput]) -> String {
        inputs
            .iter()
            .map(|input| {
                format!(
                    "asset {} from tool call {}{}",
                    input.asset_id,
                    input.source_tool_call_id,
                    input
                        .attachment
                        .name
                        .as_deref()
                        .map(|name| format!(" ({})", name))
                        .unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Drop transient pixel payloads after they have been included in one
    /// successful provider request. The textual provenance remains, so later
    /// iterations know that a visual check occurred without paying the base64
    /// cost again or pretending the pixels are still present.
    fn consume_one_shot_images(&mut self) {
        for message in &mut self.messages {
            if let Message::User {
                content,
                images,
                images_once,
            } = message
            {
                if *images_once && !images.is_empty() {
                    images.clear();
                    *images_once = false;
                    content.push_str(
                        "\n\n[The attached pixels were supplied to an earlier model iteration. They are not being retransmitted in this iteration; call read_image again only if a fresh visual check is necessary.]",
                    );
                }
            }
        }
    }

    fn validate_active_image_budget(&self) -> Result<(), String> {
        validate_image_request_budget(
            self.messages
                .iter()
                .filter_map(|message| match message {
                    Message::User { images, .. } => Some(images.as_slice()),
                    _ => None,
                })
                .flatten(),
        )
        .map_err(|error| error.to_string())
    }

    /// Apply an `update_todo` argument object to this turn's session-owned
    /// plan. Historical calls are intentionally not replayed into a new turn.
    pub(crate) fn apply_todo_arguments(&mut self, arguments: &Value) -> Result<String, String> {
        const MAX_ITEMS: usize = 32;
        const MAX_ITEM_CHARS: usize = 500;

        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("set");
        match action {
            "set" => {
                let raw_items = arguments
                    .get("items")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if raw_items.len() > MAX_ITEMS {
                    return Err(format!(
                        "Todo list has {} items; maximum is {}.",
                        raw_items.len(),
                        MAX_ITEMS
                    ));
                }

                let mut next = Vec::with_capacity(raw_items.len());
                for (index, raw) in raw_items.into_iter().enumerate() {
                    // v2 uses strings. v1 snapshots used objects with
                    // `content` + `status`; accepting both lets persisted
                    // chats hydrate without losing their plan.
                    let (content, requested_status) = match raw {
                        Value::String(content) => (content, None),
                        Value::Object(object) => {
                            let content = object
                                .get("content")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            let status = object
                                .get("status")
                                .and_then(Value::as_str)
                                .map(str::to_string);
                            (content, status)
                        }
                        _ => return Err(format!("Todo item {} must be a string.", index + 1)),
                    };
                    let content = content.trim().to_string();
                    if content.is_empty() {
                        return Err(format!("Todo item {} cannot be empty.", index + 1));
                    }
                    if content.chars().count() > MAX_ITEM_CHARS {
                        return Err(format!(
                            "Todo item {} is too long (maximum {} characters).",
                            index + 1,
                            MAX_ITEM_CHARS
                        ));
                    }
                    let status = match requested_status.as_deref() {
                        Some("completed") => TodoPlanStatus::Completed,
                        Some("in_progress") => TodoPlanStatus::InProgress,
                        _ => TodoPlanStatus::Pending,
                    };
                    next.push(TodoPlanItem { content, status });
                }

                // v2 sets the first row active. For replayed v1 state, keep
                // only its first active row (the runtime contract requires a
                // single current step); if none exists, promote the first
                // pending row.
                let mut saw_active = false;
                for item in &mut next {
                    if item.status == TodoPlanStatus::InProgress {
                        if saw_active {
                            item.status = TodoPlanStatus::Pending;
                        } else {
                            saw_active = true;
                        }
                    }
                }
                if !next.is_empty() && !saw_active {
                    if let Some(item) = next
                        .iter_mut()
                        .find(|item| item.status == TodoPlanStatus::Pending)
                    {
                        item.status = TodoPlanStatus::InProgress;
                    }
                }
                self.todo_plan = next;

                if self.todo_plan.is_empty() {
                    Ok("Todo list cleared; no execution plan is active.".to_string())
                } else {
                    Ok(format!(
                        "Execution plan activated with {} items. The active step is injected into every following model request.",
                        self.todo_plan.len()
                    ))
                }
            }
            "advance" => {
                if let Some(current) = self
                    .todo_plan
                    .iter_mut()
                    .find(|item| item.status == TodoPlanStatus::InProgress)
                {
                    current.status = TodoPlanStatus::Completed;
                }
                if let Some(next) = self
                    .todo_plan
                    .iter_mut()
                    .find(|item| item.status == TodoPlanStatus::Pending)
                {
                    next.status = TodoPlanStatus::InProgress;
                    Ok(format!(
                        "Plan advanced. Current required step: {}",
                        next.content
                    ))
                } else {
                    Ok("Plan advanced. Every step is now completed.".to_string())
                }
            }
            "complete_current" => {
                if let Some(current) = self
                    .todo_plan
                    .iter_mut()
                    .find(|item| item.status == TodoPlanStatus::InProgress)
                {
                    current.status = TodoPlanStatus::Completed;
                    Ok("Current plan step completed; no next step was promoted.".to_string())
                } else {
                    Ok("No in-progress plan step exists.".to_string())
                }
            }
            other => Err(format!(
                "Unknown todo action '{}'. Expected set, advance, or complete_current.",
                other
            )),
        }
    }

    fn todo_prompt_fragment(&self) -> Option<String> {
        if self.todo_plan.is_empty() {
            return None;
        }

        let mut lines = vec![
            "## Live execution plan (authoritative for this iteration)".to_string(),
            "This is operational state, not decorative UI. Work on the single in_progress row now. Do not skip pending rows, repeat completed rows, or claim completion before advancing the plan with update_todo.".to_string(),
        ];
        for (index, item) in self.todo_plan.iter().enumerate() {
            lines.push(format!(
                "{}. [{}] {}",
                index + 1,
                item.status.as_str(),
                item.content
            ));
        }
        Some(lines.join("\n"))
    }

    pub fn get_messages_for_api(&self, provider: &AIProvider) -> Vec<serde_json::Value> {
        let mut messages: Vec<Value> = self.messages
            .iter()
            .map(|msg| match msg {
                Message::System { content } => serde_json::json!({
                    "role": "system",
                    "content": content
                }),
                Message::User { content, images, .. } if images.is_empty() => serde_json::json!({
                    "role": "user",
                    "content": content
                }),
                Message::User { content, images, .. } => match provider {
                    AIProvider::Ollama { .. } => serde_json::json!({
                        "role": "user",
                        "content": content,
                        "images": images.iter().map(|image| image.data_base64.clone()).collect::<Vec<_>>()
                    }),
                    AIProvider::OpenAI { .. } | AIProvider::Official { .. } => {
                        let mut parts = vec![serde_json::json!({
                            "type": "text",
                            "text": content,
                        })];
                        parts.extend(images.iter().map(|image| serde_json::json!({
                            "type": "image_url",
                            "image_url": {
                                "url": image.as_data_url(),
                                "detail": image.detail.as_str(),
                            }
                        })));
                        serde_json::json!({
                            "role": "user",
                            "content": parts,
                        })
                    }
                },
                Message::Assistant { content, reasoning_content, tool_calls } => {
                    let mut obj = serde_json::json!({
                        "role": "assistant",
                    });
                    if let Some(c) = content {
                        obj["content"] = serde_json::json!(c);
                    }
                    if let Some(rc) = reasoning_content {
                        obj["reasoning_content"] = serde_json::json!(rc);
                    }
                    if let Some(tc) = tool_calls {
                        obj["tool_calls"] = serde_json::json!(tc);
                    }
                    obj
                }
                Message::Tool { tool_call_id, content } => serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tool_call_id,
                    "content": content
                }),
            })
            .collect();

        // Merge the live plan into the first system message. A number of
        // OpenAI-compatible providers only accept `system` at the beginning,
        // so appending a late standalone system message is less portable.
        if let Some(plan) = self.todo_prompt_fragment() {
            if let Some(system) = messages
                .iter_mut()
                .find(|message| message.get("role").and_then(Value::as_str) == Some("system"))
            {
                if let Some(content) = system.get_mut("content") {
                    let base = content.as_str().unwrap_or_default();
                    *content = Value::String(format!("{}\n\n---\n\n{}", base, plan));
                }
            } else {
                messages.insert(0, serde_json::json!({"role": "system", "content": plan}));
            }
        }

        messages
    }
}

/// Tool call parsing result
#[derive(Debug)]
pub struct ParsedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Agent executor that runs the tool-calling loop
pub struct AgentExecutor {
    config: AIConfig,
    client: &'static reqwest::Client,
}

impl AgentExecutor {
    pub fn new(config: AIConfig) -> Self {
        // Reuse the shared `HTTP_CLIENT` from `ai` so we keep-alive connections
        // and DNS cache across calls — and across executors. Building a fresh
        // `reqwest::Client` per executor would re-do the TLS handshake every
        // time the user opens a new chat, and would also fragment the
        // connection pool.
        Self {
            config,
            client: &crate::ai::HTTP_CLIENT,
        }
    }

    /// Run the agent loop with a user request
    pub async fn run<F>(
        &self,
        session: &mut AgentSession,
        user_request: &str,
        session_id: &str,
        message_id: &str,
        on_event: F,
    ) -> Result<String, AgentError>
    where
        F: Fn(StreamPayload) + Clone + Send + Sync + 'static,
    {
        self.run_with_user_message(
            session,
            Message::user(user_request),
            session_id,
            message_id,
            on_event,
        )
        .await
    }

    /// Run an agent turn whose user message includes one or more images.
    /// Images have already been bounded and MIME-validated by
    /// `resolve_image_attachments`; provider-specific serialization happens
    /// immediately before each API request.
    pub async fn run_multimodal<F>(
        &self,
        session: &mut AgentSession,
        user_request: &str,
        images: Vec<ImageAttachment>,
        session_id: &str,
        message_id: &str,
        on_event: F,
    ) -> Result<String, AgentError>
    where
        F: Fn(StreamPayload) + Clone + Send + Sync + 'static,
    {
        self.run_with_user_message(
            session,
            Message::user_with_images(user_request, images),
            session_id,
            message_id,
            on_event,
        )
        .await
    }

    async fn run_with_user_message<F>(
        &self,
        session: &mut AgentSession,
        user_message: Message,
        session_id: &str,
        message_id: &str,
        on_event: F,
    ) -> Result<String, AgentError>
    where
        F: Fn(StreamPayload) + Clone + Send + Sync + 'static,
    {
        tracing::debug!(
            "AgentExecutor::run started - session_id: {}, message_id: {}",
            session_id,
            message_id
        );

        // The executor is the single owner of current-turn insertion. The
        // frontend history and commands_agent must contain prior messages
        // only; centralising the write here prevents double/triple prompts.
        session.add_message(user_message);

        // Get tool definitions for API call. When the session carries an
        // explicit `allowed_tools` whitelist (set by feature toggles like
        // strict-KB), use that; otherwise expose the whole registry.
        let tools = session
            .tool_definitions_for_api(session.allowed_tools.as_deref())
            .await;
        let tools_json: Vec<Value> = tools
            .into_iter()
            .map(|t| {
                serde_json::to_value(&t)
                    .map_err(|e| AgentError::AIError(format!("Tool serialization failed: {}", e)))
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Run the agent loop
        for iteration in 0..session.max_iterations {
            tracing::debug!(
                "Agent iteration {}/{}",
                iteration + 1,
                session.max_iterations
            );

            // Check for cancellation before each iteration
            if is_session_cancelled(session_id) {
                tracing::info!("Session {} cancelled, stopping", session_id);
                clear_cancellation(session_id);
                return Err(AgentError::Cancelled);
            }

            // Build request
            session.validate_active_image_budget().map_err(|error| {
                AgentError::AIError(format!(
                    "Multimodal request exceeds the shared image budget: {}",
                    error
                ))
            })?;
            let messages = session.get_messages_for_api(&self.config.provider);

            // Make API call with tools
            let response = self
                .call_ai_with_tools(
                    &messages,
                    &tools_json,
                    session_id,
                    message_id,
                    on_event.clone(),
                )
                .await?;

            // The request succeeded, so any one-shot image payloads have now
            // reached the model. Retain provenance text but release base64
            // before parsing/executing the next tool iteration.
            session.consume_one_shot_images();

            // Parse response
            let (content, reasoning_content, tool_calls) = self.parse_response(&response)?;

            // Add assistant message to history (with reasoning_content for DeepSeek)
            session.add_message(Message::assistant(
                content.clone(),
                reasoning_content.clone(),
                tool_calls.clone(),
            ));

            // If no tool calls, we're done
            let tool_calls = match tool_calls {
                Some(tc) if !tc.is_empty() => tc,
                _ => {
                    return Ok(content.unwrap_or_else(String::new));
                }
            };

            // Parse and execute tool calls
            let parsed_calls: Vec<ParsedToolCall> = tool_calls
                .iter()
                .filter_map(|tc| {
                    let name = tc.function.name.clone();
                    let id = tc.id.clone();

                    // Parse arguments JSON
                    let arguments: Value = match serde_json::from_str(&tc.function.arguments) {
                        Ok(arguments) => arguments,
                        Err(error) => {
                            tracing::warn!(
                                "Failed to parse tool arguments for {} ({}): {}",
                                name,
                                id,
                                error
                            );
                            serde_json::json!({})
                        }
                    };

                    Some(ParsedToolCall {
                        id,
                        name,
                        arguments,
                    })
                })
                .collect();

            // Execute each tool call
            let mut pending_visual_inspections = Vec::new();
            for parsed in &parsed_calls {
                let tool_call = ToolCall {
                    id: parsed.id.clone(),
                    name: parsed.name.clone(),
                    arguments: parsed.arguments.clone(),
                };

                // `update_todo` mutates session state synchronously. Keep it
                // outside the async meta dispatch future so that future does
                // not retain a mutable session borrow across `.await` and
                // then contend with the normal registry fallback.
                let mut result = if let Some(denied) = session.tool_authorization_error(&tool_call)
                {
                    denied
                } else if tool_call.name == "update_todo" {
                    match session.apply_todo_arguments(&tool_call.arguments) {
                        Ok(summary) => ToolResult::success(&tool_call.id, summary),
                        Err(error) => ToolResult::error(&tool_call.id, error),
                    }
                } else {
                    match self
                        .try_handle_meta_tool(
                            &tool_call,
                            session,
                            session_id,
                            message_id,
                            on_event.clone(),
                        )
                        .await
                    {
                        Some(result) => result,
                        None => {
                            session
                                .tool_registry
                                .read()
                                .await
                                .execute_in_workspace(&tool_call, session.workspace())
                                .await
                        }
                    }
                };

                // Inject the correct tool_call_id from streamed data (not the placeholder)
                result.tool_call_id = parsed.id.clone();

                // Capability-bearing asset manifests are accepted only from
                // the explicitly trusted producers in multimodal.rs:
                // `read_image` (one asset) and `render_office_preview`
                // (bounded page array). Pixels are queued for the next request
                // only after every tool result in this assistant batch lands.
                if !result.is_error
                    && matches!(parsed.name.as_str(), "read_image" | "render_office_preview")
                {
                    let asset_metadata = result.output.clone();
                    match visual_inspections_from_tool_output(
                        &parsed.id,
                        &parsed.name,
                        &result.output,
                        session.workspace(),
                    ) {
                        Ok(inputs) if !inputs.is_empty() => {
                            if self.config.supports_vision == Some(false) {
                                result = ToolResult::error(
                                    &parsed.id,
                                    format!(
                                        "The selected model '{}' is configured as text-only, so it cannot visually inspect output from '{}'. Choose a vision-capable model; no visual verification was performed. Asset metadata: {}",
                                        self.config.model, parsed.name, asset_metadata,
                                    ),
                                );
                            } else {
                                let batch_start = pending_visual_inspections.len();
                                let mut queue_error = None;
                                for input in inputs {
                                    if let Err(error) = push_visual_inspection_bounded(
                                        &mut pending_visual_inspections,
                                        input,
                                    ) {
                                        queue_error = Some(error.to_string());
                                        break;
                                    }
                                }
                                if queue_error.is_none() {
                                    queue_error = session
                                        .validate_pending_visual_inspections(
                                            &pending_visual_inspections,
                                        )
                                        .err();
                                }
                                if let Some(error) = queue_error {
                                    pending_visual_inspections.truncate(batch_start);
                                    result = ToolResult::error(
                                        &parsed.id,
                                        format!(
                                            "Visual inspection batch limit reached: {}. Pixels from '{}' were not sent to the model; inspect fewer/smaller pages in one batch. Asset metadata: {}",
                                            error, parsed.name, asset_metadata,
                                        ),
                                    );
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(error) => {
                            result = ToolResult::error(
                                &parsed.id,
                                format!(
                                    "Output from '{}' could not be queued for visual inspection: {}. No visual verification was performed. Original tool output: {}",
                                    parsed.name, error, asset_metadata,
                                ),
                            );
                        }
                    }
                }

                // Compute diff summary for file modification tools
                // Only compute if we have original content; new content will be read lazily if needed
                let diff_summary: Option<FileDiffSummary> =
                    if let (Some(file_path), Some(original)) =
                        (&result.file_path, &result.original_content)
                    {
                        // Try to read new content for diff, but don't fail if it can't be read
                        let new_content = std::path::Path::new(file_path)
                            .exists()
                            .then(|| std::fs::read_to_string(file_path).ok())
                            .flatten();

                        if let Some(new_content) = new_content {
                            let diff_result = diff::compute_diff(original, &new_content);
                            let file_name = std::path::Path::new(file_path)
                                .file_name()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| file_path.clone());

                            let hunks = diff_result
                                .hunks
                                .into_iter()
                                .map(|h| StreamDiffHunk {
                                    id: h.id,
                                    old_start: h.old_range.start_line,
                                    old_lines: h
                                        .old_range
                                        .end_line
                                        .saturating_sub(h.old_range.start_line)
                                        + 1,
                                    new_start: h.new_range.start_line,
                                    new_lines: h
                                        .new_range
                                        .end_line
                                        .saturating_sub(h.new_range.start_line)
                                        + 1,
                                    changes: h
                                        .changes
                                        .into_iter()
                                        .map(|c| StreamDiffChange {
                                            tag: match c.tag {
                                                diff::ChangeType::Delete => "delete".to_string(),
                                                diff::ChangeType::Insert => "insert".to_string(),
                                                diff::ChangeType::Equal => "equal".to_string(),
                                            },
                                            old_line: c.old_line,
                                            new_line: c.new_line,
                                            content: c.content,
                                        })
                                        .collect(),
                                })
                                .collect();

                            Some(FileDiffSummary {
                                file_name,
                                added_lines: diff_result.summary.added_lines,
                                deleted_lines: diff_result.summary.deleted_lines,
                                hunks,
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                // Detect if create_word_doc succeeded (non-error, has path)
                let office_file_modified: Option<OfficeFileModified> =
                    if !result.is_error && parsed.name == "create_word_doc" {
                        if let Some(path) = result.file_path.as_ref() {
                            let format = std::path::Path::new(path)
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                                .to_lowercase();
                            Some(OfficeFileModified {
                                path: path.clone(),
                                format,
                            })
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                // Emit tool result event (includes diff info for file modifications)
                on_event(StreamPayload {
                    session_id: session_id.to_string(),
                    message_id: message_id.to_string(),
                    event_type: "tool_result".to_string(),
                    content: Some(result.output.clone()),
                    summary: None,
                    tool_call_id: Some(result.tool_call_id.clone()),
                    tool_name: None,
                    tool_args: None,
                    final_content: None,
                    error: if result.is_error {
                        Some(result.output.clone())
                    } else {
                        None
                    },
                    search_results: None,
                    done: false,
                    // Diff info for file modification tools
                    file_path: result.file_path.clone(),
                    original_content: result.original_content.clone(),
                    new_content: result.new_content.clone(),
                    diff_summary,
                    office_file_modified,
                });

                // Add tool result to message history
                session.add_message(Message::tool_result(&parsed.id, &result.output));
            }

            // Protocol invariant: never insert a user/image message between
            // an assistant's tool_calls and their tool results. We enqueue
            // exactly once here, after the full batch has completed.
            if let Err(error) = session.enqueue_visual_inspections(pending_visual_inspections) {
                // Defensive invariant: each image was already admitted by the
                // same bounded push helper above, so this should never fire.
                tracing::error!(
                    "failed to enqueue validated visual inspection batch: {}",
                    error
                );
            }
        }

        Err(AgentError::MaxIterationsReached(session.max_iterations))
    }

    /// Intercept async meta-tools (`get_tool_help` and `delegate_to`)
    /// so the loop doesn't try to dispatch them via the registry.
    ///
    /// Returns `Some(ToolResult)` for intercepted calls (caller should
    /// use this directly); returns `None` for normal tools (caller should
    /// fall through to `ToolRegistry::execute`).
    ///
    /// `session_id` / `message_id` are forwarded into the tool result event
    /// so the frontend can attribute the output correctly. For
    /// `delegate_to`, sub-agent intermediate events are emitted under a
    /// prefixed sub-message-id so the UI can render them inside a
    /// collapsible "delegated to X" card.
    // ── Meta-tool dispatch ───────────────────────────────────────────────────

    fn try_handle_meta_tool<'a, F>(
        &'a self,
        tool_call: &'a ToolCall,
        session: &'a AgentSession,
        session_id: &'a str,
        message_id: &'a str,
        on_event: F,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ToolResult>> + Send + 'a>>
    where
        F: Fn(StreamPayload) + Clone + Send + Sync + 'static,
    {
        Box::pin(async move {
            match tool_call.name.as_str() {
                "get_tool_help" => {
                    // Accept either `category` (preferred) or the older
                    // `spec` key for backwards compatibility during the
                    // transition.
                    let category = tool_call
                        .arguments
                        .get("category")
                        .or_else(|| tool_call.arguments.get("spec"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    match find_tool_spec(category) {
                        // Spec text is injected into the LLM's context via
                        // the tool result so it can use the detailed
                        // instructions on subsequent turns. The returned
                        // string is internal — the frontend only renders a
                        // tiny indicator (category name), never the spec
                        // body itself.
                        Some(text) => Some(ToolResult::success(&tool_call.id, text.to_string())),
                        None => {
                            let available = [
                                "general",
                                "word",
                                "excel",
                                "pptx",
                                "markdown",
                                "media",
                                "svg",
                                "document_converter",
                            ]
                            .join(", ");
                            let msg = format!(
                                "Unknown help category '{}'. Available: {}",
                                category, available
                            );
                            Some(ToolResult::error(&tool_call.id, msg))
                        }
                    }
                }
                "delegate_to" => {
                    let expert = tool_call
                        .arguments
                        .get("expert")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let task = tool_call
                        .arguments
                        .get("task")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let context = tool_call
                        .arguments
                        .get("context")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    // Per-expert iteration cap override: if the parent
                    // session has a user-configured override for this
                    // expert, apply it. Otherwise use the compile-time
                    // default baked into the profile.
                    let override_max_iterations = session.expert_max_iterations_for(&expert);
                    let profile = match resolve_profile(&expert, override_max_iterations) {
                        Some(p) => p,
                        None => {
                            let available: Vec<String> = list_profiles()
                                .into_iter()
                                .filter(|(n, _)| *n != "main")
                                .map(|(n, _)| n.to_string())
                                .collect();
                            let msg = format!(
                                "Unknown expert '{}'. Available: {}",
                                expert,
                                available.join(", ")
                            );
                            return Some(ToolResult::error(&tool_call.id, msg));
                        }
                    };

                    let result = self
                        .run_subagent(
                            &profile,
                            &task,
                            context.as_deref(),
                            session,
                            session_id,
                            message_id,
                            on_event,
                        )
                        .await;

                    match result {
                        Ok(summary) => Some(ToolResult::success(&tool_call.id, summary)),
                        Err(e) => Some(ToolResult::error(
                            &tool_call.id,
                            format!("[{}] {}", expert, e),
                        )),
                    }
                }
                _ => None,
            }
        })
    }

    /// Execute a sub-agent run. Reuses the parent's shared `ToolRegistry`
    /// (so `AppHandle` and lazily-added tools like `database_search`
    /// propagate), filters tool visibility via the profile, and routes the
    /// sub-agent's stream events under a sub-message-id prefixed with
    /// `"sub:"` so the UI can collapse them.
    // ── Sub-agent execution ────────────────────────────────────────────────

    fn run_subagent<'a, F>(
        &'a self,
        profile: &'a AgentProfile,
        task: &'a str,
        context: Option<&'a str>,
        parent_session: &'a AgentSession,
        session_id: &'a str,
        parent_message_id: &'a str,
        on_event: F,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, AgentError>> + Send + 'a>>
    where
        F: Fn(StreamPayload) + Clone + Send + Sync + 'static,
    {
        Box::pin(async move {
            let registry = parent_session.tool_registry.clone();
            let mut sub_session = AgentSession::new_with_profile(profile.clone(), registry)
                .with_workspace(parent_session.workspace.clone());

            // Append an optional context line so the sub-agent knows the why.
            let task_message = match context {
                Some(ctx) if !ctx.is_empty() => format!("{}\n\nContext:\n{}", task, ctx),
                _ => task.to_string(),
            };

            let sub_message_id = format!("sub:{}:{}", profile.name, uuid::Uuid::new_v4());

            // Notify the frontend that a sub-agent is starting. The frontend uses
            // this to render a collapsible "nested" activity block under the
            // delegate_to card and to route the subsequent stream events (which
            // carry sub_message_id as their message_id) into that block.
            on_event(StreamPayload::subagent_start(
                session_id,
                parent_message_id,
                &sub_message_id,
                &profile.name,
                &profile.label,
                &task,
            ));

            // Run nested. The same callback channel is reused; the on_event
            // listener (frontend) should distinguish via message_id.
            let summary = self
                .run(
                    &mut sub_session,
                    &task_message,
                    session_id,
                    &sub_message_id,
                    on_event.clone(),
                )
                .await;

            // Notify the frontend that the sub-agent has finished.
            on_event(StreamPayload::subagent_end(
                session_id,
                parent_message_id,
                &sub_message_id,
            ));

            match summary {
                Ok(s) => Ok(format!("[{} completed]\n\n{}", profile.label, s)),
                Err(e) => Err(e),
            }
        })
    }

    /// Call AI with tools
    async fn call_ai_with_tools<F>(
        &self,
        messages: &[Value],
        tools: &[Value],
        session_id: &str,
        message_id: &str,
        on_event: F,
    ) -> Result<String, AgentError>
    where
        F: Fn(StreamPayload) + Clone + Send + Sync + 'static,
    {
        let (url, headers, body) = match &self.config.provider {
            AIProvider::OpenAI { api_key, base_url } => {
                let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
                let headers = vec![("Authorization", format!("Bearer {}", api_key))];
                // OpenAI format with tools array
                let body = serde_json::json!({
                    "model": self.config.model,
                    "messages": messages,
                    "tools": tools,
                    "temperature": self.config.temperature,
                    "max_tokens": self.config.max_tokens,
                    "stream": true,
                });
                (url, headers, body)
            }
            AIProvider::Ollama { base_url } => {
                let url = format!("{}/api/chat", base_url.trim_end_matches('/'));
                let headers = vec![("Content-Type", "application/json".to_string())];
                // Ollama format: tools array inside the request
                let body = serde_json::json!({
                    "model": self.config.model,
                    "messages": messages,
                    "tools": tools,
                    "stream": true,
                });
                (url, headers, body)
            }
            AIProvider::Official { api_key } => {
                let url = "https://api.inkuo.com/v1/chat/completions".to_string();
                let headers = vec![("Authorization", format!("Bearer {}", api_key))];
                // OpenAI-compatible format
                let body = serde_json::json!({
                    "model": self.config.model,
                    "messages": messages,
                    "tools": tools,
                    "temperature": self.config.temperature,
                    "max_tokens": self.config.max_tokens,
                    "stream": true,
                });
                (url, headers, body)
            }
        };

        let mut request = self.client.post(&url).json(&body);

        for (key, value) in &headers {
            request = request.header(*key, value);
        }

        tracing::info!(
            "Sending request to {} with {} messages",
            url,
            messages.len()
        );

        let response = request
            .send()
            .await
            .map_err(|e| AgentError::AIError(format!("Request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = match response.text().await {
                Ok(body) => body,
                Err(error) => {
                    tracing::warn!(
                        "Failed to read AI error response body (status {}): {}",
                        status,
                        error
                    );
                    String::new()
                }
            };
            return Err(AgentError::AIError(format!("HTTP {}: {}", status, body)));
        }

        tracing::info!("Received response, status: {}", response.status());

        // Determine if this is Ollama (needs different tool call parsing)
        let is_ollama = matches!(&self.config.provider, AIProvider::Ollama { .. });

        // Process streaming response
        let mut buffer = String::new();
        let mut current_tool_calls: Vec<ToolCallMessage> = Vec::new();
        let mut current_content = String::new();
        let mut current_reasoning_content = String::new();
        // Counter for generating stable fallback IDs only when model provides none
        let mut fallback_id_counter: usize = 0;

        let mut stream = response.bytes_stream();
        let mut bytes_received = 0;

        // Track which tool_call indices have already had their `tool_call_start` event
        // emitted, so we can fire the start event the first time a new index appears,
        // and emit incremental args deltas on every subsequent chunk.
        let mut tool_call_started: std::collections::HashSet<usize> =
            std::collections::HashSet::new();

        // Throttle: avoid emitting a `tool_call_args_delta` for the same tool
        // call index more than once per `TOOL_ARGS_EMIT_INTERVAL_MS`. The raw
        // arg string is still accumulated fully into `entry.function.arguments`
        // regardless, so the next emission carries the up-to-date state. This
        // caps both the per-chunk IPC payload size (which scales with the
        // accumulated args) and the React render rate, without blocking the
        // SSE receive loop.
        const TOOL_ARGS_EMIT_INTERVAL_MS: u128 = 60;
        let mut last_tool_args_emit: std::collections::HashMap<usize, std::time::Instant> =
            std::collections::HashMap::new();
        let mut tool_args_has_pending: std::collections::HashSet<usize> =
            std::collections::HashSet::new();

        tracing::debug!("Starting to process stream...");

        while let Some(item) = stream.next().await {
            // Check for cancellation during streaming
            if is_session_cancelled(session_id) {
                tracing::info!("Session {} cancelled during streaming", session_id);
                clear_cancellation(session_id);
                return Err(AgentError::Cancelled);
            }

            let bytes = match item {
                Ok(b) => {
                    bytes_received += b.len();
                    b
                }
                Err(e) => {
                    tracing::error!("Stream error: {}", e);
                    break;
                }
            };

            let chunk = String::from_utf8_lossy(&bytes).to_string();
            buffer.push_str(&chunk);

            // Process complete SSE events using the shared splitter. We
            // prefer event-level boundaries ("\n\n" / "\r\n\r\n") over a
            // single "\n" so that CRLF-terminated streams from proxies
            // (and Ollama's mangled line endings) don't leak a stray '\r'
            // into the JSON parser.
            while let Some((event, rest)) = crate::openai_stream::take_next_sse_event(&buffer) {
                buffer = rest;

                // An event with no `data:` lines (e.g. event-name-only
                // frames some gateways emit) should not advance state.
                if event.trim().is_empty() {
                    continue;
                }

                for data in crate::openai_stream::iter_sse_event_data_lines(&event) {
                    if data.trim() == "[DONE]" {
                        continue;
                    }

                    tracing::trace!("SSE data: {}", data);

                    let parsed = self.parse_sse_delta(data, is_ollama);
                    tracing::trace!("[PARSING] parse result: {:?}", parsed);
                    match parsed {
                        Ok(Some(delta)) => {
                            // Update content (both content and reasoning_content for DeepSeek)
                            if let Some(content) = delta.content {
                                current_content.push_str(&content);
                                on_event(StreamPayload {
                                    session_id: session_id.to_string(),
                                    message_id: message_id.to_string(),
                                    event_type: "text".to_string(),
                                    content: Some(content),
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
                                });
                            }
                            // Also handle reasoning_content (DeepSeek's thinking).
                            // Emitted with a dedicated "reasoning" event type so the
                            // frontend can render thinking blocks separately from
                            // the final answer and collapse them once they're done.
                            if let Some(reasoning) = delta.reasoning_content {
                                if !reasoning.is_empty() {
                                    current_reasoning_content.push_str(&reasoning);
                                    on_event(StreamPayload {
                                        session_id: session_id.to_string(),
                                        message_id: message_id.to_string(),
                                        event_type: "reasoning".to_string(),
                                        content: Some(reasoning),
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
                                    });
                                }
                            }

                            // Collect tool calls and emit progressive events
                            if let Some(tool_calls) = delta.tool_calls {
                                for tc in tool_calls {
                                    // Grow capacity to include this index
                                    while current_tool_calls.len() <= tc.index {
                                        fallback_id_counter += 1;
                                        current_tool_calls.push(ToolCallMessage {
                                            id: format!("call_{}", fallback_id_counter),
                                            call_type: "function".to_string(),
                                            function: ToolCallFunction {
                                                name: String::new(),
                                                arguments: String::new(),
                                            },
                                        });
                                    }

                                    let entry = &mut current_tool_calls[tc.index];

                                    // Track whether this chunk mutated the entry's id or name. Used below to
                                    // decide whether to fire a `tool_call_args_delta` event for a
                                    // tool call we've already announced (id/name updates count
                                    // as a reason to emit, in addition to incoming arg deltas).
                                    let id_updated = if let Some(id) = &tc.id {
                                        if !id.is_empty() {
                                            entry.id = id.clone();
                                            true
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    };

                                    let name_updated = if let Some(name) = &tc.function.name {
                                        if !name.is_empty() {
                                            entry.function.name = name.clone();
                                            true
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    };

                                    // Capture the argument delta BEFORE appending so we can
                                    // emit a true incremental `args_delta` event.
                                    let arg_delta: Option<String> =
                                        if let Some(args) = &tc.function.arguments {
                                            if !args.is_empty() {
                                                entry.function.arguments.push_str(args);
                                                Some(args.clone())
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        };

                                    // Emit `tool_call_start` the first time we see this index.
                                    // This lets the frontend show the tool card with partial arguments
                                    // during streaming. The card will be finalized when tool_result arrives.
                                    if !tool_call_started.contains(&tc.index) {
                                        tool_call_started.insert(tc.index);
                                        let live_id = entry.id.clone();
                                        let live_name = entry.function.name.clone();
                                        let live_args = entry.function.arguments.clone();
                                        // Initialise the throttle clock for this index.
                                        last_tool_args_emit
                                            .insert(tc.index, std::time::Instant::now());
                                        on_event(StreamPayload {
                                            session_id: session_id.to_string(),
                                            message_id: message_id.to_string(),
                                            event_type: "tool_call_start".to_string(),
                                            content: None,
                                            summary: None,
                                            tool_call_id: Some(live_id),
                                            tool_name: Some(live_name),
                                            tool_args: Some(live_args),
                                            final_content: None,
                                            error: None,
                                            search_results: None,
                                            done: false,
                                            file_path: None,
                                            original_content: None,
                                            new_content: None,
                                            diff_summary: None,
                                            office_file_modified: None,
                                        });
                                    } else if id_updated || name_updated || arg_delta.is_some() {
                                        // Subsequent chunk for the same tool call index.
                                        // Throttle the emission so we don't flood the IPC
                                        // channel with 10000-char payloads at SSE rate.
                                        let now = std::time::Instant::now();
                                        let should_emit = tool_args_has_pending.contains(&tc.index)
                                            || now
                                                .duration_since(
                                                    last_tool_args_emit
                                                        .get(&tc.index)
                                                        .copied()
                                                        .unwrap_or(now),
                                                )
                                                .as_millis()
                                                >= TOOL_ARGS_EMIT_INTERVAL_MS;
                                        if should_emit {
                                            tool_args_has_pending.remove(&tc.index);
                                            last_tool_args_emit.insert(tc.index, now);
                                            let live_id = entry.id.clone();
                                            let live_name = entry.function.name.clone();
                                            let live_args = entry.function.arguments.clone();
                                            on_event(StreamPayload {
                                                session_id: session_id.to_string(),
                                                message_id: message_id.to_string(),
                                                event_type: "tool_call_args_delta".to_string(),
                                                content: arg_delta,
                                                summary: None,
                                                tool_call_id: Some(live_id),
                                                tool_name: Some(live_name),
                                                tool_args: Some(live_args),
                                                final_content: None,
                                                error: None,
                                                search_results: None,
                                                done: false,
                                                file_path: None,
                                                original_content: None,
                                                new_content: None,
                                                diff_summary: None,
                                                office_file_modified: None,
                                            });
                                        } else {
                                            // Mark that we owe a delta so the *next* chunk
                                            // (or the post-loop flush below) sends the
                                            // current accumulated state.
                                            tool_args_has_pending.insert(tc.index);
                                        }
                                    }
                                }
                            }
                        }
                        Ok(None) => {
                            // No content or tool_calls in this delta, skip
                            tracing::trace!("Delta has no content or tool_calls");
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse SSE delta: {}", e);
                        }
                    }
                }
            }
        }

        // Process any remaining data in the buffer (issue #9 - handle residual data)
        if !buffer.trim().is_empty() {
            tracing::debug!("Processing remaining buffer data: {}", buffer);
            for data in crate::openai_stream::iter_sse_event_data_lines(&buffer) {
                if data.trim() == "[DONE]" || data.trim().is_empty() {
                    continue;
                }
                if let Ok(Some(delta)) = self.parse_sse_delta(data, is_ollama) {
                    if let Some(content) = delta.content {
                        current_content.push_str(&content);
                    }
                    // `delta.tool_calls` from residual data is intentionally
                    // ignored here — every tool call was already accounted
                    // for in the main loop above where its delta, start
                    // event, and accumulated arguments were emitted. We
                    // only pick up any trailing `content` text.
                }
            }
        }

        // Flush any throttled tool-call args deltas so the frontend receives
        // the final accumulated state. The actual tool execution will fire
        // `tool_result` next, which already carries the full state — but
        // emitting once more here keeps the streaming UI in sync when the
        // last delta happened to be skipped by the throttle.
        for idx in &tool_args_has_pending {
            if let Some(entry) = current_tool_calls.get(*idx) {
                if !tool_call_started.contains(idx) {
                    continue;
                }
                on_event(StreamPayload {
                    session_id: session_id.to_string(),
                    message_id: message_id.to_string(),
                    event_type: "tool_call_args_delta".to_string(),
                    content: None,
                    summary: None,
                    tool_call_id: Some(entry.id.clone()),
                    tool_name: Some(entry.function.name.clone()),
                    tool_args: Some(entry.function.arguments.clone()),
                    final_content: None,
                    error: None,
                    search_results: None,
                    done: false,
                    file_path: None,
                    original_content: None,
                    new_content: None,
                    diff_summary: None,
                    office_file_modified: None,
                });
            }
        }
        tool_args_has_pending.clear();

        tracing::debug!(
            "Stream processing complete. bytes_received: {}, current_content_len: {}",
            bytes_received,
            current_content.len()
        );

        // Debug: log the final tool calls
        for (i, tc) in current_tool_calls.iter().enumerate() {
            tracing::debug!(
                "[TOOL_CALL_DEBUG] #{:02}: id='{}', name='{}', args='{}'",
                i,
                tc.id,
                tc.function.name,
                tc.function.arguments
            );
        }

        // Build final response
        let response_json = serde_json::json!({
            "content": current_content,
            "reasoning_content": current_reasoning_content,
            "tool_calls": current_tool_calls
        });

        Ok(response_json.to_string())
    }

    /// Parse SSE delta from OpenAI format (handles DeepSeek's reasoning_content)
    /// For Ollama, uses a different response format (message.tool_calls instead of delta.tool_calls)
    // ── SSE / response parsing ─────────────────────────────────────────────

    fn parse_sse_delta(
        &self,
        data: &str,
        is_ollama: bool,
    ) -> Result<Option<DeltaResponse>, String> {
        let json: Value =
            serde_json::from_str(data).map_err(|e| format!("JSON parse error: {}", e))?;

        if is_ollama {
            // Ollama format: data.message.tool_calls
            return self.parse_ollama_delta(&json);
        }

        // OpenAI format: data.choices[0].delta
        let delta = match json.get("choices") {
            Some(choices) if choices.is_array() => choices.get(0).and_then(|c| c.get("delta")),
            _ => None,
        };

        let delta = match delta {
            Some(d) => d,
            None => return Ok(None),
        };

        // Handle content (Chinese text from DeepSeek)
        let content = delta
            .get("content")
            .and_then(|c| c.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);

        // Handle reasoning_content (English thinking from DeepSeek)
        let reasoning_content = delta
            .get("reasoning_content")
            .and_then(|c| c.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);

        let tool_calls = delta
            .get("tool_calls")
            .and_then(|tc| tc.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        let index = tc.get("index")?.as_u64()? as usize;
                        let id = tc.get("id").and_then(|v| v.as_str()).map(String::from);
                        let function = tc.get("function")?;
                        let name = function
                            .get("name")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let arguments = function
                            .get("arguments")
                            .and_then(|v| v.as_str())
                            .map(String::from);

                        Some(DeltaToolCall {
                            index,
                            id,
                            function: DeltaFunction { name, arguments },
                        })
                    })
                    .collect()
            });

        Ok(Some(DeltaResponse {
            content,
            reasoning_content,
            tool_calls,
        }))
    }

    /// Parse Ollama's SSE delta format
    /// Ollama format: data.message.content and data.message.tool_calls
    fn parse_ollama_delta(&self, json: &Value) -> Result<Option<DeltaResponse>, String> {
        let message = match json.get("message") {
            Some(m) => m,
            None => return Ok(None),
        };

        // Handle content
        let content = message
            .get("content")
            .and_then(|c| c.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);

        // Ollama doesn't have reasoning_content
        let reasoning_content = None;

        // Handle tool_calls - Ollama uses message.tool_calls
        let tool_calls = message
            .get("tool_calls")
            .and_then(|tc| tc.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        // Ollama has index field in tool_calls
                        let index = tc.get("index")?.as_u64()? as usize;
                        let id = tc.get("id").and_then(|v| v.as_str()).map(String::from);
                        let function = tc.get("function")?;
                        let name = function
                            .get("name")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let arguments = function.get("arguments").and_then(|v| {
                            // Arguments can be a string or already-parsed object in Ollama
                            match v {
                                serde_json::Value::String(s) => Some(s.clone()),
                                serde_json::Value::Object(_) => {
                                    Some(serde_json::to_string(v).ok()?)
                                }
                                _ => None,
                            }
                        });

                        Some(DeltaToolCall {
                            index,
                            id,
                            function: DeltaFunction { name, arguments },
                        })
                    })
                    .collect()
            });

        Ok(Some(DeltaResponse {
            content,
            reasoning_content,
            tool_calls,
        }))
    }

    /// Parse the final response
    fn parse_response(
        &self,
        response: &str,
    ) -> Result<(Option<String>, Option<String>, Option<Vec<ToolCallMessage>>), AgentError> {
        let json: Value = serde_json::from_str(response)
            .map_err(|e| AgentError::InvalidResponse(format!("JSON parse error: {}", e)))?;

        // DeepSeek puts Chinese text in content, English thinking in reasoning_content
        let content = json
            .get("content")
            .and_then(|c| c.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);

        let reasoning_content = json
            .get("reasoning_content")
            .and_then(|c| c.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);

        let tool_calls = json
            .get("tool_calls")
            .and_then(|tc| tc.as_array())
            .map(|arr| {
                let mut parsed: Vec<ToolCallMessage> = Vec::with_capacity(arr.len());
                for (index, tc) in arr.iter().enumerate() {
                    match parse_tool_call_message(tc) {
                        Ok(message) => parsed.push(message),
                        Err(reason) => {
                            // Surface the drop so the caller doesn't silently
                            // lose a tool invocation. We log at warn rather
                            // than returning an error because the response
                            // is otherwise well-formed and the agent loop
                            // has to continue with whatever did parse.
                            tracing::warn!(
                                "Dropping malformed tool_call #{index} in parse_response: {reason}"
                            );
                        }
                    }
                }
                parsed
            });

        Ok((content, reasoning_content, tool_calls))
    }
}

/// Strictly parse a `tool_call` payload into our internal [`ToolCallMessage`]
/// representation. Any missing or wrong-typed field is treated as a hard
/// failure (we never silently coerce), so the caller can decide whether the
/// error is recoverable.

/// Create a new agent executor
pub fn create_agent_executor(config: AIConfig) -> AgentExecutor {
    AgentExecutor::new(config)
}

// Prompts are re-exported from the prompts module
pub use super::prompts::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tools::ToolRegistry;
    use crate::agent::ImageDetail;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn session() -> AgentSession {
        AgentSession::new(Arc::new(RwLock::new(ToolRegistry::new())))
    }

    fn image(name: &str) -> ImageAttachment {
        ImageAttachment {
            mime_type: "image/png".to_string(),
            data_base64: "iVBORw0KGgo=".to_string(),
            detail: ImageDetail::High,
            name: Some(name.to_string()),
            byte_len: 8,
        }
    }

    #[tokio::test]
    async fn shared_registry_uses_each_sessions_immutable_workspace() {
        let root =
            std::env::temp_dir().join(format!("inkuo_session_workspace_{}", uuid::Uuid::new_v4()));
        let workspace_a = root.join("a");
        let workspace_b = root.join("b");
        std::fs::create_dir_all(&workspace_a).unwrap();
        std::fs::create_dir_all(&workspace_b).unwrap();
        let file_a = workspace_a.join("private.txt");
        std::fs::write(&file_a, "workspace-a").unwrap();

        let registry = Arc::new(RwLock::new(ToolRegistry::new()));
        let session_a = AgentSession::new(registry.clone())
            .with_workspace(Some(workspace_a.to_string_lossy().to_string()));
        let session_b = AgentSession::new(registry.clone())
            .with_workspace(Some(workspace_b.to_string_lossy().to_string()));
        let call = ToolCall {
            id: "read-a".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": file_a}),
        };

        let allowed = registry
            .read()
            .await
            .execute_in_workspace(&call, session_a.workspace())
            .await;
        assert!(!allowed.is_error);
        assert_eq!(allowed.output, "workspace-a");

        let denied = registry
            .read()
            .await
            .execute_in_workspace(&call, session_b.workspace())
            .await;
        assert!(denied.is_error);
        assert!(denied.output.contains("outside the workspace"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn first_turn_without_workspace_cannot_read_absolute_files() {
        let root =
            std::env::temp_dir().join(format!("inkuo_no_workspace_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let secret = root.join("secret.txt");
        std::fs::write(&secret, "must-not-read").unwrap();
        let registry = Arc::new(RwLock::new(ToolRegistry::new()));
        let session = AgentSession::new(registry.clone());
        let call = ToolCall {
            id: "read-without-workspace".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": secret}),
        };

        let denied = registry
            .read()
            .await
            .execute_in_workspace(&call, session.workspace())
            .await;
        assert!(denied.is_error);
        assert!(denied
            .output
            .contains("requires a non-empty active workspace"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hidden_tools_are_rejected_at_execution_time() {
        let sandbox_off = session().with_allowed_tools(Some(vec!["read_file".to_string()]));
        let sandbox_call = ToolCall {
            id: "sandbox-hidden".to_string(),
            name: "run_sandbox_command".to_string(),
            arguments: serde_json::json!({}),
        };
        let denied = sandbox_off
            .tool_authorization_error(&sandbox_call)
            .expect("sandbox-off tool call must be denied");
        assert!(denied.is_error);
        assert!(denied.output.contains("disabled for this session"));

        let kb_strict = session().with_allowed_tools(Some(vec!["database_search".to_string()]));
        let write_call = ToolCall {
            id: "write-hidden".to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({}),
        };
        assert!(kb_strict.tool_authorization_error(&write_call).is_some());
    }

    #[tokio::test]
    async fn empty_allowlist_advertises_no_tools() {
        let restricted = session().with_allowed_tools(Some(Vec::new()));
        assert!(restricted
            .tool_definitions_for_api(Some(&[]))
            .await
            .is_empty());
    }

    #[test]
    fn live_todo_is_injected_as_system_state_and_advances() {
        let mut session = session();
        session.add_message(Message::system("base"));
        session
            .apply_todo_arguments(&serde_json::json!({
                "action": "set",
                "items": ["Inspect source", "Write fix"]
            }))
            .unwrap();
        let provider = AIProvider::OpenAI {
            api_key: "test".to_string(),
            base_url: "https://example.invalid".to_string(),
        };
        let messages = session.get_messages_for_api(&provider);
        let system = messages[0]["content"].as_str().unwrap();
        assert!(system.contains("[in_progress] Inspect source"));
        assert!(system.contains("[pending] Write fix"));

        session
            .apply_todo_arguments(&serde_json::json!({"action": "advance"}))
            .unwrap();
        let messages = session.get_messages_for_api(&provider);
        let system = messages[0]["content"].as_str().unwrap();
        assert!(system.contains("[completed] Inspect source"));
        assert!(system.contains("[in_progress] Write fix"));
    }

    #[test]
    fn multimodal_serialization_adapts_openai_and_ollama() {
        let mut session = session();
        session.add_message(Message::user_with_images(
            "inspect",
            vec![image("page.png")],
        ));
        let openai = AIProvider::OpenAI {
            api_key: "test".to_string(),
            base_url: "https://example.invalid".to_string(),
        };
        let openai_messages = session.get_messages_for_api(&openai);
        assert_eq!(openai_messages[0]["content"][1]["type"], "image_url");
        assert!(openai_messages[0]["content"][1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));

        let ollama = AIProvider::Ollama {
            base_url: "http://localhost:11434".to_string(),
        };
        let ollama_messages = session.get_messages_for_api(&ollama);
        assert_eq!(ollama_messages[0]["content"], "inspect");
        assert_eq!(ollama_messages[0]["images"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn visual_inspection_is_ordered_after_all_tool_results_and_sent_once() {
        let mut session = session();
        session.add_message(Message::assistant(
            None,
            None,
            Some(vec![
                ToolCallMessage {
                    id: "call-1".to_string(),
                    call_type: "function".to_string(),
                    function: ToolCallFunction {
                        name: "read_image".to_string(),
                        arguments: "{}".to_string(),
                    },
                },
                ToolCallMessage {
                    id: "call-2".to_string(),
                    call_type: "function".to_string(),
                    function: ToolCallFunction {
                        name: "read_file".to_string(),
                        arguments: "{}".to_string(),
                    },
                },
            ]),
        ));
        session.add_message(Message::tool_result("call-1", "asset ready"));
        session.add_message(Message::tool_result("call-2", "text ready"));
        session
            .enqueue_visual_inspections(vec![VisualInspectionInput {
                source_tool_call_id: "call-1".to_string(),
                asset_id: "asset-1".to_string(),
                attachment: image("preview.png"),
            }])
            .unwrap();

        let provider = AIProvider::OpenAI {
            api_key: "test".to_string(),
            base_url: "https://example.invalid".to_string(),
        };
        let messages = session.get_messages_for_api(&provider);
        let roles: Vec<&str> = messages
            .iter()
            .map(|message| message["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, vec!["assistant", "tool", "tool", "user"]);
        assert!(messages[3]["content"].is_array());

        session.consume_one_shot_images();
        let next = session.get_messages_for_api(&provider);
        assert!(next[3]["content"].is_string());
        assert!(next[3]["content"]
            .as_str()
            .unwrap()
            .contains("not being retransmitted"));
    }

    #[test]
    fn office_preview_tool_output_reaches_the_next_multimodal_request() {
        use std::time::Instant;

        let _registry_guard = crate::agent::tools::asset_registry::test_registry_guard();
        crate::agent::tools::asset_registry::clear();
        let workspace = std::env::temp_dir().join(format!(
            "inkuo_office_visual_bridge_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workspace).unwrap();
        let canonical_workspace = std::fs::canonicalize(&workspace).unwrap();
        let asset_id = crate::agent::tools::asset_registry::fresh_id();
        crate::agent::tools::asset_registry::insert(
            asset_id.clone(),
            crate::agent::tools::asset_registry::AssetEntry {
                mime: "image/png".to_string(),
                ext: "png".to_string(),
                data: b"\x89PNG\r\n\x1a\npreview".to_vec(),
                inserted_at: Instant::now(),
                source_path: "deck.pptx#page=1".to_string(),
                workspace_root: canonical_workspace.to_string_lossy().to_string(),
            },
        );

        let output = serde_json::json!({
            "visual_assets": [{"asset_id": asset_id, "page_number": 1}]
        })
        .to_string();
        let inputs = visual_inspections_from_tool_output(
            "render-call",
            "render_office_preview",
            &output,
            Some(workspace.to_string_lossy().as_ref()),
        )
        .unwrap();
        let mut session = session().with_workspace(Some(workspace.to_string_lossy().to_string()));
        session.add_message(Message::assistant(
            None,
            None,
            Some(vec![ToolCallMessage {
                id: "render-call".to_string(),
                call_type: "function".to_string(),
                function: ToolCallFunction {
                    name: "render_office_preview".to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
        ));
        session.add_message(Message::tool_result("render-call", &output));
        session.enqueue_visual_inspections(inputs).unwrap();

        let provider = AIProvider::OpenAI {
            api_key: "test".to_string(),
            base_url: "https://example.invalid".to_string(),
        };
        let messages = session.get_messages_for_api(&provider);
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"][1]["type"], "image_url");

        crate::agent::tools::asset_registry::clear();
        let _ = std::fs::remove_dir_all(workspace);
    }
}
