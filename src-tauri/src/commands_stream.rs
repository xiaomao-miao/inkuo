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

fn build_knowledge_instruction(user_question: &str, results: &[knowledge::SearchResult]) -> String {
    format!(
        "用户问题：\n{question}\n\n知识库片段：\n{context}",
        question = user_question,
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
    crate::commands::STREAM_CANCELLED.lock().insert(session_id);
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
    let config = ai_config::build_input_ai_config(config_input);
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

        let knowledge_instruction = build_knowledge_instruction(&instruction, &results);
        (knowledge_instruction, String::new(), Some(results))
    } else {
        (instruction, original_text, None)
    };

    let session_id_for_cb = session_id.clone();
    let message_id_for_cb = message_id.clone();

    let result = adapter
        .chat_stream(mode.clone(), instruction, original_text, |delta| {
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
                    search_results: None,
                    done: false,
                    file_path: None,
                    original_content: None,
                    new_content: None,
                    diff_summary: None,
                    office_file_modified: None,
                },
            );
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
                error: Some(error.to_string()),
                search_results: None,
                done: true,
                file_path: None,
                original_content: None,
                new_content: None,
                diff_summary: None,
                office_file_modified: None,
            },
        );
        return Err(StreamCommandError::AIRequest(error.to_string()));
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
                search_results: None,
                done: true,
                file_path: None,
                original_content: None,
                new_content: None,
                diff_summary: None,
                office_file_modified: None,
            },
        );
        return Ok(());
    }

    let final_result = match result {
        Ok(value) => value,
        Err(_) => unreachable!("result error already returned above"),
    };
    let final_content = if mode == "knowledge" {
        append_knowledge_references(&final_result, knowledge_results.as_deref().unwrap_or(&[]))
    } else {
        final_result
    };
    let search_results = if mode == "knowledge" {
        knowledge_results.as_deref().map(map_search_results)
    } else {
        None
    };

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
            final_content: Some(final_content),
            error: None,
            search_results,
            done: true,
            file_path: None,
            original_content: None,
            new_content: None,
            diff_summary: None,
            office_file_modified: None,
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
                    search_results: None,
                    done: false,
                    file_path: None,
                    original_content: None,
                    new_content: None,
                    diff_summary: None,
                    office_file_modified: None,
                },
            );
        })
        .await
        .map_err(|error| StreamCommandError::AIRequest(error.to_string()))?;

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
                search_results: None,
                done: true,
                file_path: None,
                original_content: None,
                new_content: None,
                diff_summary: None,
                office_file_modified: None,
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
            search_results: None,
            done: true,
            file_path: None,
            original_content: None,
            new_content: None,
            diff_summary: None,
            office_file_modified: None,
        },
    );

    Ok(())
}
