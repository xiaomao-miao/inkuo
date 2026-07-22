//! Embedding-model discovery and download commands.
//!
//! Used to live inside `knowledge/commands.rs` as the bottom third of
//! that file. Splitting them out keeps the KB-build / KB-search
//! commands (which own the in-memory vector-store cache) in
//! `commands.rs`, and lets the model-discovery surface evolve
//! independently — e.g. swapping the on-disk layout or adding
//! multi-model selection — without churning the rest of the file.
//!
//! `#[tauri::command]` wrappers live in `commands.rs` so the
//! `tauri::generate_handler!` macro expansion in `lib.rs` can find the
//! `__cmd__<name>` symbol in the `commands` module.
//!
//! Helpers shared with the KB-build path (`first_existing_path`,
//! `resolve_model_dir`) stay in `commands.rs` because both code paths
//! need them.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Emitter, Manager};

use super::commands::{
    emit_model_download_progress, first_existing_path, resolve_model_dir,
    KnowledgeCommandError,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct EmbeddingModelInfo {
    pub name: String,
    pub available: bool,
    pub path: Option<String>,
    pub dimensions: usize,
    pub size: String,
}

pub fn check_available_models(app: &AppHandle) -> Vec<EmbeddingModelInfo> {
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

pub async fn download_model_files(
    app: &AppHandle,
    model_name: &str,
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
    let dimensions = match model_name {
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
