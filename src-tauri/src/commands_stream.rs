use crate::{ai, commands::AppState, streaming::StreamPayload};
use tauri::{AppHandle, Emitter, State};

fn emit(app: &AppHandle, payload: StreamPayload) {
    let _ = app.emit("ai://stream", payload);
}

#[tauri::command]
pub async fn ai_stream_cancel(session_id: String) -> Result<(), String> {
    tracing::info!("Stream cancel requested for session: {}", session_id);
    crate::commands::STREAM_CANCELLED
        .lock()
        .insert(session_id);
    Ok(())
}

#[tauri::command]
pub async fn ai_chat_stream(
    session_id: String,
    message_id: String,
    mode: String,
    instruction: String,
    original_text: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    tracing::info!("ai_chat_stream start - session: {}, mode: {}", session_id, mode);
    let config = state.ai_config.read().await.clone();
    let adapter = ai::AIProviderAdapter::new(config);

    let session_id_for_cb = session_id.clone();
    let message_id_for_cb = message_id.clone();

    let result = adapter
        .chat_stream(mode, instruction, original_text, |delta| {
            if crate::commands::STREAM_CANCELLED.lock().contains(&session_id_for_cb) {
                return;
            }
            emit(
                &app,
                StreamPayload {
                    session_id: session_id_for_cb.clone(),
                    message_id: message_id_for_cb.clone(),
                    event_type: "text".to_string(),
                    content: Some(delta),
                    summary: None,
                    tool_call_id: None,
                    tool_name: None,
                    tool_args: None,
                    final_content: None,
                    error: None,
                    done: false,
                },
            );
        })
        .await;

    tracing::info!("ai_chat_stream adapter finished - session: {}, result: {:?}", session_id, result.is_ok());

    if let Err(e) = &result {
        tracing::error!("AI chat error: {}", e);
        emit(
            &app,
            StreamPayload {
                session_id: session_id.clone(),
                message_id: message_id.clone(),
                event_type: "error".to_string(),
                content: None,
                summary: None,
                tool_call_id: None,
                tool_name: None,
                tool_args: None,
                final_content: None,
                error: Some(e.to_string()),
                done: true,
            },
        );
        return Err(format!("AI error: {}", e));
    }

    if crate::commands::STREAM_CANCELLED.lock().remove(&session_id) {
        emit(
            &app,
            StreamPayload {
                session_id,
                message_id,
                event_type: "error".to_string(),
                content: None,
                summary: None,
                tool_call_id: None,
                tool_name: None,
                tool_args: None,
                final_content: None,
                error: Some("cancelled".to_string()),
                done: true,
            },
        );
        return Ok(());
    }

    emit(
        &app,
        StreamPayload {
            session_id,
            message_id,
            event_type: "text".to_string(),
            content: None,
            summary: None,
            tool_call_id: None,
            tool_name: None,
            tool_args: None,
            final_content: Some(result.unwrap()),
            error: None,
            done: true,
        },
    );

    tracing::info!("ai_chat_stream done event emitted");
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
) -> Result<(), String> {
    let config = state.ai_config.read().await.clone();
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

    let result = adapter
        .edit_stream(request, |delta| {
            if crate::commands::STREAM_CANCELLED.lock().contains(&session_id_for_cb) {
                return;
            }
            emit(
                &app,
                StreamPayload {
                    session_id: session_id_for_cb.clone(),
                    message_id: message_id_for_cb.clone(),
                    event_type: "text".to_string(),
                    content: Some(delta),
                    summary: None,
                    tool_call_id: None,
                    tool_name: None,
                    tool_args: None,
                    final_content: None,
                    error: None,
                    done: false,
                },
            );
        })
        .await
        .map_err(|e| format!("AI error: {}", e))?;

    if crate::commands::STREAM_CANCELLED.lock().remove(&session_id) {
        emit(
            &app,
            StreamPayload {
                session_id,
                message_id,
                event_type: "error".to_string(),
                content: None,
                summary: None,
                tool_call_id: None,
                tool_name: None,
                tool_args: None,
                final_content: None,
                error: Some("cancelled".to_string()),
                done: true,
            },
        );
        return Ok(());
    }

    emit(
        &app,
        StreamPayload {
            session_id,
            message_id,
            event_type: "summary".to_string(),
            content: None,
            summary: Some(result.summary),
            tool_call_id: None,
            tool_name: None,
            tool_args: None,
            final_content: Some(result.content),
            error: None,
            done: true,
        },
    );

    Ok(())
}
