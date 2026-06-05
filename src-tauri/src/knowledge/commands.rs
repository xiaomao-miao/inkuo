//! Knowledge base Tauri commands

use crate::knowledge::{
    BuildResult, Chunker, ChunkConfig, DocScanner, Embedder, ModelInfo,
    MetadataStore, SearchResult, UpdateResult, VectorStore,
};
use crate::commands::{get_embedding_model, get_chunk_size};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

/// Shared vector store cache accessible from both KB commands and agent tools.
/// This ensures both code paths use the SAME VectorStore instance, avoiding
/// WAL lock conflicts (Qdrant Edge WAL only allows single-process access).
static SHARED_STORES: std::sync::OnceLock<
    tokio::sync::RwLock<HashMap<String, VectorStore>>
> = std::sync::OnceLock::new();

fn shared_stores() -> &'static tokio::sync::RwLock<HashMap<String, VectorStore>> {
    SHARED_STORES.get_or_init(|| {
        tracing::info!("Initializing shared vector store cache");
        tokio::sync::RwLock::new(HashMap::new())
    })
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
) -> Result<VectorStore, String> {
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
) -> Result<VectorStore, String> {
    let workspace_id = get_workspace_id(workspace_path);
    let store_key = format!("{}::{}", workspace_id, model_name);
    let collection_name = format!("kb_{}", &workspace_id[..12]);

    // Check shared cache
    {
        let stores = shared_stores().read().await;
        if let Some(store) = stores.get(&store_key) {
            tracing::debug!("Vector store cache hit for {}", store_key);
            return Ok(store.clone());
        }
    }

    // Create new
    tracing::info!("Creating new vector store for {}", store_key);
    let store = VectorStore::new(
        &PathBuf::from(workspace_path),
        &collection_name,
        model_name,
    )
    .await
    .map_err(|e| e.to_string())?;

    // Store in shared cache
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
) -> Result<BuildResult, String> {
    tracing::info!(
        "[KB_BUILD] START - workspace={}, session={}",
        workspace_path, session_id
    );

    let workspace = PathBuf::from(&workspace_path);
    tracing::info!("[KB_BUILD] Workspace path: {:?}", workspace);

    if !workspace.exists() {
        tracing::error!("[KB_BUILD] Workspace does not exist: {:?}", workspace);
        return Err(format!("Workspace does not exist: {}", workspace.display()));
    }

    let workspace_id = get_workspace_id(&workspace_path);
    tracing::info!("[KB_BUILD] Workspace ID: {}", workspace_id);

    // Phase 0: Start scanning
    tracing::info!("[KB_BUILD] [PHASE 0] Emitting scanning progress");
    let _ = app.emit("kb://build-progress", serde_json::json!({
        "session_id": session_id,
        "phase": "scanning",
        "current": 0,
        "total": 4,
        "message": "初始化中...",
    }));

    tracing::info!("[KB_BUILD] Initializing DocScanner");
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
        .map_err(|e| format!("Unsupported embedding model: {}", e))?;
    tracing::info!("[KB_BUILD] Model name from settings: {}", model_name);

    tracing::info!("[KB_BUILD] Resolving model directory");
    let model_path = resolve_model_dir(&app, &model_name)
        .ok_or_else(|| {
            tracing::error!("[KB_BUILD] Model path not found for: {}", model_name);
            format!("Model '{}' not found (no model files)", model_name)
        })?;
    tracing::info!("[KB_BUILD] Model path: {:?}", model_path);

    tracing::info!("[KB_BUILD] Creating Embedder");
    let embedder = Embedder::new(&model_name, &model_path)
        .map_err(|e| {
            tracing::error!("[KB_BUILD] Embedder init failed: {}", e);
            format!("Failed to initialize embedder: {}", e)
        })?;
    tracing::info!("[KB_BUILD] Embedder created successfully");

    // Phase 1: Scan documents
    tracing::info!("[KB_BUILD] [PHASE 1] Starting document scan");
    let _ = app.emit("kb://build-progress", serde_json::json!({
        "session_id": session_id,
        "phase": "scanning",
        "current": 1,
        "total": 4,
        "message": "扫描文档中...",
    }));

    tracing::info!("[KB_BUILD] Calling scanner.scan()");
    let documents = scanner
        .scan(&workspace)
        .map_err(|e| {
            tracing::error!("[KB_BUILD] Scan failed: {}", e);
            format!("Failed to scan documents: {}", e)
        })?;
    tracing::info!("[KB_BUILD] Scan complete, found {} documents", documents.len());

    // Build file paths map (absolute paths for frontend navigation)
    let mut file_paths: HashMap<String, String> = HashMap::new();
    for doc in &documents {
        let abs_path = workspace.join(&doc.path);
        file_paths.insert(doc.id.clone(), abs_path.to_string_lossy().to_string());
    }

    // Phase 2: Chunk documents
    tracing::info!("[KB_BUILD] [PHASE 2] Starting chunking, {} documents", documents.len());
    let _ = app.emit("kb://build-progress", serde_json::json!({
        "session_id": session_id,
        "phase": "chunking",
        "current": 1,
        "total": 4,
        "message": format!("分块处理中... ({} 文档)", documents.len()),
    }));

    tracing::info!("[KB_BUILD] Calling chunker.chunk_documents()");
    let mut chunks = chunker.chunk_documents(&documents);
    tracing::info!("[KB_BUILD] Chunking complete, created {} chunks", chunks.len());

    // Phase 3: Generate embeddings
    tracing::info!("[KB_BUILD] [PHASE 3] Starting embedding, {} chunks", chunks.len());
    let _ = app.emit("kb://build-progress", serde_json::json!({
        "session_id": session_id,
        "phase": "embedding",
        "current": 2,
        "total": 4,
        "message": format!("生成向量中... ({} 块)", chunks.len()),
    }));

    tracing::info!("[KB_BUILD] Calling embedder.encode_chunks_batched()");
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
            let _ = app.emit("kb://build-progress", serde_json::json!({
                "session_id": session_id,
                "phase": "embedding",
                "current": 2,
                "total": 4,
                "message": format!("生成向量中... ({}/{} 块, 第 {}/{} 批)", completed, total, completed_batches, total_batches),
            }));
        })
        .map_err(|e| {
            tracing::error!("[KB_BUILD] Embedding failed: {}", e);
            format!("Failed to generate embeddings: {}", e)
        })?;
    tracing::info!("[KB_BUILD] Embedding complete");

    // Phase 4: Store vectors
    tracing::info!("[KB_BUILD] [PHASE 4] Starting vector storage");
    let _ = app.emit("kb://build-progress", serde_json::json!({
        "session_id": session_id,
        "phase": "storing",
        "current": 3,
        "total": 4,
        "message": "存储向量中...",
    }));

    tracing::info!("[KB_BUILD] Getting vector store");
    let vector_store = get_or_create_vector_store(&workspace_path, &model_name).await
        .map_err(|e| {
            tracing::error!("[KB_BUILD] get_or_create_vector_store failed: {}", e);
            e
        })?;

    tracing::info!("[KB_BUILD] Calling upsert_chunks()");
    vector_store
        .upsert_chunks(&chunks, &file_paths)
        .await
        .map_err(|e| {
            tracing::error!("[KB_BUILD] upsert_chunks failed: {}", e);
            format!("Failed to store vectors: {}", e)
        })?;
    tracing::info!("[KB_BUILD] upsert_chunks complete");

    tracing::info!("[KB_BUILD] Updating metadata");
    let mut metadata_store = MetadataStore::new(&workspace)
        .map_err(|e| {
            tracing::error!("[KB_BUILD] MetadataStore init failed: {}", e);
            format!("Failed to create metadata store: {}", e)
        })?;

    let collection_name = format!("kb_{}", &workspace_id[..12]);
    if metadata_store.exists() {
        metadata_store
            .load()
            .map_err(|e| {
                tracing::error!("[KB_BUILD] Metadata load failed: {}", e);
                format!("Failed to load metadata: {}", e)
            })?;
    } else {
        metadata_store
            .create(&workspace, &collection_name)
            .map_err(|e| {
                tracing::error!("[KB_BUILD] Metadata create failed: {}", e);
                format!("Failed to create metadata: {}", e)
            })?;
    }

    metadata_store
        .update(&documents, chunks.len())
        .map_err(|e| {
            tracing::error!("[KB_BUILD] Metadata update failed: {}", e);
            format!("Failed to update metadata: {}", e)
        })?;
    tracing::info!("[KB_BUILD] Metadata update complete");

    // Done
    tracing::info!("[KB_BUILD] [DONE] Emitting final progress");
    let _ = app.emit("kb://build-progress", serde_json::json!({
        "session_id": session_id,
        "phase": "done",
        "current": 4,
        "total": 4,
        "message": format!("构建完成！{} 文档，{} 块", documents.len(), chunks.len()),
    }));

    tracing::info!(
        "[KB_BUILD] COMPLETE - {} docs, {} chunks, workspace_id={}",
        documents.len(), chunks.len(), workspace_id
    );

    Ok(BuildResult {
        total_documents: documents.len(),
        total_chunks: chunks.len(),
        workspace_id,
    })
}

/// Search knowledge base
#[tauri::command]
pub async fn knowledge_search(
    app: AppHandle,
    workspace_path: String,
    query: String,
    top_k: usize,
) -> Result<Vec<SearchResult>, String> {
    tracing::info!("Searching knowledge base: {}", query);

    let model_name = get_embedding_model();
    let model_info = ModelInfo::new(&model_name)
        .map_err(|e| format!("Unsupported embedding model: {}", e))?;

    let model_path = resolve_model_dir(&app, &model_name)
        .ok_or_else(|| format!("Model '{}' not found (no model files)", model_name))?;

    let embedder = Embedder::new(&model_name, &model_path)
        .map_err(|e| format!("Failed to initialize embedder: {}", e))?;

    let vector_store = get_or_create_vector_store(&workspace_path, &model_name).await?;

    tracing::info!(
        "[KB_SEARCH] Using model {} (dim={})",
        model_name,
        model_info.dimension
    );

    let query_vector = embedder
        .encode_single(&query)
        .map_err(|e| format!("Failed to encode query: {}", e))?;

    let results = vector_store
        .search(&query_vector, top_k)
        .await
        .map_err(|e| format!("Search failed: {}", e))?;

    Ok(results)
}

/// Public search function for use by both Tauri commands and agent tools.
/// Does NOT go through the Tauri command layer (avoids double-IPC in agent context).
pub async fn search_knowledge_base(
    app: &AppHandle,
    workspace_path: &str,
    query: &str,
    top_k: usize,
) -> Result<Vec<SearchResult>, String> {
    let model_name = get_embedding_model();
    let model_info = ModelInfo::new(&model_name)
        .map_err(|e| format!("Unsupported embedding model: {}", e))?;

    let model_path = resolve_model_dir(app, &model_name)
        .ok_or_else(|| format!("Model '{}' not found (no model files)", model_name))?;

    let embedder = Embedder::new(&model_name, &model_path)
        .map_err(|e| format!("Failed to initialize embedder: {}", e))?;

    let vector_store = get_vector_store_for_search(workspace_path, &model_name).await?;

    tracing::debug!(
        "search_knowledge_base: model={} (dim={})",
        model_name,
        model_info.dimension
    );

    let query_vector = embedder.encode_single(query)
        .map_err(|e| format!("Failed to encode query: {}", e))?;

    let results = vector_store.search(&query_vector, top_k).await
        .map_err(|e| format!("Search failed: {}", e))?;

    Ok(results)
}

/// Get knowledge base status
#[tauri::command]
pub fn knowledge_status(workspace_path: String) -> Result<Option<serde_json::Value>, String> {
    let workspace = PathBuf::from(&workspace_path);
    let mut metadata_store = MetadataStore::new(&workspace)
        .map_err(|e| format!("Failed to create metadata store: {}", e))?;

    if !metadata_store.exists() {
        return Ok(None);
    }

    let metadata = metadata_store
        .load()
        .map_err(|e| format!("Failed to load metadata: {}", e))?;

    Ok(Some(serde_json::json!({
        "workspace_id": metadata.workspace_id,
        "workspace_path": metadata.workspace_path,
        "document_count": metadata.document_count,
        "chunk_count": metadata.chunk_count,
        "created_at": metadata.created_at,
        "last_updated": metadata.last_updated,
    })))
}

/// Incremental update knowledge base
#[tauri::command]
pub async fn knowledge_update(
    app: AppHandle,
    workspace_path: String,
    session_id: String,
) -> Result<UpdateResult, String> {
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
        .map_err(|e| format!("Unsupported embedding model: {}", e))?;

    let model_path = resolve_model_dir(&app, &model_name)
        .ok_or_else(|| format!("Model '{}' not found (no model files)", model_name))?;

    let embedder = Embedder::new(&model_name, &model_path)
        .map_err(|e| format!("Failed to initialize embedder: {}", e))?;

    tracing::info!(
        "[KB_UPDATE] Using model {} (dim={})",
        model_name,
        model_info.dimension
    );

    let vector_store = get_or_create_vector_store(&workspace_path, &model_name).await?;

    let mut metadata_store = MetadataStore::new(&workspace)
        .map_err(|e| format!("Failed to create metadata store: {}", e))?;

    if metadata_store.exists() {
        metadata_store
            .load()
            .map_err(|e| format!("Failed to load existing metadata: {}", e))?;
    }

    let current_docs = scanner
        .scan(&workspace)
        .map_err(|e| format!("Failed to scan documents: {}", e))?;

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
            let _ = app.emit("kb://build-progress", serde_json::json!({
                "session_id": session_id,
                "phase": "embedding",
                "current": 2,
                "total": 4,
                "message": format!("增量生成向量中... ({}/{} 块, 第 {}/{} 批)", completed, total, completed_batches, total_batches),
            }));
        })
        .map_err(|e| format!("Failed to generate embeddings: {}", e))?;

    vector_store
        .upsert_chunks(&new_chunks, &file_paths)
        .await
        .map_err(|e| format!("Failed to store vectors: {}", e))?;

    metadata_store
        .update(&owned_changed_docs, new_chunks.len())
        .map_err(|e| format!("Failed to update metadata: {}", e))?;

    Ok(UpdateResult {
        added: owned_changed_docs.len(),
        removed: 0,
        updated: new_chunks.len(),
    })
}

/// Clear knowledge base for a workspace
#[tauri::command]
pub async fn knowledge_clear(
    workspace_path: String,
) -> Result<(), String> {
    let workspace = PathBuf::from(&workspace_path);
    let workspace_id = get_workspace_id(&workspace_path);

    let metadata_store = MetadataStore::new(&workspace)
        .map_err(|e| format!("Failed to create metadata store: {}", e))?;

    if metadata_store.exists() {
        std::fs::remove_file(metadata_store.metadata_path())
            .map_err(|e| format!("Failed to delete metadata: {}", e))?;
    }

    let storage_path = dirs::data_dir()
        .map(|p| p.join("inkuo").join("knowledge").join(&workspace_id))
        .ok_or("Failed to get data directory")?;

    if storage_path.exists() {
        std::fs::remove_dir_all(&storage_path)
            .map_err(|e| format!("Failed to delete storage: {}", e))?;
    }

    let mut stores = shared_stores().write().await;
    stores.retain(|k, _| !k.starts_with(&format!("{}::", workspace_id)));

    Ok(())
}

/// Returns the first existing path among candidates, or None.
fn first_existing_path(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    for p in paths {
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Returns the model directory for a given model name.
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

/// Model availability info
#[derive(Debug, Clone, serde::Serialize)]
pub struct EmbeddingModelInfo {
    pub name: String,
    pub available: bool,
    pub path: Option<String>,
    pub dimensions: usize,
    pub size: String,
}

/// Check available embedding models
#[tauri::command]
pub fn check_available_models(app: AppHandle) -> Vec<EmbeddingModelInfo> {
    let mut models = Vec::new();

    for (name, dims, size) in [
        ("BAAI/bge-small-zh-v1.5", 512, "~25MB"),
        (
            "BAAI/bge-base-zh-v1.5",
            768,
            "~390MB",
        ),
        (
            "BAAI/bge-large-zh-v1.5",
            1024,
            "~1.3GB",
        ),
    ] {
        let model_dir = resolve_model_dir(&app, name);
        let exists = match name {
            "BAAI/bge-small-zh-v1.5" | "BAAI/bge-large-zh-v1.5" => model_dir
                .as_ref()
                .map(|p| p.exists())
                .unwrap_or(false),
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

/// Download model files for a specific model
#[tauri::command]
pub async fn download_model_files(
    app: AppHandle,
    model_name: String,
) -> Result<String, String> {
    tracing::info!("Downloading model files for: {}", model_name);

    let resource_dir = app.path().resource_dir()
        .map_err(|e| format!("Failed to get resource directory: {}", e))?;

    let model_dir = resource_dir.join("models").join(model_name.replace('/', "-"));

    std::fs::create_dir_all(&model_dir)
        .map_err(|e| format!("Failed to create model directory: {}", e))?;

    let files = [
        ("tokenizer.json", format!("https://hf-mirror.com/{}/resolve/main/tokenizer.json", model_name)),
        ("tokenizer_config.json", format!("https://hf-mirror.com/{}/resolve/main/tokenizer_config.json", model_name)),
        ("special_tokens_map.json", format!("https://hf-mirror.com/{}/resolve/main/special_tokens_map.json", model_name)),
        ("vocab.txt", format!("https://hf-mirror.com/{}/resolve/main/vocab.txt", model_name)),
        ("config.json", format!("https://hf-mirror.com/{}/resolve/main/config.json", model_name)),
    ];

    let total = files.len();
    let mut downloaded = 0;

    let _ = app.emit("model-download-progress", serde_json::json!({
        "model": model_name,
        "current": 0,
        "total": total,
        "filename": "开始下载...",
        "status": "downloading"
    }));

    for (filename, url) in files {
        let path = model_dir.join(filename);
        if path.exists() {
            tracing::debug!("File already exists: {:?}", path);
            downloaded += 1;
            let _ = app.emit("model-download-progress", serde_json::json!({
                "model": model_name,
                "current": downloaded,
                "total": total,
                "filename": filename,
                "status": "skipping"
            }));
            continue;
        }

        tracing::info!("Downloading {} from {}", filename, url);

        let result = download_file_with_progress(&app, &url, &path, &model_name, downloaded, total);

        match result {
            Ok(_) => {
                tracing::info!("Downloaded: {}", filename);
                downloaded += 1;
            }
            Err(e) => {
                tracing::warn!("Failed to download {}: {}", filename, e);
                let mirror_url = url.replace("huggingface.co", "hf-mirror.com");
                let mirror_result = download_file_with_progress(&app, &mirror_url, &path, &model_name, downloaded, total);
                if mirror_result.is_ok() {
                    tracing::info!("Downloaded {} from mirror", filename);
                    downloaded += 1;
                }
            }
        }

        let _ = app.emit("model-download-progress", serde_json::json!({
            "model": model_name,
            "current": downloaded,
            "total": total,
            "filename": filename,
            "status": "done"
        }));
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
    std::fs::write(&model_json_path, serde_json::to_string_pretty(&model_json).unwrap())
        .map_err(|e| format!("Failed to write model.json: {}", e))?;

    let _ = app.emit("model-download-progress", serde_json::json!({
        "model": model_name,
        "current": total,
        "total": total,
        "filename": "完成",
        "status": "complete"
    }));

    Ok(format!("Downloaded {} files to {:?}", downloaded, model_dir))
}

fn download_file_with_progress(
    app: &AppHandle,
    url: &str,
    path: &std::path::Path,
    model_name: &str,
    current: usize,
    total: usize,
) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client.get(url)
        .send()
        .map_err(|e| format!("Failed to send request: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let bytes = response.bytes()
        .map_err(|e| format!("Failed to read response: {}", e))?;

    let filename = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let _ = app.emit("model-download-progress", serde_json::json!({
        "model": model_name,
        "current": current,
        "total": total,
        "filename": filename,
        "status": "downloading",
        "size": bytes.len()
    }));

    std::fs::write(path, bytes)
        .map_err(|e| format!("Failed to write file: {}", e))?;

    Ok(())
}
