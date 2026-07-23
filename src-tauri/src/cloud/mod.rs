//! inkuo Cloud client
//!
//! Owns the user-facing cloud mode in inkuo: auth (register / login / refresh),
//! model list pull, and a thin client around the upstream-agnostic OpenAI-style
//! chat endpoint exposed by `inkuo Cloud Server`.
//!
//! Design intent: this module is intentionally a **client** — it never
//! reaches into the `Settings` or `ai_config` modules. Callers in the rest of
//! the codebase (commands_stream, commands_agent, inline_complete, the
//! settings store) decide when to invoke it. This keeps the dependency
//! direction one-way and makes the existing local-mode code paths completely
//! untouched at rest.
//!
//! Auth tokens live on a single shared `CloudAccount` struct that the
//! frontend keeps in its settings JSON. We never write them to disk from
//! the Rust side — the frontend store is the source of truth.

use crate::ai_config::AIProviderKind;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum CloudError {
    #[error("Not logged in")]
    NotLoggedIn,
    #[error("Network error: {0}")]
    Network(String),
    #[error("Server returned {status}: {body}")]
    Server { status: u16, body: String },
    #[error("Authentication failed")]
    AuthFailed,
    #[error("Quota exhausted")]
    QuotaExhausted,
    #[error("Invalid invite code")]
    InvalidInviteCode,
    #[error("Server returned malformed response: {0}")]
    Parse(String),
    #[error("{0}")]
    Other(String),
}

/// Internal bucketing of a single `/auth/refresh` response. Used by
/// `ensure_fresh_token` to decide between "kill the session" and
/// "retry once more". Never escapes the `cloud` module.
enum RefreshAttemptError {
    /// Server rejected the refresh token (401 / 403). The stored account
    /// is no longer usable and the caller should re-authenticate.
    AuthFailed {
        status: reqwest::StatusCode,
        body: String,
    },
    /// Transient failure: 5xx, 429, network error, malformed body, or
    /// any non-auth status. Worth a single retry; on second failure we
    /// surface the error to the caller without clearing the account.
    Retriable {
        status: reqwest::StatusCode,
        body: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CloudAccount {
    /// Base URL of the inkuo Cloud Server, e.g. `https://cloud.example.com`.
    /// Stored without a trailing slash.
    pub base_url: String,
    pub email: String,
    pub user_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub access_expires_at: chrono::DateTime<chrono::Utc>,
    pub plan_name: Option<String>,
    pub balance_cents: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudModelEntry {
    /// Server-side `model_config.id` (Guid). Use this as the `model` value
    /// when calling `/v1/chat/completions` so the server can look up the
    /// upstream mapping without ambiguity.
    pub id: String,
    pub display_name: String,
    pub model_name: String,
    pub provider: String,
    pub input_price_per_m_tokens: f64,
    pub output_price_per_m_tokens: f64,
    /// Cheap price applied to the cached slice of the prompt (e.g. OpenAI
    /// `prompt_tokens_details.cached_tokens`). 0 means the upstream does
    /// not cache-bill (fall back to input price in the Rust billing math
    /// is unnecessary — the server already does that).
    pub cached_input_price_per_m_tokens: f64,
    pub description: Option<String>,
    /// Identifies which `AIProviderKind` variant the *frontend* should
    /// surface to the AI panel. The Rust side normalizes Cloud → OpenAI
    /// for streaming, so this is mostly metadata for the UI.
    pub provider_kind: AIProviderKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudAccountInfo {
    pub id: String,
    pub email: String,
    pub balance_cents: f64,
    pub plan_name: Option<String>,
    pub monthly_token_limit: i64,
    pub subscription_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub tokens_used_this_month: i64,
    pub monthly_tokens_remaining: i64,
}

// ---- Wire format types --------------------------------------------------------

#[derive(Serialize)]
struct RegisterRequest<'a> {
    invite_code: &'a str,
    email: &'a str,
    password: &'a str,
}

#[derive(Serialize)]
struct LoginRequest<'a> {
    email: &'a str,
    password: &'a str,
}

#[derive(Serialize)]
struct RefreshRequest<'a> {
    refresh_token: &'a str,
}

#[derive(Deserialize)]
struct AuthUserDto {
    id: String,
    email: String,
    balance_cents: f64,
    plan_name: Option<String>,
    subscription_expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize)]
struct AuthResponse {
    access_token: String,
    refresh_token: String,
    expires_at: chrono::DateTime<chrono::Utc>,
    user: AuthUserDto,
}

#[derive(Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
struct ModelsEnvelope {
    #[serde(default)]
    #[allow(non_snake_case)]
    data: Vec<ModelDto>,
}

#[derive(Deserialize)]
struct ModelDto {
    id: String,
    display_name: String,
    model_name: String,
    provider: String,
    input_price_per_m_tokens: f64,
    output_price_per_m_tokens: f64,
    cached_input_price_per_m_tokens: f64,
    description: Option<String>,
}

#[derive(Deserialize)]
struct AccountMeResponse {
    id: String,
    email: String,
    balance_cents: f64,
    plan_name: Option<String>,
    monthly_token_limit: i64,
    subscription_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    tokens_used_this_month: i64,
    monthly_tokens_remaining: i64,
}

/// Wire envelope returned by the cloud server's `/v1/web_search`
/// endpoint. We accept `result` as a `serde_json::Value` (rather than
/// `BaikeResult`) so a future provider (google / bing / tavily) can
/// reuse the same endpoint with its own payload shape; the renderer is
/// the one place we commit to "this looks like Baike".
#[derive(Deserialize)]
struct CloudWebSearchEnvelope {
    provider: String,
    query: String,
    result: Option<serde_json::Value>,
}

// ---- Manager ------------------------------------------------------------------

/// Owns the currently-logged-in `CloudAccount` for the duration of the
/// Tauri process. Frontend-pushed updates overwrite this; the manager is
/// the source of truth at the call sites that issue HTTP requests.
#[derive(Clone)]
pub struct CloudClient {
    http: Client,
    inner: Arc<Mutex<Option<CloudAccount>>>,
}

impl CloudClient {
    pub fn new() -> Self {
        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
            inner: Arc::new(Mutex::new(None)),
        }
    }

// ── Account lifecycle ────────────────────────────────────────────────────────

    pub async fn set_account(&self, account: Option<CloudAccount>) {
        *self.inner.lock().await = account;
    }

    pub async fn current(&self) -> Option<CloudAccount> {
        self.inner.lock().await.clone()
    }

    // -- Auth --

    pub async fn register(
        &self,
        base_url: &str,
        invite_code: &str,
        email: &str,
        password: &str,
    ) -> Result<CloudAccount, CloudError> {
        let url = format!("{}/auth/register", base_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .json(&RegisterRequest { invite_code, email, password })
            .send()
            .await
            .map_err(|e| CloudError::Network(e.to_string()))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if status == StatusCode::BAD_REQUEST && body.contains("invite") {
            return Err(CloudError::InvalidInviteCode);
        }
        if !status.is_success() {
            return Err(map_status(status, &body));
        }

        let parsed: AuthResponse = serde_json::from_str(&body)
            .map_err(|e| CloudError::Parse(e.to_string()))?;

        let account = CloudAccount {
            base_url: base_url.to_string(),
            email: parsed.user.email,
            user_id: parsed.user.id,
            access_token: parsed.access_token,
            refresh_token: parsed.refresh_token,
            access_expires_at: parsed.expires_at,
            plan_name: parsed.user.plan_name,
            balance_cents: parsed.user.balance_cents,
        };
        *self.inner.lock().await = Some(account.clone());
        Ok(account)
    }

// ── Token management ──────────────────────────────────────────────────────

    pub async fn login(
        &self,
        base_url: &str,
        email: &str,
        password: &str,
    ) -> Result<CloudAccount, CloudError> {
        let url = format!("{}/auth/login", base_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .json(&LoginRequest { email, password })
            .send()
            .await
            .map_err(|e| CloudError::Network(e.to_string()))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(map_status(status, &body));
        }

        let parsed: AuthResponse = serde_json::from_str(&body)
            .map_err(|e| CloudError::Parse(e.to_string()))?;

        let account = CloudAccount {
            base_url: base_url.to_string(),
            email: parsed.user.email,
            user_id: parsed.user.id,
            access_token: parsed.access_token,
            refresh_token: parsed.refresh_token,
            access_expires_at: parsed.expires_at,
            plan_name: parsed.user.plan_name,
            balance_cents: parsed.user.balance_cents,
        };
        *self.inner.lock().await = Some(account.clone());
        Ok(account)
    }

    pub async fn logout(&self) {
        let _ = self.inner.lock().await.take();
    }

    /// Returns a *fresh* access token, refreshing if the stored one is
    /// expired (or within a 30s safety window). The mutex here is per-call;
    /// the stored account is updated in place so concurrent callers share
    /// the rotation.
    ///
    /// On a successful refresh the new (possibly rotated) tokens are
    /// mirrored into the in-process settings cache and persisted to
    /// disk so the next process restart picks them up. The disk write
    /// happens **off-thread** so callers don't pay a fsync on the hot
    /// path; failures are logged but never surfaced as a token error
    /// (the in-memory token is still valid until the next process).
    ///
    /// ## Failure classification
    ///
    /// Refresh responses are bucketed into three groups. Only an *auth*
    /// failure (401/403) clears the stored account — those are the only
    /// codes where the server is unambiguously telling us the refresh
    /// token is dead. Everything else (5xx, 429, network blip, malformed
    /// body) preserves the account so the next call can retry:
    ///
    /// * **401 / 403** → `CloudError::AuthFailed` and the account is
    ///   cleared. The frontend should react to this by routing the user
    ///   to the login screen.
    /// * **5xx, 429, or other 4xx** → `CloudError::Server` and the
    ///   account is preserved. We retry once with a brief delay so a
    ///   transient gateway hiccup doesn't force a re-login.
    /// * **Network error** → `CloudError::Network` and the account is
    ///   preserved (the existing single-test already pins this).
// ── Data fetching ────────────────────────────────────────────────────────

    pub async fn ensure_fresh_token(&self) -> Result<String, CloudError> {
        let mut guard = self.inner.lock().await;
        let account = guard.as_mut().ok_or(CloudError::NotLoggedIn)?;

        let now = chrono::Utc::now();
        if account.access_expires_at - chrono::Duration::seconds(30) > now {
            return Ok(account.access_token.clone());
        }

        tracing::debug!(
            user_id = %account.user_id,
            "cloud access token nearing expiry, refreshing"
        );

        let (base_url, refresh_token) =
            (account.base_url.clone(), account.refresh_token.clone());

        let mut attempt = 0u8;
        let parsed: RefreshResponse = loop {
            attempt += 1;
            match self.try_refresh_once(&base_url, &refresh_token).await {
                Ok(parsed) => break parsed,
                Err(RefreshAttemptError::AuthFailed { status, body }) => {
                    tracing::warn!(
                        user_id = %account.user_id,
                        status = status.as_u16(),
                        "cloud refresh rejected (auth); clearing stored account: {}",
                        body.chars().take(200).collect::<String>()
                    );
                    *guard = None;
                    crate::commands::clear_settings_cache_account();
                    return Err(map_status(status, &body));
                }
                Err(RefreshAttemptError::Retriable { status, body }) => {
                    if attempt >= 2 {
                        tracing::warn!(
                            user_id = %account.user_id,
                            status = status.as_u16(),
                            "cloud refresh failed after retry; preserving account: {}",
                            body.chars().take(200).collect::<String>()
                        );
                        return Err(map_status(status, &body));
                    }
                    tracing::info!(
                        user_id = %account.user_id,
                        status = status.as_u16(),
                        "cloud refresh transient failure; retrying once"
                    );
                    // Tiny backoff so we don't hammer a gateway that's
                    // already struggling. Kept short (250 ms) to stay
                    // imperceptible on the chat hot path.
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
            }
        };

        account.access_token = parsed.access_token.clone();
        account.access_expires_at = parsed.expires_at;
        if let Some(new_rt) = parsed.refresh_token {
            account.refresh_token = new_rt;
        }

        // Mirror the rotated tokens into the settings cache so the
        // chat-path's cached AIConfig sees fresh credentials on the
        // next read, and spawn an off-thread task to persist them.
        let snapshot = account.clone();
        let token_prefix: String = parsed.access_token.chars().take(8).collect();
        tracing::info!(
            user_id = %snapshot.user_id,
            token_prefix = %token_prefix,
            expires_at = %snapshot.access_expires_at,
            "cloud access token refreshed"
        );
        crate::commands::patch_settings_cache_account(snapshot.clone());
        let client_for_persist = self.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = client_for_persist.persist_current_account(&snapshot).await {
                tracing::warn!(
                    user_id = %snapshot.user_id,
                    "failed to persist rotated cloud tokens: {}",
                    e
                );
            }
        });

        Ok(account.access_token.clone())
    }

    /// Issue exactly one refresh HTTP request and bucket the response
    /// into one of three failure categories. Kept separate from
    /// `ensure_fresh_token` so the retry policy above stays readable.
    async fn try_refresh_once(
        &self,
        base_url: &str,
        refresh_token: &str,
    ) -> Result<RefreshResponse, RefreshAttemptError> {
        let url = format!("{}/auth/refresh", base_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .json(&RefreshRequest { refresh_token })
            .send()
            .await
            .map_err(|e| {
                tracing::warn!("cloud refresh network error: {}", e);
                RefreshAttemptError::Retriable {
                    status: reqwest::StatusCode::SERVICE_UNAVAILABLE,
                    body: format!("network: {}", e),
                }
            })?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if status.is_success() {
            return serde_json::from_str(&body).map_err(|e| {
                // Malformed body is treated as retriable: a transient
                // proxy stripping bytes shouldn't kill the session.
                RefreshAttemptError::Retriable {
                    status: reqwest::StatusCode::BAD_GATEWAY,
                    body: format!("parse: {}", e),
                }
            });
        }

        // 401 / 403 are the unambiguous "refresh token is no good"
        // signals — those clear the account. Everything else (5xx,
        // 429, other 4xx) is bucketed as retriable so a transient
        // gateway hiccup doesn't force a re-login.
        if matches!(
            status,
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        ) {
            Err(RefreshAttemptError::AuthFailed { status, body })
        } else {
            Err(RefreshAttemptError::Retriable { status, body })
        }
    }

    /// Persist the supplied account (or, when `None`, the currently
    /// in-memory account) back to `settings.json` and refresh the
    /// in-memory cache. Intended for two callers:
    ///
    /// 1. `ensure_fresh_token` after a successful rotation (so a
    ///    restart picks up the new refresh token).
    /// 2. The frontend's `cloud_persist_account` after a manual
    ///    login/register (already covered by `commands_cloud.rs`; this
    ///    method is the symmetric write-side helper).
    ///
    /// Returns `Ok(())` even when there is no account to persist
    /// (that's not an error — `cloud_mode_enabled=false` is a valid
    /// configuration).
// ── Model registry ───────────────────────────────────────────────────────

    pub async fn persist_current_account(
        &self,
        account: &CloudAccount,
    ) -> Result<(), CloudError> {
        // Read the latest settings from disk so we don't clobber other
        // concurrent edits (e.g. a user toggling web_search routing at
        // the same moment a refresh fires).
        let mut updated = crate::commands::get_settings_cached().unwrap_or_default();
        updated.cloud.account = Some(account.clone());

        let path = crate::commands::get_settings_path();
        let content = serde_json::to_string_pretty(&updated)
            .map_err(|e| CloudError::Other(format!("serialise settings: {}", e)))?;
        crate::commands::atomic_write_settings(&path, &content)
            .map_err(|e| CloudError::Other(format!("write settings: {}", e)))?;

        crate::commands::update_settings_cache(updated);
        Ok(())
    }

    // -- Discovery --

    pub async fn fetch_models(&self) -> Result<Vec<CloudModelEntry>, CloudError> {
        let (base_url, token) = {
            let account = self.inner.lock().await.clone().ok_or(CloudError::NotLoggedIn)?;
            (account.base_url, self.ensure_fresh_token().await?)
        };

        let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
        let resp = self
            .http
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| CloudError::Network(e.to_string()))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(map_status(status, &body));
        }

        let parsed: ModelsEnvelope = serde_json::from_str(&body)
            .map_err(|e| CloudError::Parse(e.to_string()))?;

        Ok(parsed
            .data
            .into_iter()
            .map(|m| CloudModelEntry {
                id: m.id,
                display_name: m.display_name,
                model_name: m.model_name,
                provider: m.provider,
                input_price_per_m_tokens: m.input_price_per_m_tokens,
                output_price_per_m_tokens: m.output_price_per_m_tokens,
                cached_input_price_per_m_tokens: m.cached_input_price_per_m_tokens,
                description: m.description,
                provider_kind: AIProviderKind::OpenAI, // Cloud → OpenAI wire-protocol on the client side
            })
            .collect())
    }

    pub async fn fetch_account(&self) -> Result<CloudAccountInfo, CloudError> {
        let base_url = {
            let account = self.inner.lock().await.clone().ok_or(CloudError::NotLoggedIn)?;
            account.base_url
        };
        let token = self.ensure_fresh_token().await?;

        let url = format!("{}/account/me", base_url.trim_end_matches('/'));
        let resp = self
            .http
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| CloudError::Network(e.to_string()))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(map_status(status, &body));
        }

        let parsed: AccountMeResponse = serde_json::from_str(&body)
            .map_err(|e| CloudError::Parse(e.to_string()))?;

        Ok(CloudAccountInfo {
            id: parsed.id,
            email: parsed.email,
            balance_cents: parsed.balance_cents,
            plan_name: parsed.plan_name,
            monthly_token_limit: parsed.monthly_token_limit,
            subscription_expires_at: parsed.subscription_expires_at,
            tokens_used_this_month: parsed.tokens_used_this_month,
            monthly_tokens_remaining: parsed.monthly_tokens_remaining,
        })
    }

    /// Resolve a `CloudModelEntry` (cached on the frontend) into a fully
    /// wired `AIProvider::OpenAI` configured to talk to the user's cloud
    /// server. The access token is fetched fresh so a long-lived
    /// background streaming call won't be cut off mid-response.
    pub async fn build_ai_config_for_model(
        &self,
        entry: &CloudModelEntry,
    ) -> Result<(crate::ai::AIProvider, String), CloudError> {
        let account = self.inner.lock().await.clone().ok_or(CloudError::NotLoggedIn)?;
        let token = self.ensure_fresh_token().await?;

        let base_url = format!(
            "{}/v1",
            account.base_url.trim_end_matches('/')
        );

        // The server-side model_config.id is what /v1/chat/completions
        // expects in the `model` field. Embedding it here keeps the
        // protocol identical to a normal OpenAI call.
        let provider = crate::ai::AIProvider::OpenAI {
            api_key: token,
            base_url,
        };
        let model_id = entry.id.clone();
        Ok((provider, model_id))
    }

    // -- Web search (cloud-routed) --

    /// Forward a `web_search` tool call to the cloud server's
    /// `/v1/web_search` endpoint instead of calling the upstream
    /// encyclopedia directly on the desktop. The cloud server holds the
    /// operator-supplied API key, so the desktop client doesn't need to
    /// carry its own credentials and every cloud-authenticated user
    /// transparently shares the same operator-managed key.
    ///
    /// Returns the upstream's `result` JSON on success (already parsed as
    /// `serde_json::Value` so the dispatching tool layer can pass it to a
    /// Baike-flavoured formatter that already knows how to render it),
    /// or a `CloudError` whose `Server { body, .. }` carries the
    /// operator-friendly message from the cloud endpoint on failure.
    pub async fn search_web(
        &self,
        provider_id: &str,
        query: &str,
        max_results: u32,
    ) -> Result<serde_json::Value, CloudError> {
        let (base_url, token) = {
            let account = self.inner.lock().await.clone().ok_or(CloudError::NotLoggedIn)?;
            (account.base_url, self.ensure_fresh_token().await?)
        };

        let url = format!("{}/v1/web_search", base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "provider": provider_id,
            "query": query,
            "max_results": max_results,
        });

        let resp = self
            .http
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| CloudError::Network(e.to_string()))?;

        let status = resp.status();
        let raw = resp.text().await.unwrap_or_default();

        // The cloud endpoint echoes `{ error: "..." }` on the 4xx/5xx
        // branches. We try to surface the server's `error` string as the
        // user-visible message because operators hand-crafted it for
        // their user base.
        if !status.is_success() {
            let server_msg = serde_json::from_str::<serde_json::Value>(&raw)
                .ok()
                .and_then(|v| {
                    v.get("error")
                        .and_then(|e| e.as_str())
                        .map(String::from)
                })
                .unwrap_or_default();
            // Build a richer error than `map_status` would: include the
            // server's own message instead of dumping a raw body that
            // could leak upstream HTML or internal stack frames.
            return Err(CloudError::Server {
                status: status.as_u16(),
                body: if server_msg.is_empty() {
                    raw.chars().take(500).collect()
                } else {
                    server_msg
                },
            });
        }

        // Strip the `{ provider, query, result }` envelope so the caller
        // sees just the upstream payload. The cloud wire format mirrors
        // Baike's success shape so the caller can hand it straight to
        // the existing `format_baike_result`-style helper.
        let envelope: CloudWebSearchEnvelope = serde_json::from_str(&raw)
            .map_err(|e| CloudError::Parse(format!("cloud web_search envelope: {}", e)))?;

        envelope.result.ok_or_else(|| {
            CloudError::Parse("cloud web_search: response missing `result` field".into())
        })
    }
}

fn map_status(status: StatusCode, body: &str) -> CloudError {
    match status {
        StatusCode::UNAUTHORIZED => CloudError::AuthFailed,
        StatusCode::PAYMENT_REQUIRED => CloudError::QuotaExhausted,
        _ => CloudError::Server {
            status: status.as_u16(),
            body: body.chars().take(500).collect(),
        },
    }
}

impl Default for CloudClient {
    fn default() -> Self {
        Self::new()
    }
}
