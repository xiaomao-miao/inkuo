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
mod metadata;
mod scanner;
mod vector_store;

pub use chunker::*;
pub use commands::*;
pub use config::*;
pub use embedder::*;
pub use metadata::*;
pub use scanner::*;
pub use vector_store::*;
