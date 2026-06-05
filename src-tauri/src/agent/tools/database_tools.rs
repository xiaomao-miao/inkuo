//! Database search tool: semantic search over the workspace knowledge base
//!
//! Uses the shared search infrastructure from knowledge::commands::search_knowledge_base,
//! which routes through the shared vector store cache to avoid WAL lock conflicts.

use serde_json::Value;

use super::{ToolDefinition, ToolError, ToolParameters};
use crate::knowledge::search_knowledge_base;

/// Database search tool — semantic search over the workspace knowledge base.
/// Requires that the knowledge base has been built first (via knowledge_build).
#[derive(Clone)]
pub struct DatabaseSearchTool {
    app: tauri::AppHandle,
}

impl DatabaseSearchTool {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }

    pub fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "database_search",
            "Search the workspace knowledge base using semantic (vector) search. \
            Use this when the user asks questions about code, documents, or information \
            that may be answered from indexed files in the workspace. \
            Returns the most relevant chunks ranked by semantic similarity. \
            Note: the knowledge base must be built first using knowledge_build.",
            ToolParameters::new(
                vec!["query"],
                vec![
                    (
                        "query",
                        "string",
                        Some("Natural language search query (e.g., 'how does the auth system work?')"),
                    ),
                    (
                        "workspace_path",
                        "string",
                        Some("Absolute path to the workspace root."),
                    ),
                    (
                        "top_k",
                        "integer",
                        Some("Maximum number of results to return. Default: 5. Range: 1-20."),
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
                    "database_search".to_string(),
                    "query must be a string".into(),
                )
            })?;

        let workspace_path = arguments["workspace_path"]
            .as_str()
            .ok_or_else(|| {
                ToolError::InvalidArguments(
                    "database_search".to_string(),
                    "workspace_path must be a string".into(),
                )
            })?;

        let top_k = arguments["top_k"]
            .as_i64()
            .map(|v| v.clamp(1, 20) as usize)
            .unwrap_or(5);

        // Validate workspace path
        super::validate_workspace_path(workspace_path, &Some(workspace_path.to_string()))?;

        // Search through the shared infrastructure (same cache as KB mode)
        let results = crate::knowledge::search_knowledge_base(
            &self.app,
            workspace_path,
            query,
            top_k,
        )
        .await
        .map_err(|e| ToolError::ExecutionError(e))?;

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
                 File: {}{}\n\
                 Content:\n{}\n\n",
                i + 1,
                result.score,
                result.file_path,
                lines,
                result.content.trim()
            ));
        }

        Ok(output)
    }
}
