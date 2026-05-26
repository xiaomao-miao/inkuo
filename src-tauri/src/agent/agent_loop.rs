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

use super::tools::{ToolCall, SharedToolRegistry};
use crate::ai::{AIConfig, AIProvider};
use crate::streaming::StreamPayload;

/// Maximum number of iterations to prevent infinite loops
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

    pub fn assistant(content: Option<String>, tool_calls: Option<Vec<ToolCallMessage>>) -> Self {
        Self::Assistant { content, tool_calls }
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
pub struct ToolCallMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String, // JSON string
}

/// Agent state for a single conversation
pub struct AgentSession {
    pub messages: Vec<Message>,
    pub max_iterations: usize,
    pub tool_registry: SharedToolRegistry,
}

impl AgentSession {
    pub fn new(tool_registry: SharedToolRegistry) -> Self {
        Self {
            messages: Vec::new(),
            max_iterations: DEFAULT_MAX_ITERATIONS,
            tool_registry,
        }
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
                Message::Assistant { content, tool_calls } => {
                    let mut obj = serde_json::json!({
                        "role": "assistant",
                    });
                    if let Some(c) = content {
                        obj["content"] = serde_json::json!(c);
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
    client: reqwest::Client,
}

impl AgentExecutor {
    pub fn new(config: AIConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// Run the agent loop with a user request
    pub async fn run<F>(
        &self,
        session: &mut AgentSession,
        user_request: &str,
        on_event: F,
    ) -> Result<String, AgentError>
    where
        F: Fn(StreamPayload) + Clone + Send + Sync + 'static,
    {
        // Add user message
        session.add_message(Message::user(user_request));

        // Get tool definitions for API call
        let tools = session.tool_registry.read().await.get_all_definitions();
        let tools_json: Vec<Value> = tools.iter().map(|t| serde_json::to_value(t).unwrap()).collect();

        // Run the agent loop
        for iteration in 0..session.max_iterations {
            tracing::info!(
                "Agent iteration {}/{}",
                iteration + 1,
                session.max_iterations
            );

            // Build request
            let messages = session.get_messages_for_api();

            // Make API call with tools
            let response = self
                .call_ai_with_tools(&messages, &tools_json, on_event.clone())
                .await?;

            // Parse response
            let (content, tool_calls) = self.parse_response(&response)?;

            // Check for cancellation
            if let Some(content) = &content {
                if content.contains("__CANCELLED__") {
                    return Err(AgentError::Cancelled);
                }
            }

            // Add assistant message to history
            session.add_message(Message::assistant(content.clone(), tool_calls.clone()));

            // If no tool calls, we're done
            let tool_calls = match tool_calls {
                Some(tc) if !tc.is_empty() => tc,
                _ => {
                    // Return the content
                    return Ok(content.unwrap_or_default());
                }
            };

            // Parse and execute tool calls
            let parsed_calls: Vec<ParsedToolCall> = tool_calls
                .iter()
                .filter_map(|tc| {
                    let name = tc.function.name.clone();
                    let id = tc.id.clone();

                    // Parse arguments JSON
                    let arguments: Value = serde_json::from_str(&tc.function.arguments)
                        .unwrap_or(serde_json::json!({}));

                    Some(ParsedToolCall {
                        id,
                        name,
                        arguments,
                    })
                })
                .collect();

            // Execute each tool call
            for parsed in &parsed_calls {
                // Emit tool call start event
                on_event(StreamPayload {
                    session_id: String::new(),
                    message_id: String::new(),
                    event_type: "tool_call_start".to_string(),
                    content: None,
                    summary: None,
                    tool_call_id: Some(parsed.id.clone()),
                    tool_name: Some(parsed.name.clone()),
                    tool_args: Some(serde_json::to_string(&parsed.arguments).unwrap_or_default()),
                    final_content: None,
                    error: None,
                    done: false,
                });

                // Execute tool
                let tool_call = ToolCall {
                    id: parsed.id.clone(),
                    name: parsed.name.clone(),
                    arguments: parsed.arguments.clone(),
                };

                let result = session.tool_registry.read().await.execute(&tool_call).await;

                // Emit tool result event
                on_event(StreamPayload {
                    session_id: String::new(),
                    message_id: String::new(),
                    event_type: "tool_result".to_string(),
                    content: Some(result.output.clone()),
                    summary: None,
                    tool_call_id: Some(result.tool_call_id),
                    tool_name: None,
                    tool_args: None,
                    final_content: None,
                    error: if result.is_error { Some(result.output.clone()) } else { None },
                    done: false,
                });

                // Add tool result to message history
                session.add_message(Message::tool_result(&parsed.id, &result.output));
            }
        }

        Err(AgentError::MaxIterationsReached(session.max_iterations))
    }

    /// Call AI with tools
    async fn call_ai_with_tools<F>(
        &self,
        messages: &[Value],
        tools: &[Value],
        on_event: F,
    ) -> Result<String, AgentError>
    where
        F: Fn(StreamPayload) + Clone + Send + Sync + 'static,
    {
        let (url, headers) = match &self.config.provider {
            AIProvider::OpenAI { api_key, base_url } => (
                format!("{}/chat/completions", base_url.trim_end_matches('/')),
                vec![("Authorization", format!("Bearer {}", api_key))],
            ),
            AIProvider::Ollama { base_url } => (
                format!("{}/api/chat", base_url.trim_end_matches('/')),
                vec![],
            ),
            AIProvider::Official { api_key } => (
                "https://api.inkuo.com/v1/chat/completions".to_string(),
                vec![("Authorization", format!("Bearer {}", api_key))],
            ),
        };

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
            "tools": tools,
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens,
            "stream": true,
        });

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
            let body = response.text().await.unwrap_or_default();
            return Err(AgentError::AIError(format!(
                "HTTP {}: {}",
                status, body
            )));
        }

        tracing::info!("Received response, status: {}", response.status());

        // Process streaming response
        let mut buffer = String::new();
        let mut current_tool_calls: Vec<ToolCallMessage> = Vec::new();
        let mut current_content = String::new();

        let mut stream = response.bytes_stream();
        let mut bytes_received = 0;

        tracing::info!("Starting to process stream...");

        while let Some(item) = stream.next().await {
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

            // Process complete lines
            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim_end().to_string();
                buffer = buffer[pos + 1..].to_string();

                // Skip empty lines
                if line.trim().is_empty() {
                    continue;
                }

                // Parse SSE data
                for data in crate::openai_stream::iter_sse_data_lines(&line) {
                    if data.trim() == "[DONE]" {
                        continue;
                    }

                    tracing::info!("SSE data: {}", data);

                    match self.parse_sse_delta(data) {
                        Ok(Some(delta)) => {
                            // Update content
                            if let Some(content) = delta.content {
                                current_content.push_str(&content);
                                on_event(StreamPayload {
                                    session_id: String::new(),
                                    message_id: String::new(),
                                    event_type: "text".to_string(),
                                    content: Some(content),
                                    summary: None,
                                    tool_call_id: None,
                                    tool_name: None,
                                    tool_args: None,
                                    final_content: None,
                                    error: None,
                                    done: false,
                                });
                            }

                            // Collect tool calls
                            if let Some(tool_calls) = delta.tool_calls {
                                for tc in tool_calls {
                                    // Ensure we have capacity
                                    while current_tool_calls.len() <= tc.index {
                                        current_tool_calls.push(ToolCallMessage {
                                            id: format!("call_{}", current_tool_calls.len()),
                                            call_type: "function".to_string(),
                                            function: ToolCallFunction {
                                                name: String::new(),
                                                arguments: String::new(),
                                            },
                                        });
                                    }

                                    let entry = &mut current_tool_calls[tc.index];

                                    if let Some(id) = tc.id {
                                        entry.id = id;
                                    }
                                    if let Some(name) = tc.function.name {
                                        entry.function.name = name;
                                    }
                                    if let Some(args) = tc.function.arguments {
                                        entry.function.arguments = args;
                                    }
                                }
                            }
                        }
                        Ok(None) => {
                            // No content or tool_calls in this delta, skip
                            tracing::info!("Delta has no content or tool_calls");
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse SSE delta: {}", e);
                        }
                    }
                }
            }
        }

        tracing::info!("Stream processing complete. bytes_received: {}, current_content_len: {}", bytes_received, current_content.len());

        // Build final response
        let response_json = serde_json::json!({
            "content": current_content,
            "tool_calls": current_tool_calls
        });

        Ok(response_json.to_string())
    }

    /// Parse SSE delta from OpenAI format (handles DeepSeek's reasoning_content)
    fn parse_sse_delta(&self, data: &str) -> Result<Option<DeltaResponse>, String> {
        let json: Value = serde_json::from_str(data)
            .map_err(|e| format!("JSON parse error: {}", e))?;
        let delta = match json.get("delta") {
            Some(d) => d,
            None => return Ok(None),
        };
        // Verify we have choices
        if json.get("choices").is_none() {
            return Ok(None);
        }

        // Handle both content and reasoning_content (DeepSeek)
        let content = delta
            .get("content")
            .and_then(|c| c.as_str())
            .map(String::from)
            .or_else(|| {
                delta
                    .get("reasoning_content")
                    .and_then(|c| c.as_str())
                    .map(String::from)
            });

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
            tool_calls,
        }))
    }

    /// Parse the final response
    fn parse_response(&self, response: &str) -> Result<(Option<String>, Option<Vec<ToolCallMessage>>), AgentError> {
        let json: Value = serde_json::from_str(response)
            .map_err(|e| AgentError::InvalidResponse(format!("JSON parse error: {}", e)))?;

        let content = json
            .get("content")
            .and_then(|c| c.as_str())
            .map(String::from);

        let tool_calls = json
            .get("tool_calls")
            .and_then(|tc| tc.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        Some(ToolCallMessage {
                            id: tc.get("id")?.as_str()?.to_string(),
                            call_type: tc.get("type")?.as_str()?.to_string(),
                            function: ToolCallFunction {
                                name: tc.get("function")?.get("name")?.as_str()?.to_string(),
                                arguments: tc.get("function")?.get("arguments")?.as_str()?.to_string(),
                            },
                        })
                    })
                    .collect()
            });

        Ok((content, tool_calls))
    }
}

#[derive(Debug)]
struct DeltaResponse {
    content: Option<String>,
    tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Debug)]
struct DeltaToolCall {
    index: usize,
    id: Option<String>,
    function: DeltaFunction,
}

#[derive(Debug)]
struct DeltaFunction {
    name: Option<String>,
    arguments: Option<String>,
}

/// Create a new agent executor
pub fn create_agent_executor(config: AIConfig) -> AgentExecutor {
    AgentExecutor::new(config)
}

/// System prompt for the agent
pub fn get_agent_system_prompt() -> String {
    r#"You are inkuo AI, an advanced document and code assistant with the ability to read, write, and edit files.

You have access to tools to help you accomplish tasks. Use them when needed.

## Available Tools

### read_file
Read the complete contents of a file from the filesystem.
Parameters: path (string, required), offset (integer, optional), limit (integer, optional)

### write_file
Create a new file or overwrite an existing file with given content.
Parameters: path (string, required), content (string, required)

### edit_file
Edit a specific portion of an existing file by replacing old_text with new_text.
Parameters: path (string, required), old_text (string, required), new_text (string, required)

### list_dir
List the contents of a directory.
Parameters: path (string, required)

### glob
Find all files matching a glob pattern (e.g., "**/*.rs", "src/**/*.{ts,tsx}").
Parameters: pattern (string, required), base_dir (string, required)

### grep
Search for lines containing a pattern in files. Supports regex.
Parameters: pattern (string, required), paths (array of strings, required), case_sensitive (boolean, optional)

## Guidelines

1. Always explore the workspace structure before making changes
2. Check existing files before creating new ones to avoid duplicates
3. When editing, be precise about what you're replacing
4. Provide clear summaries of changes made
5. If a tool fails, explain the error and suggest alternatives
6. For complex tasks, break them down into smaller steps

## Response Format

When you use tools, they will execute and return results. You can then continue reasoning or provide a final response.

When responding:
- Be concise but thorough
- Use code blocks for code snippets
- Format file paths in code formatting
- List changes made clearly

You are working in a local development environment. The user is working on a project. Be helpful and proactive in finding solutions."#.to_string()
}
