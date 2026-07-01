//! Inline Completion module
//!
//! Handles AI-powered inline code/text completion with Ghost text display.
//! Features:
//! - Tab-triggered completion
//! - Ghost text rendering
//! - Accept/reject with Tab/Escape

use serde::{Deserialize, Serialize};
use crate::ai::{AIProviderAdapter, AIConfig, AIError};
use crate::commands::AppState;
use tauri::State;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

/// Request for inline completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineCompletionSnippet {
    /// Snippet text around cursor
    pub text: String,
    /// Character offset of snippet start in the full document
    pub start_offset: usize,
}

/// Request for inline completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineCompletionRequest {
    /// Stable identifier for this request. The frontend mints a fresh id for
    /// every call so the backend can route cancellation to a specific
    /// in-flight request instead of cancelling every concurrent completion
    /// (e.g. in a different editor window).
    #[serde(default)]
    pub request_id: String,
    /// Current document content (either full document, or snippet text)
    pub document: String,
    /// Cursor position.
    /// - If `snippet` is None: character offset from start of full document.
    /// - If `snippet` is Some: character offset within `snippet.text`.
    pub cursor_position: usize,
    /// Programming language (for syntax-aware completion)
    pub language: String,
    /// Optional file path for context
    pub file_path: Option<String>,
    /// Optional snippet payload to avoid sending full document.
    pub snippet: Option<InlineCompletionSnippet>,
}

/// Per-segment inline formatting for docx completions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InlineStyle {
    #[serde(default)]
    pub start_offset: usize,
    #[serde(default)]
    pub end_offset: usize,
    #[serde(default)]
    pub bold: Option<bool>,
    #[serde(default)]
    pub italic: Option<bool>,
    #[serde(default)]
    pub underline: Option<bool>,
    #[serde(default)]
    pub strikethrough: Option<bool>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub highlight: Option<String>,
    #[serde(default)]
    pub font_size: Option<f32>,
    #[serde(default)]
    pub font_family: Option<String>,
}

/// A single completion item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionItem {
    /// Unique identifier
    pub id: String,
    /// The completion text to insert
    pub text: String,
    /// Display text (may be truncated for UI)
    pub display_text: String,
    /// Confidence score (0.0 - 1.0)
    pub score: f32,
    /// Range info (optional)
    pub range: Option<CompletionRange>,
    /// Per-segment styles for docx completions (optional)
    #[serde(default)]
    pub styles: Vec<InlineStyle>,
}

/// Range for the completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRange {
    pub from: usize,
    pub to: usize,
}

/// Response from inline completion request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineCompletionResponse {
    /// List of completion items (usually 1, but can have multiple)
    pub completions: Vec<CompletionItem>,
    /// Model used for completion
    pub model: String,
    /// Usage statistics
    pub usage: Option<TokenUsage>,
}

/// Token usage info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Inline completion state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineCompletionState {
    /// Whether inline completion is enabled
    pub enabled: bool,
    /// Current completion being displayed
    pub current: Option<CompletionItem>,
    /// Loading state
    pub is_loading: bool,
    /// Error message if any
    pub error: Option<String>,
}

/// Per-request cancel state. Previously a single `AtomicU64` seq counter and
/// one global `Notify` covered all in-flight completions, which meant
/// cancelling a completion in one editor window would also abort completions
/// in other windows (the `ai_inline_complete_cancel` command had no way to
/// name the request being cancelled). We now register a dedicated
/// `Arc<Notify>` per `request_id` (== `session_id` from the frontend) and
/// the cancel command looks it up by id.
type CancelRegistry = Mutex<HashMap<String, Arc<Notify>>>;

static INLINE_CANCEL_REGISTRY: std::sync::OnceLock<CancelRegistry> =
    std::sync::OnceLock::new();

fn inline_cancel_registry() -> &'static CancelRegistry {
    INLINE_CANCEL_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a fresh cancel channel for `request_id` and return it. The
/// caller must call [`release_cancel_channel`] when it finishes (success,
/// failure, or already-cancelled) so the registry does not grow unbounded
/// across many completions.
fn take_cancel_channel(request_id: &str) -> Arc<Notify> {
    let notify = Arc::new(Notify::new());
    inline_cancel_registry()
        .lock()
        .expect("cancel registry poisoned")
        .insert(request_id.to_string(), Arc::clone(&notify));
    notify
}

fn release_cancel_channel(request_id: &str) {
    inline_cancel_registry()
        .lock()
        .expect("cancel registry poisoned")
        .remove(request_id);
}

/// Wake any in-flight completion registered under `request_id`. Returns
/// `true` if a matching request was found, so callers can distinguish
/// "cancelled an in-flight call" from "no-op for an unknown id".
fn cancel_inline_request(request_id: &str) -> bool {
    let notify = inline_cancel_registry()
        .lock()
        .expect("cancel registry poisoned")
        .get(request_id)
        .map(Arc::clone);
    match notify {
        Some(notify) => {
            notify.notify_waiters();
            true
        }
        None => false,
    }
}

/// Extract context around cursor position
fn extract_context(document: &str, cursor_pos: usize, context_lines: usize) -> (String, usize) {
    // Convert character offset to byte offset. If `cursor_pos` exceeds the
    // document length we clamp to the end, matching what the editor will
    // see when the caret is parked past the last character.
    let cursor_byte = document
        .char_indices()
        .nth(cursor_pos)
        .map(|(byte, _)| byte)
        .unwrap_or_else(|| document.len());

    let lines: Vec<&str> = document.lines().collect();

    // Find the index of the line containing the cursor by walking byte
    // offsets once. If the document is empty we treat the caret as being
    // on a virtual "line 0".
    let line_index = {
        let mut acc = 0usize;
        let mut idx = lines.len(); // sentinel: cursor is on a trailing empty line
        for (i, line) in lines.iter().enumerate() {
            let line_len = line.len();
            // The caret is on this line if it falls anywhere within
            // [acc, acc + line_len], inclusive of the end-of-line position.
            if cursor_byte <= acc + line_len {
                idx = i;
                break;
            }
            acc += line_len + 1; // +1 for the newline byte
        }
        idx.min(lines.len().saturating_sub(1).max(0))
    };

    // Calculate start/end line indices
    let start_line = line_index.saturating_sub(context_lines);
    let end_line = (line_index + context_lines + 1).min(lines.len());

    // Build context string with cursor marker
    let mut context_parts = Vec::new();

    for (i, line) in lines[start_line..end_line].iter().enumerate() {
        if start_line + i == line_index {
            // Split the line at the cursor byte offset without going through
            // a `Vec<char>` round-trip. `split_at` operates on the byte
            // boundary, which is safe here because we computed `col_bytes`
            // as a byte offset above and only enter the branch when the
            // boundary falls on a UTF-8 char boundary.
            let line_start = if start_line > 0 {
                lines[..start_line].iter().map(|l| l.len() + 1).sum::<usize>()
            } else {
                0
            };
            let col_bytes = cursor_byte.saturating_sub(line_start);
            let safe_col = col_bytes.min(line.len());
            let (before, after) = line.split_at(safe_col);

            context_parts.push(format!("{}\x00\x00\x00{}\n", before, after));
        } else {
            context_parts.push(format!("{}\n", line));
        }
    }

    let context = context_parts.join("");

    // Calculate cursor position in context (in characters)
    let before_lines: String = lines[start_line..line_index.min(end_line)]
        .iter()
        .map(|l| format!("{}\n", l))
        .collect();
    let cursor_in_context = before_lines.chars().count();

    (context, cursor_in_context)
}

/// Build prompt for inline completion
fn build_completion_prompt(
    context: &str,
    cursor_pos: usize,
    language: &str,
    file_path: Option<&str>,
) -> String {
    let file_info = file_path
        .map(|p| format!("Current file: {}\n", p))
        .unwrap_or_default();

    format!(
        r#"You are an expert code completion assistant. Complete the following {language} code naturally and concisely.

{file_info}
Rules:
1. Only output the completion text, nothing else
2. Match the surrounding code style and indentation
3. Complete the logical structure (function, block, statement)
4. Keep completion concise (typically 1-5 lines)
5. Do not include explanatory comments

Code:
```
{context}
```
Cursor position: {cursor_pos}

Completion:""#,
        language = language,
        file_info = file_info,
        context = context,
        cursor_pos = cursor_pos
    )
}

/// Generate a short, URL-safe completion ID. `simple()` returns just the first
/// 12 hex characters of the v4 UUID, which is enough entropy for an ID used
/// within a single AI completion request — full UUIDs are wasted bytes here.
fn generate_completion_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Call AI model for completion with retry logic (thinking disabled for speed)
async fn get_completion(
    config: &AIConfig,
    prompt: &str,
) -> Result<String, AIError> {
    let adapter = AIProviderAdapter::new(config.clone());

    // Use lower temperature for completions (more deterministic)
    let mut config = config.clone();
    config.temperature = 0.3;

    // Inline completion prompt - minimal system instructions
    let system_prompt = "You are a text completion assistant. Only output the completion text, nothing else.";

    // Retry logic for transient errors
    let max_retries = 2;
    let mut last_error: Option<AIError> = None;

    for attempt in 0..=max_retries {
        match adapter.completion(system_prompt, prompt).await {
            Ok(result) => return Ok(result),
            Err(error) if error.is_transient() => {
                last_error = Some(error);

                if attempt < max_retries {
                    let backoff_ms = 500 * (attempt + 1) as u64;
                    tracing::warn!(
                        "Transient completion error ({}), retrying in {}ms (attempt {}/{})",
                        last_error.as_ref().map(|e| e.to_string()).unwrap_or_default(),
                        backoff_ms,
                        attempt + 1,
                        max_retries,
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await;
                    continue;
                }

                break;
            }
            Err(error) => return Err(error),
        }
    }

    Err(AIError::ModelError(format!(
        "Service unavailable after {} retries. Last error: {}",
        max_retries,
        last_error
            .as_ref()
            .map(|e| e.to_string())
            .unwrap_or_else(|| "unknown".into())
    )))
}

/// Load a prompt file from the prompts directory
fn load_prompt(name: &str) -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("prompts")
            .join(name),
    )
    .unwrap_or_else(|_| {
        tracing::warn!("Prompt file {} not found, using fallback", name);
        String::new()
    })
}

/// Build prompt for docx (Word) inline completion
fn build_docx_completion_prompt(context: &str, cursor_pos: usize) -> String {
    let prompt_template = load_prompt("docx_complete.md");
    if prompt_template.is_empty() {
        // Fallback minimal prompt
        return format!(
            r#"Complete the following document text at the cursor position.
Only output the completion text in plain JSON format:
{{"completion": "...", "styles": []}}

Document:
{context}
Cursor position: {cursor_pos}

Completion:"#
        );
    }

    format!(
        "{}\n\nDocument:\n```\n{}\n```\nCursor position: {}",
        prompt_template, context, cursor_pos
    )
}

/// Parse a styled docx completion from JSON response
#[derive(Deserialize)]
struct DocxCompletionResponse {
    completion: String,
    #[serde(default)]
    styles: Vec<InlineStyle>,
}

fn parse_docx_completion(raw: &str) -> (String, Vec<InlineStyle>) {
    // Try to extract JSON from the response
    let trimmed = raw.trim();

    // Remove markdown code blocks if present
    let json_str = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    match serde_json::from_str::<DocxCompletionResponse>(json_str) {
        Ok(resp) => (resp.completion, resp.styles),
        Err(_) => {
            // Fallback: treat the whole response as plain text
            tracing::warn!("Failed to parse docx completion JSON, treating as plain text");
            (trimmed.to_string(), vec![])
        }
    }
}

/// Main handler for inline completion request
#[tauri::command]
pub async fn ai_inline_complete(
    request: InlineCompletionRequest,
    state: State<'_, AppState>,
) -> Result<InlineCompletionResponse, String> {
    // Reject requests with no `request_id` — the cancel path needs one to
    // route the wake-up. Older frontends that don't supply it fall back to a
    // derived id, but we still want a non-empty key in the registry.
    if request.request_id.is_empty() {
        return Err("inline completion request is missing request_id".to_string());
    }
    let request_id = request.request_id.clone();
    let cancel_notify = take_cancel_channel(&request_id);

    // RAII guard so we always release the registry slot, even on early
    // returns. The Arc<Notify> is dropped here, but cancel-side already
    // took its own clone before the entry is removed, so any in-flight
    // `select!` still sees a usable channel until it drops.
    struct ReleaseGuard<'a> {
        request_id: &'a str,
    }
    impl Drop for ReleaseGuard<'_> {
        fn drop(&mut self) {
            release_cancel_channel(self.request_id);
        }
    }
    let _release = ReleaseGuard { request_id: &request_id };

    tracing::info!(
        "[WORD-INLINE] Inline completion request - id: {}, language: {}, cursor: {}, doc_len: {}, snippet: {:?}",
        request_id,
        request.language,
        request.cursor_position,
        request.document.len(),
        request.snippet.as_ref().map(|s| format!("len={}", s.text.len()))
    );

    let config = state.ai_config.read().await.clone();

    tracing::debug!("Using AI config - model: {}, provider: {:?}", config.model, config.provider);

    // Use snippet if provided to reduce payload and improve responsiveness.
    let (source_text, cursor_pos) = if let Some(snippet) = &request.snippet {
        (snippet.text.as_str(), request.cursor_position)
    } else {
        (request.document.as_str(), request.cursor_position)
    };

    // Extract context around cursor (10 lines before and after)
    let (context, cursor_in_context) = extract_context(source_text, cursor_pos, 10);

    // Build prompt based on language
    let prompt = if request.language == "docx" {
        build_docx_completion_prompt(&context, cursor_in_context)
    } else {
        build_completion_prompt(
            &context,
            cursor_in_context,
            &request.language,
            request.file_path.as_deref(),
        )
    };

    tracing::debug!("Inline completion prompt:\n{}", prompt);

    // Get completion from AI, but make the request interruptible so cancelling
    // mid-flight actually drops the underlying HTTP call instead of just
    // discarding the response after the fact. We hold a strong reference to
    // the same `Notify` that the cancel side will look up, so the wake-up
    // is delivered even if the registry entry is released in between.
    let notify = Arc::clone(&cancel_notify);
    let raw_completion = tokio::select! {
        biased;
        _ = notify.notified() => {
            return Err("cancelled".to_string());
        }
        result = get_completion(&config, &prompt) => {
            result.map_err(|e| {
                tracing::error!("AI completion error: {}", e);
                format!("AI 请求失败: {}", e)
            })?
        }
    };

    tracing::info!("[WORD-INLINE] Received completion ({} chars)", raw_completion.len());

    // Parse completion based on language
    let (completion_text, styles) = if request.language == "docx" {
        parse_docx_completion(&raw_completion)
    } else {
        // Clean up code completion
        let cleaned = raw_completion
            .trim()
            .trim_start_matches("```")
            .trim_start_matches(&request.language)
            .trim()
            .trim_end_matches("```")
            .trim()
            .to_string();
        (cleaned, vec![])
    };

    // Create completion item
    let item = CompletionItem {
        id: generate_completion_id(),
        text: completion_text.clone(),
        display_text: {
            let chars: Vec<char> = completion_text.chars().collect();
            if chars.len() > 100 {
                chars[..100].iter().collect::<String>() + "..."
            } else {
                completion_text.clone()
            }
        },
        score: 0.9,
        range: None,
        styles,
    };

    let resp = InlineCompletionResponse {
        completions: vec![item],
        model: config.model.clone(),
        usage: None,
    };
    tracing::info!(
        "Returning completion: text={} ({} chars), styles={}",
        resp.completions[0].text,
        resp.completions[0].text.chars().count(),
        resp.completions[0].styles.len()
    );
    Ok(resp)
}

/// Cancel a pending completion request by its `request_id`. Wake-up is
/// delivered through the per-request `Notify` registered by
/// `ai_inline_complete`, so cancelling one in-flight request does not affect
/// concurrent completions in other windows / editors.
#[tauri::command]
pub async fn ai_inline_complete_cancel(request_id: String) -> Result<bool, String> {
    Ok(cancel_inline_request(&request_id))
}

/// Get current inline completion state
#[tauri::command]
pub fn get_inline_completion_state() -> InlineCompletionState {
    InlineCompletionState {
        enabled: true,
        current: None,
        is_loading: false,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_context() {
        let doc = "line 1\nline 2\nline 3\nline 4\nline 5";
        let (context, cursor) = extract_context(doc, 14, 1); // Position in "line 3"

        assert!(context.contains("line 2"));
        assert!(context.contains("line 3"));
        assert!(context.contains("line 4"));
    }

    #[test]
    fn test_build_completion_prompt() {
        let context = "fn main() {\n    |\n}";
        let prompt = build_completion_prompt(context, 15, "rust", Some("main.rs"));

        assert!(prompt.contains("rust"));
        assert!(prompt.contains("main.rs"));
        assert!(prompt.contains("fn main()"));
    }

    /// Per-request cancel registry smoke test: registering two distinct ids
    /// and cancelling only one must not affect the other. This guards
    /// against regressions where the global single-counter mechanism is
    /// reintroduced (the original bug — cancelling one in-flight completion
    /// was aborting every concurrent completion in other windows).
    #[tokio::test]
    async fn cancel_registry_isolates_requests() {
        // Always start from a clean registry slot for this test so we
        // don't conflict with other tests using the same id.
        let request_a = format!("test-a-{}", uuid::Uuid::new_v4());
        let request_b = format!("test-b-{}", uuid::Uuid::new_v4());

        let notify_a = take_cancel_channel(&request_a);
        let notify_b = take_cancel_channel(&request_b);

        // Start two `tokio::select!` races — one per request — and cancel
        // only `a`. `a`'s branch should resolve via `notified()`; `b`'s
        // branch should fall through to its own never-resolving future
        // and hit the timeout. This mirrors how `ai_inline_complete`
        // actually uses the channel.
        let cancelled_a = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            async {
                // Register `a`'s waiter first so `notify_waiters` fires
                // while we are sleeping.
                let pending = notify_a.notified();
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                assert!(cancel_inline_request(&request_a));
                pending.await;
                true
            },
        )
        .await
        .unwrap_or(false);

        let b_woken = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            async {
                let pending = notify_b.notified();
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                // Cancel only `a`. `b` must remain quiet.
                assert!(cancel_inline_request(&request_a));
                pending.await;
                true
            },
        )
        .await
        .unwrap_or(false);

        assert!(cancelled_a, "request_a should have been woken by its cancel");
        assert!(!b_woken, "request_b should NOT be woken by request_a's cancel");

        // Cleaning up is idempotent and does not panic.
        release_cancel_channel(&request_a);
        release_cancel_channel(&request_b);
    }

    /// Cancelling an unknown request id is a no-op (returns `false`) rather
    /// than panicking. This is the contract `ai_inline_complete_cancel`
    /// depends on.
    #[test]
    fn cancel_unknown_id_is_noop() {
        let unknown = format!("not-registered-{}", uuid::Uuid::new_v4());
        assert!(!cancel_inline_request(&unknown));
    }
}
