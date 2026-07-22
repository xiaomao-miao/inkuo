//! Knowledge base module
//!
//! Handles:
//! - Document scanning and chunking
//! - Embedding generation using local models
//! - Vector storage with Qdrant (embedded mode)
//! - Workspace isolation (each workspace has its own knowledge base)
//! - Incremental updates based on file hash

mod chunker;
pub mod commands;
mod config;
mod embedder;
pub mod embedding_models;
mod metadata;
mod scanner;
mod vector_store;

// Re-export the embedding-model types so the `#[tauri::command]`
// wrappers in `commands.rs` (and any other caller that historically
// imported `commands::EmbeddingModelInfo`) keep compiling unchanged
// while the canonical definition lives in `embedding_models.rs`.
pub use embedding_models::EmbeddingModelInfo;

pub use chunker::*;
pub use commands::*;
pub use config::*;
pub use embedder::*;
pub use metadata::*;
pub use scanner::*;
pub use vector_store::*;
