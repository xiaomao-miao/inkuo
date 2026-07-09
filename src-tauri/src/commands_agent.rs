use crate::agent::{
    create_agent_executor,
    get_agent_system_prompt, get_ask_system_prompt, get_plan_system_prompt, list_profiles,
    AgentError, AgentSession, Message, SharedToolRegistry, ToolCallFunction, ToolCallMessage,
};
use crate::agent::tools::ToolRegistry;
use crate::ai_config::{self, AIConfigInput};
use crate::feature_toggles::{self, ToggleId};
use crate::runtime_state::{self, Mode};
use crate::streaming::{emit, StreamPayload};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, Error)]
pub enum AgentCommandError {
    #[error("Invalid frontend tool call arguments for '{tool_name}': {reason}")]
    InvalidFrontendToolArguments { tool_name: String, reason: String },
    #[error("Tool result message is missing tool_call_id")]
    MissingToolCallId,
    #[error("Failed to serialize tool definitions: {0}")]
    ToolDefinitionsSerialization(String),
    #[error("Invalid AI configuration: {0}")]
    InvalidAIConfig(String),
    #[error("Unknown feature toggle id: {0}")]
    UnknownFeatureToggle(String),
}

/// Shared tool registries for agent - separate for full and read-only modes
pub static FULL_TOOL_REGISTRY: std::sync::OnceLock<SharedToolRegistry> =
    std::sync::OnceLock::new();
pub static READ_ONLY_TOOL_REGISTRY: std::sync::OnceLock<SharedToolRegistry> =
    std::sync::OnceLock::new();

async fn get_full_tool_registry(app: &AppHandle) -> SharedToolRegistry {
    let registry = FULL_TOOL_REGISTRY
        .get_or_init(|| Arc::new(RwLock::new(ToolRegistry::new())))
        .clone();

    // Lazily attach the AppHandle so the database_search tool can issue IPC
    // calls. We previously used a `OnceLock` here but the check-then-act
    // pattern was racy (two callers could both observe the flag as unset
    // and both grab the write lock). The write guard itself plus the inner
    // `has_tool` check are sufficient to make initialisation happen
    // exactly once; the `OnceLock` only existed to short-circuit the read
    // guard after the first call, which is a micro-optimisation we don't
    // need. Subsequent callers will take the read lock and see
    // `database_search` is already present.
    {
        let reg = registry.read().await;
        if !reg.has_tool("database_search") {
            drop(reg);
            let mut reg = registry.write().await;
            if !reg.has_tool("database_search") {
                reg.set_app_handle(app.clone());
            }
        }
    }

    registry
}

async fn get_read_only_tool_registry(_app: &AppHandle) -> SharedToolRegistry {
    READ_ONLY_TOOL_REGISTRY
        .get_or_init(|| Arc::new(RwLock::new(ToolRegistry::new_read_only())))
        .clone()
}

/// Update the workspace path for both tool registries. `set_workspace` is
/// synchronous, so holding the write lock between the two updates does not
/// yield — the two registries are therefore always updated back-to-back
/// without an interleaving point, which is the property we want (so the
/// full and read-only registries never disagree about the active workspace).
async fn update_registry_workspace(workspace_path: Option<String>) {
    if let Some(registry) = FULL_TOOL_REGISTRY.get() {
        let mut registry = registry.write().await;
        registry.set_workspace(workspace_path.clone());
    }
    if let Some(registry) = READ_ONLY_TOOL_REGISTRY.get() {
        let mut registry = registry.write().await;
        registry.set_workspace(workspace_path);
    }
}

/// Message from frontend history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<Vec<FrontendToolCall>>,
    pub tool_call_id: Option<String>,
}

/// Tool call from frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontendToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

fn convert_tool_call(tc: &FrontendToolCall) -> Result<ToolCallMessage, AgentCommandError> {
    let arguments = serde_json::to_string(&tc.arguments).map_err(|error| {
        AgentCommandError::InvalidFrontendToolArguments {
            tool_name: tc.name.clone(),
            reason: error.to_string(),
        }
    })?;

    Ok(ToolCallMessage {
        id: tc.id.clone(),
        call_type: "function".to_string(),
        function: ToolCallFunction {
            name: tc.name.clone(),
            arguments,
        },
    })
}

/// Convert frontend message to agent message
fn convert_message(msg: &FrontendMessage) -> Result<Option<Message>, AgentCommandError> {
    match msg.role.as_str() {
        "system" => Ok(Some(Message::System {
            content: msg.content.clone(),
        })),
        "user" => Ok(Some(Message::User {
            content: msg.content.clone(),
        })),
        "assistant" => {
            let tool_calls = msg
                .tool_calls
                .as_ref()
                .map(|tcs| tcs.iter().map(convert_tool_call).collect())
                .transpose()?;
            Ok(Some(Message::Assistant {
                content: Some(msg.content.clone()),
                reasoning_content: None,
                tool_calls,
            }))
        }
        "tool" => {
            let tool_call_id = msg
                .tool_call_id
                .clone()
                .filter(|id| !id.trim().is_empty())
                .ok_or(AgentCommandError::MissingToolCallId)?;
            Ok(Some(Message::Tool {
                tool_call_id,
                content: msg.content.clone(),
            }))
        }
        _ => Ok(None),
    }
}

/// Hard cap on the number of LLM ↔ tool loops the Agent will perform before
/// giving up with a `MaxIterationsReached` error. `None` falls back to the
/// session default (currently 50). Surfaced from the frontend's settings
/// panel so power users can tune it without recompiling.
///
/// `expert_max_iterations` is an optional map of sub-agent profile name
/// (e.g. `"office_excel_expert"`) to per-expert iteration cap. When the
/// main agent dispatches to a sub-agent via `delegate_to`, the value (if
/// any) overrides the compile-time default in the profile. Missing keys
/// fall back to the profile's default. Values are clamped to `[1, 200]`.
/// Both shapes are exposed from the frontend's settings panel under
/// "AI → Agent 执行". See `AgentSession::with_expert_max_iterations`.
#[tauri::command]
pub async fn ai_agent_stream(
    session_id: String,
    message_id: String,
    instruction: String,
    workspace_path: Option<String>,
    mode: String,
    history: Vec<FrontendMessage>,
    max_iterations: Option<usize>,
    expert_max_iterations: Option<HashMap<String, usize>>,
    enabled_toggles: Option<Vec<String>>,
    config_input: AIConfigInput,
    app: AppHandle,
) -> Result<(), AgentCommandError> {
    tracing::info!("ai_agent_stream start - session: {}, history length: {}", session_id, history.len());

    // Update workspace path for tool validation
    update_registry_workspace(workspace_path.clone()).await;

    // Create AI config from input
    let ai_config = ai_config::build_input_ai_config(config_input)
        .map_err(|error| AgentCommandError::InvalidAIConfig(error.to_string()))?;

    tracing::info!("Using AI provider: {:?}", ai_config.provider);

    let executor = create_agent_executor(ai_config);

    // Use different tool registry and system prompt based on mode.
    //
    // The mode string is parsed into a typed `Mode` here so the rest of
    // the function can refer to it without re-matching. Unknown modes
    // fall back to Agent (and get logged), but the typed `Mode::Agent`
    // value still drives the runtime-state fragment so the LLM sees a
    // consistent declaration.
    let parsed_mode = Mode::from_str(&mode).unwrap_or_else(|| {
        tracing::warn!("Unknown mode '{}', defaulting to agent", mode);
        Mode::Agent
    });

    let (tool_registry, mut system_prompt) = match parsed_mode {
        Mode::Plan => {
            let registry = get_read_only_tool_registry(&app).await;
            let prompt = get_plan_system_prompt();
            (registry, prompt)
        }
        Mode::Ask => {
            let registry = get_read_only_tool_registry(&app).await;
            let prompt = get_ask_system_prompt();
            (registry, prompt)
        }
        Mode::Agent => {
            let registry = get_full_tool_registry(&app).await;
            let prompt = get_agent_system_prompt();
            (registry, prompt)
        }
    };

    // Parse the frontend toggle list. Unknown ids are an explicit error —
    // they imply a desync between `src/types/index.ts` and the Rust
    // registry, and silently dropping them would mask bugs.
    let enabled_toggles_raw = enabled_toggles.unwrap_or_default();
    let mut parsed_toggles: Vec<ToggleId> = Vec::with_capacity(enabled_toggles_raw.len());
    for raw in &enabled_toggles_raw {
        match ToggleId::from_str(raw) {
            Some(id) => parsed_toggles.push(id),
            None => return Err(AgentCommandError::UnknownFeatureToggle(raw.clone())),
        }
    }

    // Compute the effective tool allowlist. We ALWAYS run the base set
    // through `effective_tool_set` — never skip on an empty toggle list
    // — because individual toggles have *opposite* default semantics:
    // `kb_strict` is opt-in to restrict, while `web_search` is opt-in
    // to expose. If we returned `None` here whenever the user enabled
    // no toggles, an opt-out tool like `web_search` would always be
    // visible (which is the bug this guard used to have).
    let allowed_tools: Option<Vec<String>> = {
        let registry = tool_registry.read().await;
        let base = registry.tool_names();
        Some(feature_toggles::effective_tool_set(&base, &parsed_toggles))
    };

    let mut session = AgentSession::new(tool_registry).with_allowed_tools(allowed_tools);
    if let Some(n) = max_iterations {
        session = session.with_max_iterations(n);
    }

    // Sanitise per-expert iteration overrides:
    //  - Drop keys for unknown profiles (defence against frontend/backend
    //    drift — a typo in the settings UI shouldn't silently affect
    //    nothing and look like a success).
    //  - Clamp values to [1, 200] (the same range the frontend uses for
    //    the slider, kept in sync deliberately).
    let known_profiles: std::collections::HashSet<&'static str> =
        list_profiles().into_iter().map(|(n, _)| n).collect();
    let sanitised_expert_overrides: HashMap<String, usize> = expert_max_iterations
        .unwrap_or_default()
        .into_iter()
        .filter(|(name, _)| known_profiles.contains(name.as_str()))
        .map(|(name, n)| (name, n.clamp(1, 200)))
        .collect();
    if !sanitised_expert_overrides.is_empty() {
        session = session.with_expert_max_iterations(sanitised_expert_overrides);
    }

    // Append the runtime-state fragment next. It is the authoritative
    // declaration of the active mode and toggles for THIS turn, so we
    // place it after the mode's base prompt (static doc) but before the
    // toggle usage guidance (which only matters when the toggle is on).
    // The order in the final system prompt is therefore:
    //   1. mode base prompt (static, mode-bound)
    //   2. runtime state (this turn's truth)
    //   3. toggle inventory + usage guidance
    //   4. workspace context
    let runtime_state = runtime_state::runtime_state_fragment(parsed_mode, &parsed_toggles);
    system_prompt.push_str("\n\n---\n\n");
    system_prompt.push_str(&runtime_state);

    // Layer feature-toggle prompt fragments AFTER the mode's base prompt
    // so the toggle's rules take precedence on conflicts. Each fragment
    // already includes its own preamble / heading so the LLM treats them
    // as a distinct section.
    let fragment = feature_toggles::enabled_fragment(&parsed_toggles);
    if !fragment.is_empty() {
        system_prompt.push_str("\n\n---\n\n");
        system_prompt.push_str(&fragment);
    }

    // Add workspace context if provided
    if let Some(ws_path) = &workspace_path {
        system_prompt.push_str(&format!(
            "\n\n## Current Workspace\nThe workspace root is: {}\n",
            ws_path
        ));
    }

    // Add system message
    session.add_message(Message::system(system_prompt));

    // Add conversation history (for context memory)
    for msg in &history {
        if let Some(converted) = convert_message(msg)? {
            session.add_message(converted);
        }
    }

    // Add current user message
    session.add_message(Message::user(instruction.clone()));

    // The executor already fills `session_id` and `message_id` on every
    // payload it constructs before invoking this callback, so we just
    // forward `payload` straight to the emit channel. The single clone here
    // is the unavoidable one required by the `Fn(StreamPayload)` bound
    // (the executor clones before calling us); everything else was a
    // wasted copy.
    let app_for_emit = app.clone();
    let instruction_clone = instruction.clone();
    let callback = move |payload: StreamPayload| {
        emit(&app_for_emit, payload.clone());

        // Emit a dedicated file-written event when a file modification tool
        // succeeds. This bypasses the file watcher path-matching issue and
        // directly tells the frontend which file changed, so it can refresh
        // the editor immediately.
        if payload.event_type == "tool_result"
            && !payload.error.as_ref().map(|e| !e.is_empty()).unwrap_or(false)
        {
            if let Some(changed_path) = &payload.file_path {
                if let Err(error) = app_for_emit.emit(
                    "file-written",
                    serde_json::json!({ "path": changed_path }),
                ) {
                    tracing::warn!("Failed to emit file-written event: {}", error);
                }
            }
        }
    };

    let _cancel_guard = crate::commands::StreamCancelGuard::new(&session_id);

    match executor
        .run(&mut session, &instruction_clone, &session_id, &message_id, callback)
        .await
    {
        Ok(final_response) => {
            tracing::info!(
                "ai_agent_stream done - session: {}, response length: {}",
                session_id,
                final_response.len()
            );

            // If the user requested cancellation during the loop, the agent's
            // own cleanup in `agent_loop.rs` already cleared the flag; if not,
            // our drop guard clears it.
            if crate::commands::clear_stream_cancelled(&session_id) {
                _cancel_guard.clear();
                emit(
                    &app,
                    StreamPayload::cancelled(&session_id, &message_id),
                );
                return Ok(());
            }
            _cancel_guard.clear();

            emit(
                &app,
                StreamPayload::done(&session_id, &message_id, Some(&final_response)),
            );
        }
        Err(e) => {
            tracing::error!("ai_agent_stream error - session: {}, error: {}", session_id, e);

            let error_msg = match &e {
                AgentError::MaxIterationsReached(_) => {
                    "Agent reached maximum iterations. The task may be too complex.".to_string()
                }
                AgentError::Cancelled => "Cancelled by user".to_string(),
                _ => e.to_string(),
            };

            // Cancellation: surface it as a terminal `cancelled` event so
            // the frontend can match the same shape it does for chat/edit.
            // Other errors get the `error` event. Either way the cancel
            // guard's drop will clear any leftover flag.
            if matches!(e, AgentError::Cancelled) {
                _cancel_guard.clear();
                emit(
                    &app,
                    StreamPayload::cancelled(&session_id, &message_id),
                );
            } else {
                _cancel_guard.clear();
                emit(
                    &app,
                    StreamPayload::error(&session_id, &message_id, &error_msg),
                );
            }
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn ai_agent_cancel(session_id: String) -> Result<(), AgentCommandError> {
    tracing::info!("ai_agent_cancel - session: {}", session_id);
    crate::commands::mark_stream_cancelled(&session_id);
    Ok(())
}

#[tauri::command]
pub async fn get_available_tools(app: AppHandle) -> Result<Vec<serde_json::Value>, AgentCommandError> {
    let registry = get_full_tool_registry(&app).await;
    let tools = registry.read().await.get_all_definitions();
    let tools_json: Vec<serde_json::Value> = tools
        .iter()
        .map(|tool| serde_json::to_value(tool).map_err(|error| AgentCommandError::ToolDefinitionsSerialization(error.to_string())))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tools_json)
}

#[tauri::command]
/// Deliver the user's answer back to a suspended `ask_user` tool call.
/// `tool_call_id` must match the id that was emitted with the `ask_user`
/// stream event. `answer` is the user's selected or typed response.
pub async fn answer_ask_user(
    tool_call_id: String,
    answer: String,
) -> Result<(), String> {
    crate::agent::tools::ask_user_tools::deliver_answer(&tool_call_id, answer)
        .map_err(|e| e.to_string())
}
