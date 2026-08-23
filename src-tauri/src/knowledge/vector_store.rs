//! Vector store module - stores and retrieves embeddings using Qdrant Edge (embedded mode)
//!
//! Uses Qdrant Edge for zero-external-dependency, persistent vector storage.
//! Reference: https://qdrant.tech/documentation/edge/

use crate::knowledge::config::{Chunk, SearchResult};
use crate::knowledge::embedder::ModelInfo;
use qdrant_edge::{
    Condition, CountRequest, CreateIndex, Distance, EdgeConfigBuilder, EdgeShard,
    EdgeVectorParamsBuilder, FieldCondition, FieldIndexOperations, Filter, Match, MatchValue,
    NamedQuery, Payload, PayloadSchemaType, PointId, PointInsertOperations, PointOperations,
    PointStruct, QueryEnum, QueryRequest, ScoredPoint, ScoringQuery, UpdateOperation,
    ValueVariants, Vectors, WithPayloadInterface, WithVector,
};
use serde_json::json;
use sha2::Digest;
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
    vector_dimension: usize,
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
        Self::new_in_collection(workspace_path, model_name, "default").await
    }

    /// Open an isolated vector shard for one logical collection. The default
    /// collection intentionally keeps the historical storage path, so old
    /// indexes load without migration. Named collections live below a hashed
    /// subdirectory and can therefore return an exact `top_k` without
    /// over-fetching/filtering away results from other collections.
    pub async fn new_in_collection(
        workspace_path: &PathBuf,
        model_name: &str,
        collection: &str,
    ) -> Result<Self, VectorStoreError> {
        let model_info = ModelInfo::new(model_name)
            .map_err(|e| VectorStoreError::Init(format!("Unsupported embedding model: {}", e)))?;
        let vector_name = model_name
            .replace('/', "_")
            .replace('.', "_")
            .replace('-', "_");
        let vector_dimension = model_info.dimension;
        let workspace_id = Self::hash_workspace_path(workspace_path);
        #[cfg(test)]
        let workspace_storage_path = workspace_path
            .join(".inkuo-test-knowledge")
            .join(&workspace_id);
        #[cfg(not(test))]
        let workspace_storage_path = get_knowledge_dir()
            .map(|p| p.join(&workspace_id))
            .unwrap_or_else(|| {
                PathBuf::from(format!("/tmp/inkuo_knowledge_{}", &workspace_id[..8]))
            });
        let storage_path = if collection == "default" {
            // Preserve the historical path for zero-migration compatibility.
            workspace_storage_path.join(&vector_name)
        } else {
            let digest = sha2::Sha256::digest(collection.as_bytes());
            let collection_id = hex::encode(digest);
            // Named shards are siblings of (never children of) the default
            // Qdrant shard. Nesting under the default store could make its WAL
            // and directory traversal interfere with collection storage.
            workspace_storage_path
                .join("collections")
                .join(&collection_id[..16])
                .join(&vector_name)
        };

        let dir_existed = storage_path.exists();

        std::fs::create_dir_all(&storage_path).map_err(|e| {
            VectorStoreError::Init(format!("Failed to create storage directory: {}", e))
        })?;

        tracing::info!(
            "Initializing Qdrant Edge vector store at {:?} for model {} collection {} (dim={})",
            storage_path,
            model_name,
            collection,
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
                    std::fs::create_dir_all(&storage_path).map_err(|e| {
                        VectorStoreError::Init(format!(
                            "Failed to recreate storage directory: {}",
                            e
                        ))
                    })?;
                    EdgeShard::load(&storage_path, None).map_err(|fresh_err| {
                        VectorStoreError::Init(format!(
                            "Failed to load fresh vector store after cleanup: {:?}. \
                            The knowledge base directory may need manual removal.",
                            fresh_err
                        ))
                    })?
                } else {
                    tracing::warn!(
                        "EdgeShard::load failed at {:?}: {:?}. \
                        Creating new shard with explicit config (no prior edge_config.json found).",
                        storage_path,
                        load_err
                    );
                    let config = EdgeConfigBuilder::new()
                        .vector(
                            vector_name.as_str(),
                            EdgeVectorParamsBuilder::new(vector_dimension, Distance::Cosine)
                                .build(),
                        )
                        .build();
                    EdgeShard::new(&storage_path, config).map_err(|new_err| {
                        VectorStoreError::Init(format!(
                            "Failed to create new vector store: {:?}",
                            new_err
                        ))
                    })?
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
            tracing::warn!(
                "Could not create document_id index (may already exist): {:?}",
                e
            );
        }

        // Create payload index for file_path filtering
        if let Err(e) = shard.update(UpdateOperation::FieldIndexOperation(
            FieldIndexOperations::CreateIndex(CreateIndex {
                field_name: "file_path".try_into().unwrap(),
                field_schema: Some(PayloadSchemaType::Keyword.into()),
            }),
        )) {
            tracing::warn!(
                "Could not create file_path index (may already exist): {:?}",
                e
            );
        }

        let inner = VectorStoreInner {
            shard,
            vector_name,
            vector_dimension,
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
        let inner = inner_guard
            .as_ref()
            .ok_or_else(|| VectorStoreError::Storage("Shard not initialized".to_string()))?;

        if chunks.is_empty() {
            return Err(VectorStoreError::Storage(
                "Refusing to upsert an empty chunk set".to_string(),
            ));
        }
        if let Some(chunk) = chunks
            .iter()
            .find(|chunk| chunk.embedding.len() != inner.vector_dimension)
        {
            return Err(VectorStoreError::Storage(format!(
                "Chunk {} has embedding dimension {}; expected {}. Existing vectors were not changed.",
                chunk.id,
                chunk.embedding.len(),
                inner.vector_dimension
            )));
        }

        // Generation-scoped UUID for each chunk. We previously used
        // `enumerate()`'s index here, which collided across documents because
        // every document's chunk 0 ended up with `PointId::NumId(0)` and silently
        // overwrote each other inside Qdrant. Derive the ID from the chunk's
        // identifying tuple so:
        //   1. Retrying the same staged generation is idempotent.
        //   2. Chunks from different documents never collide.
        //   3. A re-index uses a fresh document generation, so new points can
        //      be persisted before the prior generation is deleted.
        // The namespace UUID is the inkuo knowledge base UUID; replace at
        // build-time if a workspace ever needs isolation.
        let namespace = uuid::Uuid::NAMESPACE_OID;

        let points: Vec<PointStruct> = chunks
            .iter()
            .map(|chunk| {
                let file_path = file_paths
                    .get(&chunk.document_id)
                    .cloned()
                    .unwrap_or_default();
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
                        "collection": chunk.collection,
                        "start_line": chunk.start_line as i64,
                        "end_line": chunk.end_line as i64,
                    }),
                )
            })
            .collect();

        // Convert to persisted format for upsert
        let points_persisted: Vec<_> = points.into_iter().map(|p| p.0).collect();

        inner
            .shard
            .update(UpdateOperation::PointOperation(
                PointOperations::UpsertPoints(PointInsertOperations::PointsList(points_persisted)),
            ))
            .map_err(|e| VectorStoreError::Update(format!("Upsert failed: {}", e)))?;

        tracing::info!("Stored {} chunks in Qdrant Edge", chunks.len());
        Ok(())
    }

    /// Validate, persist and flush a new document generation without touching
    /// any existing generation. Callers can then commit metadata and retire
    /// the old generation as a separate finalization step.
    pub async fn stage_document_chunks(
        &self,
        chunks: &[Chunk],
        file_paths: &HashMap<String, String>,
    ) -> Result<(), VectorStoreError> {
        self.upsert_chunks(chunks, file_paths).await?;
        self.flush().await
    }

    /// Convenience replacement for single-document callers. Multi-document
    /// commands use `stage_document_chunks` directly so they can roll back the
    /// whole staged batch if metadata commit fails.
    pub async fn replace_document_chunks(
        &self,
        chunks: &[Chunk],
        file_paths: &HashMap<String, String>,
        previous_document_id: Option<&str>,
    ) -> Result<(), VectorStoreError> {
        let new_document_id = chunks
            .first()
            .map(|chunk| chunk.document_id.as_str())
            .ok_or_else(|| {
                VectorStoreError::Storage(
                    "Refusing to replace a document with zero chunks".to_string(),
                )
            })?;
        if chunks
            .iter()
            .any(|chunk| chunk.document_id != new_document_id)
        {
            return Err(VectorStoreError::Storage(
                "A document replacement must contain chunks from exactly one generation"
                    .to_string(),
            ));
        }

        self.stage_document_chunks(chunks, file_paths).await?;

        if let Some(previous_document_id) =
            previous_document_id.filter(|previous| *previous != new_document_id)
        {
            if let Err(error) = self.delete_by_document_id(previous_document_id).await {
                let rollback = self.delete_by_document_id(new_document_id).await;
                return Err(VectorStoreError::Update(match rollback {
                    Ok(()) => format!(
                        "Old generation {previous_document_id} could not be retired: {error}; \
                         staged generation {new_document_id} was rolled back"
                    ),
                    Err(rollback_error) => format!(
                        "Old generation {previous_document_id} could not be retired: {error}; \
                         staged generation {new_document_id} also could not be rolled back: \
                         {rollback_error}. Both may remain searchable"
                    ),
                }));
            }
        }
        Ok(())
    }

    /// Search for similar chunks
    pub async fn search(
        &self,
        query_vector: &[f32],
        top_k: usize,
    ) -> Result<Vec<SearchResult>, VectorStoreError> {
        self.search_in_collection(query_vector, top_k, None).await
    }

    /// Search within a logical collection. Stores are isolated by collection,
    /// so this returns the exact top-k. The optional value is used only to
    /// label legacy payloads that predate the collection field.
    pub async fn search_in_collection(
        &self,
        query_vector: &[f32],
        top_k: usize,
        collection: Option<&str>,
    ) -> Result<Vec<SearchResult>, VectorStoreError> {
        let inner_guard = self.inner.read().await;
        let inner = inner_guard
            .as_ref()
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

                let point_collection = payload_collection_with_fallback(payload, collection);

                let document_id = payload
                    .0
                    .get("document_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();

                let content = payload
                    .0
                    .get("content")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();

                let file_path = payload
                    .0
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();

                let chunk_id = match &scored.id {
                    PointId::NumId(n) => n.to_string(),
                    PointId::Uuid(u) => u.to_string(),
                };

                let document_title = file_path.split('/').last().unwrap_or("").to_string();

                let start_line = payload
                    .0
                    .get("start_line")
                    .and_then(|v| v.as_i64())
                    .and_then(|n| usize::try_from(n).ok());

                let end_line = payload
                    .0
                    .get("end_line")
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
                    collection: point_collection,
                })
            })
            .collect();

        Ok(search_results)
    }

    /// Delete points by document ID
    pub async fn delete_by_document_id(&self, document_id: &str) -> Result<(), VectorStoreError> {
        let inner_guard = self.inner.read().await;
        let inner = inner_guard
            .as_ref()
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

        inner
            .shard
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
                .count(CountRequest {
                    filter: None,
                    exact: true,
                })
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

fn payload_collection_with_fallback(payload: &Payload, fallback: Option<&str>) -> String {
    payload
        .0
        .get("collection")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback.unwrap_or("default"))
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_payload_without_collection_falls_back_to_default() {
        let serde_json::Value::Object(payload) = serde_json::json!({ "content": "legacy" }) else {
            unreachable!("object literal")
        };
        let payload = Payload::from(payload);
        assert_eq!(payload_collection_with_fallback(&payload, None), "default");
        assert_eq!(
            payload_collection_with_fallback(&payload, Some("research")),
            "research"
        );
    }

    #[test]
    fn collection_payload_is_preserved_for_filtered_search() {
        let serde_json::Value::Object(payload) = serde_json::json!({ "collection": "research" })
        else {
            unreachable!("object literal")
        };
        let payload = Payload::from(payload);
        assert_eq!(
            payload_collection_with_fallback(&payload, Some("other")),
            "research"
        );
    }

    #[tokio::test]
    async fn named_collection_returns_complete_top_k_even_when_default_has_results() {
        let workspace =
            std::env::temp_dir().join(format!("inkuo-vector-collection-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace).unwrap();
        let model = "BAAI/bge-large-zh-v1.5";
        let dimension = ModelInfo::new(model).unwrap().dimension;
        let default_store = VectorStore::new_in_collection(&workspace, model, "default")
            .await
            .unwrap();
        let research_store = VectorStore::new_in_collection(&workspace, model, "research")
            .await
            .unwrap();
        assert!(!research_store
            .storage_path()
            .starts_with(default_store.storage_path()));

        let make_chunks = |collection: &str, count: usize| {
            (0..count)
                .map(|index| {
                    let mut embedding = vec![0.0; dimension];
                    embedding[0] = 1.0;
                    embedding[(index % (dimension - 1)) + 1] = index as f32 / 1000.0;
                    Chunk {
                        id: format!("{collection}-{index}"),
                        document_id: format!("{collection}-doc-{index}"),
                        content: format!("{collection} content {index}"),
                        chunk_index: 0,
                        start_line: 1,
                        end_line: 1,
                        embedding,
                        collection: collection.to_string(),
                    }
                })
                .collect::<Vec<_>>()
        };
        let default_chunks = make_chunks("default", 12);
        let research_chunks = make_chunks("research", 5);
        let paths: HashMap<String, String> = default_chunks
            .iter()
            .chain(research_chunks.iter())
            .map(|chunk| {
                (
                    chunk.document_id.clone(),
                    format!("/tmp/{}.md", chunk.document_id),
                )
            })
            .collect();
        default_store
            .upsert_chunks(&default_chunks, &paths)
            .await
            .unwrap();
        research_store
            .upsert_chunks(&research_chunks, &paths)
            .await
            .unwrap();

        let mut query = vec![0.0; dimension];
        query[0] = 1.0;
        let results = research_store
            .search_in_collection(&query, 5, Some("research"))
            .await
            .unwrap();
        assert_eq!(results.len(), 5);
        assert!(results.iter().all(|result| result.collection == "research"));

        let storage_root = default_store.storage_path().parent().unwrap().to_path_buf();
        drop(research_store);
        drop(default_store);
        std::fs::remove_dir_all(storage_root).ok();
        std::fs::remove_dir_all(workspace).ok();
    }

    #[tokio::test]
    async fn failed_staging_keeps_the_last_known_good_generation_searchable() {
        let workspace =
            std::env::temp_dir().join(format!("inkuo-vector-staging-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace).unwrap();
        let model = "BAAI/bge-large-zh-v1.5";
        let dimension = ModelInfo::new(model).unwrap().dimension;
        let store = VectorStore::new_in_collection(&workspace, model, "research")
            .await
            .unwrap();

        let old = Chunk {
            id: "old-0".into(),
            document_id: "old-generation".into(),
            content: "last known good content".into(),
            chunk_index: 0,
            start_line: 1,
            end_line: 1,
            embedding: vec![1.0; dimension],
            collection: "research".into(),
        };
        let invalid_new = Chunk {
            id: "new-0".into(),
            document_id: "new-generation".into(),
            content: "new content".into(),
            chunk_index: 0,
            start_line: 1,
            end_line: 1,
            embedding: vec![1.0; dimension - 1],
            collection: "research".into(),
        };
        let paths = HashMap::from([
            (old.document_id.clone(), "/tmp/old.md".into()),
            (invalid_new.document_id.clone(), "/tmp/new.md".into()),
        ]);
        store
            .upsert_chunks(std::slice::from_ref(&old), &paths)
            .await
            .unwrap();

        let error = store
            .replace_document_chunks(
                std::slice::from_ref(&invalid_new),
                &paths,
                Some(&old.document_id),
            )
            .await
            .expect_err("dimension validation must fail before deleting old vectors");
        assert!(error
            .to_string()
            .contains("Existing vectors were not changed"));

        let results = store
            .search_in_collection(&vec![1.0; dimension], 5, Some("research"))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].document_id, old.document_id);

        let storage_root = store
            .storage_path()
            .ancestors()
            .nth(3)
            .unwrap()
            .to_path_buf();
        drop(store);
        std::fs::remove_dir_all(storage_root).ok();
        std::fs::remove_dir_all(workspace).ok();
    }
}

/// Get the knowledge base directory
fn get_knowledge_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|p| p.join("inkuo").join("knowledge"))
}
