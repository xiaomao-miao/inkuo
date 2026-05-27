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
use crate::diff;
use crate::streaming::{StreamPayload, FileDiffSummary, StreamDiffHunk, StreamDiffChange};

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
        session_id: &str,
        message_id: &str,
        on_event: F,
    ) -> Result<String, AgentError>
    where
        F: Fn(StreamPayload) + Clone + Send + Sync + 'static,
    {
        tracing::info!("[DEBUG] AgentExecutor::run started - session_id: {}, message_id: {}", session_id, message_id);

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
                .call_ai_with_tools(&messages, &tools_json, session_id, message_id, on_event.clone())
                .await?;

            // Parse response
            let (content, reasoning_content, tool_calls) = self.parse_response(&response)?;

            // Check for cancellation
            if let Some(c) = &content {
                if c.contains("__CANCELLED__") {
                    return Err(AgentError::Cancelled);
                }
            }

            // Add assistant message to history (with reasoning_content for DeepSeek)
            session.add_message(Message::assistant(content.clone(), reasoning_content.clone(), tool_calls.clone()));

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
                    session_id: session_id.to_string(),
                    message_id: message_id.to_string(),
                    event_type: "tool_call_start".to_string(),
                    content: None,
                    summary: None,
                    tool_call_id: Some(parsed.id.clone()),
                    tool_name: Some(parsed.name.clone()),
                    tool_args: Some(serde_json::to_string(&parsed.arguments).unwrap_or_default()),
                    final_content: None,
                    error: None,
                    done: false,
                    file_path: None,
                    original_content: None,
                    new_content: None,
                    diff_summary: None,
                });

                // Execute tool
                let tool_call = ToolCall {
                    id: parsed.id.clone(),
                    name: parsed.name.clone(),
                    arguments: parsed.arguments.clone(),
                };

                let mut result = session.tool_registry.read().await.execute(&tool_call).await;

                // Inject the correct tool_call_id from streamed data (not the placeholder)
                result.tool_call_id = parsed.id.clone();

                // Compute diff summary for file modification tools
                let diff_summary: Option<FileDiffSummary> = if let (Some(file_path), Some(original), Some(new_content)) = (
                    &result.file_path,
                    &result.original_content,
                    &result.new_content,
                ) {
                    let diff_result = diff::compute_diff(original, new_content);
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
                    done: false,
                    // Diff info for file modification tools
                    file_path: result.file_path.clone(),
                    original_content: result.original_content.clone(),
                    new_content: result.new_content.clone(),
                    diff_summary,
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
            let body = response.text().await.unwrap_or_default();
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
                for data in crate::openai_stream::iter_sse_event_data_lines(&line) {
                    if data.trim() == "[DONE]" {
                        continue;
                    }

                    tracing::info!("SSE data: {}", data);

                    let parsed = self.parse_sse_delta(data, is_ollama);
                    tracing::info!("[PARSING] parse result: {:?}", parsed);
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
                                    done: false,
                                    file_path: None,
                                    original_content: None,
                                    new_content: None,
                                    diff_summary: None,
                                });
                            }
                            // Also handle reasoning_content (DeepSeek's thinking)
                            if let Some(reasoning) = delta.reasoning_content {
                                if !reasoning.is_empty() {
                                    current_reasoning_content.push_str(&reasoning);
                                    on_event(StreamPayload {
                                        session_id: session_id.to_string(),
                                        message_id: message_id.to_string(),
                                        event_type: "text".to_string(),
                                        content: Some(reasoning),
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
                                    });
                                }
                            }

                            // Collect tool calls
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

                                    // Only update the ID if the model provided a non-empty one.
                                    // This prevents placeholder IDs (call_1, call_2...) from
                                    // overwriting real IDs returned by the model.
                                    if let Some(id) = tc.id {
                                        if !id.is_empty() {
                                            entry.id = id;
                                        }
                                    }
                                    if let Some(name) = tc.function.name {
                                        entry.function.name = name;
                                    }
                                    if let Some(args) = tc.function.arguments {
                                        entry.function.arguments.push_str(&args);
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

        // Process any remaining data in the buffer (issue #9 - handle residual data)
        if !buffer.trim().is_empty() {
            tracing::info!("Processing remaining buffer data: {}", buffer);
            for data in crate::openai_stream::iter_sse_event_data_lines(&buffer) {
                if data.trim() == "[DONE]" || data.trim().is_empty() {
                    continue;
                }
                if let Ok(Some(delta)) = self.parse_sse_delta(data, is_ollama) {
                    if let Some(content) = delta.content {
                        current_content.push_str(&content);
                    }
                    if delta.tool_calls.is_some() {
                        // Note: This is residual data, tool calls from partial chunks are already processed above
                    }
                }
            }
        }

        tracing::info!("Stream processing complete. bytes_received: {}, current_content_len: {}", bytes_received, current_content.len());

        // Debug: log the final tool calls
        for (i, tc) in current_tool_calls.iter().enumerate() {
            tracing::info!("[TOOL_CALL_DEBUG] #{:02}: id='{}', name='{}', args='{}'",
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

        Ok((content, reasoning_content, tool_calls))
    }
}

#[derive(Debug)]
struct DeltaResponse {
    content: Option<String>,
    reasoning_content: Option<String>,
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

// Prompts are re-exported from the prompts module
pub use super::prompts::*;
