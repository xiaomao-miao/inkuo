//! Agent loop - Core execution engine for tool calling
//!
//! This module implements the agent execution loop:
//! 1. Send request to AI with tools schema
//! 2. Receive AI response (may contain tool_calls)
//! 3. Execute tools and collect results
//! 4. Continue loop until final response or max iterations
//!
//! The loop follows this pattern:
//! ```
//! request → AI → [tool_calls?] → execute → AI → [tool_calls?] → ... → final
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;
use futures_util::StreamExt;
use std::collections::HashMap;

use super::profile::AgentProfile;
use super::tools::{ToolCall, ToolResult, SharedToolRegistry};
// `find_tool_spec` is reached via the `pub use super::prompts::*` glob below
// — don't re-import it explicitly or we'll trigger the hidden_glob_reexports
// lint. Same goes for `list_profiles` and `resolve_profile`.
use crate::ai::{AIConfig, AIProvider};
use crate::diff;
use crate::streaming::{StreamPayload, FileDiffSummary, StreamDiffHunk, StreamDiffChange, OfficeFileModified, PlanResultData, PlanFileTouchItem, AskUserStreamPayload};
use crate::agent::tools::ask_user_tools::{register_pending, cancel_pending};

use crate::agent::agent_helpers::{
    chrono_from_timestamp, generate_plan_id_for_session, is_leap_year, parse_tool_call_message,
    save_plan_to_workspace, DeltaFunction, DeltaResponse, DeltaToolCall,
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
    User { content: String },
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
        Self::User { content: content.into() }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::System { content: content.into() }
    }

    pub fn assistant(content: Option<String>, reasoning_content: Option<String>, tool_calls: Option<Vec<ToolCallMessage>>) -> Self {
        Self::Assistant { content, reasoning_content, tool_calls }
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

/// Agent state for a single conversation
pub struct AgentSession {
    pub messages: Vec<Message>,
    pub max_iterations: usize,
    pub tool_registry: SharedToolRegistry,
    /// Optional tool whitelist. When `Some`, only tools whose name is in
    /// this list are advertised to the LLM in `tool_definitions_for_api`.
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
}

impl AgentSession {
    pub fn new(tool_registry: SharedToolRegistry) -> Self {
        Self {
            messages: Vec::new(),
            max_iterations: DEFAULT_MAX_ITERATIONS,
            tool_registry,
            allowed_tools: None,
            expert_max_iterations: HashMap::new(),
        }
    }

    /// Restrict the tools advertised to the LLM. Does not change which
    /// tools the registry will *execute* — that's a separate concern.
    /// Pass `None` (or use `new`) to advertise everything.
    pub fn with_allowed_tools(mut self, allowed: Option<Vec<String>>) -> Self {
        self.allowed_tools = allowed;
        self
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
            allowed_tools: if profile.allowed_tools.is_empty() {
                None
            } else {
                Some(profile.allowed_tools)
            },
            expert_max_iterations: HashMap::new(),
        }
    }

    /// Returns the tool definitions that should be advertised to the LLM
    /// for this session. For a profile-driven session the caller passes the
    /// profile's `allowed_tools` here; for legacy sessions, pass `None`.
    pub async fn tool_definitions_for_api(&self, allowed_tools: Option<&[String]>) -> Vec<Value> {
        let registry = self.tool_registry.read().await;
        let defs = match allowed_tools {
            Some(list) if !list.is_empty() => registry.filtered_definitions(list),
            _ => registry.get_all_definitions(),
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

    pub fn get_messages_for_api(&self) -> Vec<serde_json::Value> {
        self.messages
            .iter()
            .map(|msg| match msg {
                Message::System { content } => serde_json::json!({
                    "role": "system",
                    "content": content
                }),
                Message::User { content } => serde_json::json!({
                    "role": "user",
                    "content": content
                }),
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
            .collect()
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
        tracing::debug!("AgentExecutor::run started - session_id: {}, message_id: {}", session_id, message_id);

        // Add user message
        session.add_message(Message::user(user_request));

        // Get tool definitions for API call. When the session carries an
        // explicit `allowed_tools` whitelist (set by feature toggles like
        // strict-KB), use that; otherwise expose the whole registry.
        let tools = session.tool_definitions_for_api(session.allowed_tools.as_deref()).await;
        let tools_json: Vec<Value> = tools
            .into_iter()
            .map(|t| serde_json::to_value(&t).map_err(|e| AgentError::AIError(format!("Tool serialization failed: {}", e))))
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
            let messages = session.get_messages_for_api();

            // Make API call with tools
            let response = self
                .call_ai_with_tools(&messages, &tools_json, session_id, message_id, on_event.clone())
                .await?;

            // Parse response
            let (content, reasoning_content, tool_calls) = self.parse_response(&response)?;

            // Add assistant message to history (with reasoning_content for DeepSeek)
            session.add_message(Message::assistant(content.clone(), reasoning_content.clone(), tool_calls.clone()));

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
            for parsed in &parsed_calls {
                let tool_call = ToolCall {
                    id: parsed.id.clone(),
                    name: parsed.name.clone(),
                    arguments: parsed.arguments.clone(),
                };

                // Meta-tool short-circuit: `get_tool_help` and `delegate_to`
                // are handled by the executor itself, not the registry.
                let mut result = match self
                    .try_handle_meta_tool(
                        &tool_call,
                        session,
                        session_id,
                        message_id,
                        on_event.clone(),
                    )
                    .await
                {
                    Some(r) => r,
                    None => session.tool_registry.read().await.execute(&tool_call).await,
                };

                // Inject the correct tool_call_id from streamed data (not the placeholder)
                result.tool_call_id = parsed.id.clone();

                // Compute diff summary for file modification tools
                // Only compute if we have original content; new content will be read lazily if needed
                let diff_summary: Option<FileDiffSummary> = if let (Some(file_path), Some(original)) = (
                    &result.file_path,
                    &result.original_content,
                ) {
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

                        let hunks = diff_result.hunks.into_iter().map(|h| StreamDiffHunk {
                            id: h.id,
                            old_start: h.old_range.start_line,
                            old_lines: h.old_range.end_line.saturating_sub(h.old_range.start_line) + 1,
                            new_start: h.new_range.start_line,
                            new_lines: h.new_range.end_line.saturating_sub(h.new_range.start_line) + 1,
                            changes: h.changes.into_iter().map(|c| StreamDiffChange {
                                tag: match c.tag {
                                    diff::ChangeType::Delete => "delete".to_string(),
                                    diff::ChangeType::Insert => "insert".to_string(),
                                    diff::ChangeType::Equal => "equal".to_string(),
                                },
                                old_line: c.old_line,
                                new_line: c.new_line,
                                content: c.content,
                            }).collect(),
                        }).collect();

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
                let office_file_modified: Option<OfficeFileModified> = if !result.is_error
                    && parsed.name == "create_word_doc"
                {
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
                    error: if result.is_error { Some(result.output.clone()) } else { None },
                    search_results: None,
                    done: false,
                    // Diff info for file modification tools
                    file_path: result.file_path.clone(),
                    original_content: result.original_content.clone(),
                    new_content: result.new_content.clone(),
                    diff_summary,
                    office_file_modified,
                    plan_result: None,
                    ask_user: None,
                });

                // Add tool result to message history
                session.add_message(Message::tool_result(&parsed.id, &result.output));
            }

            // `create_plan` is a terminal tool — once the plan has been
            // saved and the `plan_result` event emitted, there is nothing
            // more for the model to do.  Break out of the loop immediately
            // instead of sending the tool results back for another round
            // (which would cause the model to produce a redundant
            // "I've created a plan for you..." text block).
            if parsed_calls.iter().any(|p| p.name == "create_plan") {
                return Ok(String::new());
            }
        }

        Err(AgentError::MaxIterationsReached(session.max_iterations))
    }

    /// Intercept meta-tools (`get_tool_help`, `delegate_to`) so the loop
    /// doesn't try to dispatch them via the registry.
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
                        Some(text) => Some(ToolResult::success(
                            &tool_call.id,
                            text.to_string(),
                        )),
                        None => {
                            let available = ["general", "word", "excel", "markdown"]
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
                        Err(e) => Some(ToolResult::error(&tool_call.id, format!("[{}] {}", expert, e))),
                    }
                }
                "update_todo" => {
                    // The TodoList is rendered to the user via the normal
                    // tool-call stream, so the ToolResult we hand back to
                    // the LLM just needs to confirm the update landed —
                    // but we DO want to surface the action the model
                    // chose, because (a) it tells the model that the
                    // call was accepted as the right action type, and
                    // (b) the model's "did I just do this?" loop
                    // benefits from explicit confirmation rather than a
                    // generic "OK" string it has to correlate by
                    // toolCallId alone.
                    let action = tool_call
                        .arguments
                        .get("action")
                        .and_then(|v| v.as_str())
                        .unwrap_or("set"); // v1 callers didn't pass `action`; treat as `set`.
                    let items_arr = tool_call
                        .arguments
                        .get("items")
                        .and_then(|v| v.as_array());
                    let count = items_arr.map(|a| a.len()).unwrap_or(0);
                    let summary = match action {
                        "set" => {
                            if count == 0 {
                                "Todo list cleared.".to_string()
                            } else {
                                format!(
                                    "Todo list set ({} items). Step 1 marked in_progress — call action='advance' after finishing it.",
                                    count
                                )
                            }
                        }
                        "advance" => {
                            "Current step marked completed; next pending step promoted to in_progress.".to_string()
                        }
                        "complete_current" => {
                            "Current step marked completed (no promotion).".to_string()
                        }
                        // Unknown action: the frontend will reject it,
                        // but at least tell the model what we saw so
                        // the next attempt can self-correct.
                        other => {
                            format!("Ignored unknown action '{}'.", other)
                        }
                    };
                    Some(ToolResult::success(&tool_call.id, summary))
                }
                "create_plan" => {
                    // Plan mode: parse the plan JSON, write it to disk,
                    // and emit a `plan_result` stream event so the frontend
                    // can render the PlanCard immediately (without needing
                    // to parse a ```plan fence from streaming text).
                    let args = tool_call
                        .arguments
                        .clone();
                    let content = args
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let plan_summary = args
                        .get("plan_summary")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let risk = args
                        .get("risk")
                        .and_then(|v| v.as_str())
                        .unwrap_or("low")
                        .to_string();
                    let risk_reason = args
                        .get("risk_reason")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    let files_to_touch_raw = args
                        .get("files_to_touch")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let files_to_touch: Vec<PlanFileTouchItem> = files_to_touch_raw
                        .iter()
                        .filter_map(|v| {
                            serde_json::from_value::<PlanFileTouchItem>(v.clone()).ok()
                        })
                        .collect();

                    let workspace = session.tool_registry.read().await.get_workspace().cloned();

                    // Build the Markdown content to write to disk.
                    let file_list_md = if files_to_touch.is_empty() {
                        String::new()
                    } else {
                        files_to_touch
                            .iter()
                            .map(|f| format!("- [{}] {}: {}", f.intent, f.path, f.reason))
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    let risk_md = risk_reason
                        .as_ref()
                        .map(|r| format!("**Risk**: {}\n", r))
                        .unwrap_or_default();
                    let plan_md = format!(
                        "# Plan\n\n{}\n\n## Summary\n{}\n\n## Files\n{}\n\n## Risk\n- Level: **{}**\n{}",
                        content,
                        plan_summary,
                        file_list_md,
                        risk,
                        risk_md
                    );

                    // Generate plan id and save.
                    let plan_id = generate_plan_id_for_session();
                    let saved_path = match &workspace {
                        Some(ws) => {
                            match save_plan_to_workspace(ws, &plan_id, &plan_md).await {
                                Ok(path) => path,
                                Err(e) => {
                                    tracing::error!("create_plan: failed to save plan: {}", e);
                                    return Some(ToolResult::error(
                                        &tool_call.id,
                                        format!("Failed to save plan: {}", e),
                                    ));
                                }
                            }
                        }
                        None => {
                            return Some(ToolResult::error(
                                &tool_call.id,
                                "No workspace open. Cannot save plan file.".to_string(),
                            ));
                        }
                    };

                    // Emit the plan_result event so the frontend renders the PlanCard.
                    let plan_data = PlanResultData {
                        content,
                        plan_summary,
                        files_to_touch,
                        risk,
                        risk_reason,
                        saved_path: saved_path.clone(),
                    };
                    on_event(StreamPayload::plan_result(
                        session_id,
                        message_id,
                        &tool_call.id,
                        plan_data,
                    ));

                    Some(ToolResult::success(
                        &tool_call.id,
                        format!("Plan saved to {}", saved_path),
                    ))
                }
                "ask_user" => {
                    let args = &tool_call.arguments;
                    let question = args
                        .get("question")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let options: Vec<String> = args
                        .get("options")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    let allow_custom = args
                        .get("allow_custom")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);

                    // Register a oneshot channel so the frontend can wake us up.
                    let rx = register_pending(&tool_call.id);

                    // Emit the ask_user stream event so the frontend renders the card.
                    on_event(StreamPayload::ask_user(
                        session_id,
                        message_id,
                        &tool_call.id,
                        AskUserStreamPayload {
                            question: question.clone(),
                            options: options.clone(),
                            allow_custom,
                        },
                    ));

                    // Suspend until the user responds or the session is cancelled.
                    // Use `&mut rx` so the oneshot receiver is not consumed on
                    // the first poll — the sleep branch may fire many times
                    // before the user answers, and we need rx to remain valid.
                    let mut rx = rx;
                    let answer = loop {
                        tokio::select! {
                            biased;
                            _ = tokio::time::sleep(tokio::time::Duration::from_millis(250)) => {
                                if is_session_cancelled(session_id) {
                                    clear_cancellation(session_id);
                                    cancel_pending(&tool_call.id);
                                    return Some(ToolResult::error(
                                        &tool_call.id,
                                        "ask_user cancelled by user",
                                    ));
                                }
                            }
                            result = &mut rx => {
                                match result {
                                    Ok(ans) => break ans,
                                    Err(_) => {
                                        return Some(ToolResult::error(
                                            &tool_call.id,
                                            "ask_user cancelled: sender dropped",
                                        ));
                                    }
                                }
                            }
                        }
                    };

                    // Safety: make sure no stale entry is left around.
                    cancel_pending(&tool_call.id);

                    Some(ToolResult::success(&tool_call.id, answer))
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
            let mut sub_session = AgentSession::new_with_profile(profile.clone(), registry);

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
                .run(&mut sub_session, &task_message, session_id, &sub_message_id, on_event.clone())
                .await;

            // Notify the frontend that the sub-agent has finished.
            on_event(StreamPayload::subagent_end(session_id, parent_message_id, &sub_message_id));

            match summary {
                Ok(s) => Ok(format!(
                    "[{} completed]\n\n{}",
                    profile.label,
                    s
                )),
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

        tracing::info!("Sending request to {} with {} messages", url, messages.len());

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
            return Err(AgentError::AIError(format!(
                "HTTP {}: {}",
                status, body
            )));
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
        let mut tool_call_started: std::collections::HashSet<usize> = std::collections::HashSet::new();

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
            while let Some((event, rest)) =
                crate::openai_stream::take_next_sse_event(&buffer)
            {
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
                                    plan_result: None,
                                    ask_user: None,
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
                                        plan_result: None,
                                        ask_user: None,
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
                                    let arg_delta: Option<String> = if let Some(args) = &tc.function.arguments {
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
                                            plan_result: None,
                                            ask_user: None,
                                        });
                                    } else if id_updated || name_updated || arg_delta.is_some() {
                                        // Subsequent chunk for the same tool call index.
                                        // Throttle the emission so we don't flood the IPC
                                        // channel with 10000-char payloads at SSE rate.
                                        let now = std::time::Instant::now();
                                        let should_emit = tool_args_has_pending.contains(&tc.index)
                                            || now.duration_since(
                                                last_tool_args_emit
                                                    .get(&tc.index)
                                                    .copied()
                                                    .unwrap_or(now),
                                            ).as_millis() >= TOOL_ARGS_EMIT_INTERVAL_MS;
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
                                                plan_result: None,
                                                ask_user: None,
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
                    plan_result: None,
                    ask_user: None,
                });
            }
        }
        tool_args_has_pending.clear();

        tracing::debug!("Stream processing complete. bytes_received: {}, current_content_len: {}", bytes_received, current_content.len());

        // Debug: log the final tool calls
        for (i, tc) in current_tool_calls.iter().enumerate() {
            tracing::debug!("[TOOL_CALL_DEBUG] #{:02}: id='{}', name='{}', args='{}'",
                i, tc.id, tc.function.name, tc.function.arguments);
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
    fn parse_sse_delta(&self, data: &str, is_ollama: bool) -> Result<Option<DeltaResponse>, String> {
        let json: Value = serde_json::from_str(data)
            .map_err(|e| format!("JSON parse error: {}", e))?;

        if is_ollama {
            // Ollama format: data.message.tool_calls
            return self.parse_ollama_delta(&json);
        }

        // OpenAI format: data.choices[0].delta
        let delta = match json.get("choices") {
            Some(choices) if choices.is_array() => {
                choices.get(0).and_then(|c| c.get("delta"))
            }
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
                        let id = tc
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(String::from);
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
                        let id = tc
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let function = tc.get("function")?;
                        let name = function
                            .get("name")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let arguments = function
                            .get("arguments")
                            .and_then(|v| {
                                // Arguments can be a string or already-parsed object in Ollama
                                match v {
                                    serde_json::Value::String(s) => Some(s.clone()),
                                    serde_json::Value::Object(_) => Some(serde_json::to_string(v).ok()?),
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
    fn parse_response(&self, response: &str) -> Result<(Option<String>, Option<String>, Option<Vec<ToolCallMessage>>), AgentError> {
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
