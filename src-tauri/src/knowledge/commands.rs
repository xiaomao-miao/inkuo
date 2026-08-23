//! Knowledge base Tauri commands

use crate::commands::{get_chunk_overlap, get_chunk_size, get_embedding_model};
use crate::knowledge::{
    normalize_collection, resolve_member_path, validate_collection_name, BuildResult, ChunkConfig,
    Chunker, DocScanner, Embedder, EmbeddingModelInfo, MetadataStore, ModelInfo, SearchResult,
    UpdateResult, VectorStore, SUPPORTED_EXTENSIONS,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};
use thiserror::Error;

/// Shared vector store cache accessible from both KB commands and agent tools.
/// This ensures both code paths use the SAME VectorStore instance, avoiding
/// WAL lock conflicts (Qdrant Edge WAL only allows single-process access).
static SHARED_STORES: std::sync::OnceLock<tokio::sync::RwLock<HashMap<String, VectorStore>>> =
    std::sync::OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, Error)]
pub enum KnowledgeCommandError {
    #[error("Workspace does not exist: {0}")]
    WorkspaceNotFound(String),
    #[error("Unsupported embedding model: {0}")]
    UnsupportedEmbeddingModel(String),
    #[error("Model files not found for '{0}'")]
    ModelNotFound(String),
    #[error("Failed to initialize embedder: {0}")]
    EmbedderInit(String),
    #[error("Failed to generate embeddings: {0}")]
    Embedding(String),
    #[error("Failed to encode query: {0}")]
    EncodeQuery(String),
    #[error("Failed to create vector store: {0}")]
    VectorStoreInit(String),
    #[error("Failed to store vectors: {0}")]
    StoreVectors(String),
    #[error("Knowledge search failed: {0}")]
    Search(String),
    #[error("Failed to create metadata store: {0}")]
    MetadataStoreInit(String),
    #[error("Failed to load metadata: {0}")]
    MetadataLoad(String),
    #[error("Failed to create metadata: {0}")]
    MetadataCreate(String),
    #[error("Failed to update metadata: {0}")]
    MetadataUpdate(String),
    #[error("Failed to delete metadata: {0}")]
    MetadataDelete(String),
    #[error("Failed to scan documents: {0}")]
    DocumentScan(String),
    #[error("Failed to get application data directory")]
    MissingDataDirectory,
    #[error("Failed to delete storage: {0}")]
    StorageDelete(String),
    #[error("Failed to get resource directory: {0}")]
    ResourceDirectory(String),
    #[error("Failed to create model directory: {0}")]
    ModelDirectoryCreate(String),
    #[error("Failed to serialize model metadata: {0}")]
    ModelMetadataSerialize(String),
    #[error("Failed to write model metadata: {0}")]
    ModelMetadataWrite(String),
    #[error("Failed to create HTTP client: {0}")]
    HttpClient(String),
    #[error("Failed to send request: {0}")]
    HttpRequest(String),
    #[error("HTTP error: {0}")]
    HttpStatus(String),
    #[error("Failed to read response: {0}")]
    HttpResponseRead(String),
    #[error("Failed to write downloaded file: {0}")]
    DownloadWrite(String),
    #[error("Knowledge base not initialized")]
    NotInitialized,
    #[error("File not found: {0}")]
    FileNotFound(String),
    #[error("Invalid knowledge collection: {0}")]
    InvalidCollection(String),
}

fn validated_collection(collection: Option<&str>) -> Result<String, KnowledgeCommandError> {
    validate_collection_name(collection.unwrap_or("default"))
        .map_err(KnowledgeCommandError::InvalidCollection)
}

fn shared_stores() -> &'static tokio::sync::RwLock<HashMap<String, VectorStore>> {
    SHARED_STORES.get_or_init(|| {
        tracing::info!("Initializing shared vector store cache");
        tokio::sync::RwLock::new(HashMap::new())
    })
}

fn emit_event(app: &AppHandle, event: &str, payload: serde_json::Value) {
    if let Err(error) = app.emit(event, payload) {
        tracing::warn!("Failed to emit {} event: {}", event, error);
    }
}

fn emit_build_progress(
    app: &AppHandle,
    session_id: &str,
    phase: &str,
    current: usize,
    total: usize,
    message: impl Into<String>,
) {
    emit_event(
        app,
        "kb://build-progress",
        serde_json::json!({
            "session_id": session_id,
            "phase": phase,
            "current": current,
            "total": total,
            "message": message.into(),
        }),
    );
}

fn chunk_counts(chunks: &[crate::knowledge::Chunk]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for chunk in chunks {
        *counts.entry(chunk.document_id.clone()).or_insert(0) += 1;
    }
    counts
}

fn additive_workspace_members(
    existing: &[String],
    scanned_documents: &[crate::knowledge::Document],
) -> Vec<String> {
    let mut members = existing.to_vec();
    for document in scanned_documents {
        if !members.contains(&document.path) {
            members.push(document.path.clone());
        }
    }
    members
}

#[derive(Debug)]
struct DocumentReplacement {
    path: String,
    previous_document_id: Option<String>,
    chunks: Vec<crate::knowledge::Chunk>,
}

#[derive(Debug)]
struct StagedReplacement {
    path: String,
    new_document_id: String,
    previous_document_id: Option<String>,
}

/// Build a complete replacement plan before any vector deletion. Keeping the
/// validation in a pure helper makes it impossible for a zero-chunk document
/// to delete its last known-good index and gives the ordering a focused unit
/// test independent of the embedding backend.
fn plan_document_replacements(
    documents: &[crate::knowledge::Document],
    chunks: &[crate::knowledge::Chunk],
    indexed: &HashMap<String, (String, String)>,
) -> Result<Vec<DocumentReplacement>, KnowledgeCommandError> {
    documents
        .iter()
        .map(|document| {
            let document_chunks: Vec<_> = chunks
                .iter()
                .filter(|chunk| chunk.document_id == document.id)
                .cloned()
                .collect();
            if document_chunks.is_empty() {
                return Err(KnowledgeCommandError::DocumentScan(format!(
                    "{} 未生成可索引分块；已保留旧索引",
                    document.path
                )));
            }
            Ok(DocumentReplacement {
                path: document.path.clone(),
                previous_document_id: indexed
                    .get(&document.path)
                    .map(|(_, document_id)| document_id.clone())
                    .filter(|document_id| !document_id.is_empty()),
                chunks: document_chunks,
            })
        })
        .collect()
}

async fn rollback_staged_generations(
    vector_store: &VectorStore,
    staged: &[StagedReplacement],
) -> Vec<String> {
    let mut failures = Vec::new();
    for generation in staged.iter().rev() {
        if let Err(error) = vector_store
            .delete_by_document_id(&generation.new_document_id)
            .await
        {
            failures.push(format!(
                "{} ({}): {}",
                generation.path, generation.new_document_id, error
            ));
        }
    }
    failures
}

/// Stage a complete replacement batch without retiring any old generation.
/// If any stage fails, every generation written by this batch is rolled back;
/// all previous generations therefore remain searchable.
async fn stage_document_replacements(
    vector_store: &VectorStore,
    replacements: &[DocumentReplacement],
    file_paths: &HashMap<String, String>,
) -> Result<Vec<StagedReplacement>, KnowledgeCommandError> {
    let mut staged = Vec::with_capacity(replacements.len());
    for replacement in replacements {
        let new_document_id = replacement
            .chunks
            .first()
            .map(|chunk| chunk.document_id.clone())
            .ok_or_else(|| {
                KnowledgeCommandError::DocumentScan(format!(
                    "{} 未生成可索引分块；已保留旧索引",
                    replacement.path
                ))
            })?;
        let current = StagedReplacement {
            path: replacement.path.clone(),
            new_document_id,
            previous_document_id: replacement.previous_document_id.clone(),
        };
        if let Err(error) = vector_store
            .stage_document_chunks(&replacement.chunks, file_paths)
            .await
        {
            // A backend error can be ambiguous about whether an upsert became
            // durable. Include the current id in best-effort rollback too.
            staged.push(current);
            let rollback_failures = rollback_staged_generations(vector_store, &staged).await;
            let rollback_suffix = if rollback_failures.is_empty() {
                "；本批次 staged generation 已回滚，旧索引保持不变".to_string()
            } else {
                format!(
                    "；旧索引仍保留，但以下 staged generation 回滚失败：{}",
                    rollback_failures.join(" | ")
                )
            };
            return Err(KnowledgeCommandError::StoreVectors(format!(
                "暂存 {} 失败：{}{}",
                replacement.path, error, rollback_suffix
            )));
        }
        staged.push(current);
    }
    Ok(staged)
}

fn pending_retirements(
    collection: &str,
    staged: &[StagedReplacement],
) -> Vec<crate::knowledge::PendingRetirement> {
    staged
        .iter()
        .filter_map(|generation| {
            generation
                .previous_document_id
                .as_deref()
                .filter(|previous| *previous != generation.new_document_id.as_str())
                .map(|previous| {
                    crate::knowledge::PendingRetirement::new(collection, &generation.path, previous)
                })
        })
        .collect()
}

/// Drain the durable post-commit cleanup queue. Queue entries are removed
/// from metadata only after every corresponding vector deletion succeeds, so
/// a crash or shard error is retried by the next synchronization.
async fn drain_pending_retirements(
    vector_store: &VectorStore,
    metadata_store: &mut MetadataStore,
    collection: &str,
) -> Result<(), KnowledgeCommandError> {
    let pending = metadata_store.pending_retirements_for_collection(collection);
    if pending.is_empty() {
        return Ok(());
    }
    for retirement in &pending {
        vector_store
            .delete_by_document_id(&retirement.document_id)
            .await
            .map_err(|error| {
                KnowledgeCommandError::StoreVectors(format!(
                    "新索引已生效，但旧 generation {}（{}）清理失败：{}；清理任务已持久化，将在下次同步重试",
                    retirement.document_id, retirement.path, error
                ))
            })?;
    }
    metadata_store
        .clear_pending_retirements(
            collection,
            &pending
                .iter()
                .map(|retirement| retirement.document_id.clone())
                .collect::<Vec<_>>(),
        )
        .map_err(|error| KnowledgeCommandError::MetadataUpdate(error.to_string()))?;
    Ok(())
}

async fn drain_workspace_pending_retirements(
    metadata_store: &mut MetadataStore,
    workspace_path: &str,
    model_name: &str,
) -> Result<(), KnowledgeCommandError> {
    let mut collections: Vec<String> = metadata_store
        .get_metadata()
        .map(|metadata| {
            metadata
                .pending_retirements
                .iter()
                .map(|pending| pending.collection.clone())
                .collect()
        })
        .unwrap_or_default();
    collections.sort();
    collections.dedup();
    for collection in collections {
        let vector_store =
            get_or_create_vector_store(workspace_path, model_name, &collection).await?;
        drain_pending_retirements(&vector_store, metadata_store, &collection).await?;
    }
    Ok(())
}

pub(crate) fn emit_model_download_progress(
    app: &AppHandle,
    model_name: &str,
    current: usize,
    total: usize,
    filename: &str,
    status: &str,
    size: Option<usize>,
) {
    let mut payload = serde_json::json!({
        "model": model_name,
        "current": current,
        "total": total,
        "filename": filename,
        "status": status,
    });

    if let Some(size) = size {
        payload["size"] = serde_json::json!(size);
    }

    emit_event(app, "model-download-progress", payload);
}

// ── Vector store registry ───────────────────────────────────────────────────────

/// Initialize the shared cache. Called once during app startup.
pub fn register_shared_stores() {
    let _ = shared_stores();
    tracing::info!("Shared vector store cache initialized");
}

/// Get a shared vector store for a workspace.
/// Used by agent tools; KB commands use get_or_create_vector_store.
pub async fn get_vector_store_for_search(
    workspace_path: &str,
    model_name: &str,
    collection: Option<&str>,
) -> Result<VectorStore, KnowledgeCommandError> {
    get_or_create_vector_store(
        workspace_path,
        model_name,
        &normalize_collection(collection.unwrap_or("default")),
    )
    .await
}

/// Generate workspace ID from path
fn get_workspace_id(workspace_path: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let path = PathBuf::from(workspace_path);
    let abs_path = path.canonicalize().unwrap_or(path);
    let mut s = DefaultHasher::new();
    abs_path.to_string_lossy().hash(&mut s);
    format!("{:x}", s.finish())
}

/// Get or create a vector store for a workspace.
async fn get_or_create_vector_store(
    workspace_path: &str,
    model_name: &str,
    collection: &str,
) -> Result<VectorStore, KnowledgeCommandError> {
    let workspace_id = get_workspace_id(workspace_path);
    let collection = normalize_collection(collection);
    let store_key = format!("{}::{}::{}", workspace_id, model_name, collection);
    {
        let stores = shared_stores().read().await;
        if let Some(store) = stores.get(&store_key) {
            tracing::debug!("Vector store cache hit for {}", store_key);
            return Ok(store.clone());
        }
    }

    tracing::info!("Creating new vector store for {}", store_key);
    let store =
        VectorStore::new_in_collection(&PathBuf::from(workspace_path), model_name, &collection)
            .await
            .map_err(|e| KnowledgeCommandError::VectorStoreInit(e.to_string()))?;

    let mut stores = shared_stores().write().await;
    stores.insert(store_key, store.clone());

    Ok(store)
}

/// Build knowledge base for a workspace
#[tauri::command]
pub async fn knowledge_build(
    app: AppHandle,
    workspace_path: String,
    session_id: String,
    collection: Option<String>,
) -> Result<BuildResult, KnowledgeCommandError> {
    tracing::info!(
        "[KB_BUILD] START - workspace={}, session={}",
        workspace_path,
        session_id
    );

    let workspace = PathBuf::from(&workspace_path);
    tracing::info!("[KB_BUILD] Workspace path: {:?}", workspace);

    if !workspace.exists() {
        tracing::error!("[KB_BUILD] Workspace does not exist: {:?}", workspace);
        return Err(KnowledgeCommandError::WorkspaceNotFound(
            workspace.display().to_string(),
        ));
    }

    let workspace_id = get_workspace_id(&workspace_path);
    tracing::info!("[KB_BUILD] Workspace ID: {}", workspace_id);

    tracing::info!("[KB_BUILD] [PHASE 0] Emitting scanning progress");
    emit_build_progress(&app, &session_id, "scanning", 0, 4, "初始化中...");

    let scanner = DocScanner::default();
    let collection = validated_collection(collection.as_deref())?;
    let chunk_size = get_chunk_size();
    tracing::info!("[KB_BUILD] Chunk size from settings: {}", chunk_size);

    let chunker = Chunker::new(ChunkConfig {
        target_size: chunk_size,
        overlap: get_chunk_overlap(),
        min_size: 50,
        preserve_headers: true,
    });

    let model_name = get_embedding_model();
    ModelInfo::new(&model_name)
        .map_err(|e| KnowledgeCommandError::UnsupportedEmbeddingModel(e.to_string()))?;
    tracing::info!("[KB_BUILD] Model name from settings: {}", model_name);

    let model_path = resolve_model_dir(&app, &model_name)
        .ok_or_else(|| KnowledgeCommandError::ModelNotFound(model_name.clone()))?;
    tracing::info!("[KB_BUILD] Model path: {:?}", model_path);

    let embedder = Embedder::new(&model_name, &model_path)
        .map_err(|e| KnowledgeCommandError::EmbedderInit(e.to_string()))?;
    tracing::info!("[KB_BUILD] Embedder created successfully");

    tracing::info!("[KB_BUILD] [PHASE 1] Starting document scan");
    emit_build_progress(&app, &session_id, "scanning", 1, 4, "扫描文档中...");

    let scan_report = scanner
        .scan_workspace(&workspace, &collection)
        .map_err(|e| KnowledgeCommandError::DocumentScan(e.to_string()))?;
    let documents = scan_report.documents;
    tracing::info!(
        "[KB_BUILD] Scan complete, found {} documents",
        documents.len()
    );

    let mut file_paths: HashMap<String, String> = HashMap::new();
    for doc in &documents {
        let abs_path = workspace.join(&doc.path);
        file_paths.insert(doc.id.clone(), abs_path.to_string_lossy().to_string());
    }

    tracing::info!(
        "[KB_BUILD] [PHASE 2] Starting chunking, {} documents",
        documents.len()
    );
    emit_build_progress(
        &app,
        &session_id,
        "chunking",
        1,
        4,
        format!("分块处理中... ({} 文档)", documents.len()),
    );

    let mut chunks = chunker.chunk_documents(&documents);
    tracing::info!(
        "[KB_BUILD] Chunking complete, created {} chunks",
        chunks.len()
    );

    tracing::info!(
        "[KB_BUILD] [PHASE 3] Starting embedding, {} chunks",
        chunks.len()
    );
    emit_build_progress(
        &app,
        &session_id,
        "embedding",
        2,
        4,
        format!("生成向量中... ({} 块)", chunks.len()),
    );

    let batch_size = 64usize;
    if !chunks.is_empty() {
        embedder
            .encode_chunks_batched_async(&mut chunks, batch_size)
            .await
            .map_err(|e| KnowledgeCommandError::Embedding(e.to_string()))?;
    }
    tracing::info!("[KB_BUILD] Embedding complete");

    tracing::info!("[KB_BUILD] [PHASE 4] Starting vector storage");
    emit_build_progress(&app, &session_id, "storing", 3, 4, "存储向量中...");

    let vector_store =
        get_or_create_vector_store(&workspace_path, &model_name, &collection).await?;
    let mut metadata_store = MetadataStore::new(&workspace)
        .map_err(|e| KnowledgeCommandError::MetadataStoreInit(e.to_string()))?;

    let collection_name = format!("kb_{}", &workspace_id[..12]);
    if metadata_store.exists() {
        metadata_store
            .load()
            .map_err(|e| KnowledgeCommandError::MetadataLoad(e.to_string()))?;
    } else {
        metadata_store
            .create(&workspace, &collection_name)
            .map_err(|e| KnowledgeCommandError::MetadataCreate(e.to_string()))?;
    }
    drain_workspace_pending_retirements(&mut metadata_store, &workspace_path, &model_name).await?;

    // Build the full replacement plan before changing Qdrant. Each scanned
    // document has a fresh generation id, so its new vectors can be staged
    // and flushed before the previous generation is retired.
    let previous_index: HashMap<String, (String, String)> = metadata_store
        .get_indexed_files_map_for_collection(&collection)
        .into_iter()
        .map(|(path, file)| (path, (file.hash.clone(), file.document_id.clone())))
        .collect();
    let replacement_plan = plan_document_replacements(&documents, &chunks, &previous_index)?;
    let staged = stage_document_replacements(&vector_store, &replacement_plan, &file_paths).await?;

    // "Import workspace" is additive. It must never silently remove files
    // the user explicitly imported earlier (especially absolute external
    // references that a workspace walk cannot rediscover).
    let previous_members = metadata_store.members_for_collection(&collection);
    tracing::info!("[KB_BUILD] staged document replacements complete");

    // Build is a full rebuild from scratch: every document is "new" and
    // nothing is removed. `chunk_count_by_doc` is derived from the chunks we
    // just produced so the per-document chunk_count in metadata.json is
    // accurate.
    let mut chunk_count_by_doc: HashMap<String, usize> = HashMap::new();
    for c in &chunks {
        *chunk_count_by_doc.entry(c.document_id.clone()).or_insert(0) += 1;
    }
    let retirements = pending_retirements(&collection, &staged);
    if let Err(error) = metadata_store.update_with_retirements(
        &documents,
        &chunk_count_by_doc,
        chunks.len(),
        &[],
        &retirements,
    ) {
        let rollback_failures = rollback_staged_generations(&vector_store, &staged).await;
        return Err(KnowledgeCommandError::MetadataUpdate(format!(
            "{}；staged generation 回滚{}",
            error,
            if rollback_failures.is_empty() {
                "完成".to_string()
            } else {
                format!("失败：{}", rollback_failures.join(" | "))
            }
        )));
    }
    drain_pending_retirements(&vector_store, &mut metadata_store, &collection).await?;

    let rebuilt_members = additive_workspace_members(&previous_members, &documents);
    metadata_store
        .set_collection_members(&collection, rebuilt_members)
        .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;
    metadata_store
        .clear_failures_for_paths(
            &collection,
            &documents
                .iter()
                .map(|document| document.path.clone())
                .collect::<Vec<_>>(),
        )
        .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;
    metadata_store
        .record_failures(&collection, &scan_report.failures)
        .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;

    emit_build_progress(
        &app,
        &session_id,
        "done",
        4,
        4,
        format!(
            "构建完成！{} 文档，{} 块，{} 个文件跳过",
            documents.len(),
            chunks.len(),
            scan_report.failures.len()
        ),
    );

    Ok(BuildResult {
        total_documents: documents.len(),
        total_chunks: chunks.len(),
        workspace_id,
    })
}

async fn search_knowledge_base_inner(
    app: &AppHandle,
    workspace_path: &str,
    query: &str,
    top_k: usize,
    for_search: bool,
    collection: Option<&str>,
) -> Result<Vec<SearchResult>, KnowledgeCommandError> {
    let model_name = get_embedding_model();
    let model_info = ModelInfo::new(&model_name)
        .map_err(|e| KnowledgeCommandError::UnsupportedEmbeddingModel(e.to_string()))?;

    let model_path = resolve_model_dir(app, &model_name)
        .ok_or_else(|| KnowledgeCommandError::ModelNotFound(model_name.clone()))?;

    let embedder = Embedder::new(&model_name, &model_path)
        .map_err(|e| KnowledgeCommandError::EmbedderInit(e.to_string()))?;

    let vector_store = if for_search {
        get_vector_store_for_search(workspace_path, &model_name, collection).await?
    } else {
        get_or_create_vector_store(
            workspace_path,
            &model_name,
            &normalize_collection(collection.unwrap_or("default")),
        )
        .await?
    };

    tracing::debug!(
        "knowledge search: model={} (dim={})",
        model_name,
        model_info.dimension
    );

    let query_vector = embedder
        .encode_single(query)
        .map_err(|e| KnowledgeCommandError::EncodeQuery(e.to_string()))?;

    let normalized_collection = collection.map(normalize_collection);
    vector_store
        .search_in_collection(&query_vector, top_k, normalized_collection.as_deref())
        .await
        .map_err(|e| KnowledgeCommandError::Search(e.to_string()))
}

fn search_collection_names(workspace_path: &str) -> Result<Vec<String>, KnowledgeCommandError> {
    let workspace = PathBuf::from(workspace_path);
    let mut metadata_store = MetadataStore::new(&workspace)
        .map_err(|e| KnowledgeCommandError::MetadataStoreInit(e.to_string()))?;
    if !metadata_store.exists() {
        return Ok(vec!["default".to_string()]);
    }
    let metadata = metadata_store
        .load()
        .map_err(|e| KnowledgeCommandError::MetadataLoad(e.to_string()))?;
    let mut collections: Vec<String> = metadata
        .collections
        .keys()
        .chain(metadata.indexed_files.iter().map(|file| &file.collection))
        .map(|collection| normalize_collection(collection))
        .collect();
    if collections.is_empty() {
        collections.push("default".to_string());
    }
    collections.sort();
    collections.dedup();
    Ok(collections)
}

fn merge_ranked_results(
    result_sets: impl IntoIterator<Item = Vec<SearchResult>>,
    top_k: usize,
) -> Vec<SearchResult> {
    let mut merged: Vec<SearchResult> = result_sets.into_iter().flatten().collect();
    merged.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.collection.cmp(&right.collection))
            .then_with(|| left.file_path.cmp(&right.file_path))
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    merged.truncate(top_k);
    merged
}

/// Search every indexed collection with one query embedding and return the
/// global top-k. Each isolated shard contributes at most top-k candidates,
/// which is sufficient to compute the exact top-k of their union.
pub async fn search_knowledge_base_across_collections(
    app: &AppHandle,
    workspace_path: &str,
    query: &str,
    top_k: usize,
) -> Result<Vec<SearchResult>, KnowledgeCommandError> {
    let model_name = get_embedding_model();
    let model_info = ModelInfo::new(&model_name)
        .map_err(|e| KnowledgeCommandError::UnsupportedEmbeddingModel(e.to_string()))?;
    let model_path = resolve_model_dir(app, &model_name)
        .ok_or_else(|| KnowledgeCommandError::ModelNotFound(model_name.clone()))?;
    let embedder = Embedder::new(&model_name, &model_path)
        .map_err(|e| KnowledgeCommandError::EmbedderInit(e.to_string()))?;
    let query_vector = embedder
        .encode_single(query)
        .map_err(|e| KnowledgeCommandError::EncodeQuery(e.to_string()))?;
    let collections = search_collection_names(workspace_path)?;

    tracing::debug!(
        "knowledge search across {} collections: model={} (dim={})",
        collections.len(),
        model_name,
        model_info.dimension
    );
    let mut result_sets = Vec::with_capacity(collections.len());
    for collection in collections {
        let vector_store =
            get_vector_store_for_search(workspace_path, &model_name, Some(&collection)).await?;
        let results = vector_store
            .search_in_collection(&query_vector, top_k, Some(&collection))
            .await
            .map_err(|e| {
                KnowledgeCommandError::Search(format!("collection '{}': {}", collection, e))
            })?;
        result_sets.push(results);
    }
    Ok(merge_ranked_results(result_sets, top_k))
}

// ── Knowledge commands (Tauri handlers) ─────────────────────────────────────────

#[tauri::command]
pub async fn knowledge_search(
    app: AppHandle,
    workspace_path: String,
    query: String,
    top_k: usize,
    collection: Option<String>,
) -> Result<Vec<SearchResult>, KnowledgeCommandError> {
    tracing::info!("Searching knowledge base: {}", query);
    let collection = collection
        .as_deref()
        .map(|collection| validated_collection(Some(collection)))
        .transpose()?;

    search_knowledge_base_inner(
        &app,
        &workspace_path,
        &query,
        top_k,
        false,
        collection.as_deref(),
    )
    .await
}

/// Public search function for use by both Tauri commands and agent tools.
/// Does NOT go through the Tauri command layer (avoids double-IPC in agent context).
pub async fn search_knowledge_base(
    app: &AppHandle,
    workspace_path: &str,
    query: &str,
    top_k: usize,
) -> Result<Vec<SearchResult>, KnowledgeCommandError> {
    search_knowledge_base_across_collections(app, workspace_path, query, top_k).await
}

pub async fn search_knowledge_base_in_collection(
    app: &AppHandle,
    workspace_path: &str,
    query: &str,
    top_k: usize,
    collection: Option<&str>,
) -> Result<Vec<SearchResult>, KnowledgeCommandError> {
    match collection {
        Some(collection) => {
            let collection = validated_collection(Some(collection))?;
            search_knowledge_base_inner(app, workspace_path, query, top_k, true, Some(&collection))
                .await
        }
        None => search_knowledge_base_across_collections(app, workspace_path, query, top_k).await,
    }
}

#[tauri::command]
pub fn knowledge_status(
    workspace_path: String,
) -> Result<Option<serde_json::Value>, KnowledgeCommandError> {
    let workspace = PathBuf::from(&workspace_path);
    let mut metadata_store = MetadataStore::new(&workspace)
        .map_err(|e| KnowledgeCommandError::MetadataStoreInit(e.to_string()))?;

    if !metadata_store.exists() {
        return Ok(None);
    }

    let metadata = metadata_store
        .load()
        .map_err(|e| KnowledgeCommandError::MetadataLoad(e.to_string()))?;

    let mut collection_names: Vec<String> = metadata.collections.keys().cloned().collect();
    collection_names.sort();

    let mut documents = Vec::new();
    for collection in &collection_names {
        let members = metadata
            .collections
            .get(collection)
            .cloned()
            .unwrap_or_default();
        for path in members {
            let indexed = metadata
                .indexed_files
                .iter()
                .find(|file| file.collection == *collection && file.path == path);
            let failure = metadata
                .failures
                .iter()
                .find(|failure| failure.collection == *collection && failure.path == path);
            documents.push(serde_json::json!({
                "path": path,
                "collection": collection,
                // A parse/read failure means retrieval is serving the last
                // known-good version. Surface that stale/error state instead
                // of hiding it behind the existing indexed metadata row.
                "status": if failure.is_some() { "error" } else if indexed.is_some() { "indexed" } else { "pending" },
                "chunk_count": indexed.map(|file| file.chunk_count).unwrap_or(0),
                "source_type": indexed.map(|file| file.source_type.as_str()).unwrap_or(""),
                "size_bytes": indexed.map(|file| file.size_bytes).unwrap_or(0),
                "indexed_at": indexed.map(|file| file.indexed_at),
                "error": failure.map(|failure| failure.error.as_str()),
            }));
        }
    }
    // Failed batch items are intentionally not added to members, but keeping
    // them in the status response gives the UI a precise error row and retry
    // affordance instead of a generic toast that immediately disappears.
    for failure in &metadata.failures {
        let already_listed = documents.iter().any(|document| {
            document.get("path").and_then(|value| value.as_str()) == Some(failure.path.as_str())
                && document.get("collection").and_then(|value| value.as_str())
                    == Some(failure.collection.as_str())
        });
        if !already_listed {
            documents.push(serde_json::json!({
                "path": failure.path,
                "collection": failure.collection,
                "status": "error",
                "chunk_count": 0,
                "source_type": "",
                "size_bytes": 0,
                "indexed_at": serde_json::Value::Null,
                "error": failure.error,
            }));
        }
    }

    Ok(Some(serde_json::json!({
        "workspace_id": metadata.workspace_id,
        "workspace_path": metadata.workspace_path,
        "document_count": metadata.document_count,
        "chunk_count": metadata.chunk_count,
        "created_at": metadata.created_at,
        "last_updated": metadata.last_updated,
        "members": metadata.members,
        "collections": metadata.collections,
        "collection_names": collection_names,
        "documents": documents,
        "supported_extensions": SUPPORTED_EXTENSIONS,
    })))
}

#[tauri::command]
pub async fn knowledge_update(
    app: AppHandle,
    workspace_path: String,
    session_id: String,
    collection: Option<String>,
) -> Result<UpdateResult, KnowledgeCommandError> {
    tracing::info!("Updating knowledge base for workspace: {}", workspace_path);

    let workspace = PathBuf::from(&workspace_path);
    if !workspace.is_dir() {
        return Err(KnowledgeCommandError::WorkspaceNotFound(workspace_path));
    }
    let scanner = DocScanner::default();
    let chunker = Chunker::new(ChunkConfig {
        target_size: get_chunk_size(),
        overlap: get_chunk_overlap(),
        min_size: 50,
        preserve_headers: true,
    });

    let model_name = get_embedding_model();
    let model_info = ModelInfo::new(&model_name)
        .map_err(|e| KnowledgeCommandError::UnsupportedEmbeddingModel(e.to_string()))?;

    let model_path = resolve_model_dir(&app, &model_name)
        .ok_or_else(|| KnowledgeCommandError::ModelNotFound(model_name.clone()))?;

    let embedder = Embedder::new(&model_name, &model_path)
        .map_err(|e| KnowledgeCommandError::EmbedderInit(e.to_string()))?;

    tracing::info!(
        "[KB_UPDATE] Using model {} (dim={})",
        model_name,
        model_info.dimension
    );

    let mut metadata_store = MetadataStore::new(&workspace)
        .map_err(|e| KnowledgeCommandError::MetadataStoreInit(e.to_string()))?;
    if !metadata_store.exists() {
        return Err(KnowledgeCommandError::NotInitialized);
    }
    metadata_store
        .load()
        .map_err(|e| KnowledgeCommandError::MetadataLoad(e.to_string()))?;
    drain_workspace_pending_retirements(&mut metadata_store, &workspace_path, &model_name).await?;

    let mut collections = if let Some(collection) = collection {
        vec![validated_collection(Some(&collection))?]
    } else {
        metadata_store
            .get_metadata()
            .map(|metadata| metadata.collections.keys().cloned().collect())
            .unwrap_or_else(|| vec!["default".to_string()])
    };
    collections.sort();
    collections.dedup();

    let mut result = UpdateResult {
        added: 0,
        removed: 0,
        updated: 0,
        unchanged: 0,
        failed: 0,
        failures: Vec::new(),
    };

    for (collection_index, collection) in collections.iter().enumerate() {
        emit_build_progress(
            &app,
            &session_id,
            "scanning",
            collection_index,
            collections.len().max(1),
            format!("同步集合：{}", collection),
        );
        let members = metadata_store.members_for_collection(collection);
        let report = scanner.scan_paths(&workspace, &members, collection);
        let successful_paths: Vec<String> = report
            .documents
            .iter()
            .map(|document| document.path.clone())
            .collect();
        let indexed: HashMap<String, (String, String)> = metadata_store
            .get_indexed_files_map_for_collection(collection)
            .into_iter()
            .map(|(path, file)| (path, (file.hash.clone(), file.document_id.clone())))
            .collect();

        let mut changed = Vec::new();
        for document in report.documents {
            match indexed.get(&document.path) {
                Some((hash, _)) if hash == &document.file_hash => result.unchanged += 1,
                Some(_) => {
                    result.updated += 1;
                    changed.push(document);
                }
                None => {
                    result.added += 1;
                    changed.push(document);
                }
            }
        }

        // Missing files are removed from retrieval, but remain members with an
        // error status so users can relink or remove them explicitly. Existing
        // files that merely fail parsing keep their last good vectors.
        let member_set: HashSet<&str> = members.iter().map(String::as_str).collect();
        let mut removed_paths: Vec<String> = indexed
            .keys()
            .filter(|path| !member_set.contains(path.as_str()))
            .cloned()
            .collect();
        for member in &members {
            let missing = resolve_member_path(&workspace, member)
                .map(|path| !path.is_file())
                .unwrap_or(true);
            if missing && indexed.contains_key(member) && !removed_paths.contains(member) {
                removed_paths.push(member.clone());
            }
        }

        // Complete every fallible CPU/model step before touching the last
        // known-good vectors. A model or chunking failure must leave the
        // existing searchable index intact.
        let mut chunks = chunker.chunk_documents(&changed);
        if !chunks.is_empty() {
            embedder
                .encode_chunks_batched_async(&mut chunks, 64)
                .await
                .map_err(|e| KnowledgeCommandError::Embedding(e.to_string()))?;
        }
        let replacement_plan = plan_document_replacements(&changed, &chunks, &indexed)?;

        let vector_store = if changed.is_empty() && removed_paths.is_empty() {
            None
        } else {
            Some(get_or_create_vector_store(&workspace_path, &model_name, collection).await?)
        };
        let file_paths: HashMap<String, String> = changed
            .iter()
            .filter_map(|document| {
                resolve_member_path(&workspace, &document.path)
                    .ok()
                    .map(|path| (document.id.clone(), path.to_string_lossy().to_string()))
            })
            .collect();

        // Replace one document at a time. New points are flushed before the
        // previous generation is retired, so a failed store write cannot
        // erase the last-known-good searchable index.
        let staged = if let Some(vector_store) = vector_store.as_ref() {
            stage_document_replacements(vector_store, &replacement_plan, &file_paths).await?
        } else {
            Vec::new()
        };

        let chunk_count_by_doc = chunk_counts(&chunks);
        let retirements = pending_retirements(collection, &staged);
        if let Err(error) = metadata_store.update_with_retirements(
            &changed,
            &chunk_count_by_doc,
            chunks.len(),
            &[],
            &retirements,
        ) {
            let rollback_failures = if let Some(vector_store) = vector_store.as_ref() {
                rollback_staged_generations(vector_store, &staged).await
            } else {
                Vec::new()
            };
            return Err(KnowledgeCommandError::MetadataUpdate(format!(
                "{}；staged generation 回滚{}",
                error,
                if rollback_failures.is_empty() {
                    "完成".to_string()
                } else {
                    format!("失败：{}", rollback_failures.join(" | "))
                }
            )));
        }
        if let Some(vector_store) = vector_store.as_ref() {
            drain_pending_retirements(vector_store, &mut metadata_store, collection).await?;
        }

        // Deletion is committed per path: vectors first, metadata second. A
        // shard error can no longer produce orphaned searchable points.
        for removed_path in &removed_paths {
            let one_path = vec![removed_path.clone()];
            let removed_ids = metadata_store.indexed_document_ids_for_paths(collection, &one_path);
            if let Some(vector_store) = vector_store.as_ref() {
                for document_id in removed_ids {
                    vector_store
                        .delete_by_document_id(&document_id)
                        .await
                        .map_err(|e| KnowledgeCommandError::StoreVectors(e.to_string()))?;
                }
            }
            metadata_store
                .remove_indexed_files(collection, &one_path)
                .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;
            result.removed += 1;
        }

        metadata_store
            .clear_failures_for_paths(collection, &successful_paths)
            .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;
        metadata_store
            .record_failures(collection, &report.failures)
            .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;
        result.failed += report.failures.len();
        result.failures.extend(report.failures);
    }

    emit_build_progress(
        &app,
        &session_id,
        "done",
        collections.len(),
        collections.len().max(1),
        "知识库同步完成",
    );
    Ok(result)
}

// ── Model management ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn knowledge_clear(workspace_path: String) -> Result<(), KnowledgeCommandError> {
    let workspace = PathBuf::from(&workspace_path);
    let workspace_id = get_workspace_id(&workspace_path);

    let metadata_store = MetadataStore::new(&workspace)
        .map_err(|e| KnowledgeCommandError::MetadataStoreInit(e.to_string()))?;

    if metadata_store.exists() {
        std::fs::remove_file(metadata_store.metadata_path())
            .map_err(|e| KnowledgeCommandError::MetadataDelete(e.to_string()))?;
    }

    let storage_path = dirs::data_dir()
        .map(|p| p.join("inkuo").join("knowledge").join(&workspace_id))
        .ok_or(KnowledgeCommandError::MissingDataDirectory)?;

    if storage_path.exists() {
        std::fs::remove_dir_all(&storage_path)
            .map_err(|e| KnowledgeCommandError::StorageDelete(e.to_string()))?;
    }

    let mut stores = shared_stores().write().await;
    stores.retain(|k, _| !k.starts_with(&format!("{}::", workspace_id)));

    Ok(())
}

/// Add files as members to the knowledge base
#[tauri::command]
pub async fn knowledge_add_members(
    app: AppHandle,
    workspace_path: String,
    member_paths: Vec<String>,
    session_id: String,
    collection: Option<String>,
) -> Result<UpdateResult, KnowledgeCommandError> {
    tracing::info!(
        "[KB_ADD_MEMBERS] workspace={}, members={:?}",
        workspace_path,
        member_paths
    );

    let workspace = PathBuf::from(&workspace_path);

    if !workspace.is_dir() {
        return Err(KnowledgeCommandError::WorkspaceNotFound(
            workspace.display().to_string(),
        ));
    }

    let collection = validated_collection(collection.as_deref())?;
    let mut metadata_store = MetadataStore::new(&workspace)
        .map_err(|e| KnowledgeCommandError::MetadataStoreInit(e.to_string()))?;

    // Ensure metadata exists
    if !metadata_store.exists() {
        let collection_name = format!("kb_{}", &get_workspace_id(&workspace_path)[..12]);
        metadata_store
            .create(&workspace, &collection_name)
            .map_err(|e| KnowledgeCommandError::MetadataCreate(e.to_string()))?;
    } else {
        metadata_store
            .load()
            .map_err(|e| KnowledgeCommandError::MetadataLoad(e.to_string()))?;
    }

    let scanner = DocScanner::default();
    emit_build_progress(
        &app,
        &session_id,
        "scanning",
        0,
        4,
        format!("解析 {} 个文件…", member_paths.len()),
    );
    let report = scanner.scan_paths(&workspace, &member_paths, &collection);
    let existing_members: HashSet<String> = metadata_store
        .members_for_collection(&collection)
        .into_iter()
        .collect();
    let indexed: HashMap<String, (String, String)> = metadata_store
        .get_indexed_files_map_for_collection(&collection)
        .into_iter()
        .map(|(path, file)| (path, (file.hash.clone(), file.document_id.clone())))
        .collect();

    let mut added = 0;
    let mut updated = 0;
    let mut unchanged = report.duplicate_paths;
    let mut changed_documents = Vec::new();
    let mut successful_members = Vec::new();
    for document in report.documents {
        successful_members.push(document.path.clone());
        if !existing_members.contains(&document.path) {
            added += 1;
        }
        match indexed.get(&document.path) {
            Some((hash, _)) if hash == &document.file_hash => unchanged += 1,
            Some(_) => {
                updated += 1;
                changed_documents.push(document);
            }
            None => changed_documents.push(document),
        }
    }

    if changed_documents.is_empty() {
        // Only successfully parsed files become members. Failed inputs remain
        // diagnostic rows and never pollute retrieval membership.
        metadata_store
            .add_members_to_collection(&collection, &successful_members)
            .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;
        metadata_store
            .record_failures(&collection, &report.failures)
            .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;
        metadata_store
            .clear_failures_for_paths(&collection, &successful_members)
            .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;
        emit_build_progress(&app, &session_id, "done", 4, 4, "没有需要重建的文件");
        return Ok(UpdateResult {
            added,
            removed: 0,
            updated,
            unchanged,
            failed: report.failures.len(),
            failures: report.failures,
        });
    }

    let model_name = get_embedding_model();
    let model_path = resolve_model_dir(&app, &model_name)
        .ok_or_else(|| KnowledgeCommandError::ModelNotFound(model_name.clone()))?;
    let embedder = Embedder::new(&model_name, &model_path)
        .map_err(|e| KnowledgeCommandError::EmbedderInit(e.to_string()))?;
    let vector_store =
        get_or_create_vector_store(&workspace_path, &model_name, &collection).await?;

    let chunker = Chunker::new(ChunkConfig {
        target_size: get_chunk_size(),
        overlap: get_chunk_overlap(),
        min_size: 50,
        preserve_headers: true,
    });
    emit_build_progress(&app, &session_id, "chunking", 1, 4, "正在分块…");
    let mut new_chunks = chunker.chunk_documents(&changed_documents);
    if new_chunks.is_empty() {
        return Err(KnowledgeCommandError::DocumentScan(
            "所选文件未生成可索引分块；已保留旧索引".to_string(),
        ));
    }
    emit_build_progress(&app, &session_id, "embedding", 2, 4, "正在生成向量…");
    embedder
        .encode_chunks_batched_async(&mut new_chunks, 64)
        .await
        .map_err(|e| KnowledgeCommandError::Embedding(e.to_string()))?;
    let replacement_plan = plan_document_replacements(&changed_documents, &new_chunks, &indexed)?;
    let file_paths: HashMap<String, String> = changed_documents
        .iter()
        .filter_map(|document| {
            resolve_member_path(&workspace, &document.path)
                .ok()
                .map(|path| (document.id.clone(), path.to_string_lossy().to_string()))
        })
        .collect();
    emit_build_progress(&app, &session_id, "storing", 3, 4, "正在写入索引…");
    // Embedding for the entire batch is complete before shard mutation. Each
    // new document generation is then staged before the prior one is retired.
    let staged = stage_document_replacements(&vector_store, &replacement_plan, &file_paths).await?;

    let chunk_count_by_doc = chunk_counts(&new_chunks);
    let retirements = pending_retirements(&collection, &staged);
    if let Err(error) = metadata_store.update_with_retirements(
        &changed_documents,
        &chunk_count_by_doc,
        new_chunks.len(),
        &[],
        &retirements,
    ) {
        let rollback_failures = rollback_staged_generations(&vector_store, &staged).await;
        return Err(KnowledgeCommandError::MetadataUpdate(format!(
            "{}；staged generation 回滚{}",
            error,
            if rollback_failures.is_empty() {
                "完成".to_string()
            } else {
                format!("失败：{}", rollback_failures.join(" | "))
            }
        )));
    }
    drain_pending_retirements(&vector_store, &mut metadata_store, &collection).await?;
    metadata_store
        .add_members_to_collection(&collection, &successful_members)
        .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;
    metadata_store
        .record_failures(&collection, &report.failures)
        .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;
    metadata_store
        .clear_failures_for_paths(&collection, &successful_members)
        .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;
    emit_build_progress(&app, &session_id, "done", 4, 4, "批量导入完成");
    Ok(UpdateResult {
        added,
        removed: 0,
        updated,
        unchanged,
        failed: report.failures.len(),
        failures: report.failures,
    })
}

/// Remove files from the knowledge base members
#[tauri::command]
pub async fn knowledge_remove_members(
    workspace_path: String,
    member_paths: Vec<String>,
    collection: Option<String>,
) -> Result<UpdateResult, KnowledgeCommandError> {
    tracing::info!(
        "[KB_REMOVE_MEMBERS] workspace={}, members={:?}",
        workspace_path,
        member_paths
    );

    let workspace = PathBuf::from(&workspace_path);

    if !workspace.exists() {
        return Err(KnowledgeCommandError::WorkspaceNotFound(
            workspace.display().to_string(),
        ));
    }

    let mut metadata_store = MetadataStore::new(&workspace)
        .map_err(|e| KnowledgeCommandError::MetadataStoreInit(e.to_string()))?;

    if !metadata_store.exists() {
        return Ok(UpdateResult {
            added: 0,
            removed: 0,
            updated: 0,
            unchanged: 0,
            failed: 0,
            failures: Vec::new(),
        });
    }

    metadata_store
        .load()
        .map_err(|e| KnowledgeCommandError::MetadataLoad(e.to_string()))?;

    let collection = validated_collection(collection.as_deref())?;
    let existing_members = metadata_store.members_for_collection(&collection);
    let failure_paths: HashSet<String> = metadata_store
        .get_metadata()
        .map(|metadata| {
            metadata
                .failures
                .iter()
                .filter(|failure| failure.collection == collection)
                .map(|failure| failure.path.clone())
                .collect()
        })
        .unwrap_or_default();

    let requested_count = member_paths.len();
    let to_remove: Vec<String> = member_paths
        .into_iter()
        .filter(|path| existing_members.contains(path) || failure_paths.contains(path))
        .collect();

    if to_remove.is_empty() {
        return Ok(UpdateResult {
            added: 0,
            removed: 0,
            updated: 0,
            unchanged: requested_count,
            failed: 0,
            failures: Vec::new(),
        });
    }

    let needs_vector_store = to_remove.iter().any(|path| {
        !metadata_store
            .indexed_document_ids_for_paths(&collection, std::slice::from_ref(path))
            .is_empty()
    });
    let vector_store = if needs_vector_store {
        Some(
            get_or_create_vector_store(&workspace_path, &get_embedding_model(), &collection)
                .await?,
        )
    } else {
        None
    };

    let mut removed = 0;
    for path in &to_remove {
        let one_path = vec![path.clone()];
        let document_ids = metadata_store.indexed_document_ids_for_paths(&collection, &one_path);
        if let Some(vector_store) = vector_store.as_ref() {
            for document_id in document_ids {
                vector_store
                    .delete_by_document_id(&document_id)
                    .await
                    .map_err(|e| KnowledgeCommandError::StoreVectors(e.to_string()))?;
            }
        }
        metadata_store
            .remove_indexed_files(&collection, &one_path)
            .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;
        let was_member = metadata_store
            .remove_members_from_collection(&collection, &one_path)
            .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;
        metadata_store
            .clear_failures_for_paths(&collection, &one_path)
            .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;
        removed += was_member.max(usize::from(failure_paths.contains(path)));
    }

    tracing::info!("[KB_REMOVE_MEMBERS] Done: removed {} members", removed);

    Ok(UpdateResult {
        added: 0,
        removed,
        updated: 0,
        unchanged: 0,
        failed: 0,
        failures: Vec::new(),
    })
}

// ── Members / listing ───────────────────────────────────────────────────────────

/// Get the list of member file paths in the knowledge base
#[tauri::command]
pub fn knowledge_get_members(
    workspace_path: String,
    collection: Option<String>,
) -> Result<Vec<String>, KnowledgeCommandError> {
    let workspace = PathBuf::from(&workspace_path);

    if !workspace.exists() {
        return Err(KnowledgeCommandError::WorkspaceNotFound(
            workspace.display().to_string(),
        ));
    }

    let mut metadata_store = MetadataStore::new(&workspace)
        .map_err(|e| KnowledgeCommandError::MetadataStoreInit(e.to_string()))?;

    if !metadata_store.exists() {
        return Ok(Vec::new());
    }

    metadata_store
        .load()
        .map_err(|e| KnowledgeCommandError::MetadataLoad(e.to_string()))?;

    let collection = validated_collection(collection.as_deref())?;
    let members = metadata_store.members_for_collection(&collection);

    Ok(members)
}

pub(crate) fn first_existing_path(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    for p in paths {
        if p.exists() {
            return Some(p);
        }
    }
    None
}

pub(crate) fn resolve_model_dir(app: &AppHandle, model_name: &str) -> Option<PathBuf> {
    let dir_name = model_name.replace('/', "-");

    let candidates: Vec<PathBuf> = [
        app.path()
            .resource_dir()
            .ok()
            .map(|p| p.join("models").join(&dir_name)),
        Some(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("models")
                .join(&dir_name),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();

    first_existing_path(candidates)
}

// ── Model commands ───────────────────────────────────────────────────────────────

// Thin `#[tauri::command]` wrappers around the canonical
// implementations in `embedding_models.rs`. They live here (rather than
// being re-exported) because `tauri::generate_handler!` generates
// per-module `__cmd__<name>` and `__tauri_command_name_<name>` symbols,
// and `lib.rs` registers these under `knowledge::commands::*`.
#[tauri::command]
pub fn check_available_models(app: AppHandle) -> Vec<EmbeddingModelInfo> {
    super::embedding_models::check_available_models(&app)
}

#[tauri::command]
pub async fn download_model_files(
    app: AppHandle,
    model_name: String,
) -> Result<String, KnowledgeCommandError> {
    super::embedding_models::download_model_files(&app, &model_name).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(id: &str, path: &str) -> crate::knowledge::Document {
        crate::knowledge::Document {
            id: id.into(),
            path: path.into(),
            title: path.into(),
            content: "content".into(),
            file_hash: "hash".into(),
            collection: "research".into(),
            source_type: "text".into(),
            size_bytes: 7,
        }
    }

    fn chunk(document_id: &str) -> crate::knowledge::Chunk {
        crate::knowledge::Chunk {
            id: format!("{}-0", document_id),
            document_id: document_id.into(),
            content: "content".into(),
            chunk_index: 0,
            start_line: 1,
            end_line: 1,
            embedding: vec![0.1, 0.2],
            collection: "research".into(),
        }
    }

    #[test]
    fn replacement_plan_rejects_incomplete_batch_before_vector_mutation() {
        let documents = vec![document("a", "a.md"), document("b", "b.md")];
        let indexed = HashMap::from([
            ("a.md".into(), ("hash-a".into(), "old-a".into())),
            ("b.md".into(), ("hash-b".into(), "old-b".into())),
        ]);
        let error = plan_document_replacements(&documents, &[chunk("a")], &indexed)
            .expect_err("a missing chunk must reject the entire plan");
        assert!(error.to_string().contains("b.md"));
        assert!(error.to_string().contains("保留旧索引"));
    }

    #[test]
    fn replacement_plan_carries_previous_ids_only_after_all_chunks_exist() {
        let documents = vec![document("a", "a.md"), document("b", "b.md")];
        let indexed = HashMap::from([("a.md".into(), ("hash-a".into(), "old-a".into()))]);
        let plan =
            plan_document_replacements(&documents, &[chunk("a"), chunk("b")], &indexed).unwrap();
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].previous_document_id.as_deref(), Some("old-a"));
        assert_eq!(plan[1].previous_document_id, None);
    }

    fn result(collection: &str, file: &str, score: f32) -> SearchResult {
        SearchResult {
            chunk_id: format!("{collection}-{file}"),
            document_id: format!("doc-{collection}-{file}"),
            content: file.into(),
            score,
            document_title: file.into(),
            file_path: file.into(),
            start_line: Some(1),
            end_line: Some(1),
            collection: collection.into(),
        }
    }

    #[test]
    fn all_collection_results_are_globally_ranked_and_truncated() {
        let merged = merge_ranked_results(
            vec![
                vec![
                    result("default", "a.md", 0.71),
                    result("default", "b.md", 0.52),
                ],
                vec![
                    result("research", "c.pdf", 0.93),
                    result("research", "d.docx", 0.68),
                ],
            ],
            3,
        );
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].collection, "research");
        assert_eq!(merged[0].file_path, "c.pdf");
        assert_eq!(merged[1].file_path, "a.md");
        assert_eq!(merged[2].file_path, "d.docx");
        assert!(merged.windows(2).all(|pair| pair[0].score >= pair[1].score));
    }

    #[test]
    fn workspace_import_keeps_existing_external_members() {
        let existing = vec![
            "/outside/reference.pdf".to_string(),
            "selected/brief.docx".to_string(),
        ];
        let scanned = vec![
            document("workspace-a", "README.md"),
            document("workspace-b", "selected/brief.docx"),
        ];
        let members = additive_workspace_members(&existing, &scanned);
        assert_eq!(members[0], "/outside/reference.pdf");
        assert_eq!(members[1], "selected/brief.docx");
        assert_eq!(members[2], "README.md");
        assert_eq!(
            members
                .iter()
                .filter(|path| path.as_str() == "selected/brief.docx")
                .count(),
            1
        );
    }
}
