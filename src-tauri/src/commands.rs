//! Tauri commands module
//!
//! Exposes Rust backend functionality to the frontend via IPC.

use std::collections::HashSet;
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
const DEFAULT_OLLAMA_BASE_URL: &str = "http://localhost:11434";

use crate::backup::{create_backup_path, get_backup_dir, request_backup_cleanup};
use crate::office;
use crate::{ai, diff, document, file_watcher};

pub static STREAM_CANCELLED: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

pub struct AppState {
    pub ai_config: Arc<tokio::sync::RwLock<ai::AIConfig>>,
}

fn default_base_url(provider: &str) -> &'static str {
    match provider {
        "openai" => DEFAULT_OPENAI_BASE_URL,
        "deepseek" => DEFAULT_DEEPSEEK_BASE_URL,
        "ollama" => DEFAULT_OLLAMA_BASE_URL,
        _ => DEFAULT_DEEPSEEK_BASE_URL,
    }
}

fn build_ai_provider(settings: &Settings) -> ai::AIProvider {
    match settings.ai_provider.as_str() {
        "openai" => ai::AIProvider::OpenAI {
            api_key: settings.ai_api_key.clone().unwrap_or_default(),
            base_url: settings
                .ai_base_url
                .clone()
                .unwrap_or_else(|| default_base_url("openai").to_string()),
        },
        "deepseek" => ai::AIProvider::OpenAI {
            api_key: settings.ai_api_key.clone().unwrap_or_default(),
            base_url: settings
                .ai_base_url
                .clone()
                .unwrap_or_else(|| default_base_url("deepseek").to_string()),
        },
        "ollama" => ai::AIProvider::Ollama {
            base_url: settings
                .ai_base_url
                .clone()
                .unwrap_or_else(|| default_base_url("ollama").to_string()),
        },
        "official" => ai::AIProvider::Official {
            api_key: settings.ai_api_key.clone().unwrap_or_default(),
        },
        _ => ai::AIProvider::OpenAI {
            api_key: settings.ai_api_key.clone().unwrap_or_default(),
            base_url: settings
                .ai_base_url
                .clone()
                .unwrap_or_else(|| default_base_url("deepseek").to_string()),
        },
    }
}

fn active_api_config<'a>(settings: &'a Settings) -> Option<&'a ApiConfig> {
    let active_id = settings.active_api_config_id.as_ref()?;
    settings.api_configs.iter().find(|config| config.id == *active_id)
}

fn build_provider_from_api_config(config: &ApiConfig) -> ai::AIProvider {
    match config.provider.as_str() {
        "openai" | "deepseek" => ai::AIProvider::OpenAI {
            api_key: config.api_key.clone().unwrap_or_default(),
            base_url: config
                .base_url
                .clone(),
        },
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
    }
}

fn build_ai_config(settings: &Settings) -> ai::AIConfig {
    if let Some(config) = active_api_config(settings) {
        return ai::AIConfig {
            provider: build_provider_from_api_config(config),
            model: config.model.clone(),
            temperature: config.temperature,
            max_tokens: config.max_tokens,
        };
    }

    ai::AIConfig {
        provider: build_ai_provider(settings),
        model: settings.ai_model.clone(),
        temperature: settings.ai_temperature,
        max_tokens: settings.ai_max_tokens,
    }
}

impl Default for AppState {
    fn default() -> Self {
        let settings = read_settings_from_disk().unwrap_or_else(|_| Settings::default());

        let ai_config = build_ai_config(&settings);

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
pub async fn read_document(path: String) -> Result<ReadDocumentResult, String> {
    tracing::info!("Reading document: {}", path);

    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64)
        .unwrap_or(0);

    let doc = document::Document::from_markdown(&content, &path)
        .map_err(|e| format!("Failed to parse document: {}", e))?;

    Ok(ReadDocumentResult { document: doc, content, mtime })
}

#[tauri::command]
pub async fn write_document(path: String, content: String) -> Result<(), String> {
    tracing::info!("Writing document: {}", path);

    if std::path::Path::new(&path).exists() {
        let backup_dir = get_backup_dir();
        std::fs::create_dir_all(&backup_dir)
            .map_err(|e| format!("Failed to create backup directory: {}", e))?;

        let backup_path = create_backup_path(&path);
        std::fs::copy(&path, &backup_path)
            .map_err(|e| format!("Failed to create backup: {}", e))?;

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
    pub api_configs: Vec<ApiConfig>,
    pub active_api_config_id: Option<String>,
    // Knowledge base settings
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
            // Knowledge base defaults
            embedding_model: "BAAI/bge-small-zh-v1.5".to_string(),
            embedding_model_path: None,
            chunk_size: 500,
            chunk_overlap: 50,
        }
    }
}

/// Get embedding model name from settings
pub fn get_embedding_model() -> String {
    read_settings_from_disk()
        .map(|s| s.embedding_model)
        .unwrap_or_else(|_| "BAAI/bge-small-zh-v1.5".to_string())
}

/// Get chunk size from settings
pub fn get_chunk_size() -> usize {
    read_settings_from_disk()
        .map(|s| s.chunk_size)
        .unwrap_or(500)
}

fn get_settings_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("inkuo")
        .join("settings.json")
}

pub fn read_settings_from_disk() -> Result<Settings, String> {
    let path = get_settings_path();

    if !path.exists() {
        return Ok(Settings::default());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read settings: {}", e))?;

    match serde_json::from_str::<Settings>(&content) {
        Ok(settings) => Ok(settings),
        Err(e) => {
            tracing::warn!("Failed to parse settings as new format ({}), trying merged format", e);

            let value: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse settings JSON: {}", e))?;

            if let Some(object) = value.as_object() {
                let mut merged = serde_json::to_value(Settings::default())
                    .map_err(|e| format!("Failed to serialize default settings: {}", e))?;

                if let Some(merged_object) = merged.as_object_mut() {
                    for (key, value) in object {
                        merged_object.insert(key.clone(), value.clone());
                    }
                }

                if let Ok(settings) = serde_json::from_value::<Settings>(merged) {
                    return Ok(settings);
                }
            }

            tracing::warn!("Falling back to legacy settings parser");
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
                // Knowledge base defaults for legacy settings
                embedding_model: "BAAI/bge-small-zh-v1.5".to_string(),
                embedding_model_path: None,
                chunk_size: 500,
                chunk_overlap: 50,
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

    *state.ai_config.write().await = build_ai_config(&settings);

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

#[tauri::command]
pub async fn read_office_file(path: String) -> Result<Vec<u8>, String> {
    tracing::info!("Reading office file: {}", path);
    std::fs::read(&path).map_err(|e| format!("Failed to read file: {}", e))
}

#[tauri::command]
pub async fn write_office_file(path: String, data: Vec<u8>) -> Result<(), String> {
    tracing::info!("Writing office file: {}", path);
    std::fs::write(&path, &data).map_err(|e| format!("Failed to write file: {}", e))
}

#[tauri::command]
pub async fn read_office_text(path: String) -> Result<OfficeFileResult, String> {
    tracing::info!("Reading office file as text: {}", path);

    let result = office::read_office_file(std::path::Path::new(&path))
        .map_err(|e| format!("Failed to read office file: {}", e))?;

    let (file_type, text_content) = result;

    match file_type {
        office::OfficeFileType::Word(doc) => Ok(OfficeFileResult {
            file_type: "docx".to_string(),
            text_content,
            json_content: serde_json::to_string(&doc).unwrap_or_default(),
            sheet_names: None,
        }),
        office::OfficeFileType::Excel(workbook) => Ok(OfficeFileResult {
            file_type: "xlsx".to_string(),
            text_content,
            json_content: serde_json::to_string(&workbook).unwrap_or_default(),
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
pub async fn write_office_text(
    path: String,
    json_content: String,
    format: String,
) -> Result<(), String> {
    tracing::info!("Writing office file: {} ({})", path, format);

    let path_obj = std::path::Path::new(&path);

    office::write_office_file(path_obj, &json_content)
        .map_err(|e| format!("Failed to write office file: {}", e))
}
