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

// ============================================================================
// Prompts - loaded from markdown files at compile time
// ============================================================================

/// System prompt for ask mode (conversational Q&A)
fn get_ask_prompt() -> &'static str {
    include_str!("../prompts/ask.md")
}

/// System prompt for plan mode (structured planning)
fn get_plan_prompt() -> &'static str {
    include_str!("../prompts/plan.md")
}

/// System prompt for knowledge mode (retrieval-grounded Q&A)
fn get_knowledge_prompt() -> &'static str {
    include_str!("../prompts/knowledge.md")
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
            max_tokens: Some(4096),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIStreamEvent {
    pub event_type: StreamEventType,
    pub content: Option<String>,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEventType {
    Text,
    Summary,
    Error,
}

static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
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

    /// Build chat request body with thinking disabled (for inline completion)
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

    pub async fn chat_stream<F>(
        &self,
        mode: String,
        instruction: String,
        original_text: String,
        mut on_delta: F,
    ) -> Result<String, AIError>
    where
        F: FnMut(String) + Send,
    {
        let system_prompt = match mode.as_str() {
            "plan" => get_plan_prompt(),
            "knowledge" => get_knowledge_prompt(),
            _ => get_ask_prompt(),
        };

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
            // Streaming for Ollama not implemented in this step
            AIProvider::Ollama { base_url } => {
                let text = self.call_ollama_chat(base_url, system_prompt, &user_prompt).await?;
                on_delta(text.clone());
                Ok(text)
            }
            AIProvider::Official { api_key } => {
                let base_url = "https://api.inkuo.com/v1";
                self.call_openai_compatible_chat_stream(api_key, base_url, system_prompt, &user_prompt, &mut on_delta)
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
            // Streaming for Ollama not implemented in this step
            AIProvider::Ollama { base_url } => {
                let resp = self.call_ollama(base_url, system_prompt, &user_prompt).await?;
                on_delta(resp.content.clone());
                Ok(resp)
            }
            AIProvider::Official { api_key } => {
                let base_url = "https://api.inkuo.com/v1";
                self.call_openai_compatible_edit_stream(api_key, base_url, system_prompt, &user_prompt, &mut on_delta)
                    .await
            }
        }
    }

    pub async fn chat(&self, mode: String, instruction: String, original_text: String) -> Result<String, AIError> {
        let system_prompt = match mode.as_str() {
            "plan" => get_plan_prompt(),
            "knowledge" => get_knowledge_prompt(),
            _ => get_ask_prompt(),
        };

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
                self.call_openai_compatible_chat(api_key, base_url, system_prompt, &user_prompt)
                    .await
            }
            AIProvider::Ollama { base_url } => self.call_ollama_chat(base_url, system_prompt, &user_prompt).await,
            AIProvider::Official { api_key } => {
                let base_url = "https://api.inkuo.com/v1";
                self.call_openai_compatible_chat(api_key, base_url, system_prompt, &user_prompt)
                    .await
            }
        }
    }

    /// Direct chat call with thinking disabled - optimized for inline completion
    pub async fn completion(&self, system_prompt: &str, user_prompt: &str) -> Result<String, AIError> {
        match &self.config.provider {
            AIProvider::OpenAI { api_key, base_url } => {
                self.call_openai_compatible_chat_no_thinking(api_key, base_url, system_prompt, user_prompt)
                    .await
            }
            AIProvider::Ollama { base_url } => self.call_ollama_chat(base_url, system_prompt, user_prompt).await,
            AIProvider::Official { api_key } => {
                let base_url = "https://api.inkuo.com/v1";
                self.call_openai_compatible_chat_no_thinking(api_key, base_url, system_prompt, user_prompt)
                    .await
            }
        }
    }

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
                // Official provider uses inkuo's gateway
                let base_url = "https://api.inkuo.com/v1";
                self.call_openai_compatible(api_key, base_url, system_prompt, &user_prompt).await
            }
        }
    }
    
    async fn call_openai_compatible_chat(
        &self,
        api_key: &str,
        base_url: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AIError> {
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

        Ok(content.to_string())
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

    async fn call_ollama_chat(
        &self,
        base_url: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AIError> {
        self.call_ollama_chat_only(base_url, system_prompt, user_prompt).await
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

    async fn call_ollama_chat_only(&self, base_url: &str, system_prompt: &str, user_prompt: &str) -> Result<String, AIError> {
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

    async fn call_ollama(
        &self,
        base_url: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<AIEditResponse, AIError> {
        let content = self.call_ollama_chat_only(base_url, system_prompt, user_prompt).await?;
        self.parse_ai_response(&content)
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
