//! Database search tool: semantic search over the workspace knowledge base
//!
//! Uses the shared search infrastructure from knowledge::commands::search_knowledge_base,
//! which routes through the shared vector store cache to avoid WAL lock conflicts.

use serde_json::Value;

use super::{ToolDefinition, ToolError, ToolParameters};

/// Database search tool — semantic search over the workspace knowledge base.
/// Requires that the knowledge base has been built first (via the Knowledge tab in the UI).
#[derive(Clone)]
pub struct DatabaseSearchTool {
    app: tauri::AppHandle,
}

impl DatabaseSearchTool {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new_with_label(
            "database_search",
            "搜索知识库",
            "Search the workspace knowledge base using semantic (vector) search. \
            Use this when the user asks questions about code, documents, or information \
            that may be answered from indexed files in the workspace. \
            Returns the most relevant chunks ranked by semantic similarity. When \
            `collection` is omitted, all indexed collections are searched and merged \
            into one global top-k; each result identifies its source collection. Set \
            `collection` only when the user explicitly wants one named collection. \
            Note: the knowledge base must be built first via the Knowledge tab in the UI. \
            The active workspace is determined by the registry, not by this tool's arguments, \
            so there is no `workspace_path` parameter to set.",
            ToolParameters::new(
                vec!["query"],
                vec![
                    (
                        "query",
                        "string",
                        Some("Natural language search query (e.g., 'how does the auth system work?')"),
                    ),
                    (
                        "top_k",
                        "integer",
                        Some("Maximum number of results to return. Default: 5. Range: 1-20."),
                    ),
                    (
                        "collection",
                        "string",
                        Some("Optional named knowledge collection. Omit to search every indexed collection and return the global top results."),
                    ),
                ],
            ),
        )
    }

    pub async fn execute(
        &self,
        arguments: Value,
        workspace: Option<String>,
    ) -> Result<String, ToolError> {
        let query = arguments["query"].as_str().ok_or_else(|| {
            ToolError::InvalidArguments(
                "database_search".to_string(),
                "query must be a string".into(),
            )
        })?;

        // Always use the registry-provided workspace, not an AI-supplied one.
        // Allowing the LLM to override the workspace would defeat the security
        // boundary established by the immutable AgentSession workspace. If
        // the session has no workspace, the tool simply isn't usable.
        let workspace_path = workspace.as_deref().ok_or_else(|| {
            ToolError::ExecutionError(
                "database_search requires an active workspace; none is configured".into(),
            )
        })?;

        let top_k = arguments["top_k"]
            .as_i64()
            .map(|v| v.clamp(1, 20) as usize)
            .unwrap_or(5);
        let collection = arguments["collection"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(crate::knowledge::validate_collection_name)
            .transpose()
            .map_err(|message| {
                ToolError::InvalidArguments("database_search".to_string(), message)
            })?;

        // Search through the shared infrastructure (same cache as KB mode).
        // None intentionally means every indexed collection, not "default".
        let results = crate::knowledge::search_knowledge_base_in_collection(
            &self.app,
            workspace_path,
            query,
            top_k,
            collection.as_deref(),
        )
        .await
        .map_err(|e| ToolError::ExecutionError(e.to_string()))?;

        if results.is_empty() {
            return Ok(format!(
                "No results found for query: \"{}\". Try rephrasing or check if the knowledge base contains relevant documents.",
                query
            ));
        }

        let mut output = format!(
            "Found {} result(s) for query: \"{}\"\n\n",
            results.len(),
            query
        );

        for (i, result) in results.iter().enumerate() {
            let lines = result
                .start_line
                .map(|s| {
                    result
                        .end_line
                        .map(|e| format!(" (lines {}-{})", s, e))
                        .unwrap_or_else(|| format!(" (line {})", s))
                })
                .unwrap_or_default();

            output.push_str(&format!(
                "--- Result {} [score: {:.4}] ---\n\
                 Collection: {}\n\
                 File: {}{}\n\
                 Content:\n{}\n\n",
                i + 1,
                result.score,
                result.collection,
                result.file_path,
                lines,
                result.content.trim()
            ));
        }

        Ok(output)
    }
}
