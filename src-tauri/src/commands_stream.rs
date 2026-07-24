use crate::{
    ai,
    commands::AppState,
    streaming::{emit, StreamPayload},
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, Error)]
pub enum StreamCommandError {
    #[error("AI request failed: {0}")]
    AIRequest(String),
}

#[tauri::command]
pub async fn ai_stream_cancel(session_id: String) -> Result<(), StreamCommandError> {
    tracing::info!("Stream cancel requested for session: {}", session_id);
    crate::commands::mark_stream_cancelled(&session_id);
    Ok(())
}

/// Single-shot streaming chat completion for the floating AI popovers.
///
/// Unlike `ai_agent_stream`, this command does **not** run the agent
/// loop — no tool calls, no iterations, no baseline snapshots. It
/// routes through `AIProviderAdapter::chat_stream`, which performs a
/// single streamed chat completion (ask system prompt by default)
/// and emits one `text` delta per SSE chunk. Frontends listen on
/// `ai://stream` keyed by `session_id` and accumulate the deltas
/// into their own UI state.
///
/// Cancellation: `ai_ask_cancel` flips the global per-session
/// cancellation flag that the inner `chat_stream` callback reads
/// before emitting each delta. The Rust side returns Ok early when
/// the flag is set.
#[tauri::command]
pub async fn ai_ask_stream(
    session_id: String,
    message_id: String,
    instruction: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), StreamCommandError> {
    // Resolve a fresh AIConfig per call so a rotated cloud access
    // token is picked up automatically. Mirrors the `ai_edit_stream`
    // path for consistency.
    let config = state
        .ai_config
        .resolve()
        .await
        .map_err(|e| StreamCommandError::AIRequest(format!("resolve AI config: {}", e)))?;
    let adapter = ai::AIProviderAdapter::new(config);

    let session_id_for_cb = session_id.clone();
    let message_id_for_cb = message_id.clone();

    // Cleanup guard: any return path from this function must clear the
    // stream-cancelled flag so it does not leak into the next call.
    let _cancel_guard = crate::commands::StreamCancelGuard::new(&session_id);

    // `chat_stream` does not take a separate `original_text` slot
    // (the popover template already inlines the selection into the
    // `instruction` string), so we pass an empty original_text. The
    // `mode` defaults to "ask" for any value other than "plan".
    let result = match adapter
        .chat_stream(
            "ask".to_string(),
            instruction,
            String::new(),
            |delta| {
                if crate::commands::is_stream_cancelled(&session_id_for_cb) {
                    return;
                }
                emit(
                    &app,
                    StreamPayload::text(&session_id_for_cb, &message_id_for_cb, &delta),
                );
            },
        )
        .await
    {
        Ok(value) => value,
        Err(error) => {
            let message = error.to_string();
            tracing::error!("AI ask stream error: {}", message);
            emit(
                &app,
                StreamPayload::error(&session_id, &message_id, &message),
            );
            return Err(StreamCommandError::AIRequest(message));
        }
    };

    if crate::commands::clear_stream_cancelled(&session_id) {
        _cancel_guard.clear();
        emit(
            &app,
            StreamPayload::cancelled(&session_id, &message_id),
        );
        return Ok(());
    }

    _cancel_guard.clear();

    // Emit a terminal `done` event with the final accumulated content
    // so the frontend can replace the streamed concatenation with the
    // model-resolved version (in case any post-processing happened
    // server-side, e.g. trimming whitespace).
    emit(
        &app,
        StreamPayload::done(&session_id, &message_id, Some(&result)),
    );

    Ok(())
}

/// Cancel a running `ai_ask_stream` invocation. Best-effort: the
/// command always succeeds even if the session id is unknown or the
/// underlying stream already finished.
#[tauri::command]
pub async fn ai_ask_cancel(session_id: String) -> Result<(), StreamCommandError> {
    crate::commands::mark_stream_cancelled(&session_id);
    Ok(())
}

#[tauri::command]
pub async fn ai_edit_stream(
    session_id: String,
    message_id: String,
    instruction: String,
    original_text: String,
    scope: String,
    context: Vec<ai::ContextItem>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), StreamCommandError> {
    // Resolve a fresh AIConfig per call so a rotated cloud access
    // token is picked up automatically. The previous code cloned a
    // cached AIConfig out of a shared `RwLock` — which silently went
    // stale when the cloud access token TTL elapsed.
    let config = state
        .ai_config
        .resolve()
        .await
        .map_err(|e| StreamCommandError::AIRequest(format!("resolve AI config: {}", e)))?;
    let adapter = ai::AIProviderAdapter::new(config);

    let edit_scope = match scope.as_str() {
        "selection" => ai::EditScope::Selection,
        "paragraph" => ai::EditScope::Paragraph,
        "section" => ai::EditScope::Section,
        "document" => ai::EditScope::Document,
        _ => ai::EditScope::Selection,
    };

    let request = ai::AIEditRequest {
        instruction,
        original_text: original_text.clone(),
        scope: edit_scope,
        context,
    };

    let session_id_for_cb = session_id.clone();
    let message_id_for_cb = message_id.clone();

    // Cleanup guard: any return path from this function must clear the
    // stream-cancelled flag so it does not leak into the next call. Drop
    // fires on any return path so a panic or a future refactor that adds
    // a new `?` branch cannot leak the flag.
    let _cancel_guard = crate::commands::StreamCancelGuard::new(&session_id);

    let result = match adapter
        .edit_stream(request, |delta| {
            if crate::commands::is_stream_cancelled(&session_id_for_cb) {
                return;
            }
            emit(&app, StreamPayload::text(&session_id_for_cb, &message_id_for_cb, &delta));
        })
        .await
    {
        Ok(value) => value,
        Err(error) => {
            // Surface the failure on the stream channel so the frontend UI
            // doesn't get stuck on a half-finished "loading" state — it
            // needs the terminal `error` event the same way the agent
            // stream emits one. The IPC-level `Err` return below is for
            // the tauri::command contract; both signals are needed
            // because the frontend may be listening on either.
            let message = error.to_string();
            tracing::error!("AI edit stream error: {}", message);
            emit(
                &app,
                StreamPayload::error(&session_id, &message_id, &message),
            );
            return Err(StreamCommandError::AIRequest(message));
        }
    };

    if crate::commands::clear_stream_cancelled(&session_id) {
        // User requested cancellation: tell the guard to skip its drop
        // cleanup (we just cleared) by consuming it.
        _cancel_guard.clear();
        emit(&app, StreamPayload::cancelled(&session_id, &message_id));
        return Ok(());
    }

    // Consume the guard so its Drop does not run another clear.
    _cancel_guard.clear();

    emit(
        &app,
        StreamPayload::summary(&session_id, &message_id, &result.summary, &result.content),
    );

    Ok(())
}