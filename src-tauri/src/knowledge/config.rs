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
}

/// Text chunk with embedding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub document_id: String,
    pub content: String,
    pub chunk_index: usize,
    pub embedding: Vec<f32>,
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
}
