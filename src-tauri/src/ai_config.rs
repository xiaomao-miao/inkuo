use crate::ai;
use crate::commands::{ApiConfig, Settings};
use reqwest::{Client, RequestBuilder, Response};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
const DEFAULT_OLLAMA_BASE_URL: &str = "http://localhost:11434";

#[derive(Debug, Clone, Error)]
pub enum AIConfigError {
    #[error("Failed to parse response: {0}")]
    ParseResponse(String),
    #[error("Network error: {0}")]
    Network(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AIProviderKind {
    OpenAI,
    DeepSeek,
    Ollama,
    Official,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIConfigInput {
    pub provider: AIProviderKind,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestApiConfigRequest {
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub provider: AIProviderKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AITestResult {
    pub success: bool,
    pub message: String,
}

impl Default for AIProviderKind {
    fn default() -> Self {
        Self::DeepSeek
    }
}

impl AIProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::DeepSeek => "deepseek",
            Self::Ollama => "ollama",
            Self::Official => "official",
        }
    }

    pub fn default_base_url(self) -> Option<&'static str> {
        match self {
            Self::OpenAI => Some(DEFAULT_OPENAI_BASE_URL),
            Self::DeepSeek => Some(DEFAULT_DEEPSEEK_BASE_URL),
            Self::Ollama => Some(DEFAULT_OLLAMA_BASE_URL),
            Self::Official => None,
        }
    }
}

impl fmt::Display for AIProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn default_base_url(provider: AIProviderKind) -> &'static str {
    provider.default_base_url().unwrap_or(DEFAULT_DEEPSEEK_BASE_URL)
}

pub fn build_provider(
    provider: AIProviderKind,
    api_key: Option<&str>,
    base_url: Option<&str>,
) -> ai::AIProvider {
    match provider {
        AIProviderKind::OpenAI => ai::AIProvider::OpenAI {
            api_key: api_key.unwrap_or_default().to_string(),
            base_url: base_url
                .map(str::to_string)
                .unwrap_or_else(|| default_base_url(AIProviderKind::OpenAI).to_string()),
        },
        AIProviderKind::DeepSeek => ai::AIProvider::OpenAI {
            api_key: api_key.unwrap_or_default().to_string(),
            base_url: base_url
                .map(str::to_string)
                .unwrap_or_else(|| default_base_url(AIProviderKind::DeepSeek).to_string()),
        },
        AIProviderKind::Ollama => ai::AIProvider::Ollama {
            base_url: base_url
                .map(str::to_string)
                .unwrap_or_else(|| default_base_url(AIProviderKind::Ollama).to_string()),
        },
        AIProviderKind::Official => ai::AIProvider::Official {
            api_key: api_key.unwrap_or_default().to_string(),
        },
    }
}

pub fn active_api_config<'a>(settings: &'a Settings) -> Option<&'a ApiConfig> {
    let active_id = settings.active_api_config_id.as_ref()?;
    settings.api_configs.iter().find(|config| config.id == *active_id)
}

pub fn build_provider_from_api_config(config: &ApiConfig) -> ai::AIProvider {
    build_provider(
        config.provider,
        config.api_key.as_deref(),
        Some(config.base_url.as_str()),
    )
}

pub fn build_settings_ai_config(settings: &Settings) -> ai::AIConfig {
    let config = active_api_config(settings)
        .or_else(|| settings.api_configs.iter().find(|config| config.enabled))
        .or_else(|| settings.api_configs.first())
        .expect("settings should always contain at least one API config");

    ai::AIConfig {
        provider: build_provider_from_api_config(config),
        model: config.model.clone(),
        temperature: config.temperature,
        max_tokens: config.max_tokens,
    }
}

pub fn build_input_ai_config(config_input: AIConfigInput) -> ai::AIConfig {
    ai::AIConfig {
        provider: build_provider(
            config_input.provider,
            config_input.api_key.as_deref(),
            config_input.base_url.as_deref(),
        ),
        model: config_input.model,
        temperature: config_input.temperature.unwrap_or(0.7),
        max_tokens: config_input.max_tokens,
    }
}

fn build_test_request(client: &Client, api_key: Option<&str>, url: &str) -> RequestBuilder {
    let mut request = client.post(url);

    if let Some(key) = api_key.filter(|key| !key.is_empty()) {
        request = request.header("Authorization", format!("Bearer {}", key));
    }

    request.header("Content-Type", "application/json")
}

async fn parse_test_response(response: Response) -> Result<AITestResult, AIConfigError> {
    if response.status().is_success() {
        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AIConfigError::ParseResponse(e.to_string()))?;

        if let Some(content) = response_json["choices"][0]["message"]["content"].as_str() {
            Ok(AITestResult {
                success: true,
                message: format!("连接成功！AI 回复: {}", content),
            })
        } else {
            Ok(AITestResult {
                success: true,
                message: "连接成功！".to_string(),
            })
        }
    } else {
        let status = response.status();
        let error_text = match response.text().await {
            Ok(text) => text,
            Err(error) => {
                tracing::warn!(
                    "Failed to read AI test error response body (status {}): {}",
                    status,
                    error
                );
                String::new()
            }
        };

        Ok(AITestResult {
            success: false,
            message: format!("连接失败 (HTTP {}): {}", status.as_u16(), error_text),
        })
    }
}

pub async fn test_ai_connection_impl(
    api_key: Option<&str>,
    base_url: &str,
    model: &str,
) -> Result<AITestResult, AIConfigError> {
    let client = Client::new();
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "user", "content": "Say 'Hello, connection successful!' in exactly those words."}
        ],
        "max_tokens": 50,
    });

    let response = build_test_request(&client, api_key, &url)
        .json(&body)
        .send()
        .await
        .map_err(|e| AIConfigError::Network(e.to_string()))?;

    parse_test_response(response).await
}
