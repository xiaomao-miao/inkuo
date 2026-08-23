//! Knowledge base configuration and models

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Knowledge base configuration for a workspace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeConfig {
    /// Workspace root path (used to generate isolated storage)
    pub workspace_path: PathBuf,
    /// Embedding model name
    pub embedding_model: String,
    /// Embedding dimension
    pub embedding_dim: usize,
    /// Target chunk size in characters
    pub chunk_size: usize,
    /// Chunk overlap in characters
    pub chunk_overlap: usize,
    /// Local model path
    pub model_path: PathBuf,
    /// Collection name (generated from workspace hash)
    pub collection_name: String,
}

impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self {
            workspace_path: PathBuf::new(),
            embedding_model: "BAAI/bge-large-zh-v1.5".to_string(),
            embedding_dim: 1024,
            chunk_size: 500,
            chunk_overlap: 50,
            model_path: PathBuf::new(),
            collection_name: String::new(),
        }
    }
}

/// Document metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub path: String,
    pub title: String,
    pub content: String,
    pub file_hash: String,
    /// Logical knowledge collection. `default` is used for legacy indexes
    /// and for files added before collection-aware retrieval was introduced.
    #[serde(default = "default_collection")]
    pub collection: String,
    /// Normalized source format (for example `pdf`, `docx`, `typescript`).
    /// This is surfaced in the UI so unsupported/failed imports are clear.
    #[serde(default)]
    pub source_type: String,
    #[serde(default)]
    pub size_bytes: u64,
}

pub fn default_collection() -> String {
    "default".to_string()
}

/// Text chunk with embedding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub document_id: String,
    pub content: String,
    pub chunk_index: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub embedding: Vec<f32>,
    #[serde(default = "default_collection")]
    pub collection: String,
}

/// Search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub chunk_id: String,
    pub document_id: String,
    pub content: String,
    pub score: f32,
    pub document_title: String,
    pub file_path: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    #[serde(default = "default_collection")]
    pub collection: String,
}

/// Build result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildResult {
    pub total_documents: usize,
    pub total_chunks: usize,
    pub workspace_id: String,
}

/// Update result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateResult {
    pub added: usize,
    pub removed: usize,
    pub updated: usize,
    /// Paths already present with the same content hash.
    #[serde(default)]
    pub unchanged: usize,
    /// Number of files that could not be read or parsed.
    #[serde(default)]
    pub failed: usize,
    /// Per-file diagnostics. Batch imports remain useful when one file is bad.
    #[serde(default)]
    pub failures: Vec<ImportFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportFailure {
    pub path: String,
    pub error: String,
}
