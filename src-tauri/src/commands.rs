//! Tauri commands module
//!
//! Exposes Rust backend functionality to the frontend via IPC.

use std::collections::HashSet;
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use thiserror::Error;

use crate::backup::{create_backup_path, get_backup_dir, request_backup_cleanup};
use crate::office;
use crate::{ai, ai_config::{self, AITestResult, AIProviderKind, TestApiConfigRequest}, diff, document, file_watcher};

pub static STREAM_CANCELLED: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

#[derive(Debug, Clone, Serialize, Deserialize, Error)]
pub enum AppCommandError {
    #[error("Failed to read document: {0}")]
    ReadDocument(String),
    #[error("Failed to parse document: {0}")]
    ParseDocument(String),
    #[error("Failed to create backup directory: {0}")]
    CreateBackupDirectory(String),
    #[error("Failed to create backup: {0}")]
    CreateBackup(String),
    #[error("Failed to write document: {0}")]
    WriteDocument(String),
    #[error("Failed to list directory: {0}")]
    ListDirectory(String),
    #[error("Failed to watch directory: {0}")]
    WatchDirectory(String),
    #[error("AI edit failed: {0}")]
    AIEdit(String),
    #[error("AI connection test failed: {0}")]
    TestAIConnection(String),
    #[error("Failed to read settings: {0}")]
    ReadSettings(String),
    #[error("Failed to parse settings: {0}")]
    ParseSettings(String),
    #[error("Failed to create config directory: {0}")]
    CreateConfigDirectory(String),
    #[error("Failed to serialize settings: {0}")]
    SerializeSettings(String),
    #[error("Failed to write settings: {0}")]
    WriteSettings(String),
    #[error("Failed to read office file: {0}")]
    ReadOfficeFile(String),
    #[error("Failed to serialize office document: {0}")]
    SerializeOfficeDocument(String),
    #[error("Failed to write office file: {0}")]
    WriteOfficeFile(String),
    #[error("Invalid config path")]
    InvalidConfigPath,
}

pub struct AppState {
    pub ai_config: Arc<tokio::sync::RwLock<ai::AIConfig>>,
}

impl Default for AppState {
    fn default() -> Self {
        let settings = read_settings_from_disk().unwrap_or_else(|_| Settings::default());

        let ai_config = ai_config::build_settings_ai_config(&settings);

        Self {
            ai_config: Arc::new(tokio::sync::RwLock::new(ai_config)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReadDocumentResult {
    pub document: document::Document,
    pub content: String,
    pub mtime: i64,  // Unix timestamp in milliseconds
}

#[tauri::command]
pub async fn read_document(path: String) -> Result<ReadDocumentResult, AppCommandError> {
    tracing::info!("Reading document: {}", path);

    let content = std::fs::read_to_string(&path)
        .map_err(|e| AppCommandError::ReadDocument(e.to_string()))?;

    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64)
        .unwrap_or(0);

    let doc = document::Document::from_markdown(&content, &path)
        .map_err(|e| AppCommandError::ParseDocument(e.to_string()))?;

    Ok(ReadDocumentResult { document: doc, content, mtime })
}

#[tauri::command]
pub async fn write_document(path: String, content: String) -> Result<(), AppCommandError> {
    tracing::info!("Writing document: {}", path);

    if std::path::Path::new(&path).exists() {
        let backup_dir = get_backup_dir();
        std::fs::create_dir_all(&backup_dir)
            .map_err(|e| AppCommandError::CreateBackupDirectory(e.to_string()))?;

        let backup_path = create_backup_path(&path);
        std::fs::copy(&path, &backup_path)
            .map_err(|e| AppCommandError::CreateBackup(e.to_string()))?;

        request_backup_cleanup();
    }

    // Use atomic write: write to a temp file, then rename (POSIX guarantees atomicity)
    let path_obj = std::path::Path::new(&path);
    let temp_path = path_obj.with_extension(
        path_obj
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!("{}.tmp", e))
            .unwrap_or_else(|| "tmp".to_string()),
    );

    std::fs::write(&temp_path, &content)
        .map_err(|e| AppCommandError::WriteDocument(e.to_string()))?;

    std::fs::rename(&temp_path, &path)
        .map_err(|e| AppCommandError::WriteDocument(e.to_string()))?;

    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_markdown: bool,
}

#[tauri::command]
pub async fn list_directory(path: String) -> Result<Vec<FileEntry>, AppCommandError> {
    tracing::info!("Listing directory: {}", path);

    let entries = std::fs::read_dir(&path)
        .map_err(|e| AppCommandError::ListDirectory(e.to_string()))?;

    let mut files: Vec<FileEntry> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if name.starts_with('.') {
                return None;
            }

            let is_dir = path.is_dir();
            let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let is_markdown = matches!(extension, "md" | "markdown" | "txt");

            Some(FileEntry {
                name,
                path: path.to_string_lossy().to_string(),
                is_dir,
                is_markdown,
            })
        })
        .collect();

    files.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });

    Ok(files)
}

#[tauri::command]
pub async fn search_directory(
    path: String,
    query: String,
) -> Result<Vec<FileEntry>, AppCommandError> {
    tracing::info!("Searching directory: {} for '{}'", path, query);

    if query.is_empty() {
        return Ok(vec![]);
    }

    let query_lower = query.to_lowercase();
    let mut results: Vec<FileEntry> = Vec::new();

    fn walk_dir(
        dir: &std::path::Path,
        query: &str,
        results: &mut Vec<FileEntry>,
        max_results: usize,
    ) {
        if results.len() >= max_results {
            return;
        }

        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            if results.len() >= max_results {
                break;
            }

            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            if name.starts_with('.') {
                continue;
            }

            let is_dir = path.is_dir();

            if name.to_lowercase().contains(query) {
                let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let is_markdown = matches!(extension, "md" | "markdown" | "txt");

                results.push(FileEntry {
                    name,
                    path: path.to_string_lossy().to_string(),
                    is_dir,
                    is_markdown,
                });
            }

            if is_dir {
                walk_dir(&path, query, results, max_results);
            }
        }
    }

    walk_dir(std::path::Path::new(&path), &query_lower, &mut results, 100);

    // Sort results: directories first, then by relevance (shorter paths first)
    results.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => {
                let a_depth = a.path.matches('/').count();
                let b_depth = b.path.matches('/').count();
                if a_depth != b_depth {
                    a_depth.cmp(&b_depth)
                } else {
                    a.name.to_lowercase().cmp(&b.name.to_lowercase())
                }
            }
        }
    });

    Ok(results)
}

#[tauri::command]
pub async fn watch_directory(
    path: String,
    state: State<'_, file_watcher::FileWatcherState>,
    app_handle: AppHandle,
) -> Result<(), AppCommandError> {
    tracing::info!("Starting file watcher for: {}", path);
    state
        .watch(std::path::PathBuf::from(path), app_handle)
        .map_err(|error| AppCommandError::WatchDirectory(error.to_string()))
}

#[tauri::command]
pub async fn unwatch_directory(
    state: State<'_, file_watcher::FileWatcherState>,
) -> Result<(), AppCommandError> {
    tracing::info!("Stopping file watcher");
    state.stop();
    Ok(())
}

#[tauri::command]
pub async fn compute_diff(old_text: String, new_text: String) -> Result<diff::DiffResult, AppCommandError> {
    tracing::info!("Computing diff");
    Ok(diff::compute_diff(&old_text, &new_text))
}

#[tauri::command]
pub async fn ai_edit(
    instruction: String,
    original_text: String,
    scope: String,
    context: Vec<ai::ContextItem>,
    state: State<'_, AppState>,
) -> Result<ai::AIEditResponse, AppCommandError> {
    tracing::info!("AI edit request: {}", instruction);

    let config = state.ai_config.read().await.clone();
    let adapter = ai::AIProviderAdapter::new(config);

    let edit_scope = match scope.as_str() {
        "selection" => ai::EditScope::Selection,
        "paragraph" => ai::EditScope::Paragraph,
        "section" => ai::EditScope::Section,
        "document" => ai::EditScope::Document,
        _ => ai::EditScope::Selection,
    };

    let request = ai::AIEditRequest {
        instruction,
        original_text,
        scope: edit_scope,
        context,
    };

    adapter.edit(request)
        .await
        .map_err(|e| AppCommandError::AIEdit(e.to_string()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub id: String,
    pub name: String,
    pub provider: AIProviderKind,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub is_default: bool,
    pub enabled: bool,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub theme: String,
    pub accent_color: String,
    pub editor_font_size: u32,
    pub editor_font_family: String,
    pub editor_word_wrap: bool,
    pub editor_line_numbers: bool,
    pub api_configs: Vec<ApiConfig>,
    pub active_api_config_id: Option<String>,
    pub embedding_model: String,
    pub embedding_model_path: Option<String>,
    pub chunk_size: usize,
    pub chunk_overlap: usize,
}

impl Default for Settings {
    fn default() -> Self {
        let default_api_config = ApiConfig {
            id: uuid::Uuid::new_v4().to_string(),
            name: "DeepSeek V3".to_string(),
            provider: AIProviderKind::DeepSeek,
            base_url: "https://api.deepseek.com".to_string(),
            api_key: None,
            model: "deepseek-chat".to_string(),
            is_default: true,
            enabled: true,
            temperature: 0.7,
            max_tokens: Some(4096),
        };

        Self {
            theme: "cursor-dark".to_string(),
            accent_color: "#7C5CFF".to_string(),
            editor_font_size: 14,
            editor_font_family: "JetBrains Mono, monospace".to_string(),
            editor_word_wrap: true,
            editor_line_numbers: true,
            api_configs: vec![default_api_config.clone()],
            active_api_config_id: Some(default_api_config.id),
            embedding_model: "BAAI/bge-small-zh-v1.5".to_string(),
            embedding_model_path: None,
            chunk_size: 500,
            chunk_overlap: 50,
        }
    }
}

/// Cached settings to avoid repeated disk reads.
/// Updated whenever save_settings is called.
static SETTINGS_CACHE: Lazy<Mutex<Option<Settings>>> = Lazy::new(|| Mutex::new(None));

/// Get cached settings, reading from disk only when cache is empty.
pub fn get_settings_cached() -> Result<Settings, AppCommandError> {
    {
        let guard = SETTINGS_CACHE.lock();
        if let Some(ref settings) = *guard {
            return Ok(settings.clone());
        }
    }
    let settings = read_settings_from_disk()?;
    let mut guard = SETTINGS_CACHE.lock();
    *guard = Some(settings.clone());
    Ok(settings)
}

/// Update the settings cache when settings are saved.
pub fn update_settings_cache(settings: Settings) {
    let mut guard = SETTINGS_CACHE.lock();
    *guard = Some(settings);
}

/// Get embedding model name from settings
pub fn get_embedding_model() -> String {
    get_settings_cached()
        .map(|s| s.embedding_model)
        .unwrap_or_else(|_| "BAAI/bge-small-zh-v1.5".to_string())
}

/// Get chunk size from settings
pub fn get_chunk_size() -> usize {
    get_settings_cached()
        .map(|s| s.chunk_size)
        .unwrap_or(500)
}

fn get_settings_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("inkuo")
        .join("settings.json")
}

pub fn read_settings_from_disk() -> Result<Settings, AppCommandError> {
    let path = get_settings_path();

    if !path.exists() {
        return Ok(Settings::default());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| AppCommandError::ReadSettings(e.to_string()))?;

    match serde_json::from_str::<Settings>(&content) {
        Ok(settings) => Ok(settings),
        Err(e) => {
            tracing::warn!("Failed to parse settings ({}), trying merged format", e);

            let value: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| AppCommandError::ParseSettings(format!("settings JSON: {}", e)))?;

            if let Some(object) = value.as_object() {
                let mut merged = serde_json::to_value(Settings::default())
                    .map_err(|e| AppCommandError::SerializeSettings(format!("default settings: {}", e)))?;

                if let Some(merged_object) = merged.as_object_mut() {
                    for (key, value) in object {
                        merged_object.insert(key.clone(), value.clone());
                    }
                }

                if let Ok(settings) = serde_json::from_value::<Settings>(merged) {
                    return Ok(settings);
                }
            }

            Err(AppCommandError::ParseSettings(format!(
                "settings format is invalid and no longer supports legacy single-config fields: {}",
                e
            )))
        }
    }
}

#[tauri::command]
pub async fn get_settings() -> Result<Settings, AppCommandError> {
    read_settings_from_disk()
}

#[tauri::command]
pub async fn save_settings(settings: Settings, state: State<'_, AppState>) -> Result<(), AppCommandError> {
    let path = get_settings_path();
    let config_dir = path.parent().ok_or(AppCommandError::InvalidConfigPath)?;

    std::fs::create_dir_all(config_dir)
        .map_err(|e| AppCommandError::CreateConfigDirectory(e.to_string()))?;

    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| AppCommandError::SerializeSettings(e.to_string()))?;

    std::fs::write(&path, content)
        .map_err(|e| AppCommandError::WriteSettings(e.to_string()))?;

    *state.ai_config.write().await = ai_config::build_settings_ai_config(&settings);

    Ok(())
}

pub type TestResult = AITestResult;

#[tauri::command]
pub async fn test_api_config(
    request: TestApiConfigRequest,
) -> Result<TestResult, AppCommandError> {
    tracing::info!("Testing API config: {} ({})", request.model, request.provider);
    ai_config::test_ai_connection_impl(
        request.api_key.as_deref(),
        &request.base_url,
        &request.model,
    )
    .await
    .map_err(|error| AppCommandError::TestAIConnection(error.to_string()))
}

#[tauri::command]
pub async fn read_office_file(path: String) -> Result<Vec<u8>, AppCommandError> {
    tracing::info!("Reading office file: {}", path);
    std::fs::read(&path).map_err(|e| AppCommandError::ReadOfficeFile(e.to_string()))
}

#[tauri::command]
pub async fn write_office_file(path: String, data: Vec<u8>) -> Result<(), AppCommandError> {
    tracing::info!("Writing office file: {}", path);
    std::fs::write(&path, &data).map_err(|e| AppCommandError::WriteOfficeFile(e.to_string()))
}

#[tauri::command]
pub async fn read_office_text(path: String) -> Result<OfficeFileResult, AppCommandError> {
    tracing::info!("Reading office file as text: {}", path);

    let result = office::read_office_file(std::path::Path::new(&path))
        .map_err(|e| AppCommandError::ReadOfficeFile(e.to_string()))?;

    let (file_type, text_content) = result;

    match file_type {
        office::OfficeFileType::Word(doc) => Ok(OfficeFileResult {
            file_type: "docx".to_string(),
            text_content,
            json_content: serde_json::to_string(&doc)
                .map_err(|e| AppCommandError::SerializeOfficeDocument(e.to_string()))?,
            sheet_names: None,
        }),
        office::OfficeFileType::Excel(workbook) => Ok(OfficeFileResult {
            file_type: "xlsx".to_string(),
            text_content,
            json_content: serde_json::to_string(&workbook)
                .map_err(|e| AppCommandError::SerializeOfficeDocument(e.to_string()))?,
            sheet_names: Some(workbook.sheets.iter().map(|s| s.name.clone()).collect()),
        }),
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OfficeFileResult {
    pub file_type: String,
    pub text_content: String,
    pub json_content: String,
    pub sheet_names: Option<Vec<String>>,
}

#[tauri::command]
pub async fn read_xlsx_structured(path: String) -> Result<office::XlsxWorkbook, AppCommandError> {
    tracing::info!("Reading xlsx as structured workbook: {}", path);
    let bytes = std::fs::read(&path).map_err(|e| AppCommandError::ReadOfficeFile(e.to_string()))?;
    office::read_xlsx_structured(&bytes).map_err(|e| AppCommandError::ReadOfficeFile(e.to_string()))
}

#[tauri::command]
pub async fn write_xlsx_structured(
    path: String,
    workbook: office::XlsxWorkbook,
) -> Result<(), AppCommandError> {
    tracing::info!("Writing structured xlsx workbook: {}", path);
    let path_obj = std::path::Path::new(&path);
    office::create_xlsx_workbook(&workbook, path_obj)
        .map_err(|e| AppCommandError::WriteOfficeFile(e.to_string()))
}

#[tauri::command]
pub async fn write_office_text(
    path: String,
    json_content: String,
    format: String,
) -> Result<(), AppCommandError> {
    tracing::info!("Writing office file: {} ({})", path, format);

    let path_obj = std::path::Path::new(&path);

    office::write_office_file(path_obj, &json_content)
        .map_err(|e| AppCommandError::WriteOfficeFile(e.to_string()))
}
