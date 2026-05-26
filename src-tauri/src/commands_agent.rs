//! Agent streaming commands for Tauri IPC
//!
//! Exposes the agent tool-calling functionality to the frontend

use crate::agent::{
    create_agent_executor, create_tool_registry, get_agent_system_prompt, AgentSession, Message,
    SharedToolRegistry, AgentError, ToolCallMessage, ToolCallFunction,
};
use crate::streaming::StreamPayload;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

fn emit(app: &AppHandle, payload: StreamPayload) {
    let _ = app.emit("ai://stream", payload);
}

/// Shared tool registry for agent
pub static AGENT_TOOL_REGISTRY: std::sync::OnceLock<SharedToolRegistry> =
    std::sync::OnceLock::new();

fn get_tool_registry() -> SharedToolRegistry {
    AGENT_TOOL_REGISTRY
        .get_or_init(|| create_tool_registry())
        .clone()
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIConfigInput {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: String,
}

#[tauri::command]
pub async fn ai_agent_stream(
    session_id: String,
    message_id: String,
    instruction: String,
    workspace_path: Option<String>,
    history: Vec<FrontendMessage>,
    config_input: AIConfigInput,
    app: AppHandle,
) -> Result<(), String> {
    tracing::info!("ai_agent_stream start - session: {}, history length: {}", session_id, history.len());

    // Create AI config from input
    let ai_config = crate::ai::AIConfig {
        provider: match config_input.provider.as_str() {
            "openai" | "deepseek" => crate::ai::AIProvider::OpenAI {
                api_key: config_input.api_key.unwrap_or_default(),
                base_url: config_input.base_url.unwrap_or_else(|| "https://api.deepseek.com".to_string()),
            },
            "ollama" => crate::ai::AIProvider::Ollama {
                base_url: config_input.base_url.unwrap_or_else(|| "http://localhost:11434".to_string()),
            },
            _ => crate::ai::AIProvider::OpenAI {
                api_key: config_input.api_key.unwrap_or_default(),
                base_url: config_input.base_url.unwrap_or_else(|| "https://api.deepseek.com".to_string()),
            },
        },
        model: config_input.model,
        temperature: 0.7,
        max_tokens: Some(4096),
    };

    tracing::info!("Using AI provider: {:?}", ai_config.provider);

    let executor = create_agent_executor(ai_config);

    let tool_registry = get_tool_registry();
    let mut session = AgentSession::new(tool_registry);

    // Add system prompt
    let mut system_prompt = get_agent_system_prompt();

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

    let callback = move |payload: StreamPayload| {
        let mut p = payload;
        p.session_id = session_id_clone.clone();
        p.message_id = message_id_clone.clone();
        emit(&app_clone, p);
    };

    match executor
        .run(&mut session, &instruction_clone, callback)
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
                    done: true,
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
                    done: true,
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
pub async fn get_available_tools() -> Result<Vec<serde_json::Value>, String> {
    let registry = get_tool_registry();
    let tools = registry.read().await.get_all_definitions();
    let tools_json: Vec<serde_json::Value> = tools
        .iter()
        .map(|t| serde_json::to_value(t).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tools_json)
}
