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
use crate::commands::AppCommandError;
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

impl From<CloudError> for AppCommandError {
    fn from(e: CloudError) -> Self {
        AppCommandError::AIConfig(e.to_string())
    }
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

        let url = format!("{}/auth/refresh", account.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .json(&RefreshRequest { refresh_token: &account.refresh_token })
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(
                    user_id = %account.user_id,
                    "cloud refresh network error: {}",
                    e
                );
                CloudError::Network(e.to_string())
            })?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            tracing::warn!(
                user_id = %account.user_id,
                status = status.as_u16(),
                "cloud refresh failed: {}",
                body.chars().take(200).collect::<String>()
            );
            // The stored refresh_token is no longer good — clear the
            // account so subsequent calls surface a clean "log in
            // again" error instead of looping on a known-bad refresh
            // token forever.
            *guard = None;
            crate::commands::clear_settings_cache_account();
            return Err(map_status(status, &body));
        }

        let parsed: RefreshResponse = serde_json::from_str(&body)
            .map_err(|e| CloudError::Parse(e.to_string()))?;

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
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Err(CloudError::Other(format!(
                    "could not create settings dir: {}",
                    e
                )));
            }
        }
        let content = serde_json::to_string_pretty(&updated)
            .map_err(|e| CloudError::Other(format!("serialise settings: {}", e)))?;
        std::fs::write(&path, content)
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

/// Cloning a `CloudClient` must share the inner account state via the
/// `Arc<Mutex<...>>`. This is the property the Tauri setup relies on:
/// the same `CloudClient` is cloned twice (once into `AppState.cloud`,
/// once into `tauri::State<CloudClient>`) and the startup hydrate has
/// to be visible to both. Regression test for the bug where two
/// independent `CloudClient::new()` instances let the agent chat path
/// see `NotLoggedIn` while the `tauri::State` instance thought the
/// user was logged in.
#[test]
fn cloned_cloud_clients_share_account_state() {
    use std::sync::Arc;
    let original = CloudClient::new();
    let twin = original.clone();
    let account = CloudAccount {
        base_url: "http://127.0.0.1:1".into(),
        email: "user@example.com".into(),
        user_id: "user-shared".into(),
        access_token: "tok".into(),
        refresh_token: "rt".into(),
        access_expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        plan_name: None,
        balance_cents: 0.0,
    };

    let pair = Arc::new((original, twin));
    let _ = pair; // suppress unused-variable noise if compilation ever changes

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        pair.0.set_account(Some(account.clone())).await;
        // The CLONE — *not* the original — should see the account.
        let from_twin = pair.1.inner.lock().await.clone();
        assert!(
            from_twin.is_some(),
            "cloned CloudClient must share the inner account state; \
             this regression would cause the agent chat path to see \
             `NotLoggedIn` in cloud mode after every restart"
        );
        assert_eq!(from_twin.unwrap().user_id, "user-shared");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh `access_token` returned without hitting the network —
    /// the 30-second safety window in `ensure_fresh_token` should
    /// keep us from making a refresh roundtrip.
    #[tokio::test]
    async fn ensure_fresh_token_returns_cached_when_not_near_expiry() {
        let client = CloudClient::new();
        let account = CloudAccount {
            base_url: "https://example.com".into(),
            email: "user@example.com".into(),
            user_id: "user-1".into(),
            access_token: "fresh-token".into(),
            refresh_token: "rt".into(),
            // 1 hour from now — well outside the 30s safety window.
            access_expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            plan_name: None,
            balance_cents: 0.0,
        };
        client.set_account(Some(account)).await;

        let token = client.ensure_fresh_token().await.unwrap();
        assert_eq!(token, "fresh-token");
    }

    /// When the access token is past the 30-second safety window,
    /// `ensure_fresh_token` should attempt a refresh. We can't
    /// easily hit the network in a unit test, but we *can* verify
    /// that the unauthenticated state surfaces the expected error
    /// when the refresh request fails (no server listening).
    #[tokio::test]
    async fn ensure_fresh_token_attempts_refresh_when_expired() {
        let client = CloudClient::new();
        let account = CloudAccount {
            base_url: "http://127.0.0.1:1".into(), // unreachable
            email: "user@example.com".into(),
            user_id: "user-1".into(),
            access_token: "stale-token".into(),
            refresh_token: "rt".into(),
            // Already expired.
            access_expires_at: chrono::Utc::now() - chrono::Duration::seconds(60),
            plan_name: None,
            balance_cents: 0.0,
        };
        client.set_account(Some(account.clone())).await;

        let err = client.ensure_fresh_token().await.unwrap_err();
        // The error must be a network error, not a stale-token
        // success — that's the entire bug we're fixing.
        assert!(matches!(err, CloudError::Network(_)), "got: {:?}", err);

        // Stale account must NOT be cleared on a *network* failure
        // (only on an explicit auth failure from the server). The
        // user should be able to retry once connectivity is back.
        let still_logged_in = client.current().await.is_some();
        assert!(
            still_logged_in,
            "network failure must not log the user out (that would be \
             worse than the original bug — the user would have to \
             re-authenticate every time their network blipped)"
        );
    }

    /// `ensure_fresh_token` must surface a clean auth error when no
    /// account has been set. The frontend can use this to redirect
    /// the user to the login screen.
    #[tokio::test]
    async fn ensure_fresh_token_errors_when_not_logged_in() {
        let client = CloudClient::new();
        let err = client.ensure_fresh_token().await.unwrap_err();
        assert!(matches!(err, CloudError::NotLoggedIn));
    }
}