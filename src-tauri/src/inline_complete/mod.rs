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
use std::sync::Arc;
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
///
/// We use `parking_lot::Mutex` here instead of `std::sync::Mutex`: it has no
/// poisoning semantics, so a panic in one thread can't cascade into a panic
/// (and broken cancellation) across every subsequent completion. The other
/// global registries in `commands.rs` use the same lock type for the same
/// reason.
type CancelRegistry = parking_lot::Mutex<HashMap<String, Arc<Notify>>>;

static INLINE_CANCEL_REGISTRY: std::sync::OnceLock<CancelRegistry> =
    std::sync::OnceLock::new();

// ── Cancel registry ─────────────────────────────────────────────────────────────

fn inline_cancel_registry() -> &'static CancelRegistry {
    INLINE_CANCEL_REGISTRY.get_or_init(|| parking_lot::Mutex::new(HashMap::new()))
}

/// Register a fresh cancel channel for `request_id` and return it. The
/// caller must call [`release_cancel_channel`] when it finishes (success,
/// failure, or already-cancelled) so the registry does not grow unbounded
/// across many completions.
fn take_cancel_channel(request_id: &str) -> Arc<Notify> {
    let notify = Arc::new(Notify::new());
    inline_cancel_registry()
        .lock()
        .insert(request_id.to_string(), Arc::clone(&notify));
    notify
}

fn release_cancel_channel(request_id: &str) {
    inline_cancel_registry().lock().remove(request_id);
}

/// Wake any in-flight completion registered under `request_id`. Returns
/// `true` if a matching request was found, so callers can distinguish
/// "cancelled an in-flight call" from "no-op for an unknown id".
fn cancel_inline_request(request_id: &str) -> bool {
    let notify = inline_cancel_registry()
        .lock()
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

/// Iterable cursor marker that the model can actually see in the prompt
/// (NUL bytes tend to be stripped or merged by some chat-completion APIs and
/// the model rarely treats them as a strong delimiter). The marker is
/// intentionally unusual so it does not collide with typical source content.
const CURSOR_MARKER: &str = "<|cursor|>";

/// Strip leading overlap between the model's output and the text that
/// already follows the cursor in the source document.
///
/// This handles the most common failure mode of inline completion: the model
/// repeats the trailing characters of the prefix (or worse, the entire
/// prefix) instead of starting fresh from the cursor. We compare the
/// completion against the suffix (up to a cap) and trim any matching
/// leading substring. We also strip the bare cursor marker itself, in case
/// the model echoes it back.
fn strip_repeated_prefix(completion: &str, suffix_after_cursor: &str) -> String {
    let mut text = completion;

    // Model may echo the marker back. Strip any leading occurrences.
    while let Some(rest) = text.strip_prefix(CURSOR_MARKER) {
        text = rest;
    }

    // Trim a leading newline run: the suffix already starts after the cursor
    // and any newlines the model re-emits would inflate the prefix.
    while text.starts_with('\n') || text.starts_with('\r') {
        text = &text[1..];
    }

    // Cap the comparison length so we never do O(n^2) on huge docs.
    const MAX_OVERLAP: usize = 256;
    let suffix_chars: Vec<char> = suffix_after_cursor.chars().take(MAX_OVERLAP).collect();
    let text_chars: Vec<char> = text.chars().take(MAX_OVERLAP).collect();

    // Find the longest prefix of `text_chars` that is also a prefix of
    // `suffix_chars`. We only need the longest match, so we walk from the
    // longest possible length downward and stop at the first match.
    let mut overlap = 0usize;
    let max_check = suffix_chars.len().min(text_chars.len());
    for len in (1..=max_check).rev() {
        if text_chars[..len] == suffix_chars[..len] {
            // Require a minimum overlap of 2 chars so we don't strip a
            // single trivial character (e.g. a stray space) that the user
            // almost certainly did want.
            if len >= 2 {
                overlap = len;
            }
            break;
        }
    }

    if overlap > 0 {
        // `overlap` is a *char* count, so we have to walk the original
        // string by chars (not bytes) to keep the UTF-8 boundary sane.
        let cut = text.chars().take(overlap).map(|c| c.len_utf8()).sum();
        text = &text[cut..];
    }

    text.to_string()
}

/// Extract context around cursor position.
///
/// Returns:
/// - `context`: the joined snippet text with `CURSOR_MARKER` inserted at
///   the cursor position.
/// - `cursor_in_context`: the UTF-8 byte offset of the marker within
///   `context`, suitable for [`str::split_at`].
///
/// The cursor marker is placed inside the source line itself (not on a
/// separate line) so the model sees the exact byte it needs to continue
/// from, including whatever indentation the user already has.
fn extract_context(document: &str, cursor_pos: usize, context_lines: usize) -> (String, usize) {
    // Convert character offset to byte offset. If `cursor_pos` exceeds the
    // document length we clamp to the end, matching what the editor will
    // see when the caret is parked past the last character.
    let cursor_byte = document
        .char_indices()
        .nth(cursor_pos)
        .map(|(byte, _)| byte)
        .unwrap_or_else(|| document.len());

    // Store exact byte offsets instead of using `str::lines()`: `lines()`
    // discards the final empty line, which made a caret after a trailing
    // newline jump back to the previous line. Keeping offsets also preserves
    // CRLF and avoids recomputing the wrong line start when the context window
    // begins before the caret line.
    let mut line_starts = vec![0usize];
    for (byte, ch) in document.char_indices() {
        if ch == '\n' {
            line_starts.push(byte + ch.len_utf8());
        }
    }

    // The caret belongs to the last line whose start is not after it. A caret
    // on a newline byte therefore remains at the end of the preceding line;
    // a caret immediately after it belongs to the following line.
    let line_index = line_starts
        .partition_point(|start| *start <= cursor_byte)
        .saturating_sub(1);

    let start_line = line_index.saturating_sub(context_lines);
    let end_line = (line_index + context_lines + 1).min(line_starts.len());
    let context_start = line_starts[start_line];
    let context_end = line_starts.get(end_line).copied().unwrap_or(document.len());
    let cursor_in_context = cursor_byte - context_start;

    let source = &document[context_start..context_end];
    let (before, after) = source.split_at(cursor_in_context);
    let context = format!("{before}{CURSOR_MARKER}{after}");

    (context, cursor_in_context)
}

// ── Prompt construction ─────────────────────────────────────────────────────────

/// Build prompt for inline completion (FIM-style).
///
/// `prefix` is the text *before* the cursor (within the snippet window),
/// `suffix` is the text *after* the cursor. The model is told to output
/// only the continuation — i.e. the text that should be inserted at the
/// cursor, *not* a regenerated copy of `prefix` or any portion of it.
fn build_completion_prompt(
    prefix: &str,
    suffix: &str,
    language: &str,
    file_path: Option<&str>,
) -> String {
    let file_info = file_path
        .map(|p| format!("Current file: {}\n", p))
        .unwrap_or_default();

    let prompt = "You are an expert {language} code completion assistant.\n\n{file_info}The user pressed Tab to request an inline completion. Their cursor sits between the PREFIX and SUFFIX shown below. Output ONLY the text that should be inserted between them. Do NOT repeat, rephrase, or echo any part of the prefix.\n\nRules:\n1. Output ONLY the new text to insert at the cursor. No preamble, no labels, no markdown fences, no explanation.\n2. Match the surrounding code style and indentation exactly (same number of leading spaces/tabs).\n3. Continue the current logical structure (function, block, statement) naturally.\n4. Keep completion concise (typically 1-5 lines).\n5. Do not include explanatory comments.\n6. NEVER repeat the PREFIX (even partially). If the prefix ends with `2. ` or any other list marker, continue from there. Do NOT re-emit a duplicate marker.\n7. Do NOT output the cursor marker or any closing braces / punctuation that already appears at the start of SUFFIX.\n\n```\n<|cursor_start|>PREFIX\n{prefix}<|cursor_end|><|cursor_start|>SUFFIX\n{suffix}<|cursor_end|>\n```\n\nOutput the continuation now:";

    prompt
        .replace("{language}", language)
        .replace("{file_info}", &file_info)
        .replace("{prefix}", prefix)
        .replace("{suffix}", suffix)
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

/// Build prompt for docx (Word) inline completion (FIM-style).
///
/// `prefix` is the text before the cursor, `suffix` is the text after the
/// cursor (within the snippet window surrounding the caret). The model is
/// told to output only the continuation, not a regenerated copy of the
/// prefix.
fn build_docx_completion_prompt(prefix: &str, suffix: &str, cursor_pos: usize) -> String {
    let prompt_template = load_prompt("docx_complete.md");
    if prompt_template.is_empty() {
        // Fallback minimal prompt — still FIM-style so the model is unlikely
        // to repeat the prefix even when the full prompt template is missing.
        return format!(
            "Complete the document text at the cursor position (between PREFIX and SUFFIX).\n\
             Output ONLY a JSON object: {{\"completion\": \"...\", \"styles\": []}}\n\n\
             Cursor position: {cursor_pos}\n\n\
             <|cursor_start|>PREFIX\n\
             {prefix}<|cursor_end|><|cursor_start|>SUFFIX\n\
             {suffix}<|cursor_end|>",
        );
    }

    format!(
        "{}\n\n\
         Cursor position: {cursor_pos}\n\n\
         <|cursor_start|>PREFIX\n\
         {prefix}<|cursor_end|><|cursor_start|>SUFFIX\n\
         {suffix}<|cursor_end|>",
        prompt_template,
        cursor_pos = cursor_pos,
        prefix = prefix,
        suffix = suffix,
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

    let config = state
        .ai_config
        .resolve()
        .await
        .map_err(|e| format!("resolve AI config: {}", e))?;

    tracing::debug!("Using AI config - model: {}, provider: {:?}", config.model, config.provider);

    // Use snippet if provided to reduce payload and improve responsiveness.
    let (source_text, cursor_pos) = if let Some(snippet) = &request.snippet {
        (snippet.text.as_str(), request.cursor_position)
    } else {
        (request.document.as_str(), request.cursor_position)
    };

    // Extract context around cursor (10 lines before and after). The returned
    // `context` contains the CURSOR_MARKER inlined at the cursor position; we
    // split around it so the prompt can talk about PREFIX / SUFFIX explicitly
    // and the model can be told not to repeat the prefix.
    let (context, cursor_byte_in_context) = extract_context(source_text, cursor_pos, 10);

    let (prefix, suffix) = if cursor_byte_in_context < context.len() {
        context.split_at(cursor_byte_in_context)
    } else {
        (context.as_str(), "")
    };
    let prefix = if prefix.ends_with(CURSOR_MARKER) {
        &prefix[..prefix.len() - CURSOR_MARKER.len()]
    } else {
        prefix
    };
    let suffix = if let Some(stripped) = suffix.strip_prefix(CURSOR_MARKER) {
        stripped
    } else {
        suffix
    };

    // Build prompt based on language
    let prompt = if request.language == "docx" {
        build_docx_completion_prompt(prefix, suffix, prefix.chars().count())
    } else {
        build_completion_prompt(
            prefix,
            suffix,
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

    // Strip overlapping prefix: most models occasionally regurgitate the
    // last few characters of the original prefix (or worse, the entire
    // prefix) instead of starting fresh from the cursor. Removing this
    // overlap is cheap and keeps the user-visible result "continues from
    // cursor" instead of "stutters and then continues".
    let completion_text = strip_repeated_prefix(&completion_text, suffix);

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
    use super::{extract_context, CURSOR_MARKER};

    fn split_context(document: &str, cursor: usize, context_lines: usize) -> (String, String) {
        let (context, marker_byte) = extract_context(document, cursor, context_lines);
        let (prefix, marked_suffix) = context.split_at(marker_byte);
        let suffix = marked_suffix
            .strip_prefix(CURSOR_MARKER)
            .expect("context must contain the cursor marker at the returned offset");
        (prefix.to_string(), suffix.to_string())
    }

    #[test]
    fn extract_context_marks_an_empty_document() {
        let (prefix, suffix) = split_context("", 0, 10);
        assert_eq!(prefix, "");
        assert_eq!(suffix, "");
    }

    #[test]
    fn extract_context_uses_character_offsets_for_unicode_text() {
        let document = "第一行\n中文光标位置\n第三行";
        // Seven characters: 第一行 + newline + 中文光.
        let (prefix, suffix) = split_context(document, 7, 10);
        assert_eq!(prefix, "第一行\n中文光");
        assert_eq!(suffix, "标位置\n第三行");
    }

    #[test]
    fn extract_context_keeps_a_trailing_empty_line() {
        let (prefix, suffix) = split_context("标题\n", 3, 10);
        assert_eq!(prefix, "标题\n");
        assert_eq!(suffix, "");
    }

    #[test]
    fn extract_context_crops_by_lines_without_moving_the_cursor() {
        let document = "zero\none\ntwo\nthree\nfour";
        // Cursor after "thr" on line three.
        let cursor = "zero\none\ntwo\nthr".chars().count();
        let (prefix, suffix) = split_context(document, cursor, 1);
        assert_eq!(prefix, "two\nthr");
        assert_eq!(suffix, "ee\nfour");
    }

    #[test]
    fn extract_context_clamps_an_out_of_range_cursor() {
        let (prefix, suffix) = split_context("内容", usize::MAX, 1);
        assert_eq!(prefix, "内容");
        assert_eq!(suffix, "");
    }
}
