//! AI Provider adapter module
//!
//! Handles:
//! - OpenAI-compatible API (DeepSeek, etc.)
//! - Ollama (local models)
//! - Unified streaming protocol

use serde::{Deserialize, Serialize, de::Error as DeError};
use thiserror::Error;
use futures_util::StreamExt;
use once_cell::sync::Lazy;

/// inkuo's official cloud gateway. Used by the `Official` provider below; kept
/// in one place so future swaps don't need to touch five call sites.
const OFFICIAL_BASE_URL: &str = "https://api.inkuo.com/v1";

// ============================================================================
// Prompts - loaded from markdown files at compile time
// ============================================================================

/// System prompt for the floating AI popover (single-shot explain-the-passage).
/// Reuses the edit prompt because the popover's input shape (instruction +
/// selected text) is conceptually the same as edit (instruction + original
/// text). Document-editing prompts fit — they ask for a structured
/// transformation of one piece of text.
fn get_popover_prompt() -> &'static str {
    include_str!("../prompts/edit.md")
}

/// System prompt for edit mode (document editing)
fn get_edit_prompt() -> &'static str {
    include_str!("../prompts/edit.md")
}

#[derive(Error, Debug)]
pub enum AIError {
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Authentication error: {0}")]
    AuthError(String),
    #[error("Model error: {0}")]
    ModelError(String),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    #[error("Rate limited")]
    RateLimited,
}

impl AIError {
    /// Whether this error is transient and worth retrying. Network blips,
    /// provider rate limits, and upstream 5xx responses are all considered
    /// transient — auth failures, malformed payloads, and client-side
    /// serialization issues are not.
    ///
    /// The `AIError` enum doesn't carry the upstream HTTP status (it gets
    /// flattened into the human-readable `ModelError` message), so we lean
    /// on the message format used by `handle_http_error`. That's stable
    /// enough for an opt-in retry decision; if you need stricter
    /// classification, change `handle_http_error` to expose the status
    /// separately.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::NetworkError(_) => true,
            Self::RateLimited => true,
            Self::ModelError(msg) => {
                const RETRY_TOKENS: &[&str] = &[
                    "HTTP 502",
                    "HTTP 503",
                    "HTTP 504",
                    "Service Unavailable",
                    "Bad Gateway",
                    "Gateway Timeout",
                    "Request Timeout",
                    "connect error",
                    "connection reset",
                    "connection refused",
                    "timed out",
                ];
                RETRY_TOKENS.iter().any(|token| msg.contains(token))
            }
            Self::AuthError(_) | Self::InvalidResponse(_) => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AIProvider {
    OpenAI {
        api_key: String,
        base_url: String,
    },
    Ollama {
        base_url: String,
    },
    Official {
        api_key: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIConfig {
    pub provider: AIProvider,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    /// Visual input capability when known. `None` means a custom endpoint
    /// whose capability has not been declared; the standard multimodal wire
    /// format may still be attempted.
    #[serde(default)]
    pub supports_vision: Option<bool>,
}

impl Default for AIConfig {
    fn default() -> Self {
        Self {
            // Default to a safe local provider.
            // Real API keys must come from the Settings panel (persisted settings.json).
            provider: AIProvider::Ollama {
                base_url: "http://localhost:11434".to_string(),
            },
            model: "llama3".to_string(),
            temperature: 0.7,
            max_tokens: Some(16384),
            supports_vision: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIEditRequest {
    pub instruction: String,
    pub original_text: String,
    pub scope: EditScope,
    pub context: Vec<ContextItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EditScope {
    Selection,
    Paragraph,
    Section,
    Document,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextItem {
    pub title: String,
    pub path: String,
    pub range: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIEditResponse {
    pub summary: String,
    pub content: String,
    pub rules_applied: Vec<String>,
}

pub(crate) static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .build()
        .expect("failed to build reqwest client")
});

pub struct AIProviderAdapter {
    config: AIConfig,
}

impl AIProviderAdapter {
    pub fn new(config: AIConfig) -> Self {
        Self { config }
    }

    // ─── Helper: Build chat request body ────────────────────────────────────────
    fn build_chat_body(&self, system_prompt: &str, user_prompt: &str) -> serde_json::Value {
        serde_json::json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens,
        })
    }

    /// Build chat request body for OpenAI-compatible providers with the
    /// vendor-specific `thinking` extension disabled. Do **not** use this
    /// for non-OpenAI providers — the `thinking` field is meaningless on
    /// e.g. Ollama and may even confuse some clients.
    fn build_chat_body_no_thinking(&self, system_prompt: &str, user_prompt: &str) -> serde_json::Value {
        serde_json::json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens,
            "thinking": {"type": "disabled"},
        })
    }

    fn build_ollama_body(&self, system_prompt: &str, user_prompt: &str) -> serde_json::Value {
        serde_json::json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "stream": false,
            "options": {
                "temperature": self.config.temperature,
            }
        })
    }

    // ─── Helper: Handle HTTP error response ───────────────────────────────────
    fn handle_http_error(status: reqwest::StatusCode, error_body: &str) -> AIError {
        match status.as_u16() {
            401 => AIError::AuthError("Invalid API key".to_string()),
            429 => AIError::RateLimited,
            503 => AIError::ModelError(format!(
                "Service Unavailable (503) - The API service is temporarily unavailable. \
                Please try again later or switch to a different API provider. Details: {}",
                error_body
            )),
            _ => AIError::ModelError(format!(
                "HTTP {} - {}: {}",
                status,
                status.canonical_reason().unwrap_or("Unknown"),
                error_body
            )),
        }
    }

    // ─── Helper: Stream SSE and collect text delta ─────────────────────────────
    async fn stream_sse_text(
        response: reqwest::Response,
        mut on_delta: impl FnMut(String) + Send,
    ) -> Result<String, AIError> {
        let mut full = String::new();
        let mut buffer = String::new();

        let mut stream = response.bytes_stream();
        while let Some(item) = stream.next().await {
            let bytes = item.map_err(|e| AIError::NetworkError(e.to_string()))?;
            let chunk = String::from_utf8_lossy(&bytes);
            buffer.push_str(chunk.as_ref());

            while let Some((event, rest)) = crate::openai_stream::take_next_sse_event(&buffer) {
                buffer = rest;

                for data in crate::openai_stream::iter_sse_event_data_lines(&event) {
                    if data.trim() == "[DONE]" {
                        return Ok(full);
                    }

                    if let Some(delta) = crate::openai_stream::extract_openai_delta_content(data)? {
                        if !delta.is_empty() {
                            full.push_str(&delta);
                            on_delta(delta);
                        }
                    }
                }
            }
        }

        Ok(full)
    }

// ── Streaming ───────────────────────────────────────────────────────────────

    pub async fn chat_stream<F>(
        &self,
        instruction: String,
        original_text: String,
        mut on_delta: F,
    ) -> Result<String, AIError>
    where
        F: FnMut(String) + Send,
    {
        let system_prompt = get_popover_prompt();

        let user_prompt = if original_text.trim().is_empty() {
            instruction
        } else {
            format!(
                r#"Instruction: {}

Context text:
{}"#,
                instruction, original_text
            )
        };

        match &self.config.provider {
            AIProvider::OpenAI { api_key, base_url } => {
                self.call_openai_compatible_chat_stream(api_key, base_url, system_prompt, &user_prompt, &mut on_delta)
                    .await
            }
            // Streaming for Ollama isn't supported in this provider (only
            // `agent_loop.rs` implements true streaming for Ollama, used by
            // the agent tool-calling path). For chat we round-trip the full
            // response and surface it as a single tick to the frontend.
            AIProvider::Ollama { base_url } => {
                let url = format!("{}/api/chat", base_url.trim_end_matches('/'));
                let body = self.build_ollama_body(system_prompt, &user_prompt);

                let response = HTTP_CLIENT
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| AIError::NetworkError(e.to_string()))?;

                if !response.status().is_success() {
                    return Err(AIError::ModelError(format!("HTTP {}", response.status())));
                }

                let response_json: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|e| AIError::InvalidResponse(e.to_string()))?;

                let text = response_json["message"]["content"]
                    .as_str()
                    .ok_or_else(|| AIError::InvalidResponse("Missing content in response".to_string()))?
                    .to_string();
                on_delta(text.clone());
                Ok(text)
            }
            AIProvider::Official { api_key } => {
                self.call_openai_compatible_chat_stream(api_key, OFFICIAL_BASE_URL, system_prompt, &user_prompt, &mut on_delta)
                    .await
            }
        }
    }

    pub async fn edit_stream<F>(&self, request: AIEditRequest, mut on_delta: F) -> Result<AIEditResponse, AIError>
    where
        F: FnMut(String) + Send,
    {
        let system_prompt = get_edit_prompt();

        let user_prompt = format!(
            r#"Instruction: {}

Original text:
{}

Context (optional references):
{}
"#,
            request.instruction,
            request.original_text,
            request
                .context
                .iter()
                .map(|c| format!("- {} ({})\n  Excerpt: {}", c.title, c.path, c.excerpt))
                .collect::<Vec<_>>()
                .join("\n")
        );

        match &self.config.provider {
            AIProvider::OpenAI { api_key, base_url } => {
                self.call_openai_compatible_edit_stream(api_key, base_url, system_prompt, &user_prompt, &mut on_delta)
                    .await
            }
            // Streaming for Ollama isn't supported in this provider (only
            // `agent_loop.rs` implements true streaming for Ollama, used by
            // the agent tool-calling path). For edit we round-trip the full
            // response and surface it as a single tick to the frontend.
            AIProvider::Ollama { base_url } => {
                let resp = self.call_ollama(base_url, system_prompt, &user_prompt).await?;
                on_delta(resp.content.clone());
                Ok(resp)
            }
            AIProvider::Official { api_key } => {
                self.call_openai_compatible_edit_stream(api_key, OFFICIAL_BASE_URL, system_prompt, &user_prompt, &mut on_delta)
                    .await
            }
        }
    }

// ── Sync helpers ──────────────────────────────────────────────────────────

    /// Direct chat call with thinking disabled - optimized for inline completion.
    ///
    /// Always sends `stream: true` so the inkuo Cloud server (which forces
    /// `stream: true` regardless of the incoming request -- see
    /// `cloud-server/src/Inkuso.Cloud.Api/Endpoints/Chat.cs::MapChatEndpoints`)
    /// is happy with us. Local OpenAI-compatible providers also accept
    /// `stream: true` and respond with SSE, which is exactly what
    /// `stream_sse_text` consumes. The single code path keeps both local
    /// and cloud modes working without branching on the provider.
    ///
    /// The `thinking` field is sent regardless of provider (cloud strips
    /// it server-side; local OpenAI-compatible APIs tolerate unknown JSON
    /// keys).
    pub async fn completion(&self, system_prompt: &str, user_prompt: &str) -> Result<String, AIError> {
        match &self.config.provider {
            AIProvider::OpenAI { api_key, base_url } => {
                self.call_openai_compatible_chat_stream_no_thinking(api_key, base_url, system_prompt, user_prompt)
                    .await
            }
            AIProvider::Ollama { base_url } => {
                let url = format!("{}/api/chat", base_url.trim_end_matches('/'));
                let body = self.build_ollama_body(system_prompt, user_prompt);

                let response = HTTP_CLIENT
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| AIError::NetworkError(e.to_string()))?;

                if !response.status().is_success() {
                    return Err(AIError::ModelError(format!("HTTP {}", response.status())));
                }

                let response_json: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|e| AIError::InvalidResponse(e.to_string()))?;

                response_json["message"]["content"]
                    .as_str()
                    .map(String::from)
                    .ok_or_else(|| AIError::InvalidResponse("Missing content in response".to_string()))
            }
            AIProvider::Official { api_key } => {
                self.call_openai_compatible_chat_stream_no_thinking(api_key, OFFICIAL_BASE_URL, system_prompt, user_prompt)
                    .await
            }
        }
    }

    /// Streamed OpenAI-compatible completion call with `thinking` disabled.
    /// Mirrors `call_openai_compatible_chat_stream` but includes the
    /// vendor-specific `thinking: {"type": "disabled"}` extension and
    /// collects the full text via `stream_sse_text` so the caller doesn't
    /// have to wire a `FnMut` callback. Used by `completion()` for inline
    /// auto-complete, where we want the full text in one shot.
    async fn call_openai_compatible_chat_stream_no_thinking(
        &self,
        api_key: &str,
        base_url: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AIError> {
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens,
            "stream": true,
            "thinking": {"type": "disabled"},
        });

        let response = HTTP_CLIENT
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AIError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(Self::handle_http_error(status, &error_body));
        }

        Self::stream_sse_text(response, |_| {}).await
    }

    /// Non-streamed fallback for `call_openai_compatible_chat_no_thinking`.
    ///
    /// **Deprecated for cross-provider calls** -- kept around only because
    /// `completion()` historically used it. The inkuo Cloud server always
    /// forces `stream: true` on the upstream side, so any provider routed
    /// through it returns an SSE stream. Calling `response.json()` on an
    /// SSE body fails with `InvalidResponse`. Prefer
    /// `call_openai_compatible_chat_stream_no_thinking` (the streaming
    /// sibling) which handles both SSE and JSON-only providers uniformly.
    #[allow(dead_code)]
    async fn call_openai_compatible_chat_no_thinking(
        &self,
        api_key: &str,
        base_url: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AIError> {
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let body = self.build_chat_body_no_thinking(system_prompt, user_prompt);

        let response = HTTP_CLIENT
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AIError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(Self::handle_http_error(status, &error_body));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AIError::InvalidResponse(e.to_string()))?;

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| AIError::InvalidResponse("Missing content in response".to_string()))?;

        Ok(content.to_string())
    }
    
    pub async fn edit(&self, request: AIEditRequest) -> Result<AIEditResponse, AIError> {
        let system_prompt = get_edit_prompt();

        let user_prompt = format!(
            r#"Instruction: {}

Original text:
{}

Context (optional references):
{}
"#,
            request.instruction,
            request.original_text,
            request.context.iter()
                .map(|c| format!("- {} ({})\n  Excerpt: {}", c.title, c.path, c.excerpt))
                .collect::<Vec<_>>()
                .join("\n")
        );

        match &self.config.provider {
            AIProvider::OpenAI { api_key, base_url } => {
                self.call_openai_compatible(api_key, base_url, system_prompt, &user_prompt).await
            }
            AIProvider::Ollama { base_url } => {
                self.call_ollama(base_url, system_prompt, &user_prompt).await
            }
            AIProvider::Official { api_key } => {
                self.call_openai_compatible(api_key, OFFICIAL_BASE_URL, system_prompt, &user_prompt).await
            }
        }
    }

    async fn call_openai_compatible_chat_stream<F>(
        &self,
        api_key: &str,
        base_url: &str,
        system_prompt: &str,
        user_prompt: &str,
        on_delta: &mut F,
    ) -> Result<String, AIError>
    where
        F: FnMut(String) + Send,
    {
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens,
            "stream": true,
        });

        let response = HTTP_CLIENT
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AIError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(Self::handle_http_error(status, &error_body));
        }

        Self::stream_sse_text(response, on_delta).await
    }

    async fn call_openai_compatible_edit_stream<F>(
        &self,
        api_key: &str,
        base_url: &str,
        system_prompt: &str,
        user_prompt: &str,
        on_delta: &mut F,
    ) -> Result<AIEditResponse, AIError>
    where
        F: FnMut(String) + Send,
    {
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens,
            "stream": true,
        });

        let response = HTTP_CLIENT
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AIError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(Self::handle_http_error(status, &error_body));
        }

        let full = Self::stream_sse_text(response, on_delta).await?;
        self.parse_ai_response(&full)
    }

    async fn call_openai_compatible(
        &self,
        api_key: &str,
        base_url: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<AIEditResponse, AIError> {
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let body = self.build_chat_body(system_prompt, user_prompt);

        let response = HTTP_CLIENT
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AIError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(Self::handle_http_error(status, &error_body));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AIError::InvalidResponse(e.to_string()))?;

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| AIError::InvalidResponse("Missing content in response".to_string()))?;

        self.parse_ai_response(content)
    }

    async fn call_ollama(
        &self,
        base_url: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<AIEditResponse, AIError> {
        let url = format!("{}/api/chat", base_url.trim_end_matches('/'));
        let body = self.build_ollama_body(system_prompt, user_prompt);

        let response = HTTP_CLIENT
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AIError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(AIError::ModelError(format!("HTTP {}", response.status())));
        }

        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AIError::InvalidResponse(e.to_string()))?;

        let content = response_json["message"]["content"]
            .as_str()
            .ok_or_else(|| AIError::InvalidResponse("Missing content in response".to_string()))?;
        self.parse_ai_response(content)
    }

    fn parse_ai_response(&self, content: &str) -> Result<AIEditResponse, AIError> {
        // Try to parse as JSON
        let trimmed = content.trim();
        
        // Handle potential markdown code block wrapping
        let json_str = if trimmed.starts_with("```json") {
            trimmed.trim_start_matches("```json").trim_end_matches("```").trim()
        } else if trimmed.starts_with("```") {
            trimmed.trim_start_matches("```").trim_end_matches("```").trim()
        } else {
            trimmed
        };

        // Try to extract JSON object from the response
        let json_value: serde_json::Value = serde_json::from_str(json_str)
            .or_else(|_| {
                // Try to find JSON object in the text
                let start = json_str.find('{');
                let end = json_str.rfind('}').map(|i| i + 1);
                if let (Some(s), Some(e)) = (start, end) {
                    serde_json::from_str(&json_str[s..e])
                } else {
                    Err(DeError::custom("No JSON object found"))
                }
            })
            .map_err(|e| AIError::InvalidResponse(format!("Failed to parse response: {}", e)))?;

        Ok(AIEditResponse {
            summary: json_value["summary"].as_str().unwrap_or("修改完成").to_string(),
            content: json_value["content"].as_str().unwrap_or("").to_string(),
            rules_applied: json_value["rules_applied"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default(),
        })
    }
}
