//! Knowledge base Tauri commands

use crate::commands::{get_chunk_size, get_embedding_model};
use crate::knowledge::{
    BuildResult, ChunkConfig, Chunker, DocScanner, Embedder, MetadataStore, ModelInfo,
    SearchResult, UpdateResult, VectorStore,
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

fn emit_model_download_progress(
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
        overlap: 50,
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
    let total_batches = chunks.len().div_ceil(batch_size.max(1));
    embedder
        .encode_chunks_batched(&mut chunks, batch_size, |completed, total| {
            let completed_batches = completed.div_ceil(batch_size.max(1));
            tracing::info!(
                "[KB_BUILD] Embedding progress: {}/{} chunks, batch {}/{}",
                completed,
                total,
                completed_batches,
                total_batches
            );
            emit_build_progress(
                &app,
                &session_id,
                "embedding",
                2,
                4,
                format!(
                    "生成向量中... ({}/{} 块, 第 {}/{} 批)",
                    completed, total, completed_batches, total_batches
                ),
            );
        })
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

    metadata_store
        .update(&documents, chunks.len())
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
    })))
}

#[tauri::command]
pub async fn knowledge_update(
    app: AppHandle,
    workspace_path: String,
    session_id: String,
) -> Result<UpdateResult, KnowledgeCommandError> {
    tracing::info!("Updating knowledge base for workspace: {}", workspace_path);

    let workspace = PathBuf::from(&workspace_path);
    let scanner = DocScanner::default();
    let chunk_size = get_chunk_size();
    let chunker = Chunker::new(ChunkConfig {
        target_size: chunk_size,
        overlap: 50,
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

    let (changed_docs, _removed) = metadata_store.find_changed_files(&current_docs);

    if changed_docs.is_empty() {
        return Ok(UpdateResult {
            added: 0,
            removed: 0,
            updated: 0,
        });
    }

    let mut file_paths: HashMap<String, String> = HashMap::new();
    for doc in &changed_docs {
        let abs_path = workspace.join(&doc.path);
        file_paths.insert(doc.id.clone(), abs_path.to_string_lossy().to_string());
    }

    let mut new_chunks = Vec::new();
    let mut owned_changed_docs = Vec::new();
    for doc in &changed_docs {
        let chunks = chunker.chunk_document(&doc.id, &doc.title, &doc.content);
        new_chunks.extend(chunks);
        owned_changed_docs.push((*doc).clone());
    }

    let batch_size = 64usize;
    let total_batches = new_chunks.len().div_ceil(batch_size.max(1));
    embedder
        .encode_chunks_batched(&mut new_chunks, batch_size, |completed, total| {
            let completed_batches = completed.div_ceil(batch_size.max(1));
            tracing::info!(
                "[KB_UPDATE] Embedding progress: {}/{} chunks, batch {}/{}",
                completed,
                total,
                completed_batches,
                total_batches
            );
            emit_build_progress(
                &app,
                &session_id,
                "embedding",
                2,
                4,
                format!(
                    "增量生成向量中... ({}/{} 块, 第 {}/{} 批)",
                    completed, total, completed_batches, total_batches
                ),
            );
        })
        .map_err(|e| KnowledgeCommandError::Embedding(e.to_string()))?;

    vector_store
        .upsert_chunks(&new_chunks, &file_paths)
        .await
        .map_err(|e| KnowledgeCommandError::StoreVectors(e.to_string()))?;

    metadata_store
        .update(&owned_changed_docs, new_chunks.len())
        .map_err(|e| KnowledgeCommandError::MetadataUpdate(e.to_string()))?;

    Ok(UpdateResult {
        added: owned_changed_docs.len(),
        removed: 0,
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

fn first_existing_path(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    for p in paths {
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn resolve_model_dir(app: &AppHandle, model_name: &str) -> Option<PathBuf> {
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct EmbeddingModelInfo {
    pub name: String,
    pub available: bool,
    pub path: Option<String>,
    pub dimensions: usize,
    pub size: String,
}

#[tauri::command]
pub fn check_available_models(app: AppHandle) -> Vec<EmbeddingModelInfo> {
    let mut models = Vec::new();

    for (name, dims, size) in [
        ("BAAI/bge-small-zh-v1.5", 512, "~25MB"),
        ("BAAI/bge-base-zh-v1.5", 768, "~390MB"),
        ("BAAI/bge-large-zh-v1.5", 1024, "~1.3GB"),
    ] {
        let model_dir = resolve_model_dir(&app, name);
        let exists = match name {
            "BAAI/bge-small-zh-v1.5" | "BAAI/bge-large-zh-v1.5" => {
                model_dir.as_ref().map(|p| p.exists()).unwrap_or(false)
            }
            _ => model_dir
                .as_ref()
                .map(|p| p.exists() && p.join("tokenizer.json").exists() && p.join("model.onnx").exists())
                .unwrap_or(false),
        };
        models.push(EmbeddingModelInfo {
            name: name.to_string(),
            available: exists,
            path: model_dir.map(|p| p.to_string_lossy().to_string()),
            dimensions: dims,
            size: size.to_string(),
        });
    }

    models
}

#[tauri::command]
pub async fn download_model_files(
    app: AppHandle,
    model_name: String,
) -> Result<String, KnowledgeCommandError> {
    tracing::info!("Downloading model files for: {}", model_name);

    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| KnowledgeCommandError::ResourceDirectory(e.to_string()))?;

    let model_dir = resource_dir.join("models").join(model_name.replace('/', "-"));

    std::fs::create_dir_all(&model_dir)
        .map_err(|e| KnowledgeCommandError::ModelDirectoryCreate(e.to_string()))?;

    let files = [
        (
            "tokenizer.json",
            format!("https://hf-mirror.com/{}/resolve/main/tokenizer.json", model_name),
        ),
        (
            "tokenizer_config.json",
            format!(
                "https://hf-mirror.com/{}/resolve/main/tokenizer_config.json",
                model_name
            ),
        ),
        (
            "special_tokens_map.json",
            format!(
                "https://hf-mirror.com/{}/resolve/main/special_tokens_map.json",
                model_name
            ),
        ),
        (
            "vocab.txt",
            format!("https://hf-mirror.com/{}/resolve/main/vocab.txt", model_name),
        ),
        (
            "config.json",
            format!("https://hf-mirror.com/{}/resolve/main/config.json", model_name),
        ),
    ];

    let total = files.len();
    let mut downloaded = 0;

    emit_model_download_progress(
        &app,
        &model_name,
        0,
        total,
        "开始下载...",
        "downloading",
        None,
    );

    for (filename, url) in files {
        let path = model_dir.join(filename);
        if path.exists() {
            tracing::debug!("File already exists: {:?}", path);
            downloaded += 1;
            emit_model_download_progress(
                &app,
                &model_name,
                downloaded,
                total,
                filename,
                "skipping",
                None,
            );
            continue;
        }

        tracing::info!("Downloading {} from {}", filename, url);

        let result = download_file_with_progress(&app, &url, &path, &model_name, downloaded, total);

        match result {
            Ok(_) => {
                tracing::info!("Downloaded: {}", filename);
                downloaded += 1;
            }
            Err(error) => {
                tracing::warn!("Failed to download {}: {}", filename, error);
                let mirror_url = url.replace("huggingface.co", "hf-mirror.com");
                download_file_with_progress(&app, &mirror_url, &path, &model_name, downloaded, total)?;
                tracing::info!("Downloaded {} from mirror", filename);
                downloaded += 1;
            }
        }

        emit_model_download_progress(
            &app,
            &model_name,
            downloaded,
            total,
            filename,
            "done",
            None,
        );
    }

    let model_json_path = model_dir.join("model.json");
    let dimensions = match model_name.as_str() {
        "BAAI/bge-small-zh-v1.5" => 512,
        "BAAI/bge-base-zh-v1.5" => 768,
        _ => 1024,
    };
    let model_json = serde_json::json!({
        "model_name": model_name,
        "dimensions": dimensions,
        "max_length": 512,
        "pooling": "mean",
        "normalize": true
    });
    let model_json_string = serde_json::to_string_pretty(&model_json)
        .map_err(|e| KnowledgeCommandError::ModelMetadataSerialize(e.to_string()))?;
    std::fs::write(&model_json_path, model_json_string)
        .map_err(|e| KnowledgeCommandError::ModelMetadataWrite(e.to_string()))?;

    emit_model_download_progress(
        &app,
        &model_name,
        total,
        total,
        "完成",
        "complete",
        None,
    );

    Ok(format!("Downloaded {} files to {:?}", downloaded, model_dir))
}

fn download_file_with_progress(
    app: &AppHandle,
    url: &str,
    path: &Path,
    model_name: &str,
    current: usize,
    total: usize,
) -> Result<(), KnowledgeCommandError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| KnowledgeCommandError::HttpClient(e.to_string()))?;

    let response = client
        .get(url)
        .send()
        .map_err(|e| KnowledgeCommandError::HttpRequest(e.to_string()))?;

    if !response.status().is_success() {
        return Err(KnowledgeCommandError::HttpStatus(response.status().to_string()));
    }

    let bytes = response
        .bytes()
        .map_err(|e| KnowledgeCommandError::HttpResponseRead(e.to_string()))?;

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    emit_model_download_progress(
        app,
        model_name,
        current,
        total,
        filename,
        "downloading",
        Some(bytes.len()),
    );

    std::fs::write(path, bytes)
        .map_err(|e| KnowledgeCommandError::DownloadWrite(e.to_string()))?;

    Ok(())
}
