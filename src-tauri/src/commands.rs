//! Tauri commands module
//!
//! Exposes Rust backend functionality to the frontend via IPC.

use crate::{diff, document, ai, rag, file_watcher};
use std::collections::HashSet;
use parking_lot::Mutex;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{State, AppHandle};
use tokio::sync::mpsc;

pub static STREAM_CANCELLED: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// Channel for backup cleanup requests
static BACKUP_CLEANUP_TX: Lazy<Mutex<Option<mpsc::Sender<()>>>> = Lazy::new(|| Mutex::new(None));

/// Initialize background backup cleanup task
pub fn init_backup_cleanup_task() {
    // This must be called from within a Tokio runtime
    let (tx, mut rx) = mpsc::channel::<()>(32);

    tokio::spawn(async move {
        let mut pending_cleanups: Vec<tokio::time::Instant> = Vec::new();
        let cleanup_interval = tokio::time::Duration::from_secs(60); // Run cleanup at most once per minute
        let debounce_duration = tokio::time::Duration::from_secs(30); // Wait 30s after last request

        loop {
            tokio::select! {
                _ = rx.recv() => {
                    pending_cleanups.push(tokio::time::Instant::now() + debounce_duration);
                }
                _ = tokio::time::sleep(cleanup_interval) => {
                    if let Some(next_cleanup) = pending_cleanups.first() {
                        if tokio::time::Instant::now() >= *next_cleanup {
                            // Run cleanup
                            cleanup_old_backups(10);
                            pending_cleanups.clear();
                        }
                    }
                }
            }
        }
    });

    *BACKUP_CLEANUP_TX.lock() = Some(tx);
}

/// Request a backup cleanup (will be debounced)
pub fn request_backup_cleanup() {
    if let Some(tx) = BACKUP_CLEANUP_TX.lock().as_ref() {
        let _ = tx.try_send(());
    }
}

pub struct AppState {
    pub rag_index: Arc<tokio::sync::RwLock<rag::RAGIndex>>,
    pub ai_config: Arc<tokio::sync::RwLock<ai::AIConfig>>,
}

/// Get the backup directory path (e.g. ~/.inkuo/backups/)
fn get_backup_dir() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("inkuo")
        .join("backups")
}

/// Create a backup file path based on the original file path.
/// Uses a hash of the original path to avoid collisions and stores in ~/.inkuo/backups/
fn create_backup_path(original_path: &str) -> std::path::PathBuf {
    use sha2::{Sha256, Digest};

    // Create a hash of the original path for the backup filename
    let mut hasher = Sha256::new();
    hasher.update(original_path.as_bytes());
    let hash = hex::encode(&hasher.finalize()[..8]); // Use first 8 bytes

    // Extract just the filename from the original path
    let filename = std::path::Path::new(original_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let backup_dir = get_backup_dir();
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");

    // Format: original_filename_HASH_TIMESTAMP.bak
    backup_dir.join(format!("{}_{}_{}.bak", filename, hash, timestamp))
}

/// Clean up old backup files, keeping only the most recent N backups per original file.
fn cleanup_old_backups(max_backups_per_file: usize) {
    let backup_dir = get_backup_dir();

    if !backup_dir.exists() {
        return;
    }

    // Group backups by their original file hash (extracted from filename pattern)
    // Backup format: original_filename_HASH_TIMESTAMP.bak
    let mut backups_by_hash: std::collections::HashMap<String, Vec<std::path::PathBuf>> = std::collections::HashMap::new();

    if let Ok(entries) = std::fs::read_dir(&backup_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().map(|e| e == "bak").unwrap_or(false) {
                if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                    // Extract the hash from the filename (format: name_HASH_timestamp.bak)
                    // Find the last underscore before the timestamp pattern
                    if let Some(last_underscore) = filename.rfind('_') {
                        if let Some(second_last) = filename[..last_underscore].rfind('_') {
                            let hash = &filename[second_last + 1..last_underscore];
                            backups_by_hash
                                .entry(hash.to_string())
                                .or_default()
                                .push(path);
                        }
                    }
                }
            }
        }
    }

    // For each group, sort by modification time and delete old ones
    for (_, mut backups) in backups_by_hash {
        backups.sort_by(|a, b| {
            std::fs::metadata(b).and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                .cmp(&std::fs::metadata(a).and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH))
        });

        for backup in backups.into_iter().skip(max_backups_per_file) {
            let _ = std::fs::remove_file(backup);
        }
    }
}

impl AppState {
    pub async fn get_ai_config(&self) -> Result<ai::AIConfig, String> {
        // Try to read settings with flexible parsing
        let settings_result = read_settings_from_disk();

        let (ai_provider, model, temperature, max_tokens) = if let Ok(settings) = settings_result {
            // Try to use the new multi-API config first
            if let Some(ref active_id) = settings.active_api_config_id {
                if let Some(config) = settings.api_configs.iter().find(|c| c.id == *active_id) {
                    let provider = match config.provider.as_str() {
                        "ollama" => ai::AIProvider::Ollama {
                            base_url: config.base_url.clone(),
                        },
                        "official" => ai::AIProvider::Official {
                            api_key: config.api_key.clone().unwrap_or_default(),
                        },
                        _ => ai::AIProvider::OpenAI {
                            api_key: config.api_key.clone().unwrap_or_default(),
                            base_url: config.base_url.clone(),
                        },
                    };
                    return Ok(ai::AIConfig {
                        provider,
                        model: config.model.clone(),
                        temperature: config.temperature,
                        max_tokens: config.max_tokens,
                    });
                }
            }

            // Fallback to legacy settings
            let provider = match settings.ai_provider.as_str() {
                "ollama" => ai::AIProvider::Ollama {
                    base_url: settings.ai_base_url.clone()
                        .unwrap_or_else(|| "http://localhost:11434".to_string()),
                },
                _ => ai::AIProvider::OpenAI {
                    api_key: settings.ai_api_key.clone().unwrap_or_default(),
                    base_url: settings.ai_base_url.clone()
                        .unwrap_or_else(|| "https://api.deepseek.com".to_string()),
                },
            };
            (provider, settings.ai_model, settings.ai_temperature, settings.ai_max_tokens)
        } else {
            // No settings or parse error - use defaults
            tracing::warn!("Failed to read settings, using defaults");
            (
                ai::AIProvider::Ollama {
                    base_url: "http://localhost:11434".to_string(),
                },
                "llama3".to_string(),
                0.7,
                Some(4096),
            )
        };

        Ok(ai::AIConfig {
            provider: ai_provider,
            model,
            temperature,
            max_tokens,
        })
    }
}

impl Default for AppState {
    fn default() -> Self {
        let settings = read_settings_from_disk().unwrap_or_else(|_| Settings::default());

        let ai_provider = match settings.ai_provider.as_str() {
            "openai" | "deepseek" => ai::AIProvider::OpenAI {
                api_key: settings.ai_api_key.clone().unwrap_or_default(),
                base_url: settings
                    .ai_base_url
                    .clone()
                    .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            },
            "ollama" => ai::AIProvider::Ollama {
                base_url: settings
                    .ai_base_url
                    .clone()
                    .unwrap_or_else(|| "http://localhost:11434".to_string()),
            },
            _ => ai::AIProvider::OpenAI {
                api_key: settings.ai_api_key.clone().unwrap_or_default(),
                base_url: settings
                    .ai_base_url
                    .clone()
                    .unwrap_or_else(|| "https://api.deepseek.com".to_string()),
            },
        };

        let ai_config = ai::AIConfig {
            provider: ai_provider,
            model: settings.ai_model.clone(),
            temperature: settings.ai_temperature,
            max_tokens: settings.ai_max_tokens,
        };

        Self {
            rag_index: Arc::new(tokio::sync::RwLock::new(rag::RAGIndex::new())),
            ai_config: Arc::new(tokio::sync::RwLock::new(ai_config)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReadDocumentResult {
    pub document: document::Document,
    pub content: String,
}

#[tauri::command]
pub async fn read_document(path: String) -> Result<ReadDocumentResult, String> {
    tracing::info!("Reading document: {}", path);
    
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read file: {}", e))?;
    
    let doc = document::Document::from_markdown(&content, &path)
        .map_err(|e| format!("Failed to parse document: {}", e))?;
    
    Ok(ReadDocumentResult { document: doc, content })
}

#[tauri::command]
pub async fn write_document(path: String, content: String) -> Result<(), String> {
    tracing::info!("Writing document: {}", path);

    // Create backup in dedicated backup directory
    if std::path::Path::new(&path).exists() {
        // Ensure backup directory exists
        let backup_dir = get_backup_dir();
        std::fs::create_dir_all(&backup_dir)
            .map_err(|e| format!("Failed to create backup directory: {}", e))?;

        let backup_path = create_backup_path(&path);
        std::fs::copy(&path, &backup_path)
            .map_err(|e| format!("Failed to create backup: {}", e))?;

        // Request async backup cleanup (debounced, won't block the write)
        request_backup_cleanup();
    }

    std::fs::write(&path, &content)
        .map_err(|e| format!("Failed to write file: {}", e))?;

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
pub async fn list_directory(path: String) -> Result<Vec<FileEntry>, String> {
    tracing::info!("Listing directory: {}", path);
    
    let entries = std::fs::read_dir(&path)
        .map_err(|e| format!("Failed to read directory: {}", e))?;
    
    let mut files: Vec<FileEntry> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            
            // Skip hidden files
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
    
    // Sort: directories first, then files alphabetically
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
pub async fn watch_directory(
    path: String,
    state: State<'_, file_watcher::FileWatcherState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    tracing::info!("Starting file watcher for: {}", path);
    state.watch(std::path::PathBuf::from(path), app_handle)
}

#[tauri::command]
pub async fn unwatch_directory(
    state: State<'_, file_watcher::FileWatcherState>,
) -> Result<(), String> {
    tracing::info!("Stopping file watcher");
    state.stop();
    Ok(())
}

#[tauri::command]
pub async fn compute_diff(old_text: String, new_text: String) -> Result<diff::DiffResult, String> {
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
) -> Result<ai::AIEditResponse, String> {
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
        .map_err(|e| format!("AI error: {}", e))
}

#[tauri::command]
pub async fn search_knowledge_base(
    query: String,
    limit: usize,
    state: State<'_, AppState>,
) -> Result<rag::SearchResult, String> {
    tracing::info!("Searching knowledge base: {}", query);

    let index = state.rag_index.read().await;
    let results = index.search(&query, limit);
    Ok(results)
}

#[tauri::command]
pub async fn index_workspace(
    path: String,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    tracing::info!("Indexing workspace: {}", path);

    let mut count = 0;
    let index = Arc::clone(&state.rag_index);

    fn index_dir(dir: &std::path::Path, count: &mut usize) -> Result<(), String> {
        if !dir.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();

            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.starts_with('.') {
                    index_dir(&path, count)?;
                }
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if matches!(ext, "md" | "markdown") {
                    if std::fs::read_to_string(&path).is_ok() {
                        *count += 1;
                        // Store for batch indexing
                        tracing::debug!("Found markdown file: {:?}", path);
                    }
                }
            }
        }
        Ok(())
    }

    index_dir(std::path::Path::new(&path), &mut count)?;

    // Now do the actual indexing with the RAG index
    fn index_dir_recursive(
        dir: &std::path::Path,
        index: &rag::RAGIndex,
        count: &mut usize,
    ) -> Result<(), String> {
        if !dir.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();

            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.starts_with('.') {
                    index_dir_recursive(&path, index, count)?;
                }
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if matches!(ext, "md" | "markdown") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let doc_id = uuid::Uuid::new_v4().to_string();
                        let blocks = vec![];
                        index.index_document(&doc_id, path.to_str().unwrap_or(""), &content, &blocks);
                        *count += 1;
                    }
                }
            }
        }
        Ok(())
    }

    // Access the RAG index and do indexing
    let index_guard = index.read().await;
    index_dir_recursive(std::path::Path::new(&path), &index_guard, &mut count)?;

    Ok(count)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub id: String,
    pub name: String,
    pub provider: String,
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
    pub ai_provider: String,
    pub ai_model: String,
    pub ai_api_key: Option<String>,
    pub ai_base_url: Option<String>,
    pub ai_temperature: f32,
    pub ai_max_tokens: Option<u32>,
    // New multi-API config fields
    pub api_configs: Vec<ApiConfig>,
    pub active_api_config_id: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        let default_api_config = ApiConfig {
            id: uuid::Uuid::new_v4().to_string(),
            name: "DeepSeek V3".to_string(),
            provider: "deepseek".to_string(),
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
            ai_provider: "deepseek".to_string(),
            ai_model: "deepseek-chat".to_string(),
            ai_api_key: None,
            ai_base_url: Some("https://api.deepseek.com".to_string()),
            ai_temperature: 0.7,
            ai_max_tokens: Some(4096),
            api_configs: vec![default_api_config.clone()],
            active_api_config_id: Some(default_api_config.id),
        }
    }
}

fn get_settings_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("inkuo")
        .join("settings.json")
}

fn read_settings_from_disk() -> Result<Settings, String> {
    let path = get_settings_path();

    if !path.exists() {
        return Ok(Settings::default());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read settings: {}", e))?;

    // Try to parse as Settings, if it fails try to parse as legacy format
    match serde_json::from_str::<Settings>(&content) {
        Ok(settings) => Ok(settings),
        Err(e) => {
            tracing::warn!("Failed to parse settings as new format ({}), trying legacy format", e);
            // Legacy format doesn't have api_configs, so we need to create a default one
            #[derive(Debug, Deserialize)]
            struct LegacySettings {
                theme: Option<String>,
                accent_color: Option<String>,
                editor_font_size: Option<u32>,
                editor_font_family: Option<String>,
                ai_provider: Option<String>,
                ai_model: Option<String>,
                ai_api_key: Option<String>,
                ai_base_url: Option<String>,
                ai_temperature: Option<f32>,
                ai_max_tokens: Option<u32>,
            }

            let legacy: LegacySettings = serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse legacy settings: {}", e))?;

            let default_api_config_id = uuid::Uuid::new_v4().to_string();
            let default_api_config = ApiConfig {
                id: default_api_config_id.clone(),
                name: legacy.ai_model.clone().unwrap_or_else(|| "Default".to_string()),
                provider: legacy.ai_provider.clone().unwrap_or_else(|| "openai".to_string()),
                base_url: legacy.ai_base_url.clone().unwrap_or_else(|| "https://api.deepseek.com".to_string()),
                api_key: legacy.ai_api_key.clone(),
                model: legacy.ai_model.clone().unwrap_or_else(|| "deepseek-chat".to_string()),
                is_default: true,
                enabled: true,
                temperature: legacy.ai_temperature.unwrap_or(0.7),
                max_tokens: legacy.ai_max_tokens,
            };

            Ok(Settings {
                theme: legacy.theme.unwrap_or_else(|| "cursor-dark".to_string()),
                accent_color: legacy.accent_color.unwrap_or_else(|| "#7C5CFF".to_string()),
                editor_font_size: legacy.editor_font_size.unwrap_or(14),
                editor_font_family: legacy.editor_font_family.unwrap_or_else(|| "JetBrains Mono, monospace".to_string()),
                ai_provider: legacy.ai_provider.unwrap_or_else(|| "deepseek".to_string()),
                ai_model: legacy.ai_model.unwrap_or_else(|| "deepseek-chat".to_string()),
                ai_api_key: legacy.ai_api_key,
                ai_base_url: legacy.ai_base_url,
                ai_temperature: legacy.ai_temperature.unwrap_or(0.7),
                ai_max_tokens: legacy.ai_max_tokens,
                api_configs: vec![default_api_config],
                active_api_config_id: Some(default_api_config_id),
            })
        }
    }
}

#[tauri::command]
pub async fn get_settings() -> Result<Settings, String> {
    let path = get_settings_path();
    
    if !path.exists() {
        return Ok(Settings::default());
    }
    
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read settings: {}", e))?;
    
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings: {}", e))
}

#[tauri::command]
pub async fn save_settings(settings: Settings, state: State<'_, AppState>) -> Result<(), String> {
    let path = get_settings_path();

    std::fs::create_dir_all(path.parent().unwrap())
        .map_err(|e| format!("Failed to create config directory: {}", e))?;

    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write settings: {}", e))?;

    // Find the active API config or fall back to legacy settings
    let (ai_provider, model, temperature, max_tokens) = if let Some(ref active_id) = settings.active_api_config_id {
        settings.api_configs
            .iter()
            .find(|c| c.id == *active_id)
            .map(|c| {
                let provider = match c.provider.as_str() {
                    "openai" | "deepseek" => ai::AIProvider::OpenAI {
                        api_key: c.api_key.clone().unwrap_or_default(),
                        base_url: c.base_url.clone(),
                    },
                    "ollama" => ai::AIProvider::Ollama {
                        base_url: c.base_url.clone(),
                    },
                    "official" => ai::AIProvider::Official {
                        api_key: c.api_key.clone().unwrap_or_default(),
                    },
                    _ => ai::AIProvider::OpenAI {
                        api_key: c.api_key.clone().unwrap_or_default(),
                        base_url: c.base_url.clone(),
                    },
                };
                (provider, c.model.clone(), c.temperature, c.max_tokens)
            })
            .unwrap_or_else(|| {
                // Fall back to legacy settings
                let provider = match settings.ai_provider.as_str() {
                    "openai" | "deepseek" => ai::AIProvider::OpenAI {
                        api_key: settings.ai_api_key.clone().unwrap_or_default(),
                        base_url: settings.ai_base_url.clone().unwrap_or_else(|| "https://api.deepseek.com".to_string()),
                    },
                    "ollama" => ai::AIProvider::Ollama {
                        base_url: settings.ai_base_url.clone().unwrap_or_else(|| "http://localhost:11434".to_string()),
                    },
                    _ => ai::AIProvider::OpenAI {
                        api_key: settings.ai_api_key.clone().unwrap_or_default(),
                        base_url: settings.ai_base_url.clone().unwrap_or_else(|| "https://api.deepseek.com".to_string()),
                    },
                };
                (provider, settings.ai_model.clone(), settings.ai_temperature, settings.ai_max_tokens)
            })
    } else {
        // Legacy fallback
        let provider = match settings.ai_provider.as_str() {
            "openai" | "deepseek" => ai::AIProvider::OpenAI {
                api_key: settings.ai_api_key.clone().unwrap_or_default(),
                base_url: settings.ai_base_url.clone().unwrap_or_else(|| "https://api.deepseek.com".to_string()),
            },
            "ollama" => ai::AIProvider::Ollama {
                base_url: settings.ai_base_url.clone().unwrap_or_else(|| "http://localhost:11434".to_string()),
            },
            _ => ai::AIProvider::OpenAI {
                api_key: settings.ai_api_key.clone().unwrap_or_default(),
                base_url: settings.ai_base_url.clone().unwrap_or_else(|| "https://api.deepseek.com".to_string()),
            },
        };
        (provider, settings.ai_model.clone(), settings.ai_temperature, settings.ai_max_tokens)
    };

    let ai_config = ai::AIConfig {
        provider: ai_provider,
        model,
        temperature,
        max_tokens,
    };

    *state.ai_config.write().await = ai_config;

    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestApiConfigRequest {
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub provider: String,
}

#[tauri::command]
pub async fn test_api_config(
    request: TestApiConfigRequest,
) -> Result<TestResult, String> {
    tracing::info!("Testing API config: {} ({})", request.model, request.provider);

    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", request.base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": request.model,
        "messages": [
            {"role": "user", "content": "Say 'Hello, connection successful!' in exactly those words."}
        ],
        "max_tokens": 50,
    });

    let mut request_builder = client.post(&url);

    if let Some(key) = &request.api_key {
        if !key.is_empty() {
            request_builder = request_builder.header("Authorization", format!("Bearer {}", key));
        }
    }

    request_builder = request_builder.header("Content-Type", "application/json");

    let response = request_builder
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if response.status().is_success() {
        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        if let Some(content) = response_json["choices"][0]["message"]["content"].as_str() {
            Ok(TestResult {
                success: true,
                message: format!("连接成功！AI 回复: {}", content),
            })
        } else {
            Ok(TestResult {
                success: true,
                message: "连接成功！".to_string(),
            })
        }
    } else {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();

        Ok(TestResult {
            success: false,
            message: format!("连接失败 (HTTP {}): {}", status.as_u16(), error_text),
        })
    }
}

#[tauri::command]
pub async fn test_ai_connection(
    api_key: Option<String>,
    base_url: String,
    model: String,
) -> Result<TestResult, String> {
    tracing::info!("Testing AI connection to: {}", base_url);

    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "user", "content": "Say 'Hello, connection successful!' in exactly those words."}
        ],
        "max_tokens": 50,
    });

    let mut request = client.post(&url);

    if let Some(key) = &api_key {
        if !key.is_empty() {
            request = request.header("Authorization", format!("Bearer {}", key));
        }
    }

    request = request.header("Content-Type", "application/json");

    let response = request
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if response.status().is_success() {
        let response_json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        if let Some(content) = response_json["choices"][0]["message"]["content"].as_str() {
            Ok(TestResult {
                success: true,
                message: format!("连接成功！AI 回复: {}", content),
            })
        } else {
            Ok(TestResult {
                success: true,
                message: "连接成功！".to_string(),
            })
        }
    } else {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();

        Ok(TestResult {
            success: false,
            message: format!("连接失败 (HTTP {}): {}", status.as_u16(), error_text),
        })
    }
}
