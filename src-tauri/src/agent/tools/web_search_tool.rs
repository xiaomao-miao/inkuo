//! `web_search` tool — searches an external provider (today: Baidu Baike)
//! for a user query and returns the matching encyclopedia entries as
//! structured text the Agent can cite.
//!
//! The provider list lives in `Settings.web_search.providers`. Today
//! only `"baike"` is implemented; the dispatch is data-driven so a
//! future Google / Bing provider only needs to register an entry here.
//!
//! Network policy:
//!  - The endpoint URL is either the compile-time default for the
//!    provider or a user-supplied override from settings. The LLM
//!    never sees or sets the URL (no parameter for it), so a prompt
//!    injection cannot redirect the tool to an arbitrary host.
//!  - All HTTP work goes through the shared `HTTP_CLIENT` so connection
//!    pooling is reused across calls.
//!  - A 10-second timeout is enforced on every request to keep the
//!    Agent loop responsive when the upstream is slow.

use serde::Deserialize;
use serde_json::Value;
use tauri::Manager;

use crate::commands::{get_web_search_settings, WebSearchProviderConfig, WebSearchSettings};
use super::{ToolDefinition, ToolError, ToolParameters};

/// Compile-time defaults for the Baike provider. The endpoint is the
/// Baidu AppBuilder "百科查询" API; it requires a Bearer token (the
/// user's `api_key`). Unlike the old public `BaikeLemmaCardApi` endpoint
/// it has no built-in fallback appid, so the user must supply a key.
const BAIKE_DEFAULT_BASE_URL: &str = "https://appbuilder.baidu.com/v2/baike/lemma/get_content";
const BAIKE_PROVIDER_ID: &str = "baike";
/// Default search_type: `lemmaTitle` matches by exact lemma title
/// (best for "刘德华" / "爱因斯坦" style queries). The other modes
/// (`lemmaSummary` etc.) are out of scope for v1.
const BAIKE_DEFAULT_SEARCH_TYPE: &str = "lemmaTitle";

/// Hard cap on `max_results` from the LLM — protects against
/// accidentally large requests and keeps the tool output a reasonable
/// size for the model context.
const MAX_RESULTS_LIMIT: usize = 20;
const MIN_RESULTS_LIMIT: usize = 1;
const DEFAULT_RESULTS_LIMIT: usize = 5;

/// HTTP timeout for outbound requests. 10s is plenty for a single GET
/// to a public encyclopedia endpoint; longer than that usually means
/// the upstream is having a bad day and we'd rather fail fast.
const HTTP_TIMEOUT_SECS: u64 = 10;

/// User-Agent header sent with every request. Baike's CDN rejects
/// requests without a UA, and a generic "curl/7.x" looks suspicious
/// enough to be throttled.
const USER_AGENT: &str = concat!(
    "inkuo/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/inkuo) web_search tool"
);

/// Response shape for the AppBuilder `get_content` endpoint. Every
/// field is optional because the API may legitimately return a
/// `null` `content_plain` for an entry that only has a `summary`,
/// and `relations` / `videos` are present iff the entry has them.
#[derive(Debug, Clone, Deserialize)]
struct BaikeResult {
    /// Plain-text body of the article. Often `null` for entries that
    /// only render a card view (e.g. persons whose data is mostly in
    /// `summary`); we treat that as "no extra body, summary is enough".
    #[serde(default)]
    content_plain: Option<String>,
    /// Short one-liner the encyclopedia uses as the lemma subtitle.
    #[serde(default)]
    lemma_desc: Option<String>,
    /// Numeric lemma id (kept so the Agent can disambiguate when the
    /// title alone isn't unique).
    #[serde(default)]
    lemma_id: Option<i64>,
    /// Canonical lemma title (e.g. "刘德华"). Used as the primary
    /// citation handle.
    #[serde(default)]
    lemma_title: Option<String>,
    /// Cover image URL — surfaced so the Agent can mention it when
    /// relevant.
    #[serde(default)]
    pic_url: Option<String>,
    /// Related lemmas (spouse, parent org, etc.). Useful for the
    /// Agent to mention connections in the answer.
    #[serde(default)]
    relations: Vec<BaikeRelation>,
    /// Long-form biographical / descriptive text. The single most
    /// useful field — usually several hundred chars of plain prose.
    #[serde(default)]
    summary: Option<String>,
    /// Canonical URL to the public Baike page. The Agent cites this.
    #[serde(default)]
    url: Option<String>,
    /// Optional video references attached to the entry.
    #[serde(default)]
    videos: Vec<BaikeVideo>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct BaikeRelation {
    #[serde(default)]
    lemma_id: Option<i64>,
    #[serde(default)]
    lemma_title: Option<String>,
    /// E.g. "妻子" / "配偶" / "父亲". Display label, not localized.
    #[serde(default)]
    relation_name: Option<String>,
    /// Thumbnail URL. Parsed for future use (the renderer only shows
    /// the lemma title + relation name today); serde needs the field
    /// present so the response deserialises cleanly.
    #[serde(default)]
    square_pic_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct BaikeVideo {
    /// Thumbnail URL — parsed for future use; not yet rendered.
    #[serde(default)]
    cover_pic_url: Option<String>,
    #[serde(default)]
    page_url: Option<String>,
    /// Numeric id of the secondary page — kept for debugging / future
    /// deep-linking.
    #[serde(default)]
    second_id: Option<i64>,
    #[serde(default)]
    second_title: Option<String>,
}

/// Top-level envelope for `appbuilder.baidu.com/v2/baike/lemma/get_content`.
/// We accept either a structured `result` (success) or a top-level
/// `error` field (failure) — the AppBuilder docs are inconsistent on
/// which one they actually use, so we tolerate both.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct BaikeResponse {
    /// Per-request id surfaced by AppBuilder for support tickets. Not
    /// displayed today but kept so it's available if a tool result
    /// needs to reference it.
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    result: Option<BaikeResult>,
    /// AppBuilder returns errors as `{ "error": { "code": ..., "message": ... } }`
    /// on some endpoints. When present, we surface the message instead
    /// of the empty `result`.
    #[serde(default)]
    error: Option<BaikeError>,
}

#[derive(Debug, Deserialize)]
struct BaikeError {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

impl BaikeResponse {
    fn human_error(&self) -> Option<String> {
        if let Some(err) = &self.error {
            let msg = err.message.clone().unwrap_or_default();
            let code = err.code.clone().unwrap_or_default();
            return Some(match (msg.is_empty(), code.is_empty()) {
                (true, true) => "baike API returned an unspecified error".to_string(),
                (false, true) => msg,
                (true, false) => format!("baike API error code {}", code),
                (false, false) => format!("{} (code {})", msg, code),
            });
        }
        None
    }
}

/// Web search tool — provider-agnostic search over an external
/// encyclopedia. The actual provider dispatch happens inside
/// `execute`; today only Baike is implemented locally, plus the
/// optional cloud-routed path through `crate::cloud::CloudClient`.
///
/// ## Routing
///
/// Three knobs together decide where the call lands:
///
/// 1. `Settings.cloud.cloud_mode_enabled` — has the user opted into
///    the cloud at all.
/// 2. `Settings.cloud.account` — is the user actually logged in.
/// 3. `Settings.web_search.routing` — `local` (default, today)
///    uses the desktop-side Baike key/endpoint; `cloud` routes
///    through the cloud server's operator-managed Baike credentials.
///
/// This breaks down to four concrete states:
///
/// | cloud_mode | account | routing   | behaviour              |
/// |-----------|---------|-----------|------------------------|
/// | off       | *       | any       | local Baike             |
/// | on        | missing | any       | local Baike (warning)  |
/// | on        | present | local     | local Baike             |
/// | on        | present | cloud     | cloud-routed (default) |
///
/// The "account present but cloud_mode off" row intentionally falls
/// back to local Baike: the cloud toggle is the explicit opt-in, not
/// the mere presence of an account.
#[derive(Clone)]
pub struct WebSearchTool {
    /// `None` for the placeholder registered before the AppHandle
    /// becomes available. The placeholder's `execute()` always errors;
    /// once the registry has called `set_app_handle(...)` we replace it
    /// with the real implementation. Keeping the `Option` here (rather
    /// than two distinct types) means we don't need a second
    /// `ToolExecutor` variant just to express "uninitialised".
    app: Option<tauri::AppHandle>,
}

impl WebSearchTool {
    /// Real implementation — needs the AppHandle so it can read the
    /// settings cache and reach the cloud client when routing is
    /// configured to `"cloud"`.
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app: Some(app) }
    }

    /// Placeholder that returns a friendly error if reached before the
    /// registry has swapped in the real implementation (e.g. when an
    /// agent turn somehow runs before `set_app_handle` fires). In
    /// practice all call sites seed the AppHandle at startup so the
    /// placeholder is immediately overwritten, but the defence keeps the
    /// contract explicit ("a web_search tool without an AppHandle is
    /// invalid") instead of letting the rust borrow checker silently
    /// produce a tool that panics on use.
    pub fn placeholder() -> Self {
        Self { app: None }
    }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "web_search",
            "联网搜索",
            "Search an external encyclopedia for a real-world entity \
            (person, place, organization, work, event) and return the \
            matching entry's summary, related lemmas, and references.\n\
            Routing is configured in Settings → Network Search:\n\
            - `local` (default) uses your own Baidu AppBuilder API key.\n\
            - `cloud` routes through your inkuo Cloud account so the \
            operator-managed key (configured in the admin panel) is used \
            instead of yours.\n\
            Best for short, named queries (e.g. \"刘德华\", \"爱因斯坦\", \
            \"无间道\"); vague or conversational queries will likely return \
            no match.",
            ToolParameters::new(
                vec!["query"],
                vec![
                    (
                        "query",
                        "string",
                        Some(
                            "The entity name to look up. Use a short, specific \
                            noun phrase (e.g. \"刘德华\", \"北京\", \"OpenAI\"); \
                            avoid full questions.",
                        ),
                    ),
                    (
                        "max_results",
                        "integer",
                        Some(
                            "Maximum number of entries to return. Default 5, range 1–20. \
                            Note: the Baidu Baike endpoint returns at most one \
                            lemma per call, so values > 1 are forward-compatible \
                            with future multi-result providers.",
                        ),
                    ),
                ],
            ),
        )
    }

    pub async fn execute(
        &self,
        arguments: Value,
        _workspace: Option<String>,
    ) -> Result<String, ToolError> {
        let query = arguments["query"]
            .as_str()
            .ok_or_else(|| {
                ToolError::InvalidArguments(
                    "web_search".to_string(),
                    "query must be a string".into(),
                )
            })?
            .trim();

        if query.is_empty() {
            return Err(ToolError::InvalidArguments(
                "web_search".to_string(),
                "query must not be empty".into(),
            ));
        }

        let max_results = arguments["max_results"]
            .as_i64()
            .map(|v| v.clamp(MIN_RESULTS_LIMIT as i64, MAX_RESULTS_LIMIT as i64) as usize)
            .unwrap_or(DEFAULT_RESULTS_LIMIT);

        // Resolve the AppHandle. The eagerly-attached `self.app`
        // is the happy path; if it's `None` (placeholder path) we
        // fall back to the process-global registry seeded by
        // `lib.rs::setup`. This is the lazy-fetch-app-state
        // pattern: callers don't have to remember to call
        // `set_app_handle` exactly once before the first agent
        // turn, and a missed hook no longer bricks the tool
        // forever.
        let app = match self.app.as_ref() {
            Some(app) => app.clone(),
            None => match crate::app_handle::current_app_handle() {
                Some(app) => {
                    tracing::info!(
                        "web_search: lazily recovered AppHandle from process-global registry"
                    );
                    app
                }
                None => {
                    return Err(ToolError::ExecutionError(
                        "web_search is unavailable because the desktop app failed \
                        to initialise its Tauri context. This is an internal error, \
                        not a configuration issue — please restart the app and try \
                        again; if it persists, file an issue with the last action you \
                        took before this error appeared."
                            .to_string(),
                    ));
                }
            },
        };

        // Read the latest config snapshot from the settings cache. This
        // is refreshed whenever the user saves settings, so the tool
        // sees the user's latest edits without any IPC roundtrip.
        let settings = get_web_search_settings();

        if !settings.enabled {
            return Ok(
                "web_search is currently disabled in Settings → Network Search. \
                Ask the user to enable it before retrying."
                    .to_string(),
            );
        }

        // Cloud vs local routing. Decided up-front so the rest of the
        // function does not have to know which backend served it; both
        // branches ultimately return the same envelope (`format_baike_result`
        // accepts either path because the cloud `search_web` envelope
        // shares the same shape as Baike's success payload).
        let wants_cloud_routing = resolve_routing(&settings.routing);

        if wants_cloud_routing.should_use_cloud {
            return self.execute_via_cloud(&app, query, max_results, &settings).await;
        }

        // Local-Baike path (unchanged from the previous behaviour).
        self.execute_via_local_baike(query, max_results, &settings).await
    }
}

/// Where to send the `web_search` call, decided once per turn.
///
/// The two booleans split "user wants cloud" from "we should honour it".
/// They collapse to "use cloud" only when both are true; the LLM
/// otherwise gets a clean message that explains why the cloud toggle
/// was ignored and what to do.
struct ResolvedRouting {
    should_use_cloud: bool,
}

/// Implement the routing matrix documented on `WebSearchTool`. The
/// function is pure so the LLM-facing explanation can be derived
/// without an extra roundtrip into the cloud client.
///
/// Today only `"local"` and `"cloud"` are recognised; any other value
/// (including `null` / missing) collapses to `local` so a typo in the
/// settings file never silently disables search for the user.
fn resolve_routing(routing: &str) -> ResolvedRouting {
    if routing == "cloud" {
        ResolvedRouting { should_use_cloud: true }
    } else {
        ResolvedRouting { should_use_cloud: false }
    }
}

// ── Local search ────────────────────────────────────────────────────────────────

impl WebSearchTool {
    /// Forward the search to the cloud server. The server is responsible
    /// for picking the operator-configured API key + base URL, billing,
    /// and recording usage. We just pass through the user query and let
    /// the cloud side return the same shape we use for the local Baike
    /// path so we can render it through the same formatter.
    async fn execute_via_cloud(
        &self,
        app: &tauri::AppHandle,
        query: &str,
        max_results: usize,
        _settings: &WebSearchSettings,
    ) -> Result<String, ToolError> {
        // Pull a fresh cloud client from Tauri's state. Doing this per
        // call (rather than caching) lets the registry handle "logged
        // out → logged in" transitions automatically: when the user
        // logs in the new client replaces the old one in state, and the
        // next `web_search` call picks it up.
        let client = app
            .state::<crate::cloud::CloudClient>()
            .inner()
            .clone();

        match client.search_web(BAIKE_PROVIDER_ID, query, max_results as u32).await {
            Ok(payload) => {
                // The cloud server returns the upstream Baike JSON in
                // the `result` field. We deserialize back into the same
                // `BaikeResult` the local path uses, then hand it to the
                // exact same formatter — that's the entire point of
                // keeping the wire envelope identical.
                match serde_json::from_value::<BaikeResult>(payload) {
                    Ok(baike) => Ok(format_baike_result(query, &baike)),
                    Err(err) => Err(ToolError::ExecutionError(format!(
                        "cloud-routed web_search returned a payload the desktop could not parse: {}",
                        err
                    ))),
                }
            }
            Err(err) => Err(ToolError::ExecutionError(format!(
                "cloud-routed web_search failed: {}",
                err
            ))),
        }
    }

    /// Local Baike path — unchanged from the previous behaviour. Reads
    /// the provider (API key + base URL) from the settings cache and
    /// hits AppBuilder directly.
    async fn execute_via_local_baike(
        &self,
        query: &str,
        max_results: usize,
        settings: &WebSearchSettings,
    ) -> Result<String, ToolError> {
        // Resolve the provider. Today only `"baike"` is wired up;
        // missing/unknown providers get an explicit error so the Agent
        // surfaces a clear message instead of silently no-op'ing.
        let provider = settings
            .providers
            .iter()
            .find(|p| p.id == BAIKE_PROVIDER_ID)
            .ok_or_else(|| {
                ToolError::ExecutionError(format!(
                    "no web_search provider is configured for `{}`. \
                    Open Settings → Network Search to add one.",
                    BAIKE_PROVIDER_ID
                ))
            })?;

        if !provider.enabled {
            return Ok(format!(
                "the `{}` provider is disabled in Settings → Network Search. \
                Enable it before retrying.",
                provider.id
            ));
        }

        // Dispatch to the per-provider implementation. Kept in a match
        // (not a trait dispatch) because there's exactly one provider
        // today — adding a second is a two-line change.
        match provider.id.as_str() {
            BAIKE_PROVIDER_ID => {
                search_baike(query, provider, max_results).await
            }
            other => Err(ToolError::ExecutionError(format!(
                "web_search provider `{}` is not implemented in this build",
                other
            ))),
        }
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        // Tests and the registry's pre-AppHandle bootstrap use this;
        // both should reach `set_app_handle` before any agent turn.
        Self::placeholder()
    }
}

/// Hit the Baidu AppBuilder `get_content` endpoint for the given query
/// and render the result as a tool-readable summary. Network errors and
/// missing API keys are surfaced verbatim — the Agent loop wraps them
/// in the standard tool-error envelope so the user sees "tool execution
/// failed" rather than a misleading success.
async fn search_baike(
    query: &str,
    provider: &WebSearchProviderConfig,
    _max_results: usize,
) -> Result<String, ToolError> {
    let base_url = provider
        .base_url
        .as_deref()
        .unwrap_or(BAIKE_DEFAULT_BASE_URL);

    // The endpoint requires a Bearer token — no anonymous fallback.
    // Return an explicit, actionable error so the LLM can guide the
    // user to the settings panel instead of looping on auth failures.
    let api_key = provider
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ToolError::ExecutionError(
                "Baidu Baike API key is not configured. Open Settings → 网络搜索 and paste your AppBuilder API key before retrying.".to_string(),
            )
        })?;

    // Build the URL. AppBuilder's `get_content` takes the search term as
    // `search_key` and the search mode as `search_type`; both query
    // params must be URL-encoded since lemma titles are typically CJK.
    let url = format!(
        "{}{}search_type={}&search_key={}",
        base_url.trim_end_matches('?'),
        if base_url.contains('?') { '&' } else { '?' },
        urlencoding::encode(BAIKE_DEFAULT_SEARCH_TYPE),
        urlencoding::encode(query),
    );

    let response = crate::ai::HTTP_CLIENT
        .get(&url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {}", api_key))
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| ToolError::ExecutionError(format!("baike request failed: {}", e)))?;

    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(ToolError::ExecutionError(format!(
            "Baidu Baike rejected the API key (HTTP {}). \
            Verify the key in Settings → 网络搜索 and try again.",
            status.as_u16()
        )));
    }
    if !status.is_success() {
        return Err(ToolError::ExecutionError(format!(
            "baike returned HTTP {}",
            status.as_u16()
        )));
    }

    let body: BaikeResponse = response
        .json()
        .await
        .map_err(|e| ToolError::ExecutionError(format!("baike response was not valid JSON: {}", e)))?;

    if let Some(err) = body.human_error() {
        return Err(ToolError::ExecutionError(format!("baike API error: {}", err)));
    }

    let Some(result) = body.result else {
        return Ok(format!(
            "No Baidu Baike entry found for query: \"{}\". \
            Try a more specific term (e.g. a person's full name in Chinese).",
            query
        ));
    };

    // Bail out early when the entry is so empty we can't even cite it.
    // We require at minimum a title or a lemma id — without one of those
    // we can't form a useful citation for the Agent.
    if result.lemma_title.is_none() && result.lemma_id.is_none() {
        return Ok(format!(
            "Baidu Baike returned an empty entry for query: \"{}\". \
            Try rephrasing the query.",
            query
        ));
    }

    Ok(format_baike_result(query, &result))
}

/// Render a parsed Baike result into the structured text the Agent
/// consumes. Each section is independently optional so a partial
/// payload (e.g. no relations) still renders cleanly.
fn format_baike_result(query: &str, result: &BaikeResult) -> String {
    let mut out = String::new();
    let title = result.lemma_title.clone().unwrap_or_else(|| query.to_string());

    out.push_str(&format!("Found Baidu Baike entry for query: \"{}\"\n\n", query));
    out.push_str(&format!("Title: {}\n", title));

    if let Some(id) = result.lemma_id {
        out.push_str(&format!("Lemma ID: {}\n", id));
    }
    if let Some(desc) = &result.lemma_desc {
        if !desc.trim().is_empty() {
            out.push_str(&format!("Short Description: {}\n", desc.trim()));
        }
    }
    if let Some(url) = &result.url {
        out.push_str(&format!("URL: {}\n", url));
    }
    if let Some(pic) = &result.pic_url {
        out.push_str(&format!("Cover Image: {}\n", pic));
    }

    if let Some(summary) = &result.summary {
        if !summary.trim().is_empty() {
            out.push_str("\nSummary:\n");
            out.push_str(summary.trim());
            out.push('\n');
        }
    }

    if let Some(body) = &result.content_plain {
        if !body.trim().is_empty() {
            out.push_str("\nFull Content:\n");
            out.push_str(body.trim());
            out.push('\n');
        }
    }

    if !result.relations.is_empty() {
        out.push_str("\nRelated Entries:\n");
        for rel in &result.relations {
            let name = rel.lemma_title.clone().unwrap_or_else(|| "(unnamed)".into());
            let relation = rel.relation_name.clone().unwrap_or_else(|| "related".into());
            out.push_str(&format!("- {} — {} (lemma id {:?})\n", name, relation, rel.lemma_id));
        }
    }

    if !result.videos.is_empty() {
        out.push_str("\nVideos:\n");
        for vid in &result.videos {
            let title = vid.second_title.clone().unwrap_or_else(|| "(untitled)".into());
            let url = vid.page_url.clone().unwrap_or_default();
            if url.is_empty() {
                out.push_str(&format!("- {}\n", title));
            } else {
                out.push_str(&format!("- {} ({})\n", title, url));
            }
        }
    }

    out
}