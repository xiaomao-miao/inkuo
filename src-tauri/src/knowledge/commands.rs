//! Knowledge base Tauri commands

use crate::commands::{get_chunk_overlap, get_chunk_size, get_embedding_model};
use crate::knowledge::{
    BuildResult, ChunkConfig, Chunker, DocScanner, Embedder, EmbeddingModelInfo,
    MetadataStore, ModelInfo, SearchResult, UpdateResult, VectorStore,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
) -> Result<VectorStore, KnowledgeCommandError> {
    get_or_create_vector_store(workspace_path, model_name).await
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
) -> Result<VectorStore, KnowledgeCommandError> {
    let workspace_id = get_workspace_id(workspace_path);
    let store_key = format!("{}::{}", workspace_id, model_name);
    let collection_name = format!("kb_{}", &workspace_id[..12]);

    {
        let stores = shared_stores().read().await;
        if let Some(store) = stores.get(&store_key) {
            tracing::debug!("Vector store cache hit for {}", store_key);
            return Ok(store.clone());
        }
    }

    tracing::info!("Creating new vector store for {}", store_key);
    let store = VectorStore::new(
        &PathBuf::from(workspace_path),
        &collection_name,
        model_name,
    )
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

    let documents = scanner
        .scan(&workspace)
        .map_err(|e| KnowledgeCommandError::DocumentScan(e.to_string()))?;
    tracing::info!("[KB_BUILD] Scan complete, found {} documents", documents.len());

    let mut file_paths: HashMap<String, String> = HashMap::new();
    for doc in &documents {
        let abs_path = workspace.join(&doc.path);
        file_paths.insert(doc.id.clone(), abs_path.to_string_lossy().to_string());
    }

    tracing::info!("[KB_BUILD] [PHASE 2] Starting chunking, {} documents", documents.len());
    emit_build_progress(
        &app,
        &session_id,
        "chunking",
        1,
        4,
        format!("分块处理中... ({} 文档)", documents.len()),
    );

    let mut chunks = chunker.chunk_documents(&documents);
    tracing::info!("[KB_BUILD] Chunking complete, created {} chunks", chunks.len());

    tracing::info!("[KB_BUILD] [PHASE 3] Starting embedding, {} chunks", chunks.len());
    emit_build_progress(
        &app,
        &session_id,
        "embedding",
        2,
        4,
        format!("生成向量中... ({} 块)", chunks.len()),
    );

    let batch_size = 64usize;
    embedder
        .encode_chunks_batched_async(&mut chunks, batch_size)
        .await
        .map_err(|e| KnowledgeCommandError::Embedding(e.to_string()))?;
    tracing::info!("[KB_BUILD] Embedding complete");

    tracing::info!("[KB_BUILD] [PHASE 4] Starting vector storage");
    emit_build_progress(&app, &session_id, "storing", 3, 4, "存储向量中...");

    let vector_store = get_or_create_vector_store(&workspace_path, &model_name).await?;

    vector_store
        .upsert_chunks(&chunks, &file_paths)
        .await
        .map_err(|e| KnowledgeCommandError::StoreVectors(e.to_string()))?;
    tracing::info!("[KB_BUILD] upsert_chunks complete");

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

    // Build is a full rebuild from scratch: every document is "new" and
    // nothing is removed. `chunk_count_by_doc` is derived from the chunks we
    // just produced so the per-document chunk_count in metadata.json is
    // accurate.
    let mut chunk_count_by_doc: HashMap<String, usize> = HashMap::new();
    for c in &chunks {
        *chunk_count_by_doc.entry(c.document_id.clone()).or_insert(0) += 1;
    }
    metadata_store
        .update(&documents, &chunk_count_by_doc, chunks.len(), &[])
        .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;

    emit_build_progress(
        &app,
        &session_id,
        "done",
        4,
        4,
        format!("构建完成！{} 文档，{} 块", documents.len(), chunks.len()),
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
) -> Result<Vec<SearchResult>, KnowledgeCommandError> {
    let model_name = get_embedding_model();
    let model_info = ModelInfo::new(&model_name)
        .map_err(|e| KnowledgeCommandError::UnsupportedEmbeddingModel(e.to_string()))?;

    let model_path = resolve_model_dir(app, &model_name)
        .ok_or_else(|| KnowledgeCommandError::ModelNotFound(model_name.clone()))?;

    let embedder = Embedder::new(&model_name, &model_path)
        .map_err(|e| KnowledgeCommandError::EmbedderInit(e.to_string()))?;

    let vector_store = if for_search {
        get_vector_store_for_search(workspace_path, &model_name).await?
    } else {
        get_or_create_vector_store(workspace_path, &model_name).await?
    };

    tracing::debug!(
        "knowledge search: model={} (dim={})",
        model_name,
        model_info.dimension
    );

    let query_vector = embedder
        .encode_single(query)
        .map_err(|e| KnowledgeCommandError::EncodeQuery(e.to_string()))?;

    vector_store
        .search(&query_vector, top_k)
        .await
        .map_err(|e| KnowledgeCommandError::Search(e.to_string()))
}

#[tauri::command]
pub async fn knowledge_search(
    app: AppHandle,
    workspace_path: String,
    query: String,
    top_k: usize,
) -> Result<Vec<SearchResult>, KnowledgeCommandError> {
    tracing::info!("Searching knowledge base: {}", query);

    search_knowledge_base_inner(&app, &workspace_path, &query, top_k, false).await
}

/// Public search function for use by both Tauri commands and agent tools.
/// Does NOT go through the Tauri command layer (avoids double-IPC in agent context).
pub async fn search_knowledge_base(
    app: &AppHandle,
    workspace_path: &str,
    query: &str,
    top_k: usize,
) -> Result<Vec<SearchResult>, KnowledgeCommandError> {
    search_knowledge_base_inner(app, workspace_path, query, top_k, true).await
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

    Ok(Some(serde_json::json!({
        "workspace_id": metadata.workspace_id,
        "workspace_path": metadata.workspace_path,
        "document_count": metadata.document_count,
        "chunk_count": metadata.chunk_count,
        "created_at": metadata.created_at,
        "last_updated": metadata.last_updated,
        "members": metadata.members,
    })))
}

#[tauri::command]
pub async fn knowledge_update(
    app: AppHandle,
    workspace_path: String,
    // Kept for IPC compatibility with the frontend even though the async
    // embedder wrapper no longer takes a per-batch progress callback that
    // would consume the session id.
    #[allow(unused_variables)] session_id: String,
) -> Result<UpdateResult, KnowledgeCommandError> {
    tracing::info!("Updating knowledge base for workspace: {}", workspace_path);

    let workspace = PathBuf::from(&workspace_path);
    let scanner = DocScanner::default();
    let chunk_size = get_chunk_size();
    let chunker = Chunker::new(ChunkConfig {
        target_size: chunk_size,
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

    let vector_store = get_or_create_vector_store(&workspace_path, &model_name).await?;

    let mut metadata_store = MetadataStore::new(&workspace)
        .map_err(|e| KnowledgeCommandError::MetadataStoreInit(e.to_string()))?;

    if metadata_store.exists() {
        metadata_store
            .load()
            .map_err(|e| KnowledgeCommandError::MetadataLoad(e.to_string()))?;
    }

    let current_docs = scanner
        .scan(&workspace)
        .map_err(|e| KnowledgeCommandError::DocumentScan(e.to_string()))?;

    let (changed_docs, removed) = metadata_store.find_changed_files(&current_docs);

    // If nothing changed and nothing was removed, we're done. Note the old
    // code early-returned before processing `removed`, so deleting a file
    // from the workspace used to leave its vectors in Qdrant forever.
    if changed_docs.is_empty() && removed.is_empty() {
        return Ok(UpdateResult {
            added: 0,
            removed: 0,
            updated: 0,
        });
    }

    // Resolve the document_id for each removed path by inspecting the metadata
    // we loaded earlier, so the vector store can drop the corresponding
    // chunks. Without this, removing a file would silently leak stale vectors.
    let removed_doc_ids: Vec<String> = metadata_store
        .get_metadata()
        .map(|m| {
            m.indexed_files
                .iter()
                .filter(|f| removed.iter().any(|p| p == &f.path))
                .filter_map(|f| {
                    if f.document_id.is_empty() {
                        None
                    } else {
                        Some(f.document_id.clone())
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let mut file_paths: HashMap<String, String> = HashMap::new();
    for doc in &changed_docs {
        let abs_path = workspace.join(&doc.path);
        file_paths.insert(doc.id.clone(), abs_path.to_string_lossy().to_string());
    }

    let mut new_chunks = Vec::new();
    let mut chunk_count_by_doc: HashMap<String, usize> = HashMap::new();
    let mut owned_changed_docs = Vec::new();
    for doc in &changed_docs {
        let chunks = chunker.chunk_document(&doc.id, &doc.title, &doc.content);
        chunk_count_by_doc.insert(doc.id.clone(), chunks.len());
        new_chunks.extend(chunks);
        owned_changed_docs.push((*doc).clone());
    }

    let batch_size = 64usize;
    embedder
        .encode_chunks_batched_async(&mut new_chunks, batch_size)
        .await
        .map_err(|e| KnowledgeCommandError::Embedding(e.to_string()))?;

    vector_store
        .upsert_chunks(&new_chunks, &file_paths)
        .await
        .map_err(|e| KnowledgeCommandError::StoreVectors(e.to_string()))?;

    // Drop vectors for files that disappeared from the workspace. We do this
    // *after* upserting the new chunks so a partial update that errors before
    // reaching this point simply leaves stale vectors (still searchable,
    // surfaced as "deleted on disk" next run) instead of losing the new
    // vectors to a failed delete.
    for doc_id in &removed_doc_ids {
        if let Err(e) = vector_store.delete_by_document_id(doc_id).await {
            tracing::warn!(
                "[KB_UPDATE] Failed to delete vectors for removed document {}: {}",
                doc_id,
                e
            );
        }
    }

    metadata_store
        .update(&owned_changed_docs, &chunk_count_by_doc, new_chunks.len(), &removed)
        .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;

    Ok(UpdateResult {
        added: owned_changed_docs.len(),
        removed: removed.len(),
        updated: new_chunks.len(),
    })
}

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
    // Kept for IPC compatibility with the frontend even though the async
    // embedder wrapper no longer takes a per-batch progress callback that
    // would consume the session id.
    #[allow(unused_variables)] session_id: String,
) -> Result<UpdateResult, KnowledgeCommandError> {
    tracing::info!(
        "[KB_ADD_MEMBERS] workspace={}, members={:?}",
        workspace_path,
        member_paths
    );

    let workspace = PathBuf::from(&workspace_path);

    if !workspace.exists() {
        return Err(KnowledgeCommandError::WorkspaceNotFound(
            workspace.display().to_string(),
        ));
    }

    let model_name = get_embedding_model();
    let model_path = resolve_model_dir(&app, &model_name)
        .ok_or_else(|| KnowledgeCommandError::ModelNotFound(model_name.clone()))?;

    let embedder = Embedder::new(&model_name, &model_path)
        .map_err(|e| KnowledgeCommandError::EmbedderInit(e.to_string()))?;

    let vector_store = get_or_create_vector_store(&workspace_path, &model_name).await?;

    let mut metadata_store = MetadataStore::new(&workspace)
        .map_err(|e| KnowledgeCommandError::MetadataStoreInit(e.to_string()))?;

    // Ensure metadata exists
    if !metadata_store.exists() {
        let collection_name = format!("kb_{}", &get_workspace_id(&workspace_path)[..12]);
        metadata_store.create(&workspace, &collection_name)
            .map_err(|e| KnowledgeCommandError::MetadataCreate(e.to_string()))?;
    } else {
        metadata_store.load()
            .map_err(|e| KnowledgeCommandError::MetadataLoad(e.to_string()))?;
    }

    // Filter out paths that are already members
    let existing_members: std::collections::HashSet<_> = metadata_store
        .get_metadata()
        .map(|m| m.members.iter().collect())
        .unwrap_or_default();

    let new_paths: Vec<String> = member_paths
        .into_iter()
        .filter(|p| !existing_members.contains(p))
        .collect();

    if new_paths.is_empty() {
        return Ok(UpdateResult {
            added: 0,
            removed: 0,
            updated: 0,
        });
    }

    // Add members to metadata
    metadata_store.add_members(&new_paths)
        .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;

    // Scan and index only the new member files
    let scanner = DocScanner::default();
    let chunk_size = get_chunk_size();
    let chunker = Chunker::new(ChunkConfig {
        target_size: chunk_size,
        overlap: get_chunk_overlap(),
        min_size: 50,
        preserve_headers: true,
    });

    let all_docs = scanner
        .scan(&workspace)
        .map_err(|e| KnowledgeCommandError::DocumentScan(e.to_string()))?;

    let target_docs: Vec<_> = all_docs
        .into_iter()
        .filter(|doc| new_paths.contains(&doc.path))
        .collect();

    if target_docs.is_empty() {
        return Ok(UpdateResult {
            added: new_paths.len(),
            removed: 0,
            updated: 0,
        });
    }

    let mut file_paths: HashMap<String, String> = HashMap::new();
    for doc in &target_docs {
        let abs_path = workspace.join(&doc.path);
        file_paths.insert(doc.id.clone(), abs_path.to_string_lossy().to_string());
    }

    let mut new_chunks = Vec::new();
    let mut owned_docs = Vec::new();
    let mut chunk_count_by_doc: HashMap<String, usize> = HashMap::new();
    for doc in &target_docs {
        let chunks = chunker.chunk_document(&doc.id, &doc.title, &doc.content);
        chunk_count_by_doc.insert(doc.id.clone(), chunks.len());
        new_chunks.extend(chunks);
        owned_docs.push(doc.clone());
    }

    let batch_size = 64usize;
    embedder
        .encode_chunks_batched_async(&mut new_chunks, batch_size)
        .await
        .map_err(|e| KnowledgeCommandError::Embedding(e.to_string()))?;

    vector_store
        .upsert_chunks(&new_chunks, &file_paths)
        .await
        .map_err(|e| KnowledgeCommandError::StoreVectors(e.to_string()))?;

    // Update metadata with the new indexed files. Use the merge semantics so
    // pre-existing entries in `indexed_files` are preserved.
    metadata_store
        .update(&owned_docs, &chunk_count_by_doc, new_chunks.len(), &[])
        .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;

    tracing::info!(
        "[KB_ADD_MEMBERS] Done: added {} members, {} chunks",
        new_paths.len(),
        new_chunks.len()
    );

    Ok(UpdateResult {
        added: new_paths.len(),
        removed: 0,
        updated: new_chunks.len(),
    })
}

/// Remove files from the knowledge base members
#[tauri::command]
pub async fn knowledge_remove_members(
    workspace_path: String,
    member_paths: Vec<String>,
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

    let model_name = get_embedding_model();
    let vector_store = get_or_create_vector_store(&workspace_path, &model_name).await?;

    let mut metadata_store = MetadataStore::new(&workspace)
        .map_err(|e| KnowledgeCommandError::MetadataStoreInit(e.to_string()))?;

    if !metadata_store.exists() {
        return Ok(UpdateResult {
            added: 0,
            removed: 0,
            updated: 0,
        });
    }

    metadata_store.load()
        .map_err(|e| KnowledgeCommandError::MetadataLoad(e.to_string()))?;

    // Find which paths are actually members
    let existing_members = metadata_store
        .get_metadata()
        .map(|m| m.members.clone())
        .unwrap_or_default();

    let to_remove: Vec<String> = member_paths
        .into_iter()
        .filter(|p| existing_members.contains(p))
        .collect();

    if to_remove.is_empty() {
        return Ok(UpdateResult {
            added: 0,
            removed: 0,
            updated: 0,
        });
    }

    // Get all docs to find document IDs for the removed paths
    let scanner = DocScanner::default();
    let all_docs = scanner
        .scan(&workspace)
        .map_err(|e| KnowledgeCommandError::DocumentScan(e.to_string()))?;

    // Delete vectors for files matching removed paths
    for doc in &all_docs {
        if to_remove.contains(&doc.path) {
            vector_store
                .delete_by_document_id(&doc.id)
                .await
                .map_err(|e| KnowledgeCommandError::StoreVectors(e.to_string()))?;
        }
    }

    // Remove members from metadata
    let removed = metadata_store.remove_members(&to_remove)
        .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;

    // Update document count and chunk count
    if let Some(metadata) = metadata_store.get_metadata() {
        let remaining_count = metadata.members.len();
        // Recalculate chunk count based on remaining members
        let remaining_docs: Vec<_> = all_docs
            .into_iter()
            .filter(|doc| metadata.members.contains(&doc.path))
            .collect();
        let remaining_chunks: usize = remaining_docs.len() * 2; // rough estimate

        metadata_store.metadata.as_mut().map(|m| {
            m.document_count = remaining_count;
            m.chunk_count = remaining_chunks;
            m.last_updated = chrono::Utc::now();
        });
        metadata_store.save()
            .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;
    }

    tracing::info!("[KB_REMOVE_MEMBERS] Done: removed {} members", removed);

    Ok(UpdateResult {
        added: 0,
        removed,
        updated: 0,
    })
}

/// Get the list of member file paths in the knowledge base
#[tauri::command]
pub fn knowledge_get_members(
    workspace_path: String,
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

    metadata_store.load()
        .map_err(|e| KnowledgeCommandError::MetadataLoad(e.to_string()))?;

    let members = metadata_store
        .get_metadata()
        .map(|m| m.members.clone())
        .unwrap_or_default();

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
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models").join(&dir_name)),
    ]
    .into_iter()
    .flatten()
    .collect();

    first_existing_path(candidates)
}

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

