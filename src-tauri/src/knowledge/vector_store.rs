//! Vector store module - stores and retrieves embeddings using Qdrant Edge (embedded mode)
//!
//! Uses Qdrant Edge for zero-external-dependency, persistent vector storage.
//! Reference: https://qdrant.tech/documentation/edge/

use crate::knowledge::config::{Chunk, SearchResult};
use crate::knowledge::embedder::ModelInfo;
use qdrant_edge::{
    Condition, CountRequest, CreateIndex, Distance, EdgeConfigBuilder, EdgeShard,
    EdgeVectorParamsBuilder, FieldCondition, FieldIndexOperations, Filter, Match, MatchValue,
    NamedQuery, Payload, PayloadSchemaType, PointInsertOperations, PointOperations,
    PointId, PointStruct, QueryEnum, QueryRequest, ScoringQuery,
    ScoredPoint, UpdateOperation, ValueVariants, WithPayloadInterface, WithVector, Vectors,
};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Vector store error
#[derive(Debug, thiserror::Error)]
pub enum VectorStoreError {
    #[error("Failed to initialize Qdrant: {0}")]
    Init(String),
    #[error("Failed to store vectors: {0}")]
    Storage(String),
    #[error("Failed to search: {0}")]
    Search(String),
    #[error("Failed to update: {0}")]
    Update(String),
}

/// Inner state of the vector store
struct VectorStoreInner {
    shard: EdgeShard,
    vector_name: String,
}

/// Vector store using Qdrant Edge (embedded mode)
#[derive(Clone)]
pub struct VectorStore {
    inner: Arc<RwLock<Option<VectorStoreInner>>>,
    storage_path: PathBuf,
}

impl VectorStore {
    /// Create a new vector store for a workspace
    pub async fn new(
        workspace_path: &PathBuf,
        _collection_name: &str,
        model_name: &str,
    ) -> Result<Self, VectorStoreError> {
        let model_info = ModelInfo::new(model_name)
            .map_err(|e| VectorStoreError::Init(format!("Unsupported embedding model: {}", e)))?;
        let vector_name = model_name.replace('/', "_").replace('.', "_").replace('-', "_");
        let vector_dimension = model_info.dimension;
        let workspace_id = Self::hash_workspace_path(workspace_path);
        let storage_path = get_knowledge_dir()
            .map(|p| p.join(&workspace_id).join(&vector_name))
            .unwrap_or_else(|| PathBuf::from(format!("/tmp/inkuo_knowledge_{}", &workspace_id[..8])).join(&vector_name));

        let dir_existed = storage_path.exists();

        std::fs::create_dir_all(&storage_path)
            .map_err(|e| VectorStoreError::Init(format!("Failed to create storage directory: {}", e)))?;

        tracing::info!(
            "Initializing Qdrant Edge vector store at {:?} for model {} (dim={})",
            storage_path,
            model_name,
            vector_dimension
        );

        let shard = match EdgeShard::load(&storage_path, None) {
            Ok(s) => {
                tracing::info!("Loaded existing Qdrant Edge shard");
                s
            }
            Err(load_err) => {
                if dir_existed {
                    tracing::warn!(
                        "EdgeShard::load failed at {:?}: {:?}. \
                        Directory appears to be a stale/incomplete build. Cleaning up and retrying.",
                        storage_path, load_err
                    );
                    std::fs::remove_dir_all(&storage_path).ok();
                    std::fs::create_dir_all(&storage_path)
                        .map_err(|e| VectorStoreError::Init(format!("Failed to recreate storage directory: {}", e)))?;
                    EdgeShard::load(&storage_path, None)
                        .map_err(|fresh_err| VectorStoreError::Init(format!(
                            "Failed to load fresh vector store after cleanup: {:?}. \
                            The knowledge base directory may need manual removal.",
                            fresh_err
                        )))?
                } else {
                    tracing::warn!(
                        "EdgeShard::load failed at {:?}: {:?}. \
                        Creating new shard with explicit config (no prior edge_config.json found).",
                        storage_path, load_err
                    );
                    let config = EdgeConfigBuilder::new()
                        .vector(
                            vector_name.as_str(),
                            EdgeVectorParamsBuilder::new(vector_dimension, Distance::Cosine).build(),
                        )
                        .build();
                    EdgeShard::new(&storage_path, config)
                        .map_err(|new_err| VectorStoreError::Init(format!(
                            "Failed to create new vector store: {:?}",
                            new_err
                        )))?
                }
            }
        };

        // Create payload index for document_id filtering
        if let Err(e) = shard.update(UpdateOperation::FieldIndexOperation(
            FieldIndexOperations::CreateIndex(CreateIndex {
                field_name: "document_id".try_into().unwrap(),
                field_schema: Some(PayloadSchemaType::Keyword.into()),
            }),
        )) {
            tracing::warn!("Could not create document_id index (may already exist): {:?}", e);
        }

        // Create payload index for file_path filtering
        if let Err(e) = shard.update(UpdateOperation::FieldIndexOperation(
            FieldIndexOperations::CreateIndex(CreateIndex {
                field_name: "file_path".try_into().unwrap(),
                field_schema: Some(PayloadSchemaType::Keyword.into()),
            }),
        )) {
            tracing::warn!("Could not create file_path index (may already exist): {:?}", e);
        }

        let inner = VectorStoreInner {
            shard,
            vector_name,
        };

        Ok(Self {
            inner: Arc::new(RwLock::new(Some(inner))),
            storage_path,
        })
    }

// ── Helpers ─────────────────────────────────────────────────────────────────

    fn hash_workspace_path(path: &PathBuf) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let abs_path = path.canonicalize().unwrap_or_else(|_| path.clone());
        let mut s = DefaultHasher::new();
        abs_path.to_string_lossy().hash(&mut s);
        format!("{:x}", s.finish())
    }

    /// Insert chunks into the vector store
    pub async fn upsert_chunks(
        &self,
        chunks: &[Chunk],
        file_paths: &HashMap<String, String>,
    ) -> Result<(), VectorStoreError> {
        let inner_guard = self.inner.read().await;
        let inner = inner_guard.as_ref()
            .ok_or_else(|| VectorStoreError::Storage("Shard not initialized".to_string()))?;

        // Stable, content-derived UUID for each chunk. We previously used
        // `enumerate()`'s index here, which collided across documents because
        // every document's chunk 0 ended up with `PointId::NumId(0)` and silently
        // overwrote each other inside Qdrant. Derive the ID from the chunk's
        // identifying tuple so:
        //   1. The same logical chunk always gets the same ID across re-indexes
        //      (idempotent upsert, no shard bloat on every incremental update).
        //   2. Chunks from different documents never collide.
        //   3. The ID does NOT include `content`, so editing a single word in
        //      a long chunk does not invalidate its vector; only the line range
        //      and document identity matter for ID stability.
        // The namespace UUID is the inkuo knowledge base UUID; replace at
        // build-time if a workspace ever needs isolation.
        let namespace = uuid::Uuid::NAMESPACE_OID;

        let points: Vec<PointStruct> = chunks
            .iter()
            .map(|chunk| {
                let file_path = file_paths.get(&chunk.document_id).cloned().unwrap_or_default();
                let id_seed = format!(
                    "{}|{}|{}|{}",
                    chunk.document_id, chunk.chunk_index, chunk.start_line, chunk.end_line
                );
                let point_id = PointId::Uuid(uuid::Uuid::new_v5(&namespace, id_seed.as_bytes()));

                PointStruct::new(
                    point_id,
                    Vectors::new_named([(inner.vector_name.as_str(), chunk.embedding.clone())]),
                    json!({
                        "document_id": chunk.document_id,
                        "content": chunk.content,
                        "chunk_index": chunk.chunk_index as i64,
                        "file_path": file_path,
                        "start_line": chunk.start_line as i64,
                        "end_line": chunk.end_line as i64,
                    }),
                )
            })
            .collect();

        // Convert to persisted format for upsert
        let points_persisted: Vec<_> = points.into_iter().map(|p| p.0).collect();

        inner.shard
            .update(UpdateOperation::PointOperation(
                PointOperations::UpsertPoints(PointInsertOperations::PointsList(points_persisted)),
            ))
            .map_err(|e| VectorStoreError::Update(format!("Upsert failed: {}", e)))?;

        tracing::info!("Stored {} chunks in Qdrant Edge", chunks.len());
        Ok(())
    }

    /// Search for similar chunks
    pub async fn search(&self, query_vector: &[f32], top_k: usize) -> Result<Vec<SearchResult>, VectorStoreError> {
        let inner_guard = self.inner.read().await;
        let inner = inner_guard.as_ref()
            .ok_or_else(|| VectorStoreError::Search("Shard not initialized".to_string()))?;

        let request = QueryRequest {
            prefetches: vec![],
            query: Some(ScoringQuery::Vector(QueryEnum::Nearest(NamedQuery {
                query: query_vector.to_vec().into(),
                using: Some(inner.vector_name.clone()),
            }))),
            filter: None,
            score_threshold: None,
            limit: top_k,
            offset: 0,
            params: None,
            with_vector: WithVector::Bool(false),
            with_payload: WithPayloadInterface::Bool(true),
        };

        let results: Vec<ScoredPoint> = inner
            .shard
            .query(request)
            .map_err(|e| VectorStoreError::Search(format!("Query failed: {}", e)))?;

        let search_results: Vec<SearchResult> = results
            .into_iter()
            .filter_map(|scored| {
                let payload: &Payload = scored.payload.as_ref()?;

                let document_id = payload.0.get("document_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();

                let content = payload.0.get("content")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();

                let file_path = payload.0.get("file_path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();

                let chunk_id = match &scored.id {
                    PointId::NumId(n) => n.to_string(),
                    PointId::Uuid(u) => u.to_string(),
                };

                let document_title = file_path
                    .split('/')
                    .last()
                    .unwrap_or("")
                    .to_string();

                let start_line = payload.0.get("start_line")
                    .and_then(|v| v.as_i64())
                    .and_then(|n| usize::try_from(n).ok());

                let end_line = payload.0.get("end_line")
                    .and_then(|v| v.as_i64())
                    .and_then(|n| usize::try_from(n).ok());

                Some(SearchResult {
                    chunk_id,
                    document_id,
                    content,
                    score: scored.score,
                    document_title,
                    file_path,
                    start_line,
                    end_line,
                })
            })
            .collect();

        Ok(search_results)
    }

    /// Delete points by document ID
    pub async fn delete_by_document_id(&self, document_id: &str) -> Result<(), VectorStoreError> {
        let inner_guard = self.inner.read().await;
        let inner = inner_guard.as_ref()
            .ok_or_else(|| VectorStoreError::Storage("Shard not initialized".to_string()))?;

        let filter = Filter {
            should: None,
            min_should: None,
            must: Some(vec![Condition::Field(FieldCondition::new_match(
                "document_id".try_into().unwrap(),
                Match::Value(MatchValue {
                    value: ValueVariants::String(document_id.to_string()),
                }),
            ))]),
            must_not: None,
        };

        inner.shard
            .update(UpdateOperation::PointOperation(
                PointOperations::DeletePointsByFilter(filter),
            ))
            .map_err(|e| VectorStoreError::Update(format!("Delete failed: {}", e)))?;

        Ok(())
    }

    /// Get the number of stored vectors.
    ///
    /// Returns the count along with an optional error so callers can surface
    /// real failures (disk corruption, lock conflicts) instead of being
    /// misled into believing the knowledge base is empty when `count()`
    /// actually failed.
    pub async fn try_len(&self) -> Result<usize, VectorStoreError> {
        let inner_guard = self.inner.read().await;
        match inner_guard.as_ref() {
            Some(inner) => inner
                .shard
                .count(CountRequest { filter: None, exact: true })
                .map_err(|e| VectorStoreError::Update(format!("count failed: {}", e))),
            None => Ok(0),
        }
    }

    /// Check if the store is empty.
    ///
    /// Like [`try_len`], returns an error when the underlying count could
    /// not be performed. Callers that just want a yes/no answer can use
    /// `try_len().map(|n| n == 0)`; the previous infallible `is_empty()`
    /// silently masked storage failures as "empty", which could trick
    /// users into triggering a full rebuild that overwrites intact data.
    pub async fn is_empty(&self) -> Result<bool, VectorStoreError> {
        self.try_len().await.map(|n| n == 0)
    }

    /// Get the storage path
    pub fn storage_path(&self) -> &PathBuf {
        &self.storage_path
    }

    /// Flush data to disk
    pub async fn flush(&self) -> Result<(), VectorStoreError> {
        let inner_guard = self.inner.read().await;
        if let Some(inner) = inner_guard.as_ref() {
            inner.shard.flush();
        }
        Ok(())
    }
}

/// Get the knowledge base directory
fn get_knowledge_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|p| p.join("inkuo").join("knowledge"))
}
