//! Agent streaming commands for Tauri IPC
//!
//! Exposes the agent tool-calling functionality to the frontend

use crate::agent::{
    create_agent_executor,
    get_agent_system_prompt, get_ask_system_prompt, AgentSession, Message,
    SharedToolRegistry, AgentError, ToolCallMessage, ToolCallFunction,
};
use crate::agent::tools::ToolRegistry;
use crate::ai_config::{self, AIConfigInput};
use crate::streaming::StreamPayload;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;

fn emit(app: &AppHandle, payload: StreamPayload) {
    let _ = app.emit("ai://stream", payload);
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
    // Lazily add database_search tool with AppHandle (idempotent)
    static DB_SEARCH_ADDED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if DB_SEARCH_ADDED.get().is_none() {
        let mut reg = registry.write().await;
        if !reg.has_tool("database_search") {
            reg.set_app_handle(app.clone());
            DB_SEARCH_ADDED.get_or_init(|| {});
        }
    }
    registry
}

async fn get_read_only_tool_registry(_app: &AppHandle) -> SharedToolRegistry {
    READ_ONLY_TOOL_REGISTRY
        .get_or_init(|| Arc::new(RwLock::new(ToolRegistry::new_read_only())))
        .clone()
}

/// Update the workspace path for both tool registries
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

/// Convert frontend message to agent message
fn convert_message(msg: &FrontendMessage) -> Option<Message> {
    match msg.role.as_str() {
        "system" => Some(Message::System {
            content: msg.content.clone(),
        }),
        "user" => Some(Message::User {
            content: msg.content.clone(),
        }),
        "assistant" => {
            let tool_calls = msg.tool_calls.as_ref().map(|tcs| {
                tcs.iter()
                    .map(|tc| ToolCallMessage {
                        id: tc.id.clone(),
                        call_type: "function".to_string(),
                        function: ToolCallFunction {
                            name: tc.name.clone(),
                            arguments: serde_json::to_string(&tc.arguments).unwrap_or_default(),
                        },
                    })
                    .collect()
            });
            Some(Message::Assistant {
                content: Some(msg.content.clone()),
                reasoning_content: None,
                tool_calls,
            })
        }
        "tool" => Some(Message::Tool {
            tool_call_id: msg.tool_call_id.clone().unwrap_or_default(),
            content: msg.content.clone(),
        }),
        _ => None,
    }
}

#[tauri::command]
pub async fn ai_agent_stream(
    session_id: String,
    message_id: String,
    instruction: String,
    workspace_path: Option<String>,
    read_only: bool,
    history: Vec<FrontendMessage>,
    config_input: AIConfigInput,
    app: AppHandle,
) -> Result<(), String> {
    tracing::info!("ai_agent_stream start - session: {}, history length: {}", session_id, history.len());

    // Update workspace path for tool validation
    update_registry_workspace(workspace_path.clone()).await;

    // Create AI config from input
    let ai_config = ai_config::build_input_ai_config(config_input);

    tracing::info!("Using AI provider: {:?}", ai_config.provider);

    let executor = create_agent_executor(ai_config);

    // Use different tool registry based on mode
        let tool_registry = if read_only {
            get_read_only_tool_registry(&app).await
        } else {
            get_full_tool_registry(&app).await
        };
    let mut session = AgentSession::new(tool_registry);

    // Use different system prompt based on mode
    let mut system_prompt = if read_only {
        get_ask_system_prompt()
    } else {
        get_agent_system_prompt()
    };

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
        if let Some(converted) = convert_message(msg) {
            session.add_message(converted);
        }
    }

    // Add current user message
    session.add_message(Message::user(instruction.clone()));

    let session_id_clone = session_id.clone();
    let message_id_clone = message_id.clone();
    let app_clone = app.clone();
    let instruction_clone = instruction.clone();

    tracing::info!("[DEBUG] Setting up callback for session: {}, message: {}", session_id_clone, message_id_clone);

    let callback = move |payload: StreamPayload| {
        tracing::info!("[DEBUG] Callback fired - event_type: {}, session_id: {}, message_id: {}",
            payload.event_type, payload.session_id, payload.message_id);
        let mut p = payload.clone();
        p.session_id = session_id_clone.clone();
        p.message_id = message_id_clone.clone();
        emit(&app_clone, p);

        // Emit a dedicated file-written event when a file modification tool succeeds.
        // This bypasses the file watcher path-matching issue and directly tells the
        // frontend which file changed, so it can refresh the editor immediately.
        if payload.event_type == "tool_result"
            && !payload.error.as_ref().map(|e| !e.is_empty()).unwrap_or(false)
            && payload.file_path.is_some()
        {
            let changed_path = payload.file_path.clone().unwrap();
            let _ = app_clone.emit("file-written", serde_json::json!({
                "path": changed_path,
            }));
        }
    };

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

            emit(
                &app,
                StreamPayload {
                    session_id,
                    message_id,
                    event_type: "done".to_string(),
                    content: None,
                    summary: None,
                    tool_call_id: None,
                    tool_name: None,
                    tool_args: None,
                    final_content: Some(final_response),
                    error: None,
                    search_results: None,
                    done: true,
                    file_path: None,
                    original_content: None,
                    new_content: None,
                    diff_summary: None,
                    office_file_modified: None,
                },
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

            emit(
                &app,
                StreamPayload {
                    session_id,
                    message_id,
                    event_type: "error".to_string(),
                    content: None,
                    summary: None,
                    tool_call_id: None,
                    tool_name: None,
                    tool_args: None,
                    final_content: None,
                    error: Some(error_msg),
                    search_results: None,
                    done: true,
                    file_path: None,
                    original_content: None,
                    new_content: None,
                    diff_summary: None,
                    office_file_modified: None,
                },
            );
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn ai_agent_cancel(session_id: String) -> Result<(), String> {
    tracing::info!("ai_agent_cancel - session: {}", session_id);
    crate::commands::STREAM_CANCELLED
        .lock()
        .insert(session_id);
    Ok(())
}

#[tauri::command]
pub async fn get_available_tools(app: AppHandle) -> Result<Vec<serde_json::Value>, String> {
    let registry = get_full_tool_registry(&app).await;
    let tools = registry.read().await.get_all_definitions();
    let tools_json: Vec<serde_json::Value> = tools
        .iter()
        .map(|t| serde_json::to_value(t).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tools_json)
}
