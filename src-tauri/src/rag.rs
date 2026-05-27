//! RAG (Retrieval-Augmented Generation) module
//! 
//! Handles:
//! - Document chunking
//! - Local vector storage (simplified implementation)
//! - Search and retrieval
//! - Context assembly with citations
//! - Persistence to disk for index durability

use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingChunk {
    pub chunk_id: String,
    pub doc_id: String,
    pub text: String,
    pub block_ids: Vec<String>,
    pub start_line: usize,
    pub end_line: usize,
    pub embedding: Vec<f32>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    pub source_doc: String,
    pub source_path: String,
    pub range: String,
    pub snippet: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub chunks: Vec<SearchChunk>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchChunk {
    pub chunk: EmbeddingChunk,
    pub score: f32,
    pub citation: Citation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceIndex {
    pub doc_id: String,
    pub path: String,
    pub chunks: Vec<EmbeddingChunk>,
    pub file_hash: String,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
struct ChunkData {
    text: String,
    block_ids: Vec<String>,
    start_line: usize,
    end_line: usize,
}

/// Persisted RAG index data (serializable to/from disk)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedRAGIndex {
    pub chunks: Vec<EmbeddingChunk>,
    pub doc_index: Vec<WorkspaceIndex>,
}

pub struct RAGIndex {
    // In a production implementation, this would use sqlite-vec or similar
    // For now, we use a simple in-memory store with basic text matching
    chunks: Arc<RwLock<HashMap<String, EmbeddingChunk>>>,
    doc_index: Arc<RwLock<HashMap<String, WorkspaceIndex>>>,
    // Persistence path for saving/loading index
    persistence_path: Option<PathBuf>,
}

impl Default for RAGIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl RAGIndex {
    pub fn new() -> Self {
        Self {
            chunks: Arc::new(RwLock::new(HashMap::new())),
            doc_index: Arc::new(RwLock::new(HashMap::new())),
            persistence_path: None,
        }
    }

    /// Create a new RAGIndex with a persistence path
    pub fn with_persistence(app_data_dir: PathBuf) -> Self {
        let persistence_path = app_data_dir.join("rag_index.json");
        let mut index = Self {
            chunks: Arc::new(RwLock::new(HashMap::new())),
            doc_index: Arc::new(RwLock::new(HashMap::new())),
            persistence_path: Some(persistence_path),
        };
        // Try to load persisted data
        index.load_from_disk();
        index
    }

    /// Set the persistence path
    pub fn set_persistence_path(&mut self, path: PathBuf) {
        self.persistence_path = Some(path);
    }

    /// Persist the index to disk
    pub fn persist_to_disk(&self) -> Result<(), String> {
        let Some(path) = &self.persistence_path else {
            tracing::warn!("No persistence path configured for RAGIndex");
            return Ok(());
        };

        let chunks = self.chunks.read();
        let doc_index = self.doc_index.read();

        let persisted = PersistedRAGIndex {
            chunks: chunks.values().cloned().collect(),
            doc_index: doc_index.values().cloned().collect(),
        };

        let json = serde_json::to_string_pretty(&persisted)
            .map_err(|e| format!("Failed to serialize RAG index: {}", e))?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }

        std::fs::write(path, json)
            .map_err(|e| format!("Failed to write RAG index: {}", e))?;

        tracing::info!("RAG index persisted to {:?}", path);
        Ok(())
    }

    /// Load the index from disk
    fn load_from_disk(&mut self) {
        let Some(path) = &self.persistence_path else {
            return;
        };

        if !path.exists() {
            tracing::info!("No persisted RAG index found at {:?}", path);
            return;
        }

        match std::fs::read_to_string(path) {
            Ok(json) => {
                match serde_json::from_str::<PersistedRAGIndex>(&json) {
                    Ok(persisted) => {
                        let mut chunks = self.chunks.write();
                        let mut doc_index = self.doc_index.write();

                        for chunk in persisted.chunks {
                            chunks.insert(chunk.chunk_id.clone(), chunk);
                        }

                        for doc in persisted.doc_index {
                            doc_index.insert(doc.doc_id.clone(), doc);
                        }

                        tracing::info!(
                            "Loaded RAG index with {} chunks and {} documents",
                            chunks.len(),
                            doc_index.len()
                        );
                    }
                    Err(e) => {
                        tracing::error!("Failed to parse persisted RAG index: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to read persisted RAG index: {}", e);
            }
        }
    }

    /// Clear the persisted index file
    pub fn clear_persisted(&self) -> Result<(), String> {
        let Some(path) = &self.persistence_path else {
            return Ok(());
        };

        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|e| format!("Failed to remove persisted index: {}", e))?;
            tracing::info!("Cleared persisted RAG index");
        }
        Ok(())
    }
    
    pub fn index_document(&self, doc_id: &str, path: &str, content: &str, blocks: &[super::document::Block]) {
        let chunks = self.chunk_content(content, blocks);
        
        // Generate simple pseudo-embeddings using word frequencies
        // In production, this would use actual embedding models
        let chunks_with_embeddings: Vec<EmbeddingChunk> = chunks.into_iter().map(|chunk| {
            EmbeddingChunk {
                chunk_id: uuid::Uuid::new_v4().to_string(),
                doc_id: doc_id.to_string(),
                text: chunk.text.clone(),
                block_ids: chunk.block_ids,
                start_line: chunk.start_line,
                end_line: chunk.end_line,
                embedding: self.generate_pseudo_embedding(&chunk.text),
                updated_at: chrono::Utc::now(),
            }
        }).collect();
        
        // Store chunks
        let mut chunks_lock = self.chunks.write();
        for chunk in &chunks_with_embeddings {
            chunks_lock.insert(chunk.chunk_id.clone(), chunk.clone());
        }
        
        // Update doc index
        let mut doc_lock = self.doc_index.write();
        doc_lock.insert(doc_id.to_string(), WorkspaceIndex {
            doc_id: doc_id.to_string(),
            path: path.to_string(),
            chunks: chunks_with_embeddings,
            file_hash: format!("{:x}", sha2::Sha256::digest(content.as_bytes())),
            updated_at: chrono::Utc::now(),
        });

        // Auto-persist after indexing
        if let Err(e) = self.persist_to_disk() {
            tracing::error!("Failed to persist RAG index: {}", e);
        }
    }
    
    fn chunk_content(&self, _content: &str, blocks: &[super::document::Block]) -> Vec<ChunkData> {
        // Simple chunking by paragraphs/sections
        let mut chunks = Vec::new();
        let mut current_text = String::new();
        let mut current_block_ids = Vec::new();
        let mut start_line = 1usize;
        
        for block in blocks {
            // Start a new chunk if we hit a heading or code block
            if matches!(block.kind, super::document::BlockKind::Heading { .. }) ||
               matches!(block.kind, super::document::BlockKind::CodeBlock { .. }) {
                if !current_text.is_empty() {
                    chunks.push(ChunkData {
                        text: current_text.clone(),
                        block_ids: current_block_ids.clone(),
                        start_line,
                        end_line: start_line + current_text.lines().count() - 1,
                    });
                    current_text.clear();
                    current_block_ids.clear();
                }
                start_line = block.range.start_line;
            }
            
            if !current_text.is_empty() {
                current_text.push('\n');
            }
            current_text.push_str(&block.text);
            current_block_ids.push(block.id.clone());
            
            // Chunk size limit (roughly 500 chars)
            if current_text.len() > 500 {
                chunks.push(ChunkData {
                    text: current_text.clone(),
                    block_ids: current_block_ids.clone(),
                    start_line,
                    end_line: block.range.end_line,
                });
                current_text.clear();
                current_block_ids.clear();
                start_line = block.range.end_line + 1;
            }
        }
        
        // Add remaining chunk
        if !current_text.is_empty() {
            chunks.push(ChunkData {
                text: current_text,
                block_ids: current_block_ids,
                start_line,
                end_line: start_line + 1,
            });
        }
        
        chunks
    }
    
    fn generate_pseudo_embedding(&self, text: &str) -> Vec<f32> {
        // Simple word frequency-based pseudo-embedding
        // In production, use actual embedding models
        let lower_text = text.to_lowercase();
        let words: Vec<&str> = lower_text.split_whitespace().collect();
        let mut embedding = vec![0.0f32; 64];
        
        for (i, word) in words.iter().take(64).enumerate() {
            let hash = self.simple_hash(word);
            embedding[i] = (hash % 100) as f32 / 100.0;
        }
        
        embedding
    }
    
    fn simple_hash(&self, s: &str) -> usize {
        s.bytes().fold(0usize, |acc, b| acc.wrapping_mul(31).wrapping_add(b as usize))
    }
    
    pub fn search(&self, query: &str, limit: usize) -> SearchResult {
        let query_embedding = self.generate_pseudo_embedding(query);
        let chunks = self.chunks.read();
        
        let mut results: Vec<SearchChunk> = chunks.values()
            .map(|chunk| {
                let score = self.cosine_similarity(&query_embedding, &chunk.embedding);
                SearchChunk {
                    chunk: chunk.clone(),
                    score,
                    citation: Citation {
                        source_doc: chunk.doc_id.clone(),
                        source_path: String::new(),
                        range: format!("{}:{}", chunk.start_line, chunk.end_line),
                        snippet: chunk.text.chars().take(200).collect(),
                        hash: format!("{:x}", sha2::Sha256::digest(chunk.text.as_bytes())),
                    },
                }
            })
            .collect();
        
        // Sort by score descending
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        results.truncate(limit);
        
        SearchResult {
            total: results.len(),
            chunks: results,
        }
    }
    
    fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        
        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }
    
    pub fn remove_document(&self, doc_id: &str) {
        let mut chunks = self.chunks.write();
        let doc_lock = self.doc_index.read();
        
        if let Some(index) = doc_lock.get(doc_id) {
            for chunk in &index.chunks {
                chunks.remove(&chunk.chunk_id);
            }
        }
        
        drop(doc_lock);
        let mut doc_lock = self.doc_index.write();
        doc_lock.remove(doc_id);

        // Persist after removal
        drop(chunks);
        if let Err(e) = self.persist_to_disk() {
            tracing::error!("Failed to persist RAG index after document removal: {}", e);
        }
    }
}
