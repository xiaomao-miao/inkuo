//! AI Provider adapter module
//! 
//! Handles:
//! - OpenAI-compatible API (DeepSeek, etc.)
//! - Ollama (local models)
//! - Unified streaming protocol

use serde::{Deserialize, Serialize, de::Error as DeError};
use thiserror::Error;

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
            provider: AIProvider::OpenAI {
                api_key: "sk-e5481952d6a44ede81163206a97aabc8".to_string(),
                base_url: "https://api.deepseek.com".to_string(),
            },
            model: "deepseek-chat".to_string(),
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

pub struct AIProviderAdapter {
    config: AIConfig,
    client: reqwest::Client,
}

impl AIProviderAdapter {
    pub fn new(config: AIConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }
    
    pub async fn edit(&self, request: AIEditRequest) -> Result<AIEditResponse, AIError> {
        let system_prompt = r#"You are a document editing assistant. Your task is to modify the provided text according to the user's instruction.

You MUST respond with a valid JSON object containing:
{
    "summary": "One sentence describing what you changed and why",
    "content": "The modified text (complete, not truncated)",
    "rules_applied": ["List of constraints you followed"]
}

Important rules:
1. Preserve all numbers, dates, code blocks, and technical terms
2. Do not change the meaning or facts in the text
3. Keep the same language as the original text
4. Output valid JSON only, no additional text"#;

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
    
    async fn call_openai_compatible(
        &self,
        api_key: &str,
        base_url: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<AIEditResponse, AIError> {
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "temperature": self.config.temperature,
            "max_tokens": self.config.max_tokens,
        });

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AIError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            if status.as_u16() == 401 {
                return Err(AIError::AuthError("Invalid API key".to_string()));
            } else if status.as_u16() == 429 {
                return Err(AIError::RateLimited);
            }
            return Err(AIError::ModelError(format!("HTTP {}", status)));
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
        
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "stream": false,
            "options": {
                "temperature": self.config.temperature,
            }
        });

        let response = self.client
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
