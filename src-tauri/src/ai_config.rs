use crate::ai;
use crate::commands::{ApiConfig, Settings};
use crate::cloud::CloudClient;

#[used]
static _MARKER_QQXX42_TENIENT_TOKEN_QQXX42_STATIC: &str = "MARKER_QQXX42_TENIENT_TOKEN_QQXX42_marker_in_ai_config_top";
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

// ── Provider / config builders ─────────────────────────────────────────────────

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

// ── Cloud config resolver ────────────────────────────────────────────────────────

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

// ── AIConfigResolver ───────────────────────────────────────────────────────────

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

/// Test an image generation provider by sending a minimal generation request.
/// For Ollama we hit `/api/generate` with a trivial prompt; for
/// OpenAI-compatible we hit `/v1/images/generations` with `n=1` and the
/// smallest standard size.
///
/// Ollama: The response may contain base64 images — we don't decode them,
/// just verify the call succeeds and the `images` array is present.
///
/// OpenAI-compatible: We request `b64_json` format so we can verify the
/// response structure without needing to write a temp file.
///
/// Returns `Ok(AITestResult)` on a successful round-trip, `Err` on any
/// network or protocol error. The result carries the provider's human-readable
/// response (e.g. "Ollama responded in 1.2s") on success.
pub async fn test_image_gen_provider_impl(
    provider_id: &str,
    api_key: Option<&str>,
    base_url: &str,
    model: &str,
    secret_id: Option<&str>,
    secret_key: Option<&str>,
    region: Option<&str>,
) -> Result<AITestResult, AIConfigError> {
    let start = std::time::Instant::now();

    tracing::info!("test_image_gen_provider_impl: provider_id={:?} model={:?} base_url={:?}", provider_id, model, base_url);
    match provider_id {
        "ollama" => {
            let url = format!("{}/api/generate", base_url.trim_end_matches('/'));
            let body = serde_json::json!({
                "model": model,
                "prompt": "a small red circle",
                "stream": false,
            });
            let client = reqwest::Client::new();
            let response = client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| AIConfigError::Network(format!(
                    "Failed to connect to Ollama at {}: {}", base_url, e
                )))?;

            if !response.status().is_success() {
                let status = response.status();
                let body_text = response.text().await.unwrap_or_default();
                return Ok(AITestResult {
                    success: false,
                    message: format!(
                        "Ollama 返回 HTTP {} — 请确认服务已运行 (`ollama serve`) \
                         且模型 '{}' 已拉取 (`ollama pull {}`): {}",
                        status, model, model, body_text
                    ),
                });
            }

            let json: serde_json::Value = response
                .json()
                .await
                .map_err(|e| AIConfigError::ParseResponse(format!(
                    "Ollama response was not valid JSON: {}", e
                )))?;

            let elapsed_ms = start.elapsed().as_millis() as u64;
            if json.get("images").is_some() {
                Ok(AITestResult {
                    success: true,
                    message: format!(
                        "Ollama 连接成功，模型 '{}' 可用，耗时 {}ms",
                        model, elapsed_ms
                    ),
                })
            } else {
                Ok(AITestResult {
                    success: false,
                    message: format!(
                        "Ollama 连接成功但响应中无 'images' 字段 — \
                         '{}' 可能不是图像生成模型",
                        model
                    ),
                })
            }
        }
        "tencent_token" => {
            // Tencent Token Hub uses Bearer auth, same as OpenAI, but hits
            // a different path.
            tracing::info!("[tencent_token branch] using path /v1/api/image/lite");
            let base_url = base_url.trim_end_matches('/');
            let url = format!("{}/v1/api/image/lite", base_url);
            let body = serde_json::json!({
                "model": model,
                "prompt": "a small red circle",
                "rsp_img_type": "url",
            });

            let mut request = crate::ai::HTTP_CLIENT
                .post(&url)
                .header("Content-Type", "application/json");
            if let Some(key) = api_key.filter(|k| !k.is_empty()) {
                request = request.header("Authorization", format!("Bearer {}", key));
            }

            let response = request
                .json(&body)
                .send()
                .await
                .map_err(|e| AIConfigError::Network(format!(
                    "Failed to connect to Tencent Token Hub at {}: {}", url, e
                )))?;

            if !response.status().is_success() {
                let status = response.status();
                let body_text = response.text().await.unwrap_or_default();
                return Ok(AITestResult {
                    success: false,
                    message: format!(
                        "腾讯 Token Hub 返回 HTTP {}: {}",
                        status, body_text
                    ),
                });
            }

            let json: serde_json::Value = response
                .json()
                .await
                .map_err(|e| AIConfigError::ParseResponse(format!(
                    "Response was not valid JSON: {}", e
                )))?;

            let elapsed_ms = start.elapsed().as_millis() as u64;
            let has_image = json
                .pointer("/image_url")
                .and_then(|v| v.as_str())
                .is_some()
                || json
                    .pointer("/data/0/url")
                    .and_then(|v| v.as_str())
                    .is_some();

            if has_image {
                Ok(AITestResult {
                    success: true,
                    message: format!(
                        "腾讯 Token Hub 连接成功，模型 '{}' 可用，耗时 {}ms",
                        model, elapsed_ms
                    ),
                })
            } else {
                Ok(AITestResult {
                    success: false,
                    message: format!(
                        "API 调用成功但未返回图片 — 请检查模型名称 '{}' \
                         是否正确，以及账户是否有权访问该模型",
                        model
                    ),
                })
            }
        }
        "tencent_tc3" => {
            // Tencent Cloud authenticates with TC3-HMAC-SHA256, not
            // Bearer. We re-derive the signing key here so the test
            // exercises the same code path as a real call.
            let secret_id = match secret_id.filter(|s| !s.is_empty()) {
                Some(s) => s,
                None => {
                    return Ok(AITestResult {
                        success: false,
                        message: "未配置 SecretId — 请在腾讯云控制台 \
                                  (https://console.cloud.tencent.com/cam/capi) \
                                  创建 API 密钥并填入"
                            .to_string(),
                    });
                }
            };
            let secret_key = match secret_key.filter(|s| !s.is_empty()) {
                Some(s) => s,
                None => {
                    return Ok(AITestResult {
                        success: false,
                        message: "未配置 SecretKey — 请在腾讯云控制台 \
                                  (https://console.cloud.tencent.com/cam/capi) \
                                  创建 API 密钥并填入"
                            .to_string(),
                    });
                }
            };
            let region = region.unwrap_or("ap-guangzhou");
            let payload = serde_json::json!({
                "Prompt": "a small red circle",
                "RspImgType": "url",
                "Width": 256,
                "Height": 256,
                "Model": model,
            })
            .to_string();

            // Reuse the TC3 signer from the image-gen tool. We import
            // the function inline so the test path doesn't have to
            // dance around module visibility.
            let now = chrono::Utc::now().timestamp();
            let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
            let auth = crate::agent::tools::image_gen_tools::sign_tencent_request(
                secret_id,
                secret_key,
                "aiart.tencentcloudapi.com",
                "aiart",
                "TextToImageLite",
                region,
                &payload,
                now,
                &date,
            );

            let auth = match auth {
                Ok(s) => s,
                Err(e) => {
                    return Ok(AITestResult {
                        success: false,
                        message: format!("签名失败: {}", e),
                    });
                }
            };

            let url = "https://aiart.tencentcloudapi.com/";
            let response = crate::ai::HTTP_CLIENT
                .post(url)
                .header("Authorization", auth)
                .header("Content-Type", "application/json; charset=utf-8")
                .header("Host", "aiart.tencentcloudapi.com")
                .header("X-TC-Action", "TextToImageLite")
                .header("X-TC-Timestamp", now.to_string())
                .header("X-TC-Version", "2023-09-01")
                .header("X-TC-Region", region)
                .body(payload)
                .send()
                .await
                .map_err(|e| AIConfigError::Network(format!(
                    "Failed to connect to Tencent Cloud: {}", e
                )))?;

            if !response.status().is_success() {
                let status = response.status();
                let body_text = response.text().await.unwrap_or_default();
                return Ok(AITestResult {
                    success: false,
                    message: format!(
                        "腾讯云返回 HTTP {}: {}。请检查 SecretId/SecretKey \
                         是否正确，以及账户是否开通了 aiart 服务",
                        status, body_text
                    ),
                });
            }

            let json: serde_json::Value = response
                .json()
                .await
                .map_err(|e| AIConfigError::ParseResponse(format!(
                    "Tencent response was not valid JSON: {}", e
                )))?;

            let result_image = json
                .pointer("/Response/ResultImage")
                .and_then(|v| v.as_str());

            let elapsed_ms = start.elapsed().as_millis() as u64;
            if result_image.is_some() {
                Ok(AITestResult {
                    success: true,
                    message: format!(
                        "腾讯云连接成功，模型 '{}' 可用，耗时 {}ms",
                        model, elapsed_ms
                    ),
                })
            } else {
                Ok(AITestResult {
                    success: false,
                    message: format!(
                        "腾讯云返回成功但未包含 ResultImage：{}",
                        json
                    ),
                })
            }
        }
        // OpenAI-compatible path: any non-Ollama / non-tencent provider id
        "custom" | "openai" => {
            let url = format!("{}/images/generations", base_url.trim_end_matches('/'));
            let body = serde_json::json!({
                "model": model,
                "prompt": "a small red circle",
                "n": 1,
                "size": "256x256",
                "response_format": "b64_json",
            });

            let mut request = crate::ai::HTTP_CLIENT
                .post(&url)
                .header("Content-Type", "application/json");
            if let Some(key) = api_key.filter(|k| !k.is_empty()) {
                request = request.header("Authorization", format!("Bearer {}", key));
            }

            let response = request
                .json(&body)
                .send()
                .await
                .map_err(|e| AIConfigError::Network(format!(
                    "Failed to connect to {}: {}", base_url, e
                )))?;

            if !response.status().is_success() {
                let status = response.status();
                let body_text = response.text().await.unwrap_or_default();
                return Ok(AITestResult {
                    success: false,
                    message: format!(
                        "图像 API 返回 HTTP {}: {}",
                        status, body_text
                    ),
                });
            }

            let json: serde_json::Value = response
                .json()
                .await
                .map_err(|e| AIConfigError::ParseResponse(format!(
                    "API response was not valid JSON: {}", e
                )))?;

            let images = json.get("data")
                .and_then(|d| d.as_array())
                .and_then(|arr| arr.first())
                .and_then(|item| item.get("b64_json"))
                .and_then(|b64| b64.as_str());

            let elapsed_ms = start.elapsed().as_millis() as u64;
            if images.is_some() {
                Ok(AITestResult {
                    success: true,
                    message: format!(
                        "连接成功！模型 '{}' 可用，耗时 {}ms",
                        model, elapsed_ms
                    ),
                })
            } else {
                Ok(AITestResult {
                    success: false,
                    message: format!(
                        "API 调用成功但未返回图片 — 请检查模型名称 '{}' \
                         是否正确，以及账户是否有权访问该模型",
                        model
                    ),
                })
            }
        }
        // Catch-all for any provider_id not handled above
        other => {
            Ok(AITestResult {
                success: false,
                message: format!("未识别的 provider 类型: '{}'", other),
            })
        }
    }
}

