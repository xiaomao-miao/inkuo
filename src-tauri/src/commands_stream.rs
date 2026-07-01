use crate::{
    ai,
    ai_config::{self, AIConfigInput},
    commands::AppState,
    knowledge,
    streaming::{emit, KnowledgeSearchResult, StreamPayload},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use tauri::{AppHandle, State};
use thiserror::Error;
use urlencoding;

#[derive(Debug, Clone, Serialize, Deserialize, Error)]
pub enum StreamCommandError {
    #[error("Knowledge mode requires a workspace path")]
    MissingWorkspacePath,
    #[error("AI request failed: {0}")]
    AIRequest(String),
    #[error("Knowledge search failed: {0}")]
    KnowledgeSearch(String),
}

fn build_knowledge_context(results: &[knowledge::SearchResult]) -> String {
    if results.is_empty() {
        return "未检索到可用的知识库片段。".to_string();
    }

    results
        .iter()
        .enumerate()
        .map(|(index, item)| {
            format!(
                "[片段 {idx}]\n标题: {title}\n路径: {path}\n相似度: {score:.4}\n内容:\n{content}",
                idx = index + 1,
                title = item.document_title,
                path = item.file_path,
                score = item.score,
                content = item.content,
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn build_knowledge_instruction(
    user_question: &str,
    results: &[knowledge::SearchResult],
    selection: &str,
) -> String {
    let selection_block = if selection.trim().is_empty() {
        String::new()
    } else {
        format!("\n\n用户当前选中的文本：\n{}\n", selection)
    };

    format!(
        "用户问题：\n{question}{selection}\n\n知识库片段：\n{context}",
        question = user_question,
        selection = selection_block,
        context = build_knowledge_context(results),
    )
}

fn build_knowledge_references(results: &[knowledge::SearchResult]) -> String {
    let references: BTreeSet<String> = results
        .iter()
        .map(|item| {
            let fragment = match (item.start_line, item.end_line) {
                (Some(sl), Some(el)) if sl != el => format!("#{},{}", sl, el),
                (Some(sl), _) => format!("#{}", sl),
                _ => String::new(),
            };
            let encoded_path = urlencoding::encode(&item.file_path);
            format!(
                "- [{} — {}]({}{})",
                item.document_title,
                item.file_path,
                encoded_path,
                fragment,
            )
        })
        .collect();

    if references.is_empty() {
        "## 参考来源\n- 未检索到可用于回答当前问题的知识库片段".to_string()
    } else {
        format!("## 参考来源\n{}", references.into_iter().collect::<Vec<_>>().join("\n"))
    }
}

fn append_knowledge_references(answer: &str, results: &[knowledge::SearchResult]) -> String {
    let trimmed = answer.trim_end();
    let references = build_knowledge_references(results);

    if trimmed.contains("## 参考来源") {
        trimmed.to_string()
    } else if trimmed.is_empty() {
        references
    } else {
        format!("{}\n\n{}", trimmed, references)
    }
}

fn map_search_results(results: &[knowledge::SearchResult]) -> Vec<KnowledgeSearchResult> {
    results
        .iter()
        .map(|item| KnowledgeSearchResult {
            chunk_id: item.chunk_id.clone(),
            document_id: item.document_id.clone(),
            content: item.content.clone(),
            score: item.score,
            document_title: item.document_title.clone(),
            file_path: item.file_path.clone(),
            start_line: item.start_line,
            end_line: item.end_line,
        })
        .collect()
}

#[tauri::command]
pub async fn ai_stream_cancel(session_id: String) -> Result<(), StreamCommandError> {
    tracing::info!("Stream cancel requested for session: {}", session_id);
    crate::commands::mark_stream_cancelled(&session_id);
    Ok(())
}

#[tauri::command]
pub async fn ai_chat_stream(
    session_id: String,
    message_id: String,
    mode: String,
    instruction: String,
    original_text: Option<String>,
    workspace_path: Option<String>,
    config_input: AIConfigInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), StreamCommandError> {
    tracing::info!("ai_chat_stream start - session: {}, mode: {}", session_id, mode);
    let _ = state;
    let config = ai_config::build_input_ai_config(config_input)
        .map_err(|error| StreamCommandError::AIRequest(error.to_string()))?;
    let adapter = ai::AIProviderAdapter::new(config);
    let original_text = original_text.unwrap_or_default();

    let (instruction, original_text, knowledge_results) = if mode == "knowledge" {
        let workspace_path = workspace_path
            .as_deref()
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_string)
            .ok_or(StreamCommandError::MissingWorkspacePath)?;

        let results = knowledge::commands::knowledge_search(
            app.clone(),
            workspace_path,
            instruction.clone(),
            8,
        )
        .await
        .map_err(|error| StreamCommandError::KnowledgeSearch(error.to_string()))?;

        // Pass any user-provided selection through to the LLM as additional
        // grounding context (it might be the exact phrase the user is asking
        // about). The retrieved knowledge chunks still drive the prompt; the
        // selection is appended only if present.
        let knowledge_instruction = build_knowledge_instruction(&instruction, &results, original_text.as_str());
        (knowledge_instruction, String::new(), Some(results))
    } else {
        (instruction, original_text, None)
    };

    let session_id_for_cb = session_id.clone();
    let message_id_for_cb = message_id.clone();

    let result = adapter
        .chat_stream(mode.clone(), instruction, original_text, |delta| {
            if crate::commands::is_stream_cancelled(&session_id_for_cb) {
                return;
            }
            emit(&app, StreamPayload::text(&session_id_for_cb, &message_id_for_cb, &delta));
        })
        .await;

    tracing::info!(
        "ai_chat_stream adapter finished - session: {}, result: {:?}",
        session_id,
        result.is_ok()
    );

    if let Err(error) = &result {
        tracing::error!("AI chat error: {}", error);
        emit(
            &app,
            StreamPayload::error(&session_id, &message_id, &error.to_string()),
        );
        return Err(StreamCommandError::AIRequest(error.to_string()));
    }

    if crate::commands::clear_stream_cancelled(&session_id) {
        emit(&app, StreamPayload::cancelled(&session_id, &message_id));
        return Ok(());
    }

    // Adapter errors were already converted to a stream `error` event and
    // surfaced via early `return Err(...)` above, so any error reaching here
    // would be a control-flow regression. Treat it the same way as the
    // early-return path and bail out without emitting a final event.
    let final_content = match result {
        Ok(value) => {
            if mode == "knowledge" {
                append_knowledge_references(&value, knowledge_results.as_deref().unwrap_or(&[]))
            } else {
                value
            }
        }
        Err(error) => {
            tracing::error!("ai_chat_stream: adapter error leaked past early return: {}", error);
            return Err(StreamCommandError::AIRequest(error.to_string()));
        }
    };

    if mode == "knowledge" {
        let search_results = knowledge_results
            .as_deref()
            .map(map_search_results)
            .unwrap_or_default();
        emit(
            &app,
            StreamPayload::final_text_with_results(
                &session_id,
                &message_id,
                &final_content,
                search_results,
            ),
        );
    } else {
        emit(
            &app,
            StreamPayload::done(&session_id, &message_id, Some(&final_content)),
        );
    }

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
) -> Result<(), StreamCommandError> {
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
            // needs the terminal `error` event the same way `ai_chat_stream`
            // emits one. The IPC-level `Err` return below is for the
            // tauri::command contract; both signals are needed because the
            // frontend may be listening on either.
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
        emit(&app, StreamPayload::cancelled(&session_id, &message_id));
        return Ok(());
    }

    emit(
        &app,
        StreamPayload::summary(&session_id, &message_id, &result.summary, &result.content),
    );

    Ok(())
}
