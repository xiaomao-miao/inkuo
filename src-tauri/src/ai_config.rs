use crate::ai;
use crate::commands::{ApiConfig, Settings};
use crate::cloud::CloudClient;
use reqwest::{Client, RequestBuilder, Response};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
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
    #[error("Provider '{provider}' requires an API key but none was provided")]
    MissingApiKey { provider: String },
    #[error("Cloud routing failed: {0}")]
    Cloud(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AIProviderKind {
    OpenAI,
    DeepSeek,
    Ollama,
    Official,
    /// Routes through the user's inkuo Cloud account. `base_url` is
    /// `<cloud_server>/v1` and `api_key` is the user's JWT — the
    /// server validates the token and forwards to the upstream LLM.
    /// Always uses the OpenAI wire protocol underneath, so this branch
    /// is identical to `OpenAI` from `ai::AIProvider`'s point of view.
    Cloud,
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
            Self::Cloud => "cloud",
        }
    }
}

impl fmt::Display for AIProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn build_provider(
    provider: AIProviderKind,
    api_key: Option<&str>,
    base_url: Option<&str>,
) -> Result<ai::AIProvider, AIConfigError> {
    // OpenAI-compatible cloud providers require an API key. Returning an
    // error here (instead of silently sending `Authorization: Bearer ` with
    // an empty token, which produces confusing upstream 401s) lets callers
    // surface a clear "missing API key" message to the user.
    let require_key = matches!(
        provider,
        AIProviderKind::OpenAI | AIProviderKind::DeepSeek | AIProviderKind::Official | AIProviderKind::Cloud
    );
    let key_is_empty = api_key.map(str::trim).map_or(true, str::is_empty);
    if require_key && key_is_empty {
        return Err(AIConfigError::MissingApiKey {
            provider: provider.as_str().to_string(),
        });
    }

    let key = api_key.unwrap_or_default().to_string();
    let resolve_base = |url: Option<&str>, fallback: &'static str| {
        url.map(str::to_string)
            .unwrap_or_else(|| fallback.to_string())
    };

    Ok(match provider {
        AIProviderKind::OpenAI => ai::AIProvider::OpenAI {
            api_key: key,
            base_url: resolve_base(base_url, DEFAULT_OPENAI_BASE_URL),
        },
        AIProviderKind::DeepSeek => ai::AIProvider::OpenAI {
            api_key: key,
            base_url: resolve_base(base_url, DEFAULT_DEEPSEEK_BASE_URL),
        },
        AIProviderKind::Ollama => ai::AIProvider::Ollama {
            base_url: resolve_base(base_url, DEFAULT_OLLAMA_BASE_URL),
        },
        AIProviderKind::Official => ai::AIProvider::Official { api_key: key },
        // Cloud routes use the OpenAI wire protocol, just with the
        // user's cloud server as the base URL and a JWT as the api_key.
        // `require_key` above treats it like a cloud provider because
        // the server returns 401 for an empty token.
        AIProviderKind::Cloud => ai::AIProvider::OpenAI {
            api_key: key,
            base_url: resolve_base(
                base_url,
                "https://cloud.inkuo.com/v1",
            ),
        },
    })
}

pub fn active_api_config<'a>(settings: &'a Settings) -> Option<&'a ApiConfig> {
    let active_id = settings.active_api_config_id.as_ref()?;
    settings.api_configs.iter().find(|config| config.id == *active_id)
}

pub fn build_provider_from_api_config(config: &ApiConfig) -> Result<ai::AIProvider, AIConfigError> {
    build_provider(
        config.provider,
        config.api_key.as_deref(),
        Some(config.base_url.as_str()),
    )
}

pub fn build_settings_ai_config(settings: &Settings) -> Result<ai::AIConfig, AIConfigError> {
    // Cloud-mode branch: when the user has opted into cloud mode AND has
    // a logged-in account AND has selected an active cloud model, route
    // through the inkuo Cloud Server with the user's JWT. The actual HTTP
    // request uses the regular OpenAI wire protocol — only the base URL
    // and auth token differ.
    if settings.cloud.cloud_mode_enabled {
        if let (Some(account), Some(model_id)) = (
            settings.cloud.account.as_ref(),
            settings.cloud.active_cloud_model_id.as_ref(),
        ) {
            let entry = settings
                .cloud
                .cached_models
                .iter()
                .find(|m| &m.id == model_id);

            if let Some(entry) = entry {
                return Ok(ai::AIConfig {
                    provider: ai::AIProvider::OpenAI {
                        api_key: account.access_token.clone(),
                        base_url: format!(
                            "{}/v1",
                            account.base_url.trim_end_matches('/')
                        ),
                    },
                    model: entry.id.clone(),
                    temperature: 0.7,
                    max_tokens: None,
                });
            }
        }
    }

    // Local-mode branch (unchanged): pick the active API config and
    // build a provider from it. The cloud branch above returns early so
    // no existing local configuration is touched when the user is in
    // cloud mode.
    let config = active_api_config(settings)
        .or_else(|| settings.api_configs.iter().find(|config| config.enabled))
        .or_else(|| settings.api_configs.first())
        .ok_or_else(|| AIConfigError::MissingApiKey {
            provider: "none".to_string(),
        })?;

    Ok(ai::AIConfig {
        provider: build_provider_from_api_config(config)?,
        model: config.model.clone(),
        temperature: config.temperature,
        max_tokens: config.max_tokens,
    })
}

pub fn build_input_ai_config(config_input: AIConfigInput) -> Result<ai::AIConfig, AIConfigError> {
    Ok(ai::AIConfig {
        provider: build_provider(
            config_input.provider,
            config_input.api_key.as_deref(),
            config_input.base_url.as_deref(),
        )?,
        model: config_input.model,
        temperature: config_input.temperature.unwrap_or(0.7),
        max_tokens: config_input.max_tokens,
    })
}

/// Async equivalent of `build_input_ai_config` that re-resolves the
/// access token for cloud-mode inputs via `CloudClient`. Used by the
/// agent command: the frontend sends its current snapshot of the
/// access token, but a long-lived session can outlive the token's
/// TTL, so we always go through the in-process `CloudClient` to
/// pick up a fresh (possibly just-refreshed) token instead of
/// trusting the snapshot.
///
/// In local mode the input is forwarded as-is (local API keys
/// don't expire so caching them per-session is safe).
pub async fn build_input_ai_config_async(
    config_input: AIConfigInput,
    cloud: Arc<CloudClient>,
) -> Result<ai::AIConfig, AIConfigError> {
    if matches!(config_input.provider, AIProviderKind::Cloud) {
        let settings = crate::commands::get_settings_cached().map_err(|e| {
            AIConfigError::Cloud(format!("read settings for cloud re-resolve: {}", e))
        })?;
        let entry = settings
            .cloud
            .cached_models
            .iter()
            .find(|m| m.id == config_input.model)
            .ok_or_else(|| {
                AIConfigError::Cloud(format!(
                    "cloud input model '{}' is not in cached_models; \
                     refresh the cloud account from the settings panel",
                    config_input.model
                ))
            })?;
        let (provider, model_id) = cloud
            .build_ai_config_for_model(entry)
            .await
            .map_err(|e| AIConfigError::Cloud(e.to_string()))?;
        return Ok(ai::AIConfig {
            provider,
            model: model_id,
            temperature: config_input.temperature.unwrap_or(0.7),
            max_tokens: config_input.max_tokens,
        });
    }
    build_input_ai_config(config_input)
}

/// Async resolver that produces a *fresh* `AIConfig` on demand.
///
/// ## Why
///
/// The previous design cached a single `AIConfig` (cloning
/// `settings.cloud.account.access_token` into the `api_key` field) and
/// reused it across requests. That meant a short-lived `access_token`
/// silently went stale in the cache and every chat / stream call started
/// failing with a 401 after the TTL — without any auto-refresh, because
/// the cache snapshot never went through `CloudClient::ensure_fresh_token`.
///
/// The resolver closes that gap:
///  - **Cloud mode**: each `resolve()` asks `CloudClient` for a fresh
///    token, which transparently refreshes the access token via the
///    refresh token when it is near expiry.
///  - **Local mode**: falls back to the cached settings-based config
///    (the previous behaviour); local API keys don't expire so caching
///    is safe.
///
/// Holding an `Arc<CloudClient>` (not a raw reference) means the
/// resolver can be cloned cheaply and shared across threads.
pub struct AIConfigResolver {
    cloud: Arc<CloudClient>,
}

impl AIConfigResolver {
    pub fn new(cloud: CloudClient) -> Self {
        Self { cloud: Arc::new(cloud) }
    }

    /// Build the AIConfig appropriate for the current settings. In
    /// cloud mode this hits `CloudClient` to obtain a fresh access
    /// token (refreshing if needed); in local mode it reads the
    /// cached settings and picks the active API config.
    ///
    /// Returns `Err(AIConfigError::Cloud(...))` when the user is in
    /// cloud mode but not logged in (the previous behaviour was a
    /// silent fallback to local — which produced confusing "why is
    /// my cloud account not being used?" bug reports).
    pub async fn resolve(&self) -> Result<ai::AIConfig, AIConfigError> {
        let settings = crate::commands::get_settings_cached()
            .map_err(|e| AIConfigError::Cloud(format!("read settings: {}", e)))?;

        if settings.cloud.cloud_mode_enabled {
            let entry = settings
                .cloud
                .cached_models
                .iter()
                .find(|m| {
                    settings
                        .cloud
                        .active_cloud_model_id
                        .as_deref()
                        .map(|id| id == m.id)
                        .unwrap_or(false)
                })
                .ok_or_else(|| {
                    AIConfigError::Cloud(
                        "cloud mode is enabled but no active cloud model is selected; \
                         open the Cloud settings panel and pick a model"
                            .to_string(),
                    )
                })?;

            let (provider, model_id) = self
                .cloud
                .build_ai_config_for_model(entry)
                .await
                .map_err(|e| {
                    tracing::warn!(
                        "cloud AI config build failed: {}; falling back to local \
                         is no longer automatic because cloud_mode_enabled is true",
                        e
                    );
                    AIConfigError::Cloud(e.to_string())
                })?;

            return Ok(ai::AIConfig {
                provider,
                model: model_id,
                temperature: 0.7,
                max_tokens: None,
            });
        }

        // Local-mode branch: behaviour unchanged.
        build_settings_ai_config(&settings)
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
    // Reuse the shared `HTTP_CLIENT` from `ai` so we keep-alive connections
    // and DNS cache across calls. Building a fresh `reqwest::Client` per
    // test would re-do the TLS handshake every time.
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "user", "content": "Say 'Hello, connection successful!' in exactly those words."}
        ],
        "max_tokens": 50,
    });

    let response = build_test_request(&crate::ai::HTTP_CLIENT, api_key, &url)
        .json(&body)
        .send()
        .await
        .map_err(|e| AIConfigError::Network(e.to_string()))?;

    parse_test_response(response).await
}

