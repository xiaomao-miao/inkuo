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
    pub async fn ensure_fresh_token(&self) -> Result<String, CloudError> {
        let mut guard = self.inner.lock().await;
        let account = guard.as_mut().ok_or(CloudError::NotLoggedIn)?;

        let now = chrono::Utc::now();
        if account.access_expires_at - chrono::Duration::seconds(30) > now {
            return Ok(account.access_token.clone());
        }

        let url = format!("{}/auth/refresh", account.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .json(&RefreshRequest { refresh_token: &account.refresh_token })
            .send()
            .await
            .map_err(|e| CloudError::Network(e.to_string()))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(map_status(status, &body));
        }

        let parsed: RefreshResponse = serde_json::from_str(&body)
            .map_err(|e| CloudError::Parse(e.to_string()))?;

        account.access_token = parsed.access_token;
        account.access_expires_at = parsed.expires_at;
        if let Some(new_rt) = parsed.refresh_token {
            account.refresh_token = new_rt;
        }
        Ok(account.access_token.clone())
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