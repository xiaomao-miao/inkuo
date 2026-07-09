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

use crate::commands::{get_web_search_settings, WebSearchProviderConfig};
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
/// `execute`; today only Baike is implemented.
#[derive(Clone)]
pub struct WebSearchTool;

impl WebSearchTool {
    pub fn new() -> Self {
        Self
    }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "web_search",
            "联网搜索",
            "Search Baidu Baike for a real-world entity (person, place, \
            organization, work, event) and return the matching encyclopedia \
            entry's summary, related lemmas, and video references.\n\
            The provider is configured by the user in Settings → \
            网络搜索 (Baidu Baike today, more providers may be added later).\n\
            The `web_search` tool always requires the user's Baidu AppBuilder \
            API key; without it, calls return a friendly error pointing at \
            the Settings panel. Best for short, named queries (e.g. \"刘德华\", \
            \"爱因斯坦\", \"无间道\"); vague or conversational queries will \
            likely return no match.",
            ToolParameters::new(
                vec!["query"],
                vec![
                    (
                        "query",
                        "string",
                        Some(
                            "The lemma title or entity name to look up. Use a \
                            short, specific noun phrase (e.g. \"刘德华\", \"北京\", \
                            \"OpenAI\"); avoid full questions.",
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
        Self::new()
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